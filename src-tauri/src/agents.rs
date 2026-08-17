//! Coding-agent CLIs driven from the chat panel.
//!
//! Depot is not an AI client. It runs whichever agent CLI the user already has
//! installed and signed in — Claude Code, Codex, Gemini, Copilot, opencode — in
//! that tool's own non-interactive mode, and turns its output into structured
//! events for the panel: thinking, text, tool calls as they happen, and a final
//! result. No API key ever passes through Depot.
//!
//! Adding an engine is a row in [`ENGINES`] plus, if it speaks something richer
//! than plain lines, a branch in [`Protocol`]. Everything else — binary
//! resolution, environment, cancellation, session continuity, auth
//! diagnostics — is shared.
//!
//! A checkpoint of the workspace is taken before every run (see the chat
//! panel), so anything an agent does can be kept or reverted file by file.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::OnceCell;
use tokio::time::timeout;

/* ── engine table ──────────────────────────────────────────────── */

/// How an engine's stdout is read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// `claude -p --output-format stream-json`: NDJSON with token-level deltas.
    ClaudeStreamJson,
    /// `codex exec --json`: NDJSON of thread/turn/item events.
    CodexJsonl,
    /// Anything else: each stdout line is shown as reply text.
    PlainText,
}

pub struct Engine {
    pub id: &'static str,
    pub label: &'static str,
    /// Bare command name, resolved against PATH and the usual install spots.
    pub bin: &'static str,
    pub protocol: Protocol,
    /// Shown when the CLI is missing.
    pub install: &'static str,
    /// Shown when the CLI is installed but not authenticated.
    pub sign_in: &'static str,
}

/// The CLIs Depot knows how to drive. Presets, not a closed list: an engine
/// that is not installed is simply reported unavailable.
pub const ENGINES: &[Engine] = &[
    Engine {
        id: "claude",
        label: "Claude Code",
        bin: "claude",
        protocol: Protocol::ClaudeStreamJson,
        install: "npm install -g @anthropic-ai/claude-code",
        sign_in: "claude",
    },
    Engine {
        id: "codex",
        label: "Codex",
        bin: "codex",
        protocol: Protocol::CodexJsonl,
        install: "npm install -g @openai/codex",
        sign_in: "codex login",
    },
    Engine {
        id: "gemini",
        label: "Gemini CLI",
        bin: "gemini",
        protocol: Protocol::PlainText,
        install: "npm install -g @google/gemini-cli",
        sign_in: "gemini",
    },
    Engine {
        id: "copilot",
        label: "Copilot CLI",
        bin: "copilot",
        protocol: Protocol::PlainText,
        install: "npm install -g @github/copilot",
        sign_in: "copilot",
    },
    Engine {
        id: "opencode",
        label: "opencode",
        bin: "opencode",
        protocol: Protocol::PlainText,
        install: "npm install -g opencode-ai",
        sign_in: "opencode auth login",
    },
];

pub fn engine(id: &str) -> Option<&'static Engine> {
    ENGINES.iter().find(|e| e.id == id)
}

/* ── wire types ────────────────────────────────────────────────── */

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub id: String,
    pub label: String,
    /// True when the CLI resolves — on PATH, in a known install spot, or via
    /// the user's login shell.
    pub available: bool,
    /// e.g. "2.1.234", when the CLI answered `--version`.
    pub version: Option<String>,
    /// Absolute path we will actually run, for the diagnostics panel.
    pub path: Option<String>,
    pub install: String,
    pub sign_in: String,
    /// True when this engine streams thinking and tool calls, rather than
    /// plain lines of text.
    pub structured: bool,
}

/// What the chat panel found out about one engine when a run failed to
/// authenticate. Values are never included — only which variables are set.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EngineDoctor {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub install: String,
    pub sign_in: String,
    /// Human-readable findings, in the order they matter.
    pub notes: Vec<String>,
}

/// Options from the chat panel. Anything unset falls back to the user's own
/// CLI configuration.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentOptions {
    /// Engine id from [`ENGINES`]; defaults to Claude Code.
    pub engine: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    /// False starts a fresh conversation instead of resuming the last one.
    pub resume: Option<bool>,
}

impl AgentOptions {
    fn engine(&self) -> &'static Engine {
        self.engine
            .as_deref()
            .and_then(engine)
            .unwrap_or(&ENGINES[0])
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentDone {
    pub id: String,
    pub code: Option<i32>,
    pub error: Option<String>,
}

/* ── binary resolution ─────────────────────────────────────────── */

/// Resolves a bare command name against PATH, the way a shell would.
fn on_path(command: &str) -> bool {
    if command.contains('/') || command.contains('\\') {
        return Path::new(command).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|e| e.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };

    std::env::split_paths(&paths).any(|dir| {
        exts.iter().any(|ext| {
            let candidate = dir.join(format!("{command}{ext}"));
            candidate.is_file()
        })
    })
}

/// Well-known install spots, checked when the process PATH does not have the
/// CLI. macOS apps launched from Finder get a bare
/// `/usr/bin:/bin:/usr/sbin:/sbin` PATH, so a CLI installed via npm, a native
/// installer, nvm, volta, bun or Homebrew is invisible to us unless we look
/// for it ourselves.
fn candidate_paths(bin: &str) -> Vec<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{bin}.exe")
    } else {
        bin.to_string()
    };
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = Path::new(&home);
        for rel in [
            ".local/bin",      // native installers
            ".claude/local/bin",
            ".codex/bin",
            ".npm-global/bin", // npm with a custom prefix
            ".volta/bin",
            ".bun/bin",
            ".fnm/aliases/default/bin",
            "bin",
        ] {
            out.push(home.join(rel).join(&exe));
        }
        // nvm keeps one bin dir per Node version; newest first.
        let nvm = home.join(".nvm/versions/node");
        if let Ok(versions) = std::fs::read_dir(&nvm) {
            let mut versions: Vec<PathBuf> = versions.flatten().map(|e| e.path()).collect();
            versions.sort();
            for v in versions.into_iter().rev() {
                out.push(v.join("bin").join(&exe));
            }
        }
    }
    out.push(PathBuf::from("/usr/local/bin").join(&exe));
    out.push(PathBuf::from("/opt/homebrew/bin").join(&exe));
    out
}

