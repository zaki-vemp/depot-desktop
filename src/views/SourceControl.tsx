/**
 * The source-control panel: what changed, staged versus not, and a way to
 * commit it. Clicking a row opens the diff for that file in an editor tab.
 */
import { useState } from "react";
import { Icon } from "../lib/icons";
import type { GitFile, GitRepo } from "../types";

/** Single letter and colour per state, the way git itself abbreviates them. */
const MARK: Record<GitFile["kind"], { letter: string; tone: string; label: string }> = {
  modified: { letter: "M", tone: "mod", label: "Modified" },
  added: { letter: "A", tone: "add", label: "Added" },
  deleted: { letter: "D", tone: "del", label: "Deleted" },
  renamed: { letter: "R", tone: "mod", label: "Renamed" },
  copied: { letter: "C", tone: "mod", label: "Copied" },
  untracked: { letter: "U", tone: "new", label: "Untracked" },
  conflicted: { letter: "!", tone: "conflict", label: "Conflicted" },
};

function Row({
  file,
  active,
  onOpen,
  actions,
}: {
  file: GitFile;
  active: boolean;
  onOpen: () => void;
  actions: Array<{ icon: Parameters<typeof Icon>[0]["name"]; title: string; run: () => void; danger?: boolean }>;
}) {
  const mark = MARK[file.kind];
  const dir = file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : "";

  return (
    <div className={active ? "scm-row on" : "scm-row"} onClick={onOpen} title={file.origPath ? `${file.origPath} → ${file.path}` : file.path}>
      <span className="scm-name">{file.name}</span>
      {dir && <span className="scm-dir">{dir}</span>}
      <span className="scm-actions">
        {actions.map((a) => (
          <button
            key={a.title}
            className={a.danger ? "icon-mini danger" : "icon-mini"}
            title={a.title}
            onClick={(e) => {
              e.stopPropagation();
              a.run();
            }}
          >
            <Icon name={a.icon} size={13} />
          </button>
        ))}
      </span>
      <span className={`scm-mark ${mark.tone}`} title={mark.label}>
        {mark.letter}
      </span>
    </div>
  );
}

function Section({
  label,
  files,
  activePath,
  onOpen,
  rowActions,
  bulk,
}: {
  label: string;
  files: GitFile[];
  activePath: string;
  onOpen: (file: GitFile) => void;
  rowActions: (file: GitFile) => Array<{ icon: Parameters<typeof Icon>[0]["name"]; title: string; run: () => void; danger?: boolean }>;
  bulk: Array<{ icon: Parameters<typeof Icon>[0]["name"]; title: string; run: () => void; danger?: boolean }>;
}) {
  const [open, setOpen] = useState(true);
  if (!files.length) return null;

  return (
    <div className="scm-section">
      <div className="scm-section-head">
        <button className="scm-section-toggle" onClick={() => setOpen((o) => !o)}>
          <Icon name={open ? "chevronDown" : "chevronRight"} size={11} />
          <span>{label}</span>
        </button>
        {bulk.map((b) => (
          <button
            key={b.title}
            className={b.danger ? "icon-mini danger" : "icon-mini"}
            title={b.title}
            onClick={b.run}
          >
            <Icon name={b.icon} size={13} />
          </button>
        ))}
        <span className="scm-count">{files.length}</span>
      </div>
      {open &&
        files.map((f) => (
          <Row
            key={`${f.staged ? "s" : "u"}:${f.path}`}
            file={f}
            active={activePath === f.absPath}
            onOpen={() => onOpen(f)}
            actions={rowActions(f)}
          />
        ))}
    </div>
  );
}

export function SourceControl({
  repo,
  activePath,
  busy,
  message,
  onMessage,
  onOpenDiff,
  onStage,
  onUnstage,
  onDiscard,
  onCommit,
  onRefresh,
}: {
  /** Null when the workspace folder is not inside a git repository. */
  repo: GitRepo | null;
  activePath: string;
  busy: boolean;
  message: string;
  onMessage: (value: string) => void;
  onOpenDiff: (file: GitFile) => void;
  onStage: (paths: string[]) => void;
  onUnstage: (paths: string[]) => void;
  onDiscard: (files: GitFile[]) => void;
  onCommit: () => void;
  onRefresh: () => void;
}) {
  if (!repo) {
    return (
      <div className="scm">
        <div className="scm-head">
          <span className="code-explorer-title as-label">Source control</span>
          <button className="icon-mini" title="Check again" onClick={onRefresh}>
            <Icon name="reload" size={13} />
          </button>
        </div>
        <div className="tree-loading" style={{ padding: "14px 12px", lineHeight: 1.5 }}>
          This folder is not inside a git repository — or git is not installed.
        </div>
      </div>
    );
  }

  const total = repo.staged.length + repo.unstaged.length;

  return (
    <div className="scm">
      <div className="scm-head">
        <span className="code-explorer-title as-label">Source control</span>
        <button className="icon-mini" title="Refresh" onClick={onRefresh} disabled={busy}>
          <Icon name="reload" size={13} />
        </button>
      </div>

      <div className="scm-branch" title={repo.upstream ? `Tracking ${repo.upstream}` : "No upstream"}>
        <Icon name="net" size={13} />
        <span className="scm-branch-name">{repo.branch}</span>
        {repo.ahead > 0 && <span className="tag tag-neutral">↑{repo.ahead}</span>}
        {repo.behind > 0 && <span className="tag tag-neutral">↓{repo.behind}</span>}
      </div>

      <div className="scm-commit">
        <textarea
          className="input scm-message"
          rows={2}
          placeholder={`Message (⌘/Ctrl Enter to commit on ${repo.branch})`}
          value={message}
          onChange={(e) => onMessage(e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
              e.preventDefault();
              onCommit();
            }
          }}
        />
        <button
          className="btn btn-primary btn-block"
          disabled={busy || !message.trim() || !repo.staged.length}
          title={
            !repo.staged.length
              ? "Stage something first"
              : !message.trim()
                ? "Write a commit message"
                : `Commit ${repo.staged.length} file${repo.staged.length === 1 ? "" : "s"}`
          }
          onClick={onCommit}
        >
          <Icon name="check" size={15} />
          Commit {repo.staged.length ? `(${repo.staged.length})` : ""}
        </button>
      </div>

      <div className="scm-list">
        <Section
          label="Staged changes"
          files={repo.staged}
          activePath={activePath}
          onOpen={onOpenDiff}
          bulk={[
            {
              icon: "up",
              title: "Unstage everything",
              run: () => onUnstage(repo.staged.map((f) => f.path)),
            },
          ]}
          rowActions={(f) => [
            { icon: "up", title: `Unstage ${f.name}`, run: () => onUnstage([f.path]) },
          ]}
        />

        <Section
          label="Changes"
          files={repo.unstaged}
          activePath={activePath}
          onOpen={onOpenDiff}
          bulk={[
            {
              icon: "plus",
              title: "Stage everything",
              run: () => onStage(repo.unstaged.map((f) => f.path)),
            },
            {
              icon: "loop",
              title: "Discard every change",
              run: () => onDiscard(repo.unstaged),
              danger: true,
            },
          ]}
          rowActions={(f) => [
            { icon: "loop", title: `Discard changes to ${f.name}`, run: () => onDiscard([f]), danger: true },
            { icon: "plus", title: `Stage ${f.name}`, run: () => onStage([f.path]) },
          ]}
        />

        {!total && <div className="scm-clean">No changes — the working tree is clean.</div>}
      </div>
    </div>
  );
}
