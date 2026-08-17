/**
 * The coding-agent chat panel.
 *
 * Depot is not an AI client: it runs whichever agent CLI the user already has
 * installed and signed in — Claude Code, Codex, Gemini, Copilot, opencode — in
 * that tool's non-interactive mode, and renders its structured event stream:
 * thinking, replies, and every tool call as it happens. No API key ever passes
 * through Depot.
 *
 * Turns continue the same conversation until "New conversation" is pressed, so
 * follow-ups do not start the agent cold.
 *
 * A checkpoint of the workspace is taken before every run, so each file the
 * agent touched can be kept or put back individually — the whole reason it is
 * safe to let a CLI edit files unattended.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, onAgentDone, onAgentEvent } from "../api";
import { Icon } from "../lib/icons";
import type { IconName } from "../lib/icons";
import type {
  AgentChange,
  AgentEvent,
  ChatMessage,
  ChatPart,
  EngineDoctor,
  EngineStatus,
} from "../types";

const uid = () => `run-${Math.random().toString(36).slice(2, 10)}-${Date.now()}`;

const KIND_MARK: Record<AgentChange["kind"], { letter: string; tone: string }> = {
  modified: { letter: "M", tone: "mod" },
  added: { letter: "A", tone: "add" },
  deleted: { letter: "D", tone: "del" },
};

const TOOL_ICON: Record<string, IconName> = {
  Edit: "edit",
  MultiEdit: "edit",
  NotebookEdit: "edit",
  Write: "edit",
  Read: "doc",
  Bash: "terminal",
  Grep: "search",
  Glob: "search",
  LS: "folder",
  Task: "sparkle",
  TodoWrite: "list",
  WebFetch: "globe",
  WebSearch: "globe",
};

/**
 * Suggestions, not a closed list — the field is free text, so a model released
 * after this build still works. Engines with no well-known aliases get none.
 */
const MODEL_HINTS: Record<string, string[]> = {
  claude: ["sonnet", "opus", "haiku", "fable"],
};

const EFFORTS: Array<{ value: string; label: string }> = [
  { value: "default", label: "Effort: default" },
  { value: "low", label: "Low effort" },
  { value: "medium", label: "Medium effort" },
  { value: "high", label: "High effort" },
  { value: "xhigh", label: "Extra high effort" },
  { value: "max", label: "Max effort" },
];

type Mode = { value: string; label: string; note: string };

const CLAUDE_MODES: Mode[] = [
  {
    value: "acceptEdits",
    label: "Accept edits",
    note: "File edits are applied automatically; a checkpoint is taken first, so anything can be reverted.",
  },
  {
    value: "auto",
    label: "Auto",
    note: "The agent decides which tools need approval. Anything it cannot approve stays blocked.",
  },
  {
    value: "plan",
    label: "Plan only",
    note: "The agent proposes a plan without changing any files.",
  },
  {
    value: "bypassPermissions",
    label: "Bypass permissions",
    note: "Every tool runs without asking, including shell commands. Checkpoints still cover file edits.",
  },
];

/** Codex has no approval prompt in `exec` mode, so the choice sets the sandbox. */
const CODEX_MODES: Mode[] = [
  {
    value: "acceptEdits",
    label: "Accept edits",
    note: "Codex may write inside this folder. A checkpoint is taken first, so anything can be reverted.",
  },
  {
    value: "plan",
    label: "Plan only",
    note: "Read-only sandbox: Codex can look around and plan, but cannot change files.",
  },
  {
    value: "bypassPermissions",
    label: "Bypass sandbox",
    note: "No sandbox and no approvals, including network and writes outside this folder.",
  },
];

function modesFor(engine: string): Mode[] {
  if (engine === "claude") return CLAUDE_MODES;
  if (engine === "codex") return CODEX_MODES;
  return [];
}

/** "claude-sonnet-4-5" → "sonnet-4-5". */
const shortModel = (model: string) => model.replace(/^claude-/, "");