/// Last resort: ask the user's login shell, which has sourced whatever rc file
/// puts the CLI on PATH — the same lookup an interactive terminal does. rc
/// files can print anything, so the answer is the last line naming a real file.
async fn shell_lookup(bin: &str) -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let lookup = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("command -v {bin}")])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let out = timeout(Duration::from_secs(4), lookup).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8(out.stdout).ok()?;
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with('/') && Path::new(l).is_file())
        .map(str::to_string)
}

type Resolved = Arc<Mutex<HashMap<String, String>>>;

fn resolved() -> &'static Resolved {
    static RESOLVED: OnceLock<Resolved> = OnceLock::new();
    RESOLVED.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// The absolute path to an engine's CLI, or the bare name when it is already
/// on the process PATH. Hits are cached; a miss is not, so installing a CLI
/// while Depot runs is picked up on the next try.
async fn resolve(bin: &str) -> Option<String> {
    if let Some(path) = resolved().lock().unwrap().get(bin) {
        return Some(path.clone());
    }
    let found = if on_path(bin) {
        Some(bin.to_string())
    } else {
        match candidate_paths(bin).into_iter().find(|p| p.is_file()) {
            Some(p) => Some(p.display().to_string()),
            None => shell_lookup(bin).await,
        }
    };
    if let Some(path) = &found {
        resolved().lock().unwrap().insert(bin.into(), path.clone());
    }
    found
}

/* ── environment ───────────────────────────────────────────────── */

/// Variables a user sets deliberately, which survive the scrub below even
/// though they share its prefixes.
const USER_OWNED: &[&str] = &[
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "CLAUDE_CONFIG_DIR",
];

/// True for the variables that mean "you are a managed child of a running
/// Claude Code host".
///
/// This is the bug that makes a chat panel fail while the same CLI works in a
/// terminal. When Claude Code runs a child under a host — the desktop app, the
/// Agent SDK — it hands that child a messaging socket, a host session id, and
/// `CLAUDE_CODE_SDK_HAS_OAUTH_REFRESH`, which tells the child *not* to refresh
/// its own token because the host will do it. If Depot is started from inside
/// such a session (`npm run tauri dev` run by an agent, or from that agent's
/// terminal), those variables are in Depot's environment, and Depot's login
/// shell inherits them too. Passing them to our own `claude -p` makes it wait
/// for a host that is not listening — so an expired token is never refreshed,
/// and the CLI reports "OAuth session expired and could not be refreshed"
/// instantly, without ever reaching the network.
///
/// A terminal has none of these, which is exactly why the terminal works.
fn is_host_session_var(key: &str) -> bool {
    if USER_OWNED.contains(&key) {
        return false;
    }
    key == "CLAUDECODE"
        || key.starts_with("CLAUDE_CODE_")
        || key.starts_with("CLAUDE_AGENT_SDK")
        || key.starts_with("CLAUDE_PLUGIN_")
        || key.starts_with("CLAUDE_PREVIEW_")
        || key.starts_with("CODEX_COMPANION_")
        || matches!(key, "CLAUDE_PID" | "CLAUDE_EFFORT" | "CLAUDE_PROJECT_DIR")
}

/// A host also points its child at its own endpoint and hands it a token to
/// use there. Those two are legitimate settings on their own, so they are
/// dropped only when the host markers prove where they came from — a user's
/// own gateway or API key is left alone.
const HOST_ROUTING: &[&str] = &["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN"];

fn in_host_session(env: &HashMap<String, String>) -> bool {
    ["CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_SDK_HAS_OAUTH_REFRESH", "CLAUDECODE"]
        .iter()
        .any(|k| env.contains_key(*k) || std::env::var_os(k).is_some())
}

/// Removes every trace of a host-managed session from a captured environment,
/// so a child agent starts as if it had been launched from a fresh terminal.
/// Returns the names it dropped, for the diagnostics panel.
fn scrub_host_session(env: &mut HashMap<String, String>) -> Vec<String> {
    let host = in_host_session(env);
    let mut dropped: Vec<String> = env
        .keys()
        .filter(|k| is_host_session_var(k) || (host && HOST_ROUTING.contains(&k.as_str())))
        .cloned()
        .collect();
    dropped.sort();
    for key in &dropped {
        env.remove(key);
    }
    dropped
}

/// Variables that decide *how* an agent authenticates. Depot never reads their
/// values; it only reports which are set, because "my terminal works and the
/// app does not" is nearly always one of these being set in one place and not
/// the other.
const AUTH_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "CODEX_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
];

