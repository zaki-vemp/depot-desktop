//! Coding-agent CLIs driven from the chat panel.
//!
//! Depot does not talk to any model itself. It shells out to whichever agent
//! CLI the user already has installed and signed in — Claude Code, Codex,
//! Copilot CLI, opencode — in that tool's own non-interactive mode, and streams
//! the output into the panel. Nothing here holds an API key.
//!
//! Presets are defaults, not a fixed list: the command and every argument are
//! editable in the UI, so a CLI that shipped after this build (or one with
//! different flags) still works without a code change.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// Where the prompt is substituted in a preset's argument list.
const PROMPT_TOKEN: &str = "{{prompt}}";

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreset {
    pub id: String,
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    /// True when `command` resolves on PATH.
    pub available: bool,
    /// Shown under the picker, so the permission posture is never a surprise.
    pub note: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentChunk {
    pub id: String,
    /// `stdout` or `stderr`.
    pub stream: String,
    pub line: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentDone {
    pub id: String,
    pub code: Option<i32>,
    pub error: Option<String>,
}

/// Built-in presets. Flags are the real non-interactive interfaces of each CLI
/// as of this build; the UI lets the user edit them.
///
/// Each one is deliberately given permission to edit files in the workspace,
/// because a chat agent that cannot change anything is useless — and every
/// change it makes is captured by a checkpoint first, so it can be reverted.
/// The note on each preset says so plainly in the picker.
fn presets() -> Vec<AgentPreset> {
    vec![
        AgentPreset {
            id: "claude".into(),
            label: "Claude Code".into(),
            command: "claude".into(),
            args: vec![
                "-p".into(),
                PROMPT_TOKEN.into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
            ],
            available: false,
            note: "Edits are accepted automatically; other tools still prompt.".into(),
        },
        AgentPreset {
            id: "codex".into(),
            label: "Codex".into(),
            command: "codex".into(),
            args: vec![
                "exec".into(),
                "--sandbox".into(),
                "workspace-write".into(),
                PROMPT_TOKEN.into(),
            ],
            available: false,
            note: "Sandboxed to writes inside this workspace.".into(),
        },
        AgentPreset {
            id: "copilot".into(),
            label: "Copilot CLI".into(),
            command: "copilot".into(),
            args: vec![
                "-p".into(),
                PROMPT_TOKEN.into(),
                "--allow-all-tools".into(),
                "--no-color".into(),
            ],
            available: false,
            note: "All tools allowed — required for non-interactive runs.".into(),
        },
        AgentPreset {
            id: "opencode".into(),
            label: "opencode".into(),
            command: "opencode".into(),
            args: vec!["run".into(), PROMPT_TOKEN.into()],
            available: false,
            note: "Uses the model configured in opencode.".into(),
        },
        AgentPreset {
            id: "cursor".into(),
            label: "Cursor Agent".into(),
            command: "cursor-agent".into(),
            args: vec!["-p".into(), PROMPT_TOKEN.into(), "--force".into()],
            available: false,
            note: "Cursor's headless agent.".into(),
        },
        AgentPreset {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            command: "gemini".into(),
            args: vec!["--yolo".into(), "-p".into(), PROMPT_TOKEN.into()],
            available: false,
            note: "Runs with tool confirmations skipped.".into(),
        },
        AgentPreset {
            id: "kimi".into(),
            label: "Kimi CLI".into(),
            command: "kimi".into(),
            args: vec!["--print".into(), PROMPT_TOKEN.into()],
            available: false,
            note: "Flags are a best guess — edit them if the run fails.".into(),
        },
        AgentPreset {
            id: "aider".into(),
            label: "Aider".into(),
            command: "aider".into(),
            args: vec![
                "--yes-always".into(),
                "--no-auto-commits".into(),
                "--message".into(),
                PROMPT_TOKEN.into(),
            ],
            available: false,
            note: "Commits are left to you, not made automatically.".into(),
        },
    ]
}

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

pub fn list() -> Vec<AgentPreset> {
    presets()
        .into_iter()
        .map(|mut p| {
            p.available = on_path(&p.command);
            p
        })
        .collect()
}

/// Forwards one of the child's pipes to the panel, a line at a time.
/// Generic because `ChildStdout` and `ChildStderr` are different types.
fn pump<R>(app: AppHandle, id: String, stream: &'static str, pipe: R)
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
{
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let sent = app.emit(
                "agent:out",
                AgentChunk {
                    id: id.clone(),
                    stream: stream.to_string(),
                    line,
                },
            );
            if sent.is_err() {
                break;
            }
        }
    });
}

type Registry = Arc<Mutex<HashMap<String, Child>>>;

fn running() -> &'static Registry {
    static RUNNING: OnceLock<Registry> = OnceLock::new();
    RUNNING.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Spawns an agent and streams its output line by line.
///
/// If the argument list contains `{{prompt}}` it is substituted; otherwise the
/// prompt is piped to stdin, which is how some CLIs prefer to take it.
pub async fn run(
    app: &AppHandle,
    id: String,
    command: String,
    args: Vec<String>,
    cwd: String,
    prompt: String,
) -> Result<(), String> {
    if running().lock().unwrap().contains_key(&id) {
        return Err("That run is already going".into());
    }
    let dir = Path::new(&cwd);
    if !dir.is_dir() {
        return Err(format!("{cwd} is not a folder"));
    }

    let uses_token = args.iter().any(|a| a.contains(PROMPT_TOKEN));
    let final_args: Vec<String> = args
        .iter()
        .map(|a| a.replace(PROMPT_TOKEN, &prompt))
        .collect();

    let mut cmd = Command::new(&command);
    cmd.args(&final_args)
        .current_dir(dir)
        // Agents are chatty in colour; the panel renders plain text.
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("TERM", "dumb")
        // Nothing can answer an interactive credential prompt from here.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if uses_token {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .kill_on_drop(true);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| {
        format!("Could not start `{command}`: {e}. Is it installed and on your PATH?")
    })?;

    if !uses_token {
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(prompt.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    running().lock().unwrap().insert(id.clone(), child);

    // One task per stream so stderr is never held back behind stdout.
    if let Some(pipe) = stdout {
        pump(app.clone(), id.clone(), "stdout", pipe);
    }
    if let Some(pipe) = stderr {
        pump(app.clone(), id.clone(), "stderr", pipe);
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
    fn every_preset_has_a_prompt_slot_or_takes_stdin() {
        for p in presets() {
            // Without the token the prompt goes to stdin, which is fine, but a
            // preset that mentions it must spell it exactly.
            for arg in &p.args {
                if arg.contains("prompt") && arg.contains("{{") {
                    assert_eq!(arg, PROMPT_TOKEN, "malformed token in preset {}", p.id);
                }
            }
        }
    }

    #[test]
    fn preset_ids_are_unique() {
        let all = presets();
        let mut ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate preset id");
    }

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
}
