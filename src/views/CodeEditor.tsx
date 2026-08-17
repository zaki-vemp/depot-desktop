/**
 * The code workspace: an explorer tree, Monaco with its own file tabs, and a
 * dock of pty-backed terminals — Depot's take on the VS Code layout.
 *
 * Editing is real: buffers are Monaco models that keep their own undo stack and
 * view state across tab switches, and ⌘/Ctrl-S writes them back to disk. The
 * whole workspace stays mounted while another Depot tab is on screen, so an
 * unsaved buffer or a running command is never thrown away by a tab click.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
// Path helpers are shared with the shell so Windows drive roots and UNC paths
// behave the same everywhere they are joined or walked upwards.
import { api, baseName, joinPath, parentDir as parentOf } from "../api";
import { changeCounts, diffLines } from "../lib/diff";
import { chromeIconFor, extOf, formatBytes } from "../lib/files";
import { Icon } from "../lib/icons";
import { languageForPath, monaco, setupMonaco } from "../lib/monaco";
import type { AgentChange, DirEntry, EditorDoc, GitFile, GitRepo } from "../types";
import { AgentChat } from "./AgentChat";
import { SourceControl } from "./SourceControl";
import { TerminalPanel } from "./TerminalPanel";

const MAX_EDITABLE = 4 * 1024 * 1024;
const MIN_TERM_HEIGHT = 120;
const MIN_EDITOR_HEIGHT = 140;
/** How often the source-control panel re-reads status while it is on screen. */
const GIT_POLL_MS = 4000;

/**
 * A tab is either the file itself or a diff of that file against a revision.
 * Both share one `Doc` — so the right-hand side of a diff is the live buffer,
 * and editing or saving from the diff view works exactly as it does in the
 * plain editor.
 */
interface TabDesc {
  key: string;
  path: string;
  /**
   * `staged` diffs HEAD against the index, which is what the next commit will
   * contain. `work` diffs the index against the working tree, and its right
   * side is the live buffer, so it stays editable and saveable.
   * Absent for a normal editing tab.
   */
  diff?: "staged" | "work" | "agent";
  /** Agent diff tabs: which checkpoint the left-hand side comes from. */
  checkpoint?: string;
}

type DiffSide = "staged" | "work" | "agent";
const diffKey = (side: DiffSide, path: string) => `diff:${side}:${path}`;
const diffLabel = (side: DiffSide) =>
  side === "staged" ? "Staged" : side === "agent" ? "AI" : "Changes";

interface Doc extends EditorDoc {
  model: monaco.editor.ITextModel;
  /** Contents at HEAD, for the change gutter. Undefined until git answers. */
  head?: string;
}

/** Single-letter git mark drawn at the end of a changed row in the tree. */
const GIT_LETTER: Record<GitFile["kind"], string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  untracked: "U",
  conflicted: "!",
};

/* ── explorer ─────────────────────────────────────────────────────────────── */

function TreeNode({
  entry,
  depth,
  expanded,
  tree,
  activePath,
  status,
  onToggle,
  onOpen,
  onContext,
}: {
  entry: DirEntry;
  depth: number;
  expanded: Set<string>;
  /** Directory → its listed entries; a missing key means "not loaded yet". */
  tree: Record<string, DirEntry[]>;
  activePath: string;
  /** Absolute path → git state, for the letter and tint on changed rows. */
  status: Record<string, GitFile["kind"]>;
  onToggle: (entry: DirEntry) => void;
  onOpen: (entry: DirEntry) => void;
  onContext: (entry: DirEntry, x: number, y: number) => void;
}) {
  const open = expanded.has(entry.path);
  const kids = tree[entry.path];
  const git = status[entry.path];

  return (
    <>
      <button
        className={[
          "tree-row",
          entry.path === activePath ? "on" : "",
          git ? `git-${git}` : "",
        ]
          .filter(Boolean)
          .join(" ")}
        style={{ paddingLeft: 8 + depth * 12 }}
        onClick={() => (entry.isDir ? onToggle(entry) : onOpen(entry))}
        onContextMenu={(e) => {
          e.preventDefault();
          onContext(entry, e.clientX, e.clientY);
        }}
        title={git ? `${entry.path} — ${git}` : entry.path}
      >
        <span className="tree-twist">
          {entry.isDir && <Icon name={open ? "chevronDown" : "chevronRight"} size={12} />}
        </span>
        <Icon name={entry.isDir ? "folder" : chromeIconFor(entry)} size={14} />
        <span className="tree-name">{entry.name}</span>
        {git && <span className="tree-git">{GIT_LETTER[git]}</span>}
      </button>
      {entry.isDir &&
        open &&
        (kids
          ? kids.map((child) => (
              <TreeNode
                key={child.path}
                entry={child}
                depth={depth + 1}
                expanded={expanded}
                tree={tree}
                activePath={activePath}
                status={status}
                onToggle={onToggle}
                onOpen={onOpen}
                onContext={onContext}
              />
            ))
          : (
            <div className="tree-loading" style={{ paddingLeft: 20 + depth * 12 }}>
              Loading…
            </div>
          ))}
    </>
  );
}

/* ── workspace ────────────────────────────────────────────────────────────── */

