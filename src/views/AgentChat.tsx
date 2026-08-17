/**
 * The agent chat panel.
 *
 * Depot is not an AI client: it runs whichever coding-agent CLI the user
 * already has installed and signed in, in that tool's non-interactive mode,
 * and streams its output here. No API key ever passes through Depot.
 *
 * A checkpoint of the workspace is taken before every run, so each file the
 * agent touched can be kept or put back individually — the whole reason it is
 * safe to let a CLI edit files unattended.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, onAgentDone, onAgentOut } from "../api";
import { Icon } from "../lib/icons";
import type { AgentChange, AgentPreset, ChatMessage } from "../types";

const uid = () => `run-${Math.random().toString(36).slice(2, 10)}-${Date.now()}`;

const KIND_MARK: Record<AgentChange["kind"], { letter: string; tone: string }> = {
  modified: { letter: "M", tone: "mod" },
  added: { letter: "A", tone: "add" },
  deleted: { letter: "D", tone: "del" },
};

/** Strips ANSI escapes that survive `NO_COLOR` in some CLIs. */
// eslint-disable-next-line no-control-regex
const ANSI = /\[[0-9;?]*[A-Za-z]|\][^]*/g;
const clean = (line: string) => line.replace(ANSI, "");

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
  const [presets, setPresets] = useState<AgentPreset[]>([]);
  const [agentId, setAgentId] = useState("");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [prompt, setPrompt] = useState("");
  const [runId, setRunId] = useState("");
  const [busy, setBusy] = useState(false);
  const [editingArgs, setEditingArgs] = useState(false);
  const [argOverride, setArgOverride] = useState<Record<string, string>>({});

  const transcript = useRef<HTMLDivElement>(null);
  const report = useRef(onError);
  report.current = onError;
  const notify = useRef(onFilesChanged);
  notify.current = onFilesChanged;

  useEffect(() => {
    void api
      .agentList()
      .then((list) => {
        setPresets(list);
        setAgentId((current) => current || list.find((p) => p.available)?.id || list[0]?.id || "");
      })
      .catch((e) => report.current(String(e)));
  }, []);

  const agent = useMemo(() => presets.find((p) => p.id === agentId), [presets, agentId]);
  const effectiveArgs = useMemo(() => {
    if (!agent) return [];
    const override = argOverride[agent.id];
    if (override === undefined) return agent.args;
    // Whitespace-separated, honouring quotes so a flag value can contain spaces.
    return override.match(/"[^"]*"|'[^']*'|\S+/g)?.map((a) => a.replace(/^["']|["']$/g, "")) ?? [];
  }, [agent, argOverride]);

  // A hidden panel has no scroll height to set, so pin to the bottom again
  // when it comes back rather than leaving it stranded mid-transcript.
  useEffect(() => {
    if (!visible || !transcript.current) return;
    transcript.current.scrollTop = transcript.current.scrollHeight;
  }, [messages, visible]);

  /* ── streaming ──────────────────────────────────────────────── */

  useEffect(() => {
    let stop: Array<() => void> = [];
    let dead = false;

    void (async () => {
      const off = [
        await onAgentOut((e) => {
          setMessages((all) =>
            all.map((m) =>
              m.id === e.id
                ? { ...m, text: `${m.text}${m.text ? "\n" : ""}${clean(e.line)}` }
                : m,
            ),
          );
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
        all.map((m) =>
          m.id === id
            ? {
                ...m,
                streaming: false,
                failed,
                changes,
                text:
                  m.text ||
                  (error
                    ? `Could not run the agent: ${error}`
                    : failed
                      ? `The agent exited with code ${code}.`
                      : "(no output)"),
              }
            : m,
        ),
      );

      if (changes.length) notify.current(changes.map((c) => c.absPath));
      else if (checkpoint) api.checkpointDiscard(checkpoint).catch(() => {});
    },
    [],
  );

  /* ── running ────────────────────────────────────────────────── */

  const send = useCallback(async () => {
    const text = prompt.trim();
    if (!text || busy) return;
    if (!agent) {
      report.current("Pick an agent first.");
      return;
    }
    if (!agent.available) {
      report.current(`\`${agent.command}\` is not on your PATH. Install it, or edit the command.`);
      return;
    }

    const id = uid();
    setPrompt("");
    setBusy(true);
    setRunId(id);
    setMessages((all) => [
      ...all,
      { id: `${id}-you`, role: "you", text },
      { id, role: "agent", text: "", streaming: true },
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
      await api.agentRun(id, agent.command, effectiveArgs, root, text);
    } catch (e) {
      setRunId("");
      await finish(id, null, String(e));
    } finally {
      setBusy(false);
    }
  }, [agent, busy, effectiveArgs, finish, prompt, root]);

  const cancel = useCallback(() => {
    if (!runId) return;
    void api.agentCancel(runId).catch((e) => report.current(String(e)));
  }, [runId]);

  /* ── keep / revert ──────────────────────────────────────────── */

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

  /* ── render ─────────────────────────────────────────────────── */

  const anyAvailable = presets.some((p) => p.available);

  return (
    <div className="agent">
      <div className="agent-head">
        <span className="code-explorer-title as-label">Chat</span>
        <button className="icon-mini" title="Clear conversation" onClick={() => setMessages([])}>
          <Icon name="trash" size={13} />
        </button>
        <button className="icon-mini" title="Hide chat" onClick={onClose}>
          <Icon name="close" size={13} />
        </button>
      </div>

      <div className="agent-picker">
        <select
          className="input"
          value={agentId}
          onChange={(e) => setAgentId(e.target.value)}
          disabled={busy}
        >
          {presets.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
              {p.available ? "" : " — not installed"}
            </option>
          ))}
        </select>
        <button
          className={editingArgs ? "icon-mini on" : "icon-mini"}
          title="Edit the command line"
          onClick={() => setEditingArgs((v) => !v)}
        >
          <Icon name="gear" size={13} />
        </button>
      </div>

      {agent && <div className="agent-note">{agent.note}</div>}

      {editingArgs && agent && (
        <div className="agent-args">
          <label>Arguments — {"{{prompt}}"} is replaced with your message</label>
          <input
            className="input"
            value={argOverride[agent.id] ?? agent.args.join(" ")}
            onChange={(e) => setArgOverride((all) => ({ ...all, [agent.id]: e.target.value }))}
            spellCheck={false}
          />
          <div className="agent-args-cmd">
            {agent.command} {effectiveArgs.join(" ")}
          </div>
        </div>
      )}

      <div className="agent-transcript" ref={transcript}>
        {!messages.length && (
          <div className="agent-empty">
            <Icon name="code" size={26} />
            <p>
              {anyAvailable
                ? "Ask the agent to change something in this folder. Every file it touches can be kept or reverted afterwards."
                : "No agent CLI found on your PATH. Install Claude Code, Codex, Copilot CLI or opencode, then reopen this panel."}
            </p>
          </div>
        )}

        {messages.map((m) => (
          <div key={m.id} className={`agent-msg ${m.role}${m.failed ? " failed" : ""}`}>
            <div className="agent-msg-role">
              {m.role === "you" ? "You" : m.role === "agent" ? agent?.label ?? "Agent" : "Depot"}
              {m.streaming && <span className="agent-dots" aria-label="running" />}
            </div>
            {m.text && <pre className="agent-msg-body">{m.text}</pre>}

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
        <textarea
          className="input agent-input"
          rows={3}
          placeholder={
            anyAvailable
              ? "Describe the change you want (⌘/Ctrl Enter to send)"
              : "Install an agent CLI to use this panel"
          }
          value={prompt}
          disabled={!anyAvailable}
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
              disabled={!prompt.trim() || busy || !agent?.available}
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