/// The user's login-shell environment, captured once. A GUI app on macOS
/// inherits a bare environment, but an agent CLI may rely on shell-set vars to
/// authenticate — an API key, a base URL, a proxy, an `apiKeyHelper` path — so
/// the child gets the shell's env, not ours. `env -0` NUL-separates pairs so
/// values with newlines survive.
async fn shell_env() -> HashMap<String, String> {
    static ENV: OnceCell<HashMap<String, String>> = OnceCell::const_new();
    if let Some(env) = ENV.get() {
        return env.clone();
    }
    let mut map = HashMap::new();
    if !cfg!(windows) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let probe = Command::new(shell)
            .args(["-l", "-i", "-c", "env -0"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        if let Ok(Ok(out)) = timeout(Duration::from_secs(5), probe).await {
            if out.status.success() {
                for pair in out.stdout.split(|b| *b == 0) {
                    if let Some(eq) = pair.iter().position(|b| *b == b'=') {
                        map.insert(
                            String::from_utf8_lossy(&pair[..eq]).into_owned(),
                            String::from_utf8_lossy(&pair[eq + 1..]).into_owned(),
                        );
                    }
                }
            }
        }
    }
    // A login shell of its own is not a Claude Code session, but it inherits
    // ours when Depot was started from inside one.
    scrub_host_session(&mut map);
    // Only cache a real capture; a failed probe retries next time.
    if !map.is_empty() {
        let _ = ENV.set(map.clone());
    }
    map
}

/// PATH for the child process: the directory holding the CLI first (an
/// npm-installed CLI is a shim that needs `node` from the same place), then
/// the usual GUI-invisible bins, then the best base we have — the login
/// shell's PATH when we captured it, else whatever we inherited.
fn augmented_path(bin: &str, shell_env: &HashMap<String, String>) -> String {
    let mut dirs: Vec<String> = Vec::new();
    if let Some(parent) = Path::new(bin).parent() {
        if !parent.as_os_str().is_empty() {
            dirs.push(parent.display().to_string());
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(format!("{}/.local/bin", Path::new(&home).display()));
    }
    dirs.push("/opt/homebrew/bin".into());
    dirs.push("/usr/local/bin".into());
    let base = shell_env
        .get("PATH")
        .cloned()
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let sep = if cfg!(windows) { ";" } else { ":" };
    format!("{}{sep}{base}", dirs.join(sep))
}

/// Applies the shared environment for a child agent: the login shell's
/// variables, a PATH that can find both the CLI and its runtime, and the
/// scrubbing that keeps a nested-session environment from breaking auth.
fn apply_env(cmd: &mut Command, bin: &str, shell_env: &HashMap<String, String>) {
    for (key, value) in shell_env {
        cmd.env(key, value);
    }
    // The shell capture is already clean; Depot's *own* inherited environment
    // is not, and the child would otherwise pick it up from there.
    let host = in_host_session(shell_env);
    for (key, _) in std::env::vars_os()
        .filter_map(|(k, v)| k.into_string().ok().map(|k| (k, v)))
        .filter(|(k, _)| is_host_session_var(k) || (host && HOST_ROUTING.contains(&k.as_str())))
    {
        cmd.env_remove(key);
    }
    cmd.env("PATH", augmented_path(bin, shell_env))
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        // Nothing here can answer an interactive credential prompt.
        .env("GIT_TERMINAL_PROMPT", "0");
}

/* ── status and diagnostics ────────────────────────────────────── */

async fn version_of(bin: &str, shell_env: &HashMap<String, String>) -> Option<String> {
    let mut cmd = Command::new(bin);
    cmd.arg("--version");
    apply_env(&mut cmd, bin, shell_env);
    cmd.stdin(Stdio::null());
    let out = timeout(Duration::from_secs(10), cmd.output()).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let cleaned = line.trim().replace(" (Claude Code)", "");
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Which agent CLIs are installed, and at what version. The panel uses this to
/// populate the engine picker and to explain, per engine, what is missing.
pub async fn status() -> Vec<EngineStatus> {
    let shell_env = shell_env().await;
    let mut out = Vec::with_capacity(ENGINES.len());
    for e in ENGINES {
        let path = resolve(e.bin).await;
        let version = match &path {
            Some(p) => version_of(p, &shell_env).await,
            None => None,
        };
        out.push(EngineStatus {
            id: e.id.into(),
            label: e.label.into(),
            available: path.is_some(),
            version,
            path,
            install: e.install.into(),
            sign_in: e.sign_in.into(),
            structured: e.protocol != Protocol::PlainText,
        });
    }
    out
}

/// Everything Depot can say about why an engine will not run, without ever
/// reading a credential. This is what the panel shows under an auth failure,
/// because the CLI's own message ("OAuth session expired") does not say which
/// of the two environments — terminal or app — it was talking about.
pub async fn doctor(id: String) -> Result<EngineDoctor, String> {
    let Some(e) = engine(&id) else {
        return Err(format!("Unknown engine `{id}`"));
    };
    let shell_env = shell_env().await;
    let path = resolve(e.bin).await;
    let version = match &path {
        Some(p) => version_of(p, &shell_env).await,
        None => None,
    };

    let mut notes = Vec::new();
    match &path {
        None => notes.push(format!(
            "`{}` was not found on your PATH, in the usual install directories, or via your login shell. Install it with `{}`.",
            e.bin, e.install
        )),
        Some(p) if p == e.bin => {
            notes.push(format!("`{}` resolves on Depot's own PATH.", e.bin))
        }
        Some(p) => notes.push(format!("Depot runs `{p}`.")),
    }
    if path.is_some() && version.is_none() {
        notes.push(format!(
            "`{} --version` did not answer, so the CLI may be broken or blocked by macOS Gatekeeper.",
            e.bin
        ));
    }

    if shell_env.is_empty() {
        notes.push(
            "Depot could not read your login shell's environment, so the agent runs with the app's bare environment. If your shell sets an API key or proxy, the agent will not see it."
                .into(),
        );
    } else {
        notes.push(format!(
            "Depot passes your login shell's environment ({} variables) to the agent.",
            shell_env.len()
        ));
    }

    // The single most common reason a terminal works and this panel does not.
    let mut inherited: HashMap<String, String> = std::env::vars().collect();
    let dropped = scrub_host_session(&mut inherited);
    if !dropped.is_empty() {
        notes.push(format!(
            "Depot itself was started from inside a Claude Code session, so it inherited {} of that session's variables ({}). They are removed before the agent starts, because they tell the CLI a host will refresh its login — which is what turns a valid sign-in into \"OAuth session expired\". Restarting Depot from a plain terminal avoids this entirely.",
            dropped.len(),
            dropped.join(", ")
        ));
    }

    // Names only, never values — this is a diagnostics panel, not a dump.
    let set: Vec<&str> = AUTH_VARS
        .iter()
        .copied()
        .filter(|k| shell_env.contains_key(*k) || std::env::var_os(k).is_some())
        .collect();
    if set.is_empty() {
        notes.push(
            "No authentication variables are set, so the agent uses the credentials it saved when you signed in.".into(),
        );
    } else {
        notes.push(format!(
            "These authentication variables are set and will be passed through: {}. If one of them is stale, it overrides your saved login and the agent fails to authenticate — unset it and try again.",
            set.join(", ")
        ));
    }

    for (label, rel) in [("Claude Code", ".claude/.credentials.json"), ("Codex", ".codex/auth.json")] {
        if !e.label.starts_with(label) {
            continue;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        match home.map(|h| h.join(rel)) {
            Some(p) if p.is_file() => notes.push(format!("Saved credentials found at ~/{rel}.")),
            _ if cfg!(target_os = "macos") => notes.push(format!(
                "No ~/{rel}, so credentials live in the macOS Keychain. Run `{}` in a terminal to refresh them.",
                e.sign_in
            )),
            _ => notes.push(format!(
                "No ~/{rel}. Run `{}` in a terminal to sign in.",
                e.sign_in
            )),
        }
    }

    Ok(EngineDoctor {
        id: e.id.into(),
        label: e.label.into(),
        available: path.is_some(),
        version,
        path,
        install: e.install.into(),
        sign_in: e.sign_in.into(),
        notes,
    })
}

/* ── session continuity ────────────────────────────────────────── */

type Sessions = Arc<Mutex<HashMap<String, String>>>;

fn sessions() -> &'static Sessions {
    static SESSIONS: OnceLock<Sessions> = OnceLock::new();
    SESSIONS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn session_key(engine: &str, cwd: &str) -> String {
    format!("{engine}\u{0}{cwd}")
}

fn remember_session(engine: &str, cwd: &str, id: &str) {
    if id.is_empty() {
        return;
    }
    sessions()
        .lock()
        .unwrap()
        .insert(session_key(engine, cwd), id.to_string());
}

fn last_session(engine: &str, cwd: &str) -> Option<String> {
    sessions().lock().unwrap().get(&session_key(engine, cwd)).cloned()
}

/// Drops the stored conversation so the next turn starts cold. The panel calls
/// this from "Clear conversation".
pub fn reset(engine: String, cwd: String) {
    sessions().lock().unwrap().remove(&session_key(&engine, &cwd));
}

/* ── event helpers ─────────────────────────────────────────────── */

fn emit(app: &AppHandle, payload: Value) -> bool {
    app.emit("agent:event", payload).is_ok()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// Path relative to the workspace, so the card says `src/App.tsx`, not the
/// whole absolute path.
fn rel<'a>(cwd: &str, path: &'a str) -> &'a str {
    path.strip_prefix(cwd)
        .map(|rest| rest.trim_start_matches(['/', '\\']))
        .unwrap_or(path)
}

/// True when a CLI's failure is really "you are not signed in here". Every
/// agent phrases it differently, so match the vocabulary rather than one
/// message.
fn looks_like_auth_failure(text: &str) -> bool {
    let t = text.to_lowercase();
    let signal = [
        "failed to authenticate",
        "oauth",
        "unauthorized",
        "not logged in",
        "not authenticated",
        "please log in",
        "please sign in",
        "run `claude login`",
        "invalid api key",
        "authentication_error",
        "401",
    ];
    signal.iter().any(|s| t.contains(s))
}

/// The one failure with a one-line fix. Depot says where to apply it, because
/// the CLI cannot tell that it was Depot and not a terminal that ran it.
fn auth_event(id: &str, e: &Engine, text: &str) -> Value {
    json!({
        "id": id,
        "kind": "auth",
        "engine": e.id,
        "label": e.label,
        "signIn": e.sign_in,
        "cause": truncate(text.trim(), 400),
        "text": format!(
            "{} could not authenticate. Run `{}` in a terminal to sign in again, then resend. If your terminal already works, an environment variable set in one place and not the other is usually the difference — open diagnostics below.",
            e.label, e.sign_in
        ),
    })
}

/* ── Claude Code: stream-json ──────────────────────────────────── */

/// Turns a `tool_use` block into the panel's `tool` event: a one-line summary,
/// an expandable detail (the code being written, or the command being run),
/// and +/- line counts for edits.
fn tool_call(id: &str, cwd: &str, name: &str, input: &Value) -> Value {
    let get = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or("");

    let (summary, detail, added, removed): (String, Option<String>, usize, usize) = match name {
        "Edit" | "MultiEdit" | "NotebookEdit" => {
            let path = rel(cwd, get("file_path")).to_string();
            let (mut a, mut r) = (0usize, 0usize);
            if let Some(edits) = input.get("edits").and_then(Value::as_array) {
                for e in edits {
                    r += e.get("old_string").and_then(Value::as_str).unwrap_or("").lines().count();
                    a += e.get("new_string").and_then(Value::as_str).unwrap_or("").lines().count();
                }
            } else {
                r = get("old_string").lines().count();
                a = get("new_string").lines().count();
            }
            let new_code = if name == "MultiEdit" {
                input
                    .get("edits")
                    .and_then(Value::as_array)
                    .map(|edits| {
                        edits
                            .iter()
                            .filter_map(|e| e.get("new_string").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n…\n")
                    })
                    .unwrap_or_default()
            } else {
                get("new_string").to_string()
            };
            (format!("Edit {path}"), Some(truncate(&new_code, 1200)), a, r)
        }
        "Write" => {
            let path = rel(cwd, get("file_path")).to_string();
            let content = get("content");
            (
                format!("Write {path}"),
                Some(truncate(content, 1200)),
                content.lines().count(),
                0,
            )
        }
        "Read" => (format!("Read {}", rel(cwd, get("file_path"))), None, 0, 0),
        "Bash" => {
            let cmd = get("command");
            (truncate(cmd.lines().next().unwrap_or(""), 90), Some(truncate(cmd, 1200)), 0, 0)
        }
        "Glob" | "Grep" => (get("pattern").to_string(), None, 0, 0),
        "Task" => (get("description").to_string(), None, 0, 0),
        "WebFetch" => (get("url").to_string(), None, 0, 0),
        "WebSearch" => (get("query").to_string(), None, 0, 0),
        "TodoWrite" => ("Updating tasks".into(), None, 0, 0),
        "LS" => (format!("List {}", rel(cwd, get("path"))), None, 0, 0),
        _ => (truncate(&input.to_string(), 90), None, 0, 0),
    };

    json!({
        "id": id,
        "kind": "tool",
        "toolId": input.get("tool_use_id").and_then(Value::as_str).unwrap_or(""),
        "name": name,
        "summary": summary,
        "detail": detail,
        "added": added,
        "removed": removed,
    })
}

/// Per-run parser state. Claude needs to know whether partial-message deltas
/// arrived; Codex needs the text it has already shown, to turn repeated
/// snapshots into deltas.
#[derive(Default)]
struct RunState {
    saw_partials: bool,
    shown: HashMap<String, String>,
}

/// Parses one NDJSON line from `claude -p --output-format stream-json` and
/// emits the matching panel event. Unknown or non-JSON lines become logs.
///
/// `saw_partials` tracks whether `--include-partial-messages` deltas have
/// arrived; without them the whole `assistant` message is used instead, so
/// older CLIs still stream something sensible.
fn claude_line(app: &AppHandle, id: &str, cwd: &str, v: &Value, state: &mut RunState) {
    match v.get("type").and_then(Value::as_str).unwrap_or("") {
        "system" => {
            if v.get("subtype").and_then(Value::as_str) == Some("init") {
                let session = v.get("session_id").and_then(Value::as_str).unwrap_or("");
                remember_session("claude", cwd, session);
                emit(
                    app,
                    json!({
                        "id": id,
                        "kind": "init",
                        "model": v.get("model").and_then(Value::as_str).unwrap_or(""),
                        "sessionId": session,
                    }),
                );
            }
        }
        "stream_event" => {
            state.saw_partials = true;
            let event = &v["event"];
            if event.get("type").and_then(Value::as_str) == Some("content_block_delta") {
                let delta = &event["delta"];
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        emit(app, json!({ "id": id, "kind": "text", "text": delta["text"] }));
                    }
                    Some("thinking_delta") => {
                        emit(app, json!({ "id": id, "kind": "thinking", "text": delta["thinking"] }));
                    }
                    _ => {}
                }
            }
        }
        "assistant" => {
            if let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_use") => {
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                            let mut payload = tool_call(
                                id,
                                cwd,
                                name,
                                block.get("input").unwrap_or(&Value::Null),
                            );
                            // The tool_use block id is what tool_result refers to.
                            if let Some(tid) = block.get("id").and_then(Value::as_str) {
                                payload["toolId"] = json!(tid);
                            }
                            emit(app, payload);
                        }
                        // Fallback for CLIs without partial messages.
                        Some("text") if !state.saw_partials => {
                            let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                            if !text.is_empty() {
                                emit(app, json!({ "id": id, "kind": "text", "text": text }));
                            }
                        }
                        Some("thinking") if !state.saw_partials => {
                            let text = block.get("thinking").and_then(Value::as_str).unwrap_or("");
                            if !text.is_empty() {
                                emit(app, json!({ "id": id, "kind": "thinking", "text": text }));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "user" => {
            if let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                        emit(
                            app,
                            json!({
                                "id": id,
                                "kind": "toolDone",
                                "toolId": block.get("tool_use_id").and_then(Value::as_str).unwrap_or(""),
                                "isError": block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                            }),
                        );
                    }
                }
            }
        }
        "result" => {
            if let Some(session) = v.get("session_id").and_then(Value::as_str) {
                remember_session("claude", cwd, session);
            }
            let text = v.get("result").and_then(Value::as_str).unwrap_or("");
            let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            if is_error && looks_like_auth_failure(text) {
                emit(app, auth_event(id, &ENGINES[0], text));
            }
            emit(
                app,
                json!({
                    "id": id,
                    "kind": "result",
                    "text": text,
                    "isError": is_error,
                    "costUsd": v.get("total_cost_usd").and_then(Value::as_f64),
                    "durationMs": v.get("duration_ms").and_then(Value::as_u64),
                    "turns": v.get("num_turns").and_then(Value::as_u64),
                }),
            );
        }
        _ => {}
    }
}

/* ── Codex: `codex exec --json` ────────────────────────────────── */

/// Codex spells item types in both snake_case and camelCase depending on
/// version and transport; normalise before matching.
fn codex_kind(item: &Value) -> String {
    item.get("type")
        .or_else(|| item.get("item_type"))
        .or_else(|| item.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace('_', "")
}

fn codex_text(item: &Value) -> String {
    for key in ["text", "message", "content", "summary"] {
        if let Some(s) = item.get(key).and_then(Value::as_str) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

/// A one-line summary and an expandable detail for a Codex tool item, in the
/// same shape the Claude tool cards use, so the panel renders both identically.
fn codex_tool(id: &str, cwd: &str, item_id: &str, kind: &str, item: &Value) -> Value {
    let get = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("");
    let (name, summary, detail, added, removed) = match kind {
        "commandexecution" => {
            let cmd = get("command");
            (
                "Bash",
                truncate(cmd.lines().next().unwrap_or(""), 90),
                Some(truncate(cmd, 1200)),
                0,
                0,
            )
        }
        "filechange" | "applypatch" => {
            let changes = item.get("changes").and_then(Value::as_array);
            let paths: Vec<String> = changes
                .map(|c| {
                    c.iter()
                        .filter_map(|ch| {
                            ch.get("path")
                                .and_then(Value::as_str)
                                .map(|p| rel(cwd, p).to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();
            let summary = match paths.len() {
                0 => "Edit files".to_string(),
                1 => format!("Edit {}", paths[0]),
                n => format!("Edit {n} files"),
            };
            ("Edit", summary, Some(paths.join("\n")), 0, 0)
        }
        "mcptoolcall" => (
            "Task",
            format!("{}·{}", get("server"), get("tool")),
            None,
            0,
            0,
        ),
        "websearch" => ("WebSearch", get("query").to_string(), None, 0, 0),
        "todolist" => ("TodoWrite", "Updating tasks".to_string(), None, 0, 0),
        _ => (
            "code",
            truncate(&item.to_string(), 90),
            None,
            0,
            0,
        ),
    };

    json!({
        "id": id,
        "kind": "tool",
        "toolId": item_id,
        "name": name,
        "summary": summary,
        "detail": detail,
        "added": added,
        "removed": removed,
    })
}

/// Emits only what is new in a repeated snapshot. Codex sends `item.updated`
/// with the whole message so far; the panel appends, so send the suffix. If a
/// snapshot is not an extension of what we showed (a rewrite), start over.
fn suffix<'a>(state: &mut RunState, key: &str, whole: &'a str) -> &'a str {
    let seen = state.shown.entry(key.to_string()).or_default();
    let new = if whole.starts_with(seen.as_str()) {
        &whole[seen.len()..]
    } else {
        whole
    };
    *seen = whole.to_string();
    new
}

/// Parses one NDJSON line from `codex exec --json` into the same panel events
/// Claude produces, so a turn looks the same whichever engine ran it.
fn codex_line(app: &AppHandle, id: &str, cwd: &str, v: &Value, state: &mut RunState) {
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "thread.started" | "session.created" => {
            let session = v
                .get("thread_id")
                .or_else(|| v.get("session_id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            remember_session("codex", cwd, session);
            emit(
                app,
                json!({
                    "id": id,
                    "kind": "init",
                    "model": v.get("model").and_then(Value::as_str).unwrap_or(""),
                    "sessionId": session,
                }),
            );
        }
        "item.started" | "item.updated" | "item.completed" => {
            let Some(item) = v.get("item") else { return };
            let kind = codex_kind(item);
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
            match kind.as_str() {
                "assistantmessage" | "agentmessage" | "message" => {
                    let whole = codex_text(item);
                    let new = suffix(state, item_id, &whole);
                    if !new.is_empty() {
                        emit(app, json!({ "id": id, "kind": "text", "text": new }));
                    }
                }
                "reasoning" => {
                    let whole = codex_text(item);
                    let new = suffix(state, item_id, &whole);
                    if !new.is_empty() {
                        emit(app, json!({ "id": id, "kind": "thinking", "text": new }));
                    }
                }
                "error" => {
                    let text = codex_text(item);
                    if looks_like_auth_failure(&text) {
                        emit(app, auth_event(id, engine("codex").unwrap(), &text));
                    }
                    emit(app, json!({ "id": id, "kind": "log", "text": text }));
                }
                _ => {
                    // Everything else is a tool: one card when it starts, a
                    // tick when it finishes.
                    if ty == "item.started" {
                        emit(app, codex_tool(id, cwd, item_id, &kind, item));
                    } else if ty == "item.completed" {
                        let failed = item
                            .get("exit_code")
                            .and_then(Value::as_i64)
                            .is_some_and(|c| c != 0)
                            || item.get("status").and_then(Value::as_str) == Some("failed");
                        emit(
                            app,
                            json!({
                                "id": id,
                                "kind": "toolDone",
                                "toolId": item_id,
                                "isError": failed,
                            }),
                        );
                    }
                }
            }
        }
        "turn.completed" => {
            let usage = v.get("usage");
            emit(
                app,
                json!({
                    "id": id,
                    "kind": "result",
                    "text": "",
                    "isError": false,
                    "costUsd": Value::Null,
                    "durationMs": v.get("duration_ms").and_then(Value::as_u64),
                    "turns": usage
                        .and_then(|u| u.get("output_tokens"))
                        .and_then(Value::as_u64)
                        .map(|_| 1u64),
                }),
            );
        }
        "turn.failed" | "error" => {
            let text = v
                .pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| v.get("message").and_then(Value::as_str))
                .unwrap_or("Codex failed")
                .to_string();
            if looks_like_auth_failure(&text) {
                emit(app, auth_event(id, engine("codex").unwrap(), &text));
            }
            emit(
                app,
                json!({ "id": id, "kind": "result", "text": text, "isError": true }),
            );
        }
        _ => {}
    }
}

/* ── running ───────────────────────────────────────────────────── */

/// One stdout line, routed to the parser for whichever engine produced it.
/// Non-JSON output from a structured engine becomes a log line rather than
/// being dropped — that is where a crash message usually is.
fn handle_line(
    app: &AppHandle,
    id: &str,
    cwd: &str,
    protocol: Protocol,
    line: &str,
    state: &mut RunState,
) {
    if protocol == Protocol::PlainText {
        emit(app, json!({ "id": id, "kind": "text", "text": format!("{line}\n") }));
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        emit(app, json!({ "id": id, "kind": "log", "text": line }));
        return;
    };
    match protocol {
        Protocol::ClaudeStreamJson => claude_line(app, id, cwd, &v, state),
        Protocol::CodexJsonl => codex_line(app, id, cwd, &v, state),
        Protocol::PlainText => unreachable!("handled above"),
    }
}

/// Adds the flags that differ per engine: model, effort, what may run without
/// asking, and whether this turn continues the last conversation.
fn build_args(cmd: &mut Command, e: &Engine, cwd: &str, options: &AgentOptions) {
    let model = options.model.clone().filter(|m| !m.is_empty() && m != "default");
    let effort = options.effort.clone().filter(|m| !m.is_empty() && m != "default");
    let mode = options
        .permission_mode
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "acceptEdits".into());
    let resume = options.resume.unwrap_or(true).then(|| last_session(e.id, cwd)).flatten();

    match e.protocol {
        Protocol::ClaudeStreamJson => {
            cmd.args([
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ]);
            if let Some(model) = model {
                cmd.arg("--model").arg(model);
            }
            if let Some(effort) = effort {
                cmd.arg("--effort").arg(effort);
            }
            cmd.arg("--permission-mode").arg(mode);
            if let Some(session) = resume {
                cmd.arg("--resume").arg(session);
            }
        }
        Protocol::CodexJsonl => {
            cmd.arg("exec");
            if let Some(session) = resume {
                cmd.arg("resume").arg(session);
            }
            cmd.args(["--json", "--skip-git-repo-check", "--cd", cwd]);
            if let Some(model) = model {
                cmd.arg("--model").arg(model);
            }
            // Codex has no permission prompt in exec mode, so the panel's
            // choice maps onto how much the sandbox allows.
            match mode.as_str() {
                "plan" => {
                    cmd.args(["--sandbox", "read-only"]);
                }
                "bypassPermissions" => {
                    cmd.arg("--dangerously-bypass-approvals-and-sandbox");
                }
                _ => {
                    cmd.args(["--sandbox", "workspace-write"]);
                }
            }
            // Prompt arrives on stdin.
            cmd.arg("-");
        }
        Protocol::PlainText => {
            // The common shape across the remaining CLIs: a non-interactive
            // flag, then the prompt on stdin.
            match e.id {
                "gemini" => {
                    cmd.arg("-p");
                    if let Some(model) = model {
                        cmd.arg("--model").arg(model);
                    }
                }
                "copilot" => {
                    cmd.args(["-p", "--allow-all-tools"]);
                    if let Some(model) = model {
                        cmd.arg("--model").arg(model);
                    }
                }
                "opencode" => {
                    cmd.arg("run");
                    if let Some(model) = model {
                        cmd.arg("--model").arg(model);
                    }
                }
                _ => {}
            }
        }
    }
}

type Registry = Arc<Mutex<HashMap<String, Child>>>;

fn running() -> &'static Registry {
    static RUNNING: OnceLock<Registry> = OnceLock::new();
    RUNNING.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Spawns the selected agent CLI in its non-interactive mode and streams
/// structured events to the panel.
///
/// The prompt goes to stdin rather than argv, so long prompts never hit
/// argument-length limits and no shell wrapper ever parses their text.
pub async fn run(
    app: &AppHandle,
    id: String,
    cwd: String,
    prompt: String,
    options: AgentOptions,
) -> Result<(), String> {
    if running().lock().unwrap().contains_key(&id) {
        return Err("That run is already going".into());
    }
    let dir = Path::new(&cwd);
    if !dir.is_dir() {
        return Err(format!("{cwd} is not a folder"));
    }
    let e = options.engine();
    let Some(bin) = resolve(e.bin).await else {
        return Err(format!(
            "{} CLI not found. Install it ({}), then try again.",
            e.label, e.install
        ));
    };

    // A GUI app inherits a bare environment; the CLI may authenticate via
    // shell-set vars (API key, base URL, proxy, apiKeyHelper), so overlay the
    // login shell's env first, then pin the values we need to control.
    let shell_env = shell_env().await;

    let mut cmd = Command::new(&bin);
    cmd.current_dir(dir);
    build_args(&mut cmd, e, &cwd, &options);
    apply_env(&mut cmd, &bin, &shell_env);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|err| {
        format!(
            "Could not start `{}`: {err}. Is {} installed and on your PATH?",
            e.bin, e.label
        )
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    running().lock().unwrap().insert(id.clone(), child);

    if let Some(pipe) = stdout {
        let handle = app.clone();
        let run_id = id.clone();
        let root = cwd.clone();
        let protocol = e.protocol;
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            let mut state = RunState::default();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                handle_line(&handle, &run_id, &root, protocol, &line, &mut state);
            }
        });
    }
    if let Some(pipe) = stderr {
        let handle = app.clone();
        let run_id = id.clone();
        let engine_id = e.id;
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            let mut told = false;
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                // Some CLIs report a bad login on stderr and then exit without
                // ever emitting a result, so the panel would otherwise show a
                // bare exit code.
                if !told && looks_like_auth_failure(&line) {
                    told = true;
                    if let Some(e) = engine(engine_id) {
                        emit(&handle, auth_event(&run_id, e, &line));
                    }
                }
                if !emit(&handle, json!({ "id": run_id, "kind": "log", "text": line })) {
                    break;
                }
            }
        });
    }

    // Reap separately so this command can return as soon as the process is up.
    let handle = app.clone();
    let wait_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let child = running().lock().unwrap().remove(&wait_id);
        let (code, error) = match child {
            Some(mut c) => match c.wait().await {
                Ok(status) => (status.code(), None),
                Err(e) => (None, Some(e.to_string())),
            },
            // Cancelled before we got here.
            None => (None, None),
        };
        let _ = handle.emit("agent:done", AgentDone { id: wait_id, code, error });
    });

    Ok(())
}