/** Folds one streamed event into the turn it belongs to. */
function applyEvent(m: ChatMessage, e: AgentEvent): ChatMessage {
  if (m.id !== e.id) return m;
  const parts = [...(m.parts ?? [])];
  switch (e.kind) {
    case "init":
      return { ...m, model: e.model };
    case "text": {
      const last = parts[parts.length - 1];
      if (last?.kind === "text") parts[parts.length - 1] = { ...last, text: last.text + e.text };
      else parts.push({ kind: "text", text: e.text });
      return { ...m, parts };
    }
    case "thinking": {
      const last = parts[parts.length - 1];
      if (last?.kind === "thinking") parts[parts.length - 1] = { ...last, text: last.text + e.text };
      else parts.push({ kind: "thinking", text: e.text });
      return { ...m, parts };
    }
    case "tool":
      parts.push({
        kind: "tool",
        toolId: e.toolId,
        name: e.name,
        summary: e.summary,
        detail: e.detail,
        added: e.added,
        removed: e.removed,
        done: false,
        isError: false,
      });
      return { ...m, parts };
    case "toolDone": {
      const i = parts.findIndex((p) => p.kind === "tool" && p.toolId === e.toolId);
      const p = i >= 0 ? parts[i] : undefined;
      if (p?.kind === "tool") parts[i] = { ...p, done: true, isError: e.isError };
      return { ...m, parts };
    }
    case "auth":
      // The CLI is installed but not signed in here; the banner carries the fix.
      return { ...m, auth: e, failed: true };
    case "log":
      return { ...m, logs: m.logs ? `${m.logs}\n${e.text}` : e.text };
    case "result": {
      const bits: string[] = [];
      if (e.durationMs != null) bits.push(`${(e.durationMs / 1000).toFixed(1)}s`);
      if (e.turns != null) bits.push(`${e.turns} turn${e.turns === 1 ? "" : "s"}`);
      if (e.costUsd != null) bits.push(`$${e.costUsd.toFixed(4)}`);
      // If nothing streamed (older CLI), the final result is the reply.
      const hasText = parts.some((p) => p.kind === "text" && p.text.trim());
      // An auth banner already says it better than the raw message would.
      if (e.text.trim() && !hasText && !m.auth) parts.push({ kind: "text", text: e.text });
      return {
        ...m,
        parts,
        meta: bits.join(" · ") || undefined,
        failed: e.isError || m.failed,
      };
    }

    default:
      return m;
  }
}

/** Collapsible chain-of-thought: open while it streams, folded afterwards. */
function ThinkingBlock({ text, live }: { text: string; live: boolean }) {
  const [open, setOpen] = useState(false);
  const expanded = open || live;
  return (
    <div className="agent-thinking">
      <button className="agent-thinking-head" onClick={() => setOpen((v) => !v)}>
        <Icon name={expanded ? "chevronDown" : "chevronRight"} size={11} />
        {live ? "Thinking…" : "Thought"}
      </button>
      {expanded && <div className="agent-thinking-body">{text}</div>}
    </div>
  );
}

/** One tool call: icon, what it did, +/- for edits, spinner until it finishes. */
function ToolCard({ part }: { part: Extract<ChatPart, { kind: "tool" }> }) {
  const [open, setOpen] = useState(false);
  const icon = TOOL_ICON[part.name] ?? "code";
  return (
    <div className={`agent-tool${part.isError ? " failed" : ""}`}>
      <button
        className="agent-tool-head"
        onClick={() => part.detail && setOpen((v) => !v)}
        title={part.detail ? (open ? "Hide details" : "Show what was changed") : part.summary}
      >
        <Icon name={icon} size={12} />
        <span className="agent-tool-summary">{part.summary}</span>
        {(part.added > 0 || part.removed > 0) && (
          <span className="agent-tool-diff">
            {part.added > 0 && <span className="add">+{part.added}</span>}
            {part.removed > 0 && <span className="del">−{part.removed}</span>}
          </span>
        )}
        {part.done ? (
          part.isError ? (
            <Icon name="warn" size={12} className="agent-tool-err" />
          ) : (
            <Icon name="check" size={12} className="agent-tool-ok" />
          )
        ) : (
          <span className="agent-tool-spin" aria-label="running" />
        )}
      </button>
      {open && part.detail && <pre className="agent-tool-detail">{part.detail}</pre>}
    </div>
  );
}