export function CodeEditor({
  root,
  initialFile,
  openRequest,
  theme,
  visible,
  showHidden,
  onError,
  onDirtyCount,
  onPrompt,
  onOpenFolder,
  onChangeFolder,
}: {
  /** Folder the explorer is rooted at. */
  root: string;
  initialFile?: string;
  /** "Open this file too" from elsewhere in the app; `nonce` re-triggers it. */
  openRequest?: { file: string; nonce: number };
  theme: "light" | "dark";
  /** False while another Depot tab is on screen — Monaco cannot measure then. */
  visible: boolean;
  showHidden: boolean;
  onError: (message: string) => void;
  onDirtyCount?: (count: number) => void;
  /** Reuses the app's own prompt dialog for new-file / new-folder names. */
  onPrompt: (options: { title: string; label: string; value: string; okLabel: string; onOk: (value: string) => void }) => void;
  /** Picks a folder and opens it as a second workspace tab. */
  onOpenFolder: () => void;
  /** Picks a folder and re-roots this workspace onto it. */
  onChangeFolder: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const diffHost = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const diffEditor = useRef<monaco.editor.IStandaloneDiffEditor | null>(null);
  const docs = useRef(new Map<string, Doc>());
  const viewStates = useRef(new Map<string, monaco.editor.ICodeEditorViewState | null>());
  /**
   * Models owned by a diff tab. `original` is always a git revision. `modified`
   * is only set for staged diffs, where the right side is the index rather
   * than the live buffer; unstaged diffs reuse the buffer's own model.
   */
  const originals = useRef(
    new Map<string, { original: monaco.editor.ITextModel; modified?: monaco.editor.ITextModel }>(),
  );
  const gutter = useRef<monaco.editor.IEditorDecorationsCollection | null>(null);

  const [openTabs, setOpenTabs] = useState<TabDesc[]>([]);
  const [current, setCurrent] = useState("");
  const [panel, setPanel] = useState<"files" | "scm">("files");
  const [repo, setRepo] = useState<GitRepo | null>(null);
  const [gitBusy, setGitBusy] = useState(false);
  const [commitMessage, setCommitMessage] = useState("");
  /** Bumped on every buffer edit so the gutter recomputes. */
  const [editTick, setEditTick] = useState(0);
  const [gutterCounts, setGutterCounts] = useState({ added: 0, removed: 0 });
  const [dirty, setDirty] = useState<Record<string, boolean>>({});
  const [meta, setMeta] = useState<Record<string, { language: string; readonly: boolean }>>({});
  const [cursor, setCursor] = useState({ line: 1, column: 1 });
  const [loading, setLoading] = useState("");

  const [children, setChildren] = useState<Record<string, DirEntry[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [treeMenu, setTreeMenu] = useState<{ entry: DirEntry; x: number; y: number } | null>(null);

  const [terminals, setTerminals] = useState<string[]>([]);
  const [activeTerm, setActiveTerm] = useState("");
  const [chatOpen, setChatOpen] = useState(false);
  const [chatMounted, setChatMounted] = useState(false);
  const [chatWidth, setChatWidth] = useState(400);
  const [termOpen, setTermOpen] = useState(true);
  const [termHeight, setTermHeight] = useState(260);

  const report = useRef(onError);
  report.current = onError;

  /* ── monaco lifecycle ───────────────────────────────────────── */

  useEffect(() => {
    if (!host.current) return;
    setupMonaco();
    const instance = monaco.editor.create(host.current, {
      theme: theme === "dark" ? "depot-dark" : "depot-light",
      automaticLayout: true,
      fontSize: 13,
      fontFamily:
        'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
      minimap: { enabled: true, renderCharacters: false },
      scrollBeyondLastLine: false,
      renderWhitespace: "selection",
      smoothScrolling: true,
      tabSize: 2,
      padding: { top: 10, bottom: 10 },
    });
    editor.current = instance;

    const cursorSub = instance.onDidChangeCursorPosition((e) =>
      setCursor({ line: e.position.lineNumber, column: e.position.column }),
    );

    return () => {
      cursorSub.dispose();
      instance.dispose();
      diffEditor.current?.dispose();
      diffEditor.current = null;
      editor.current = null;
      docs.current.forEach((doc) => doc.model.dispose());
      docs.current.clear();
      originals.current.forEach((sides) => {
        sides.original.dispose();
        sides.modified?.dispose();
      });
      originals.current.clear();
    };
    // Created once; theme and models are applied by the effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    monaco.editor.setTheme(theme === "dark" ? "depot-dark" : "depot-light");
  }, [theme]);

  useEffect(() => {
    if (chatOpen) setChatMounted(true);
  }, [chatOpen]);

  // Opening or closing the side dock changes how much room Monaco has.
  useEffect(() => {
    const raf = requestAnimationFrame(() => {
      editor.current?.layout();
      diffEditor.current?.layout();
    });
    return () => cancelAnimationFrame(raf);
  }, [chatOpen, chatWidth]);

  // Hidden elements measure as 0×0, so re-lay-out the moment the tab returns.
  useEffect(() => {
    if (!visible) return;
    const raf = requestAnimationFrame(() => editor.current?.layout());
    return () => cancelAnimationFrame(raf);
  }, [visible]);

  useEffect(() => {
    onDirtyCount?.(Object.values(dirty).filter(Boolean).length);
  }, [dirty, onDirtyCount]);

  /* ── change gutter ──────────────────────────────────────────── */

  // Marks each line as added, modified or deleted against HEAD. Debounced,
  // because it reruns on every keystroke and a large file is worth 5ms only
  // once the typing pauses.
  useEffect(() => {
    const code = editor.current;
    const desc = openTabs.find((t) => t.key === current);
    if (!code) return;

    const doc = desc && !desc.diff ? docs.current.get(desc.path) : undefined;
    if (!doc || doc.head === undefined) {
      gutter.current?.clear();
      setGutterCounts({ added: 0, removed: 0 });
      return;
    }

    const timer = setTimeout(() => {
      const changes = diffLines(doc.head ?? "", doc.model.getValue());
      const lines = doc.model.getLineCount();
      const decorations = changes.map((c) => {
        if (c.type === "deleted") {
          const line = Math.min(Math.max(c.start, 1), lines);
          return {
            range: new monaco.Range(line, 1, line, 1),
            options: {
              linesDecorationsClassName:
                c.start === 0 ? "gutter-mark del at-top" : "gutter-mark del",
            },
          };
        }
        return {
          range: new monaco.Range(Math.min(c.start, lines), 1, Math.min(c.end, lines), 1),
          options: {
            linesDecorationsClassName: `gutter-mark ${c.type === "added" ? "add" : "mod"}`,
          },
        };
      });

      if (!gutter.current) gutter.current = code.createDecorationsCollection([]);
      gutter.current.set(decorations);
      setGutterCounts(changeCounts(changes));
    }, 250);

    return () => clearTimeout(timer);
  }, [current, editTick, openTabs]);

  /* ── explorer data ──────────────────────────────────────────── */

  const loadDir = useCallback(
    async (dir: string) => {
      try {
        const entries = await api.listDir(dir);
        setChildren((all) => ({ ...all, [dir]: entries }));
      } catch (e) {
        report.current(String(e));
        setChildren((all) => ({ ...all, [dir]: [] }));
      }
    },
    [],
  );

  useEffect(() => {
    // Re-rooting onto another folder: the old tree's expansion state and
    // cached listings describe paths that are no longer on screen.
    setExpanded(new Set());
    setChildren({});
    setTreeMenu(null);
    void loadDir(root);
  }, [root, loadDir]);

  const refreshTree = useCallback(() => {
    const dirs = [root, ...Array.from(expanded)];
    dirs.forEach((dir) => void loadDir(dir));
  }, [root, expanded, loadDir]);

  const toggleDir = useCallback(
    (entry: DirEntry) => {
      setExpanded((all) => {
        const next = new Set(all);
        if (next.has(entry.path)) next.delete(entry.path);
        else {
          next.add(entry.path);
          if (!children[entry.path]) void loadDir(entry.path);
        }
        return next;
      });
    },
    [children, loadDir],
  );

  /* ── source control ─────────────────────────────────────────── */

  const repoRef = useRef<GitRepo | null>(null);
  repoRef.current = repo;

  /** Absolute path → repo-relative, forward-slashed, the way git wants it. */
  const relativeTo = useCallback((absPath: string) => {
    const root = repoRef.current?.root;
    if (!root) return null;
    const normal = absPath.replace(/\\/g, "/");
    const base = root.replace(/\\/g, "/").replace(/\/+$/, "");
    if (normal === base) return "";
    if (!normal.startsWith(`${base}/`)) return null;
    return normal.slice(base.length + 1);
  }, []);

  const refreshGit = useCallback(async () => {
    try {
      const info = await api.gitInfo(root);
      setRepo(info);
    } catch {
      // git missing or unreadable repo: the panel says so, no error toast.
      setRepo(null);
    }
  }, [root]);

  useEffect(() => {
    setRepo(null);
    setCommitMessage("");
    void refreshGit();
  }, [refreshGit]);

  // Poll only while the panel is actually on screen. Commands run in the
  // built-in terminal change the repo behind our back, so some polling is the
  // only way the list stays honest.
  useEffect(() => {
    if (!visible || panel !== "scm") return;
    const id = setInterval(() => void refreshGit(), GIT_POLL_MS);
    return () => clearInterval(id);
  }, [visible, panel, refreshGit]);

  /** Pulls the committed copy of a file so the gutter has something to compare. */
  const loadHead = useCallback(
    async (path: string) => {
      const rel = relativeTo(path);
      const root2 = repoRef.current?.root;
      const doc = docs.current.get(path);
      if (!doc || !root2 || rel == null) return;
      try {
        doc.head = await api.gitShow(root2, "HEAD", rel);
        setEditTick((t) => t + 1);
      } catch {
        doc.head = undefined;
      }
    },
    [relativeTo],
  );

  // Once git answers, backfill HEAD for everything already open.
  useEffect(() => {
    if (!repo) return;
    docs.current.forEach((doc) => {
      if (doc.head === undefined) void loadHead(doc.path);
    });
  }, [repo, loadHead]);

  const gitAction = useCallback(
    async (run: (root: string) => Promise<unknown>) => {
      const root2 = repoRef.current?.root;
      if (!root2) return;
      setGitBusy(true);
      try {
        await run(root2);
        await refreshGit();
        // Staging or discarding moves HEAD-relative content under our feet.
        docs.current.forEach((doc) => void loadHead(doc.path));
      } catch (e) {
        report.current(String(e));
      } finally {
        setGitBusy(false);
      }
    },
    [loadHead, refreshGit],
  );

  /* ── documents ──────────────────────────────────────────────── */

  const showing = useRef("");
  const tabsRef = useRef<TabDesc[]>([]);
  tabsRef.current = openTabs;

  /** Creates the diff editor on first use — most sessions never open one. */
  const ensureDiffEditor = useCallback(() => {
    if (diffEditor.current || !diffHost.current) return diffEditor.current;
    diffEditor.current = monaco.editor.createDiffEditor(diffHost.current, {
      theme: theme === "dark" ? "depot-dark" : "depot-light",
      automaticLayout: true,
      fontSize: 13,
      fontFamily:
        'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
      renderSideBySide: true,
      // The left side is a committed revision; only the buffer is editable.
      originalEditable: false,
      readOnly: false,
      scrollBeyondLastLine: false,
      minimap: { enabled: false },
      ignoreTrimWhitespace: false,
    });
    return diffEditor.current;
    // `theme` is applied globally by setTheme, so it need not re-create this.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const showDoc = useCallback(
    (key: string) => {
      const desc = tabsRef.current.find((t) => t.key === key);
      const doc = desc && docs.current.get(desc.path);
      const code = editor.current;
      if (!desc || !code) return;
      // A staged diff shows HEAD against the index and needs no open buffer;
      // everything else does.
      if (!doc && desc.diff !== "staged") return;

      // Park the outgoing file's scroll position and cursor before swapping.
      const previous = tabsRef.current.find((t) => t.key === showing.current);
      if (previous && !previous.diff && showing.current !== key) {
        viewStates.current.set(previous.key, code.saveViewState());
      }
      showing.current = key;

      if (desc.diff) {
        const diff = ensureDiffEditor();
        const sides = originals.current.get(key);
        const right = sides?.modified ?? doc?.model;
        if (diff && sides && right) {
          // Staged diffs show the index on the right, which is a snapshot and
          // not editable. Unstaged diffs show the live buffer.
          diff.setModel({ original: sides.original, modified: right });
          diff.updateOptions({ readOnly: desc.diff === "staged" });
        }
      } else if (doc) {
        code.setModel(doc.model);
        const state = viewStates.current.get(key);
        if (state) code.restoreViewState(state);
      }

      setCurrent(key);
      if (visible && !desc.diff) code.focus();
    },
    [ensureDiffEditor, visible],
  );

  const openFile = useCallback(
    async (path: string) => {
      if (docs.current.has(path)) {
        setOpenTabs((all) => (all.some((t) => t.key === path) ? all : [...all, { key: path, path }]));
        showDoc(path);
        return;
      }
      setLoading(path);
      try {
        if (!(await api.isTextFile(path))) {
          throw new Error(`${baseName(path)} is not a text file — open it from the file list instead.`);
        }
        const text = await api.readText(path);
        const language = languageForPath(path);
        // Always an absolute, authority-less URI: `depot://C:/a.ts` would
        // parse the drive letter as the host and collide across drives.
        const model = monaco.editor.createModel(
          text,
          language,
          monaco.Uri.parse(`depot:///${encodeURI(path.replace(/\\/g, "/").replace(/^\/+/, ""))}`),
        );
        // Keep the file's own line endings. Without this a CRLF file saves
        // back as LF, rewriting every line, and reads as dirty on open.
        model.setEOL(
          /\r\n/.test(text)
            ? monaco.editor.EndOfLineSequence.CRLF
            : monaco.editor.EndOfLineSequence.LF,
        );
        // Baseline from the model, not the raw text: if Monaco normalised
        // mixed endings, "dirty" should still mean "you changed something".
        const loaded = model.getValue();
        model.onDidChangeContent(() => {
          const doc = docs.current.get(path);
          if (!doc) return;
          setDirty((all) => {
            const next = model.getValue() !== doc.saved;
            return all[path] === next ? all : { ...all, [path]: next };
          });
          setEditTick((t) => t + 1);
        });
        docs.current.set(path, { path, name: baseName(path), saved: loaded, language, model });
        setMeta((all) => ({ ...all, [path]: { language, readonly: false } }));
        setOpenTabs((all) => (all.some((t) => t.key === path) ? all : [...all, { key: path, path }]));
        showDoc(path);
        void loadHead(path);
      } catch (e) {
        report.current(String(e));
      } finally {
        setLoading((l) => (l === path ? "" : l));
      }
    },
    [showDoc],
  );

  const save = useCallback(
    async (path: string) => {
      const doc = docs.current.get(path);
      if (!doc) return;
      const text = doc.model.getValue();
      if (text.length > MAX_EDITABLE) {
        report.current(`${doc.name} is larger than ${formatBytes(MAX_EDITABLE)} — save it elsewhere.`);
        return;
      }
      try {
        await api.writeText(path, text);
        doc.saved = text;
        setDirty((all) => ({ ...all, [path]: false }));
        // Saving is what turns a buffer edit into a working-tree change.
        void refreshGit();
      } catch (e) {
        report.current(String(e));
      }
    },
    [refreshGit],
  );

  const saveAll = useCallback(async () => {
    for (const path of Object.keys(dirty)) {
      if (dirty[path]) await save(path);
    }
  }, [dirty, save]);

  const closeDoc = useCallback(
    (key: string) => {
      const desc = tabsRef.current.find((t) => t.key === key);
      if (!desc) return;
      // The buffer stays alive while any other tab still shows this file.
      const lastForPath =
        tabsRef.current.filter((t) => t.path === desc.path && t.key !== key).length === 0;

      const drop = () => {
        const sides = originals.current.get(key);
        sides?.original.dispose();
        sides?.modified?.dispose();
        originals.current.delete(key);
        viewStates.current.delete(key);
        if (lastForPath) {
          docs.current.get(desc.path)?.model.dispose();
          docs.current.delete(desc.path);
          setDirty((all) => {
            const next = { ...all };
            delete next[desc.path];
            return next;
          });
        }
        setOpenTabs((all) => {
          const remaining = all.filter((t) => t.key !== key);
          if (key === current) {
            const fallback = remaining[remaining.length - 1];
            if (fallback) requestAnimationFrame(() => showDoc(fallback.key));
            else {
              editor.current?.setModel(null);
              diffEditor.current?.setModel(null);
              showing.current = "";
              setCurrent("");
            }
          }
          return remaining;
        });
      };

      // Only the real editing tab guards unsaved work; closing a diff view of
      // the same file leaves the buffer untouched.
      if (!desc.diff && lastForPath && dirty[desc.path]) {
        onPrompt({
          title: `${baseName(desc.path)} has unsaved changes`,
          label: 'Type "discard" to close without saving',
          value: "",
          okLabel: "Close",
          onOk: (answer) => {
            if (answer.trim().toLowerCase() === "discard") drop();
            else report.current("Close cancelled — the file still has unsaved changes.");
          },
        });
        return;
      }
      drop();
    },
    [current, dirty, onPrompt, showDoc],
  );

  /**
   * Opens a diff tab for a changed file. Staged rows compare HEAD against the
   * index; unstaged rows compare against the working tree, which is the live
   * buffer — so the right-hand side stays editable.
   */
  const openDiff = useCallback(
    async (file: GitFile) => {
      const side: "staged" | "work" = file.staged ? "staged" : "work";
      const key = diffKey(side, file.absPath);

      if (file.kind === "deleted") {
        report.current(`${file.name} was deleted — there is nothing left to diff.`);
        return;
      }

      const root2 = repoRef.current?.root;
      const rel = relativeTo(file.absPath);
      if (!root2 || rel == null) return;

      try {
        // The working-tree diff needs the buffer for its right-hand side.
        if (side === "work" && !docs.current.has(file.absPath)) {
          await openFile(file.absPath);
          if (!docs.current.has(file.absPath)) return;
        }

        const language = languageForPath(file.absPath);
        // staged: HEAD → index.  work: index → working tree.
        // An untracked file exists in neither HEAD nor the index, so the left
        // side is legitimately empty and the whole file reads as added.
        const leftRev = side === "staged" ? "HEAD" : ":";
        const left = file.kind === "untracked" ? "" : await api.gitShow(root2, leftRev, rel);
        const right = side === "staged" ? await api.gitShow(root2, ":", rel) : null;

        let sides = originals.current.get(key);
        if (!sides) {
          sides = {
            original: monaco.editor.createModel(left, language),
            modified: right === null ? undefined : monaco.editor.createModel(right, language),
          };
          originals.current.set(key, sides);
        } else {
          if (sides.original.getValue() !== left) sides.original.setValue(left);
          if (sides.modified && right !== null && sides.modified.getValue() !== right) {
            sides.modified.setValue(right);
          }
        }

        setOpenTabs((all) =>
          all.some((t) => t.key === key) ? all : [...all, { key, path: file.absPath, diff: side }],
        );
        requestAnimationFrame(() => showDoc(key));
      } catch (e) {
        report.current(String(e));
      }
    },
    [openFile, relativeTo, showDoc],
  );

  // Initial file, once Monaco exists.
  const opened = useRef(false);
  useEffect(() => {
    if (opened.current || !initialFile) return;
    opened.current = true;
    void openFile(initialFile);
  }, [initialFile, openFile]);

  useEffect(() => {
    if (!openRequest?.file) return;
    void openFile(openRequest.file);
    // Only the nonce marks a fresh request; the path alone may repeat.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openRequest?.nonce]);

  /* ── terminals ──────────────────────────────────────────────── */

  const terminalsRef = useRef<string[]>([]);
  terminalsRef.current = terminals;

  const addTerminal = useCallback(() => {
    const id = `term-${Math.random().toString(36).slice(2, 10)}`;
    setTerminals((all) => [...all, id]);
    setActiveTerm(id);
    setTermOpen(true);
  }, []);

  // Ref-guarded: React double-invokes effects in development, and a second
  // shell would spawn before the first `setTerminals` had landed.
  const seeded = useRef(false);
  useEffect(() => {
    if (seeded.current) return;
    seeded.current = true;
    addTerminal();
  }, [addTerminal]);

  const closeTerminal = useCallback((id: string) => {
    void api.termClose(id).catch(() => {});
    setTerminals((all) => {
      const remaining = all.filter((t) => t !== id);
      setActiveTerm((cur) => (cur === id ? remaining[remaining.length - 1] ?? "" : cur));
      return remaining;
    });
  }, []);

  // Kill every pty when the workspace itself goes away.
  useEffect(() => {
    return () => {
      terminalsRef.current.forEach((id) => void api.termClose(id).catch(() => {}));
    };
  }, []);

  /* ── dock resizing ──────────────────────────────────────────── */

  const stage = useRef<HTMLDivElement>(null);
  const workspace = useRef<HTMLDivElement>(null);
  const dragTerm = useRef(false);
  const dragChat = useRef(false);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (dragChat.current && workspace.current) {
        const rect = workspace.current.getBoundingClientRect();
        const next = rect.right - e.clientX;
        setChatWidth(Math.max(280, Math.min(next, rect.width - 420)));
        return;
      }
      if (!dragTerm.current || !stage.current) return;
      const rect = stage.current.getBoundingClientRect();
      const next = rect.bottom - e.clientY;
      setTermHeight(Math.max(MIN_TERM_HEIGHT, Math.min(next, rect.height - MIN_EDITOR_HEIGHT)));
    };
    const onUp = () => {
      if (dragChat.current) {
        dragChat.current = false;
        document.body.classList.remove("col-resizing");
        editor.current?.layout();
        diffEditor.current?.layout();
      }
      if (!dragTerm.current) return;
      dragTerm.current = false;
      document.body.classList.remove("row-resizing");
      editor.current?.layout();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  /* ── keyboard ───────────────────────────────────────────────── */

  const activeTab = openTabs.find((t) => t.key === current);
  const currentPath = activeTab?.path ?? "";

  useEffect(() => {
    if (!visible) return;
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      const key = e.key.toLowerCase();
      if (key === "s") {
        e.preventDefault();
        e.stopPropagation();
        if (e.shiftKey) void saveAll();
        else if (currentPath) void save(currentPath);
      } else if (key === "i") {
        e.preventDefault();
        setChatOpen((open) => !open);
      } else if (key === "`") {
        e.preventDefault();
        setTermOpen((open) => !open);
      } else if (key === "w" && current) {
        e.preventDefault();
        e.stopPropagation();
        closeDoc(current);
      }
    };
    // Capture: beat the app-level shortcuts to ⌘S / ⌘W while a file is open.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [visible, current, currentPath, save, saveAll, closeDoc]);

  useEffect(() => {
    if (!treeMenu) return;
    const close = () => setTreeMenu(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [treeMenu]);

  /* ── render ─────────────────────────────────────────────────── */

  const rootEntries = useMemo(() => {
    const entries = children[root] ?? [];
    return showHidden ? entries : entries.filter((e) => !e.name.startsWith("."));
  }, [children, root, showHidden]);

  const visibleChildren = useMemo(() => {
    if (showHidden) return children;
    const filtered: Record<string, DirEntry[]> = {};
    for (const [dir, entries] of Object.entries(children)) {
      filtered[dir] = entries.filter((e) => !e.name.startsWith("."));
    }
    return filtered;
  }, [children, showHidden]);

  const changeTotal = (repo?.staged.length ?? 0) + (repo?.unstaged.length ?? 0);

  /** Absolute path → git state, so the explorer can tint changed files. */
  const statusByPath = useMemo(() => {
    const map: Record<string, GitFile["kind"]> = {};
    for (const f of [...(repo?.unstaged ?? []), ...(repo?.staged ?? [])]) {
      // Unstaged wins when a file is in both lists: it is the newer state.
      if (!map[f.absPath]) map[f.absPath] = f.kind;
    }
    return map;
  }, [repo]);

  /** Discarding is the one destructive action here, so it always confirms. */
  const confirmDiscard = useCallback(
    (files: GitFile[]) => {
      if (!files.length) return;
      const deletes = files.filter((f) => f.kind === "untracked");
      const what =
        files.length === 1
          ? files[0].name
          : `${files.length} files`;
      onPrompt({
        title: `Discard changes to ${what}?`,
        label:
          deletes.length > 0
            ? `Type "discard" to confirm. ${deletes.length} untracked file${deletes.length === 1 ? " is" : "s are"} deleted outright — this cannot be undone.`
            : 'Type "discard" to confirm. Edits are thrown away and cannot be recovered.',
        value: "",
        okLabel: "Discard",
        onOk: (answer) => {
          if (answer.trim().toLowerCase() !== "discard") {
            report.current("Discard cancelled — nothing was changed.");
            return;
          }
          void gitAction(async (r) => {
            await api.gitDiscard(r, files.map((f) => f.path));
            // The buffers on screen still hold the discarded text; reload them.
            for (const f of files) {
              const doc = docs.current.get(f.absPath);
              if (!doc) continue;
              try {
                const fresh = await api.readText(f.absPath);
                doc.model.setValue(fresh);
                doc.saved = doc.model.getValue();
                setDirty((all) => ({ ...all, [f.absPath]: false }));
              } catch {
                // File is gone now (it was untracked) — leave the tab be.
              }
            }
          });
        },
      });
    },
    [gitAction, onPrompt],
  );

  /**
   * Pulls files back off disk after something outside the editor rewrote them
   * — an agent run, or undoing one. A buffer with unsaved edits is left alone
   * and flagged, because silently replacing the user's typing would be worse
   * than a stale tab.
   */
  const reloadFromDisk = useCallback(
    async (paths: string[]) => {
      const clobbered: string[] = [];
      for (const path of paths) {
        const doc = docs.current.get(path);
        if (!doc) continue;
        if (dirty[path]) {
          clobbered.push(baseName(path));
          continue;
        }
        try {
          const fresh = await api.readText(path);
          if (fresh === doc.model.getValue()) continue;
          // pushEditOperations keeps undo history, so ⌘Z still works.
          doc.model.pushEditOperations(
            [],
            [{ range: doc.model.getFullModelRange(), text: fresh }],
            () => null,
          );
          doc.saved = doc.model.getValue();
          setDirty((all) => ({ ...all, [path]: false }));
        } catch {
          // Deleted by the agent; the tab stays open showing the last content.
        }
      }
      if (clobbered.length) {
        report.current(
          `Kept your unsaved edits in ${clobbered.join(", ")} — reopen to see the agent's version.`,
        );
      }
      void refreshGit();
      refreshTree();
      docs.current.forEach((doc) => void loadHead(doc.path));
    },
    [dirty, loadHead, refreshGit, refreshTree],
  );

  /** Diffs one agent-changed file against the pre-run checkpoint. */
  const openAgentDiff = useCallback(
    async (checkpoint: string, change: AgentChange) => {
      const key = diffKey("agent", change.absPath);
      try {
        const before = await api.checkpointOriginal(checkpoint, change.path);

        // A deleted file has no buffer to show on the right.
        let right: monaco.editor.ITextModel | undefined;
        if (change.kind === "deleted") {
          right = monaco.editor.createModel("", languageForPath(change.absPath));
        } else if (!docs.current.has(change.absPath)) {
          await openFile(change.absPath);
        }
        const doc = docs.current.get(change.absPath);
        if (!doc && !right) return;

        const language = doc?.language ?? languageForPath(change.absPath);
        let sides = originals.current.get(key);
        if (!sides) {
          sides = { original: monaco.editor.createModel(before, language), modified: right };
          originals.current.set(key, sides);
        } else if (sides.original.getValue() !== before) {
          sides.original.setValue(before);
        }

        setOpenTabs((all) =>
          all.some((t) => t.key === key)
            ? all
            : [...all, { key, path: change.absPath, diff: "agent", checkpoint }],
        );
        requestAnimationFrame(() => showDoc(key));
      } catch (e) {
        report.current(String(e));
      }
    },
    [openFile, showDoc],
  );

  const commit = useCallback(() => {
    void gitAction(async (r) => {
      await api.gitCommit(r, commitMessage, false);
      setCommitMessage("");
    });
  }, [commitMessage, gitAction]);

  const currentMeta = currentPath ? meta[currentPath] : undefined;
  const dirtyCount = Object.values(dirty).filter(Boolean).length;
  const termCwd = current ? parentOf(current) : root;

  const promptNew = (dir: string, isDir: boolean) => {
    onPrompt({
      title: isDir ? "New folder" : "New file",
      label: `In ${baseName(dir) || dir}`,
      value: isDir ? "untitled" : "untitled.txt",
      okLabel: "Create",
      onOk: (name) => {
        const trimmed = name.trim();
        if (!trimmed) return;
        const target = joinPath(dir, trimmed);
        const create = isDir ? api.mkdir(target) : api.createFile(target);
        void create
          .then(() => {
            setExpanded((all) => new Set(all).add(dir));
            return loadDir(dir);
          })
          .then(() => {
            if (!isDir) void openFile(target);
          })
          .catch((e) => report.current(String(e)));
      },
    });
  };

  return (
    <div className="code-workspace" ref={workspace}>
      <div className="code-explorer">
        <div className="code-side-switch">
          <button
            className={panel === "files" ? "side-switch-opt on" : "side-switch-opt"}
            onClick={() => setPanel("files")}
          >
            <Icon name="documents" size={14} />
            <span className="side-switch-label">Explorer</span>
          </button>
          <button
            className={panel === "scm" ? "side-switch-opt on" : "side-switch-opt"}
            onClick={() => setPanel("scm")}
            title={repo ? `On branch ${repo.branch}` : "Source control"}
          >
            <Icon name="net" size={14} />
            <span className="side-switch-label">Source control</span>
            {changeTotal > 0 && (
              <span className="side-switch-badge">{changeTotal > 99 ? "99+" : changeTotal}</span>
            )}
          </button>
        </div>

        {panel === "scm" ? (
          <SourceControl
            repo={repo}
            activePath={currentPath}
            busy={gitBusy}
            message={commitMessage}
            onMessage={setCommitMessage}
            onOpenDiff={(f) => void openDiff(f)}
            onStage={(paths) => void gitAction((r) => api.gitStage(r, paths))}
            onUnstage={(paths) => void gitAction((r) => api.gitUnstage(r, paths))}
            onDiscard={confirmDiscard}
            onCommit={commit}
            onRefresh={() => void refreshGit()}
          />
        ) : (
        <>
        <div className="code-explorer-head">
          <button
            className="code-explorer-title"
            title={`${root}\n\nClick to edit a different folder in this tab`}
            onClick={onChangeFolder}
          >
            <span>{baseName(root) || root}</span>
            <Icon name="chevronDown" size={11} />
          </button>
          <button
            className="icon-mini"
            title="Open another folder in a new tab…"
            onClick={onOpenFolder}
          >
            <Icon name="folderOpen" size={13} />
          </button>
          <button className="icon-mini" title="New file" onClick={() => promptNew(root, false)}>
            <Icon name="plus" size={13} />
          </button>
          <button className="icon-mini" title="New folder" onClick={() => promptNew(root, true)}>
            <Icon name="folderPlus" size={13} />
          </button>
          <button className="icon-mini" title="Refresh" onClick={refreshTree}>
            <Icon name="reload" size={13} />
          </button>
        </div>
        <div className="code-tree">
          {rootEntries.map((entry) => (
            <TreeNode
              key={entry.path}
              entry={entry}
              depth={0}
              expanded={expanded}
              tree={visibleChildren}
              activePath={currentPath}
              status={statusByPath}
              onToggle={toggleDir}
              onOpen={(e) => void openFile(e.path)}
              onContext={(e, x, y) => setTreeMenu({ entry: e, x, y })}
            />
          ))}
          {!rootEntries.length && <div className="tree-loading">Empty folder</div>}
        </div>
        </>
        )}
      </div>

      <div className="code-stage" ref={stage}>
        <div className="code-tabs">
          {openTabs.map((tab) => (
            <div
              key={tab.key}
              className={tab.key === current ? "code-tab on" : "code-tab"}
              onClick={() => showDoc(tab.key)}
              title={tab.diff ? `${tab.path} — ${diffLabel(tab.diff)} diff` : tab.path}
            >
              <Icon
                name={tab.diff ? "arrows" : chromeIconFor({ isDir: false, ext: extOf(tab.path) })}
                size={13}
              />
              <span className="title">{baseName(tab.path)}</span>
              {tab.diff && <span className="code-tab-kind">{diffLabel(tab.diff)}</span>}
              <button
                className={!tab.diff && dirty[tab.path] ? "code-tab-close dirty" : "code-tab-close"}
                aria-label={`Close ${baseName(tab.path)}`}
                onClick={(e) => {
                  e.stopPropagation();
                  closeDoc(tab.key);
                }}
              >
                {!tab.diff && dirty[tab.path] ? <span className="dot" /> : <Icon name="close" size={12} />}
              </button>
            </div>
          ))}
          <div className="spacer" />
          <button
            className="btn btn-ghost btn-icon"
            title="Save (⌘/Ctrl S)"
            disabled={!currentPath || !dirty[currentPath]}
            onClick={() => currentPath && void save(currentPath)}
          >
            <Icon name="save" size={16} />
          </button>
          <button
            className="btn btn-ghost btn-icon"
            title="Save all (⌘/Ctrl ⇧ S)"
            disabled={!dirtyCount}
            onClick={() => void saveAll()}
          >
            <Icon name="check" size={16} />
          </button>
          <button
            className={termOpen ? "btn btn-ghost btn-icon on" : "btn btn-ghost btn-icon"}
            title="Toggle terminal (⌘/Ctrl `)"
            onClick={() => setTermOpen((open) => !open)}
          >
            <Icon name="terminal" size={16} />
          </button>
          <button
            className={chatOpen ? "btn btn-ghost btn-icon on" : "btn btn-ghost btn-icon"}
            title="Toggle AI chat (⌘/Ctrl I)"
            onClick={() => setChatOpen((open) => !open)}
          >
            <Icon name="sparkle" size={16} />
          </button>
        </div>

        {/* Both editors stay mounted; only the one the active tab needs shows.
            Re-creating Monaco per tab switch would lose undo and scroll. */}
        <div
          className="code-editor-area"
          style={{ display: openTabs.length && !activeTab?.diff ? undefined : "none" }}
        >
          <div className="code-monaco" ref={host} />
        </div>
        <div
          className="code-editor-area"
          style={{ display: activeTab?.diff ? undefined : "none" }}
        >
          <div className="code-monaco" ref={diffHost} />
        </div>

        {!openTabs.length && (
          <div className="code-empty">
            <Icon name="edit" size={30} />
            <div className="heading" style={{ fontSize: 17 }}>
              {loading ? `Opening ${baseName(loading)}…` : "No file open"}
            </div>
            <p className="text-muted">
              Pick a file from the tree to edit it. ⌘/Ctrl S saves, ⌘/Ctrl ` toggles the terminal.
            </p>
          </div>
        )}

        {/* Hidden, never unmounted: tearing the dock down would kill every
            shell running in it just because the panel was collapsed. */}
        <>
            <div
              className="code-dock-grip"
              style={{ display: termOpen ? undefined : "none" }}
              onMouseDown={() => {
                dragTerm.current = true;
                document.body.classList.add("row-resizing");
              }}
            />
            <div className="code-dock" style={{ height: termHeight, display: termOpen ? undefined : "none" }}>
              <div className="code-dock-head">
                <span className="code-dock-label">Terminal</span>
                {terminals.map((id, i) => (
                  <button
                    key={id}
                    className={id === activeTerm ? "term-tab on" : "term-tab"}
                    onClick={() => setActiveTerm(id)}
                  >
                    <Icon name="terminal" size={12} />
                    <span>{i + 1}</span>
                    <span
                      className="term-tab-close"
                      role="button"
                      tabIndex={-1}
                      aria-label={`Close terminal ${i + 1}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        closeTerminal(id);
                      }}
                    >
                      <Icon name="close" size={11} />
                    </span>
                  </button>
                ))}
                <button className="icon-mini" title="New terminal" onClick={addTerminal}>
                  <Icon name="plus" size={13} />
                </button>
                <div className="spacer" />
                <span className="text-muted code-dock-cwd" title={termCwd}>
                  {termCwd}
                </span>
                <button className="icon-mini" title="Hide terminal" onClick={() => setTermOpen(false)}>
                  <Icon name="close" size={13} />
                </button>
              </div>
              <div className="code-dock-body">
                {terminals.map((id) => (
                  <div
                    key={id}
                    className="code-term-slot"
                    style={{ display: id === activeTerm ? undefined : "none" }}
                  >
                    <TerminalPanel
                      id={id}
                      cwd={root}
                      theme={theme}
                      visible={visible && termOpen && id === activeTerm}
                      onError={onError}
                    />
                  </div>
                ))}
              </div>
            </div>
        </>

        <div className="code-status">
          {repo && (
            <button
              className="code-status-branch"
              title={`On branch ${repo.branch}${repo.upstream ? ` · tracking ${repo.upstream}` : ""}`}
              onClick={() => setPanel("scm")}
            >
              <Icon name="net" size={12} />
              {repo.branch}
              {changeTotal > 0 && <span className="tag tag-accent">{changeTotal}</span>}
            </button>
          )}
          <span>{currentPath ? baseName(currentPath) : "—"}</span>
          <span className="text-muted">{currentMeta?.language ?? "plaintext"}</span>
          <span className="text-muted">
            Ln {cursor.line}, Col {cursor.column}
          </span>
          {(gutterCounts.added > 0 || gutterCounts.removed > 0) && (
            <span className="code-status-diff" title="Lines changed against HEAD">
              <span className="add">+{gutterCounts.added}</span>
              <span className="del">−{gutterCounts.removed}</span>
            </span>
          )}
          <div className="spacer" />
          {dirtyCount > 0 && <span className="tag tag-accent">{dirtyCount} unsaved</span>}
          <span className="text-muted">{openTabs.length} open</span>
          <span className="text-muted">UTF-8</span>
        </div>
      </div>

      {/* Mounted whenever it has been opened, hidden rather than unmounted, so
          a running agent and its pending changes survive a panel toggle. */}
      {chatMounted && (
        <>
          <div
            className="code-side-grip"
            style={{ display: chatOpen ? undefined : "none" }}
            onMouseDown={() => {
              dragChat.current = true;
              document.body.classList.add("col-resizing");
            }}
          />
          <div
            className="code-chat"
            style={{ width: chatWidth, display: chatOpen ? undefined : "none" }}
          >
            <AgentChat
              root={root}
              visible={visible && chatOpen}
              onError={onError}
              onOpenAgentDiff={(checkpoint, change) => void openAgentDiff(checkpoint, change)}
              onFilesChanged={(paths) => void reloadFromDisk(paths)}
              onClose={() => setChatOpen(false)}
            />
          </div>
        </>
      )}

      {treeMenu && (
        <div className="ctx" style={{ left: treeMenu.x, top: treeMenu.y }} onClick={(e) => e.stopPropagation()}>
          {!treeMenu.entry.isDir && (
            <button onClick={() => { void openFile(treeMenu.entry.path); setTreeMenu(null); }}>
              Open in editor
            </button>
          )}
          <button
            onClick={() => {
              promptNew(treeMenu.entry.isDir ? treeMenu.entry.path : parentOf(treeMenu.entry.path), false);
              setTreeMenu(null);
            }}
          >
            New file here
          </button>
          <button
            onClick={() => {
              promptNew(treeMenu.entry.isDir ? treeMenu.entry.path : parentOf(treeMenu.entry.path), true);
              setTreeMenu(null);
            }}
          >
            New folder here
          </button>
          <hr />
          <button
            onClick={() => {
              onPrompt({
                title: "Rename",
                label: "New name",
                value: treeMenu.entry.name,
                okLabel: "Rename",
                onOk: (name) => {
                  const dir = parentOf(treeMenu.entry.path);
                  void api
                    .rename(treeMenu.entry.path, joinPath(dir, name.trim()))
                    .then(() => loadDir(dir))
                    .catch((e) => report.current(String(e)));
                },
              });
              setTreeMenu(null);
            }}
          >
            Rename…
          </button>
          <button
            className="danger"
            onClick={() => {
              const target = treeMenu.entry;
              void api
                .trash(target.path)
                .then(() => loadDir(parentOf(target.path)))
                .catch((e) => report.current(String(e)));
              setTreeMenu(null);
            }}
          >
            Move to Trash
          </button>
          <hr />
          <button
            onClick={() => {
              void api.reveal(treeMenu.entry.path).catch((e) => report.current(String(e)));
              setTreeMenu(null);
            }}
          >
            Reveal in file manager
          </button>
        </div>
      )}
    </div>
  );
}