pub async fn cancel(id: String) -> Result<(), String> {
    let child = running().lock().unwrap().remove(&id);
    if let Some(mut child) = child {
        let _ = child.kill().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_command_that_exists_and_one_that_does_not() {
        // `cargo` is running this test, so it is certainly on PATH.
        assert!(on_path("cargo"));
        assert!(!on_path("definitely-not-a-real-binary-xyzzy"));
    }

    #[test]
    fn absolute_paths_are_checked_directly() {
        assert!(!on_path("/definitely/not/here"));
    }

    #[test]
    fn edit_summary_counts_lines_and_shortens_paths() {
        let input = json!({
            "file_path": "/repo/src/App.tsx",
            "old_string": "a\nb",
            "new_string": "a\nb\nc",
        });
        let event = tool_call("run-1", "/repo", "Edit", &input);
        assert_eq!(event["summary"], json!("Edit src/App.tsx"));
        assert_eq!(event["added"], json!(3));
        assert_eq!(event["removed"], json!(2));
    }

    #[test]
    fn bash_summary_is_the_first_line() {
        let input = json!({ "command": "npm run build\nnpm test" });
        let event = tool_call("run-1", "/repo", "Bash", &input);
        assert_eq!(event["summary"], json!("npm run build"));
        assert_eq!(event["detail"], json!("npm run build\nnpm test"));
    }

    #[test]
    fn every_engine_id_resolves() {
        for e in ENGINES {
            assert_eq!(engine(e.id).map(|found| found.id), Some(e.id));
        }
        assert!(engine("nope").is_none());
    }

    #[test]
    fn unknown_engine_falls_back_to_claude() {
        let options = AgentOptions { engine: Some("nope".into()), ..Default::default() };
        assert_eq!(options.engine().id, "claude");
        assert_eq!(AgentOptions::default().engine().id, "claude");
    }

    #[test]
    fn codex_item_types_normalise_across_spellings() {
        assert_eq!(codex_kind(&json!({ "type": "command_execution" })), "commandexecution");
        assert_eq!(codex_kind(&json!({ "type": "commandExecution" })), "commandexecution");
        assert_eq!(codex_kind(&json!({ "item_type": "assistant_message" })), "assistantmessage");
    }

    #[test]
    fn repeated_snapshots_become_deltas() {
        let mut state = RunState::default();
        assert_eq!(suffix(&mut state, "item-1", "Hello"), "Hello");
        assert_eq!(suffix(&mut state, "item-1", "Hello there"), " there");
        // A rewrite is not an extension, so the whole thing is sent again.
        assert_eq!(suffix(&mut state, "item-1", "Different"), "Different");
    }

    #[test]
    fn auth_failures_are_recognised_across_wordings() {
        assert!(looks_like_auth_failure(
            "Failed to authenticate: OAuth session expired and could not be refreshed"
        ));
        assert!(looks_like_auth_failure("Error: 401 Unauthorized"));
        assert!(looks_like_auth_failure("You are not logged in."));
        assert!(!looks_like_auth_failure("Wrote 12 lines to src/main.rs"));
    }

    #[test]
    fn a_host_managed_session_is_recognised_but_user_settings_survive() {
        // What a Claude Code host hands its child.
        for key in [
            "CLAUDECODE",
            "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_SDK_HAS_OAUTH_REFRESH",
            "CLAUDE_CODE_HOST_SESSION_ID",
            "CLAUDE_AGENT_SDK_VERSION",
            "CLAUDE_PLUGIN_DATA",
            "CODEX_COMPANION_SESSION_ID",
            "CLAUDE_PID",
        ] {
            assert!(is_host_session_var(key), "{key} should be scrubbed");
        }
        // What a user sets on purpose.
        for key in ["CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CONFIG_DIR", "ANTHROPIC_API_KEY", "PATH"] {
            assert!(!is_host_session_var(key), "{key} should survive");
        }
    }

    #[test]
    fn host_routing_is_dropped_only_inside_a_host_session() {
        let mut inside: HashMap<String, String> = [
            ("CLAUDE_CODE_MESSAGING_SOCKET", "/tmp/sock"),
            ("ANTHROPIC_BASE_URL", "https://host.internal"),
            ("ANTHROPIC_API_KEY", "sk-user-owned"),
            ("PATH", "/usr/bin"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let dropped = scrub_host_session(&mut inside);
        assert!(dropped.contains(&"ANTHROPIC_BASE_URL".to_string()));
        assert!(dropped.contains(&"CLAUDE_CODE_MESSAGING_SOCKET".to_string()));
        // A user's own key and PATH are never touched.
        assert!(inside.contains_key("ANTHROPIC_API_KEY"));
        assert!(inside.contains_key("PATH"));
    }

    #[test]
    fn sessions_are_scoped_to_engine_and_folder() {
        remember_session("claude", "/a", "s1");
        remember_session("codex", "/a", "s2");
        assert_eq!(last_session("claude", "/a").as_deref(), Some("s1"));
        assert_eq!(last_session("codex", "/a").as_deref(), Some("s2"));
        assert_eq!(last_session("claude", "/b"), None);
        reset("claude".into(), "/a".into());
        assert_eq!(last_session("claude", "/a"), None);
        reset("codex".into(), "/a".into());
    }
}