/**
 * Shown when a CLI that is installed refuses for want of a sign-in. The CLI
 * cannot tell that Depot, and not a terminal, ran it — so it reports an
 * expired session without saying which environment expired. Diagnostics close
 * that gap: they name the binary Depot runs and which authentication
 * variables it passes through, never their values.
 */
function AuthNotice({ auth }: { auth: Extract<AgentEvent, { kind: "auth" }> }) {
  const [doctor, setDoctor] = useState<EngineDoctor | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const diagnose = useCallback(async () => {
    setBusy(true);
    try {
      setDoctor(await api.agentDoctor(auth.engine));
      setError("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [auth.engine]);

  return (
    <div className="agent-auth">
      <div className="agent-auth-head">
        <Icon name="warn" size={13} />
        <span>{auth.label} is not signed in here</span>
      </div>
      <p className="agent-auth-body">{auth.text}</p>
      {auth.cause && <pre className="agent-auth-cause">{auth.cause}</pre>}
      <div className="agent-auth-row">
        <code className="agent-auth-cmd">{auth.signIn}</code>
        <div className="spacer" />
        {!doctor && (
          <button className="agent-chip" disabled={busy} onClick={() => void diagnose()}>
            {busy ? "Checking…" : "Diagnose"}
          </button>
        )}
      </div>
      {error && <pre className="agent-auth-cause">{error}</pre>}
      {doctor && (
        <ul className="agent-auth-notes">
          {doctor.version && (
            <li>
              {doctor.label} {doctor.version}
            </li>
          )}
          {doctor.notes.map((note, i) => (
            <li key={i}>{note}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ChangeRow({
  change,
  busy,
  onOpen,
  onKeep,
  onRevert,
}: {
  change: AgentChange;
  busy: boolean;
  onOpen: () => void;
  onKeep: () => void;
  onRevert: () => void;
}) {
  const mark = KIND_MARK[change.kind];
  const dir = change.path.includes("/") ? change.path.slice(0, change.path.lastIndexOf("/")) : "";

  return (
    <div className="agent-change" title={change.path}>
      <button className="agent-change-open" onClick={onOpen}>
        <span className={`scm-mark ${mark.tone}`}>{mark.letter}</span>
        <span className="agent-change-name">{change.name}</span>
        {dir && <span className="agent-change-dir">{dir}</span>}
      </button>
      <button className="agent-chip keep" disabled={busy} title="Keep this change" onClick={onKeep}>
        <Icon name="check" size={12} />
        Keep
      </button>
      <button
        className="agent-chip revert"
        disabled={busy || !change.revertible}
        title={
          change.revertible
            ? "Put this file back to how it was before the run"
            : "Depot has no baseline for this file, so it cannot be undone automatically"
        }
        onClick={onRevert}
      >
        <Icon name="loop" size={12} />
        Revert
      </button>
    </div>
  );
}

export function AgentChat({
  root,
  visible,
  onError,
  onOpenAgentDiff,
  onFilesChanged,
  onClose,
}: {
  root: string;
  visible: boolean;
  onError: (message: string) => void;
  /** Opens a diff of one agent-changed file against the pre-run checkpoint. */
  onOpenAgentDiff: (checkpoint: string, change: AgentChange) => void;
  /** Buffers on screen are stale once the agent (or a revert) rewrites a file. */
  onFilesChanged: (paths: string[]) => void;
  onClose: () => void;
}) {
  const [engines, setEngines] = useState<EngineStatus[] | null>(null);
  const [engineId, setEngineId] = useState("claude");
  const [model, setModel] = useState("");
  const [effort, setEffort] = useState("default");
  const [mode, setMode] = useState("acceptEdits");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [prompt, setPrompt] = useState("");
  const [runId, setRunId] = useState("");
  const [busy, setBusy] = useState(false);

  const transcript = useRef<HTMLDivElement>(null);
  const report = useRef(onError);
  report.current = onError;
  const notify = useRef(onFilesChanged);
  notify.current = onFilesChanged;

  useEffect(() => {
    void api
      .agentEngines()
      .then((found) => {
        setEngines(found);
        // Land on something installed rather than on a picker that cannot run.
        setEngineId((current) =>
          found.some((e) => e.id === current && e.available)
            ? current
            : (found.find((e) => e.available)?.id ?? current),
        );
      })
      .catch((e) => report.current(String(e)));
  }, []);

  const engine = useMemo(
    () => engines?.find((e) => e.id === engineId) ?? null,
    [engines, engineId],
  );
  const modes = modesFor(engineId);

  // Each engine has its own vocabulary for what may run unattended, so keep
  // the selection valid when switching between them.
  useEffect(() => {
    if (modes.length && !modes.some((m) => m.value === mode)) setMode(modes[0].value);
  }, [engineId, mode, modes]);

  // A hidden panel has no scroll height to set, so pin to the bottom again
  // when it comes back rather than leaving it stranded mid-transcript.
  useEffect(() => {
    if (!visible || !transcript.current) return;
    transcript.current.scrollTop = transcript.current.scrollHeight;
  }, [messages, visible]);

  /* ── streaming ─────────────────────────────────────────────── */

  useEffect(() => {
    let stop: Array<() => void> = [];
    let dead = false;

    void (async () => {
      const off = [
        await onAgentEvent((e) => {
          setMessages((all) => all.map((m) => applyEvent(m, e)));
        }),
        await onAgentDone((e) => {
          setRunId((current) => (current === e.id ? "" : current));
          void finish(e.id, e.code, e.error ?? null);
        }),
      ];
      if (dead) off.forEach((fn) => fn());
      else stop = off;
    })();

    return () => {
      dead = true;
      stop.forEach((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Resolves what the run changed and attaches it to that turn. */
  const finish = useCallback(
    async (id: string, code: number | null, error: string | null) => {
      let checkpoint = "";
      setMessages((all) => {
        const turn = all.find((m) => m.id === id);
        checkpoint = turn?.checkpoint ?? "";
        return all.map((m) => (m.id === id ? { ...m, streaming: false } : m));
      });

      const failed = Boolean(error) || (code !== null && code !== 0);
      let changes: AgentChange[] = [];
      if (checkpoint) {
        try {
          changes = await api.checkpointChanges(checkpoint);
        } catch (e) {
          report.current(String(e));
        }
      }

      setMessages((all) =>
        all.map((m) => {
          if (m.id !== id) return m;
          const said = m.text || m.parts?.some((p) => p.kind === "text" && p.text.trim());
          return {
            ...m,
            streaming: false,
            failed: failed || m.failed,
            changes,
            text:
              m.text ||
              (said || m.auth
                ? ""
                : error
                  ? `Could not run the agent: ${error}`
                  : failed
                    ? `The agent exited with code ${code}.`
                    : ""),
          };
        }),
      );

      if (changes.length) notify.current(changes.map((c) => c.absPath));
      else if (checkpoint) api.checkpointDiscard(checkpoint).catch(() => {});
    },
    [],
  );

  /* ── running ───────────────────────────────────────────────── */

  const send = useCallback(async () => {
    const text = prompt.trim();
    if (!text || busy) return;
    if (!engine?.available) {
      report.current(
        `\`${engineId}\` is not on your PATH. Install it, then reopen this panel.`,
      );
      return;
    }

    const id = uid();
    setPrompt("");
    setBusy(true);
    setRunId(id);
    setMessages((all) => [
      ...all,
      { id: `${id}-you`, role: "you", text },
      { id, role: "agent", text: "", parts: [], streaming: true, engine: engine.label },
    ]);

    try {
      // Snapshot first: nothing the agent does is unreviewable.
      const checkpoint = await api.checkpointCreate(root);
      setMessages((all) =>
        all.map((m) => (m.id === id ? { ...m, checkpoint: checkpoint.id } : m)),
      );
      if (checkpoint.truncated) {
        setMessages((all) => [
          ...all,
          {
            id: `${id}-warn`,
            role: "system",
            text: "This folder was too large to capture completely, so some changes may not be revertible.",
          },
        ]);
      }
      await api.agentRun(id, root, text, {
        engine: engineId,
        model,
        effort,
        permissionMode: mode,
        resume: true,
      });
    } catch (e) {
      setRunId("");
      await finish(id, null, String(e));
    } finally {
      setBusy(false);
    }
  }, [busy, effort, engine, engineId, finish, mode, model, prompt, root]);

  const cancel = useCallback(() => {
    if (!runId) return;
    void api.agentCancel(runId).catch((e) => report.current(String(e)));
  }, [runId]);

  /** Clears the transcript *and* the agent's own memory of the conversation. */
  const clearChat = useCallback(() => {
    setMessages([]);
    void api.agentReset(engineId, root).catch((e) => report.current(String(e)));
  }, [engineId, root]);

  /* ── keep / revert ─────────────────────────────────────────── */

  const drop = useCallback((messageId: string, path: string) => {
    setMessages((all) =>
      all.map((m) =>
        m.id === messageId ? { ...m, changes: (m.changes ?? []).filter((c) => c.path !== path) } : m,
      ),
    );
  }, []);

  const keep = useCallback((messageId: string, change: AgentChange) => {
    // Keeping is just accepting what is already on disk.
    drop(messageId, change.path);
  }, [drop]);

  const revert = useCallback(
    async (messageId: string, checkpoint: string, changes: AgentChange[]) => {
      const revertible = changes.filter((c) => c.revertible);
      if (!revertible.length) return;
      try {
        await api.checkpointRevert(checkpoint, revertible.map((c) => c.path));
        revertible.forEach((c) => drop(messageId, c.path));
        notify.current(revertible.map((c) => c.absPath));
      } catch (e) {
        report.current(String(e));
      }
    },
    [drop],
  );

  const keepAll = useCallback(
    (messageId: string, changes: AgentChange[]) => {
      changes.forEach((c) => drop(messageId, c.path));
    },
    [drop],
  );

  /* ── render ────────────────────────────────────────────────── */

  const available = Boolean(engine?.available);
  const modeNote = modes.find((m) => m.value === mode)?.note ?? "";
  const hints = MODEL_HINTS[engineId] ?? [];

  return (
    <div className="agent">
      <div className="agent-head">
        <select
          className="input agent-engine"
          value={engineId}
          disabled={busy || !engines}
          title="Which agent CLI runs this conversation"
          onChange={(e) => setEngineId(e.target.value)}
        >
          {(engines ?? [{ id: "claude", label: "Claude Code", available: true }]).map((e) => (
            <option key={e.id} value={e.id}>
              {e.available ? e.label : `${e.label} (not installed)`}
            </option>
          ))}
        </select>
        <span className={`agent-status ${engines ? (available ? "on" : "off") : ""}`}>
          {!engines
            ? "Looking for agent CLIs…"
            : available
              ? (engine?.version ?? "installed")
              : "not found"}
        </span>
        <div className="spacer" />
        <button
          className="icon-mini"
          title="New conversation — the agent forgets this thread"
          onClick={clearChat}
        >
          <Icon name="trash" size={13} />
        </button>
        <button className="icon-mini" title="Hide chat" onClick={onClose}>
          <Icon name="close" size={13} />
        </button>
      </div>

      <div className="agent-transcript" ref={transcript}>
        {!messages.length && (
          <div className="agent-empty">
            <Icon name="sparkle" size={26} />
            <p>
              {available
                ? `Ask ${engine?.label} to change something in this folder. Watch it think, edit files and run commands — then keep or revert every file it touched.`
                : `${engine?.label ?? "This CLI"} was not found on your PATH. Install it (${engine?.install ?? ""}), sign in with \`${engine?.signIn ?? ""}\`, then reopen this panel.`}
            </p>
          </div>
        )}

        {messages.map((m) => (
          <div key={m.id} className={`agent-msg ${m.role}${m.failed ? " failed" : ""}`}>
            <div className="agent-msg-role">
              {m.role === "you"
                ? "You"
                : m.role === "agent"
                  ? `${m.engine ?? "Agent"}${m.model ? ` · ${shortModel(m.model)}` : ""}`
                  : "Depot"}
              {m.streaming && <span className="agent-dots" aria-label="running" />}
            </div>

            {m.text && <pre className="agent-msg-body">{m.text}</pre>}

            {m.parts?.map((p, i) =>
              p.kind === "text" ? (
                <div key={i} className="agent-text">
                  {p.text}
                </div>
              ) : p.kind === "thinking" ? (
                <ThinkingBlock key={i} text={p.text} live={Boolean(m.streaming)} />
              ) : (
                <ToolCard key={p.toolId || i} part={p} />
              ),
            )}

            {m.auth && <AuthNotice auth={m.auth} />}

            {m.streaming && !m.parts?.length && (
              <div className="agent-text agent-starting">Starting {m.engine ?? "agent"}…</div>
            )}

            {m.meta && <div className="agent-meta">{m.meta}</div>}
            {m.failed && m.logs && <pre className="agent-logs">{m.logs}</pre>}

            {m.changes && m.changes.length > 0 && m.checkpoint && (
              <div className="agent-changes">
                <div className="agent-changes-head">
                  <span>
                    {m.changes.length} file{m.changes.length === 1 ? "" : "s"} changed
                  </span>
                  <div className="spacer" />
                  <button className="agent-chip keep" onClick={() => keepAll(m.id, m.changes!)}>
                    Keep all
                  </button>
                  <button
                    className="agent-chip revert"
                    onClick={() => void revert(m.id, m.checkpoint!, m.changes!)}
                  >
                    Revert all
                  </button>
                </div>
                {m.changes.map((c) => (
                  <ChangeRow
                    key={c.path}
                    change={c}
                    busy={busy}
                    onOpen={() => onOpenAgentDiff(m.checkpoint!, c)}
                    onKeep={() => keep(m.id, c)}
                    onRevert={() => void revert(m.id, m.checkpoint!, [c])}
                  />
                ))}
              </div>
            )}
          </div>
        ))}
      </div>

      <div className="agent-compose">
        <div className="agent-opts">
          <input
            className="input agent-opt"
            list="agent-model-hints"
            value={model}
            disabled={busy}
            placeholder="Model: default"
            title="Which model the agent uses; blank keeps the CLI's own default"
            onChange={(e) => setModel(e.target.value)}
          />
          <datalist id="agent-model-hints">
            {hints.map((h) => (
              <option key={h} value={h} />
            ))}
          </datalist>
          {engineId === "claude" && (
            <select
              className="input agent-opt"
              value={effort}
              disabled={busy}
              title="How hard the agent thinks"
              onChange={(e) => setEffort(e.target.value)}
            >
              {EFFORTS.map((e) => (
                <option key={e.value} value={e.value}>
                  {e.label}
                </option>
              ))}
            </select>
          )}
          {modes.length > 0 && (
            <select
              className="input agent-opt"
              value={mode}
              disabled={busy}
              title="What the agent may do without asking"
              onChange={(e) => setMode(e.target.value)}
            >
              {modes.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
          )}
        </div>
        <div className="agent-note">
          {modeNote ||
            (engine && !engine.structured
              ? `${engine.label} has no structured stream, so its replies arrive as plain text.`
              : "")}
        </div>
        <textarea
          className="input agent-input"
          rows={3}
          placeholder={
            available
              ? "Describe the change you want (⌘/Ctrl Enter to send)"
              : `Install ${engine?.label ?? "an agent CLI"} to use this panel`
          }
          value={prompt}
          disabled={!available}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
              e.preventDefault();
              void send();
            }
          }}
        />
        <div className="agent-compose-row">
          <span className="text-muted agent-cwd" title={root}>
            {root}
          </span>
          <div className="spacer" />
          {runId ? (
            <button className="btn btn-secondary" onClick={cancel}>
              Stop
            </button>
          ) : (
            <button
              className="btn btn-primary"
              disabled={!prompt.trim() || busy || !available}
              onClick={() => void send()}
            >
              <Icon name="forward" size={14} />
              Send
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
