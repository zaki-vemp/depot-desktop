import {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { api, baseName, driveDestPath, fileUrl, joinPath, onTransfer, parentDir } from "./api";
import {
  chromeIconFor,
  extOf,
  formatBytes,
  formatDate,
  formatSpeed,
  iconFor,
  kindLabel,
  viewerKind,
} from "./lib/files";
import { FileIcon, Icon, type IconName } from "./lib/icons";
import type {
  AppSettings,
  DirEntry,
  DiskUsage,
  DriveAccount,
  Place,
  SocialAppKind,
  SourceKind,
  Tab,
  TorrentInfo,
  Transfer,
  UiPrefs,
} from "./types";
import { MediaPlayer } from "./views/MediaPlayer";
import { OfficePreview } from "./views/OfficePreview";
import { OpenWithMenu } from "./views/OpenWith";
import { Connections, ProviderHeading as ProviderSettingsHeading } from "./views/Connections";
import "./App.css";

// Monaco is several megabytes of parsed JavaScript. Keeping it behind a lazy
// import means a session that never opens a code tab never pays for it.
const CodeEditor = lazy(() =>
  import("./views/CodeEditor").then((m) => ({ default: m.CodeEditor })),
);

let seq = 1;
const uid = (prefix: string) => `${prefix}-${seq++}-${Date.now()}`;

/** Sidebar mark for each well-known place the backend reports. */
const PLACE_ICONS: Record<string, IconName> = {
  home: "home",
  desktop: "desktop",
  documents: "documents",
  downloads: "download",
  pictures: "image",
  music: "music",
  movies: "film",
  videos: "film",
  trash: "trash",
};

const SOCIAL_APPS: Record<SocialAppKind, { title: string; url: string; icon: IconName }> = {
  facebook: { title: "Facebook", url: "https://www.facebook.com/", icon: "facebook" },
  instagram: { title: "Instagram", url: "https://www.instagram.com/", icon: "instagram" },
};

function normalizeWebUrl(raw: string) {
  const t = raw.trim();
  if (!t) return "";
  if (/^[a-z][a-z0-9+.-]*:/i.test(t)) return t;
  if (t.startsWith("//")) return `https:${t}`;
  return `https://${t}`;
}

function webUrlHost(value: string) {
  try {
    return new URL(normalizeWebUrl(value)).hostname.replace(/^www\./, "");
  } catch {
    return value;
  }
}

/**
 * Tab label for a workspace. Two folders called `src` are easy to open at
 * once, so a colliding name is qualified with its parent.
 */
function workspaceTitle(root: string, existing: Tab[]) {
  const name = baseName(root) || root;
  const clash = existing.some(
    (t) => t.kind === "editor" && t.path !== root && (baseName(t.path || "") || t.path) === name,
  );
  if (!clash) return name;
  const parent = baseName(parentDir(root));
  return parent && parent !== name ? `${name} — ${parent}` : root;
}

function duplicateName(name: string) {
  const i = name.lastIndexOf(".");
  if (i > 0) return `${name.slice(0, i)} copy${name.slice(i)}`;
  return `${name} copy`;
}

function transferRoute(
  item: DirEntry,
  destSource: SourceKind,
  destAccountId: string | undefined,
  cut: boolean,
) {
  const fromDrive = item.source === "gdrive";
  const toDrive = destSource === "gdrive";
  if (fromDrive && toDrive) {
    const cross = Boolean(item.accountId && destAccountId && item.accountId !== destAccountId);
    if (cut) return cross ? "Move Drive accounts" : "Move on Drive";
    return cross ? "Drive → Drive" : "Copy on Drive";
  }
  if (fromDrive) return "Drive → Local";
  if (toDrive) return "Local → Drive";
  return cut ? "Move" : "Copy";
}

function transferOp(item: DirEntry, destSource: SourceKind, cut: boolean): Transfer["op"] {
  if (cut) return "move";
  if (item.source === "gdrive" && destSource === "local") return "download";
  if (item.source === "local" && destSource === "gdrive") return "upload";
  return "copy";
}

const PREFS_KEY = "depot.prefs";
const DEFAULT_PREFS: UiPrefs = {
  theme: "light",
  view: "grid",
  showHidden: false,
  useTrash: true,
  systemFallback: true,
  confirmDelete: true,
  sidebarOpen: true,
  inspectorOpen: true,
};

function loadPrefs(): UiPrefs {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    return raw ? { ...DEFAULT_PREFS, ...(JSON.parse(raw) as UiPrefs) } : DEFAULT_PREFS;
  } catch {
    return DEFAULT_PREFS;
  }
}

function platformLabel() {
  const ua = navigator.userAgent;
  if (ua.includes("Mac")) return "macOS";
  if (ua.includes("Win")) return "Windows";
  return "Linux";
}

const REVEAL_IN = platformLabel() === "macOS" ? "Finder" : platformLabel() === "Windows" ? "Explorer" : "file manager";

interface Prompt {
  title: string;
  label?: string;
  value: string;
  okLabel: string;
  danger?: boolean;
  requireExact?: string;
  onOk: (value: string) => void | Promise<void>;
}

export default function App() {
  const [prefs, setPrefs] = useState<UiPrefs>(loadPrefs);
  const [places, setPlaces] = useState<Place[]>([]);
  const [accounts, setAccounts] = useState<DriveAccount[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeId, setActiveId] = useState("");
  const [entries, setEntries] = useState<DirEntry[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string>("");
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [clipboard, setClipboard] = useState<{ mode: "copy" | "cut"; items: DirEntry[] } | null>(null);
  const [quickLook, setQuickLook] = useState<DirEntry | null>(null);
  const [torrents, setTorrents] = useState<TorrentInfo[]>([]);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [magnet, setMagnet] = useState("");
  const [usage, setUsage] = useState<DiskUsage | null>(null);
  const [volumeUsage, setVolumeUsage] = useState<Record<string, DiskUsage>>({});
  const [driveQuota, setDriveQuota] = useState<Record<string, DiskUsage>>({});
  const [settings, setSettings] = useState<AppSettings>({
    googleClientId: "",
    googleClientSecret: "",
    oneDriveClientId: "",
    oneDriveClientSecret: "",
    dropboxClientId: "",
    dropboxClientSecret: "",
    s3Endpoint: "",
    s3Region: "",
    s3Bucket: "",
    s3AccessKeyId: "",
    s3SecretAccessKey: "",
    torrentDownloadDir: "",
  });
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; entry: DirEntry | null } | null>(null);
  /** Per editor tab: "open this file", bumped so a repeat request still fires. */
  const [editorRequests, setEditorRequests] = useState<Record<string, { file: string; nonce: number }>>({});
  const [editorDirty, setEditorDirty] = useState<Record<string, number>>({});

  const queueRef = useRef<Promise<void>>(Promise.resolve());
  const active = tabs.find((t) => t.id === activeId) ?? tabs[0];
  const activeKind = active?.kind;

  /* ── preferences ──────────────────────────────────────────── */

  useEffect(() => {
    document.documentElement.dataset.theme = prefs.theme;
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  }, [prefs]);

  // macOS draws its traffic lights over the header; reserve room for them.
  useEffect(() => {
    document.documentElement.dataset.os = platformLabel();
  }, []);

  const setPref = useCallback(<K extends keyof UiPrefs>(key: K, value: UiPrefs[K]) => {
    setPrefs((p) => ({ ...p, [key]: value }));
  }, []);

  /* ── tabs ─────────────────────────────────────────────────── */

  const patchTab = useCallback((id: string, patch: Partial<Tab>) => {
    setTabs((all) => all.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  }, []);

  const pushTab = useCallback((tab: Tab) => {
    setTabs((all) => [...all, tab]);
    setActiveId(tab.id);
  }, []);

  const openLocal = useCallback(
    (path: string, title?: string) => {
      const name = title || baseName(path);
      pushTab({
        id: uid("tab"),
        kind: "files",
        title: name,
        path,
        source: "local",
        history: [{ loc: path, title: name }],
        historyIndex: 0,
      });
    },
    [pushTab],
  );

  const openDrive = useCallback(
    (account: DriveAccount) => {
      pushTab({
        id: uid("tab"),
        kind: "files",
        title: account.email,
        path: `gdrive://${account.id}/root`,
        source: "gdrive",
        accountId: account.id,
        folderId: "root",
        history: [{ loc: "root", title: account.email }],
        historyIndex: 0,
      });
    },
    [pushTab],
  );

  /**
   * Opens a code workspace on `root`.
   *
   * Implicit routes — "Edit code" on a file, the inspector button — reuse a
   * workspace already rooted there rather than spawning a second explorer and
   * a second shell. `newTab` is for the explicit "open folder" actions, where
   * asking for another workspace on the same folder is the whole point.
   */
  const openEditor = useCallback(
    (root: string, file?: string, newTab = false) => {
      if (!root) {
        setError("Pick a folder to edit first.");
        return;
      }
      const existing = newTab ? undefined : tabs.find((t) => t.kind === "editor" && t.path === root);
      if (existing) {
        setActiveId(existing.id);
        if (file) {
          setEditorRequests((all) => ({
            ...all,
            [existing.id]: { file, nonce: (all[existing.id]?.nonce ?? 0) + 1 },
          }));
        }
        return;
      }
      pushTab({
        id: uid("tab"),
        kind: "editor",
        title: workspaceTitle(root, tabs),
        path: root,
        source: "local",
        file,
        history: [],
        historyIndex: -1,
      });
    },
    [pushTab, tabs],
  );

  /** Native folder chooser, then a fresh workspace tab on what was picked. */
  const pickWorkspace = useCallback(
    async (startIn?: string) => {
      try {
        const picked = await api.pickFolder("Choose a folder to edit", startIn);
        if (picked) openEditor(picked, undefined, true);
      } catch (e) {
        setError(String(e));
      }
    },
    [openEditor],
  );

  /** Re-roots an existing workspace onto another folder. */
  const rerootEditor = useCallback(
    async (tabId: string, startIn?: string) => {
      try {
        const picked = await api.pickFolder("Choose a folder to edit", startIn);
        if (!picked) return;
        setTabs((all) =>
          all.map((t) =>
            t.id === tabId
              ? { ...t, path: picked, title: workspaceTitle(picked, all.filter((o) => o.id !== t.id)) }
              : t,
          ),
        );
      } catch (e) {
        setError(String(e));
      }
    },
    [],
  );

  const openToolTab = useCallback(
    (kind: Tab["kind"], title: string) => {
      const existing = tabs.find((t) => t.kind === kind);
      if (existing) {
        setActiveId(existing.id);
        return;
      }
      pushTab({ id: uid("tab"), kind, title, history: [], historyIndex: -1 });
    },
    [pushTab, tabs],
  );

  const openWebTab = useCallback(
    (url = "https://archive.org") => {
      pushTab({
        id: uid("tab"),
        kind: "web",
        title: url.replace(/^https?:\/\//, ""),
        url,
        history: [],
        historyIndex: -1,
      });
    },
    [pushTab],
  );

  const openSocialApp = useCallback(
    (app: SocialAppKind) => {
      const existing = tabs.find((tab) => tab.kind === "app" && tab.app === app);
      if (existing) {
        setActiveId(existing.id);
        return;
      }
      const config = SOCIAL_APPS[app];
      pushTab({
        id: uid(`app-${app}`),
        kind: "app",
        app,
        title: config.title,
        url: config.url,
        history: [],
        historyIndex: -1,
      });
    },
    [pushTab, tabs],
  );

  const closeTab = useCallback(
    (id: string) => {
      const drop = () =>
        setTabs((all) => {
          if (all.length === 1) return all;
          const target = all.find((t) => t.id === id);
          if (target?.kind === "web" || target?.kind === "app") {
            void api.webClose(id).catch(() => undefined);
          }
          const next = all.filter((t) => t.id !== id);
          if (id === activeId) setActiveId(next[next.length - 1].id);
          return next;
        });

      // Closing a code workspace throws away every buffer in it, so make the
      // unsaved ones say so first.
      const unsaved = editorDirty[id] ?? 0;
      if (unsaved > 0) {
        setActiveId(id);
        setPrompt({
          title: `${unsaved} unsaved file${unsaved === 1 ? "" : "s"} in this workspace`,
          label: 'Type "discard" to close and lose those edits',
          value: "",
          okLabel: "Close anyway",
          danger: true,
          requireExact: "discard",
          onOk: drop,
        });
        return;
      }
      drop();
    },
    [activeId, editorDirty],
  );

  /** Navigates the active files tab and records the jump for back/forward. */
  const navigate = useCallback(
    (loc: string, title: string) => {
      if (!active || active.kind !== "files") return;
      const trimmed = active.history.slice(0, active.historyIndex + 1);
      const history = [...trimmed, { loc, title }];
      patchTab(active.id, {
        title,
        history,
        historyIndex: history.length - 1,
        ...(active.source === "gdrive" ? { folderId: loc } : { path: loc }),
      });
    },
    [active, patchTab],
  );

  const stepHistory = useCallback(
    (delta: number) => {
      if (!active || active.kind !== "files") return;
      const index = active.historyIndex + delta;
      const entry = active.history[index];
      if (!entry) return;
      patchTab(active.id, {
        title: entry.title,
        historyIndex: index,
        ...(active.source === "gdrive" ? { folderId: entry.loc } : { path: entry.loc }),
      });
    },
    [active, patchTab],
  );

  const goUp = useCallback(async () => {
    if (!active || active.kind !== "files") return;
    if (active.source === "gdrive") {
      if (active.folderId !== "root") navigate("root", "Drive");
      return;
    }
    const parent = await api.parent(active.path || "");
    if (parent) navigate(parent, baseName(parent));
  }, [active, navigate]);

  /* ── data loading ─────────────────────────────────────────── */

  const loadPlaces = useCallback(async () => {
    try {
      setPlaces(await api.places());
      setAccounts(await api.driveAccounts());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    (async () => {
      await loadPlaces();
      try {
        setSettings(await api.settings());
      } catch {
        /* first run: no settings file yet */
      }
      const home = await api.home();
      setTabs([
        {
          id: uid("tab"),
          kind: "files",
          title: "Home",
          path: home,
          source: "local",
          history: [{ loc: home, title: "Home" }],
          historyIndex: 0,
        },
      ]);
    })();
  }, [loadPlaces]);

  useEffect(() => {
    if (tabs.length && !tabs.some((t) => t.id === activeId)) setActiveId(tabs[0].id);
  }, [tabs, activeId]);

  const refreshFiles = useCallback(async () => {
    if (!active || active.kind !== "files") return;
    setBusy(true);
    setError("");
    try {
      const list =
        active.source === "gdrive" && active.accountId
          ? await api.listDrive(active.accountId, active.folderId === "root" ? undefined : active.folderId)
          : await api.listDir(active.path || "");
      setEntries(list);
      setSelected([]);
    } catch (e) {
      setError(String(e));
      setEntries([]);
    } finally {
      setBusy(false);
    }
  }, [active]);

  useEffect(() => {
    if (activeKind === "files") void refreshFiles();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active?.id, active?.path, active?.folderId, activeKind]);

  useEffect(() => {
    if (activeKind !== "files" || active?.source !== "local" || !active.path) return;
    api.diskUsage(active.path).then(setUsage).catch(() => setUsage(null));
  }, [active?.path, active?.source, activeKind]);

  useEffect(() => {
    const tick = async () => {
      try {
        setTorrents(await api.torrents());
      } catch {
        /* engine not started yet */
      }
    };
    void tick();
    const id = setInterval(() => void tick(), 2000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    if (activeKind !== "drives") return;
    for (const p of places.filter((x) => x.kind === "volume" || x.kind === "home")) {
      api
        .diskUsage(p.path)
        .then((u) => setVolumeUsage((m) => ({ ...m, [p.path]: u })))
        .catch(() => undefined);
    }
    for (const a of accounts) {
      api
        .driveQuota(a.id)
        .then((u) => setDriveQuota((m) => ({ ...m, [a.id]: u })))
        .catch(() => undefined);
    }
  }, [activeKind, places, accounts]);

  /* ── transfers ────────────────────────────────────────────── */

  useEffect(() => {
    const unlisten = onTransfer((ev) => {
      setTransfers((all) =>
        all.map((t) => {
          if (t.id !== ev.id) return t;
          const now = Date.now();
          const dt = Math.max(1, now - t.updatedAt) / 1000;
          const speed = ev.moved > t.moved ? (ev.moved - t.moved) / dt : t.speed;
          return {
            ...t,
            moved: ev.moved,
            total: ev.total || t.total,
            speed,
            updatedAt: now,
            state: ev.state,
            error: ev.error ?? t.error,
          };
        }),
      );
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const enqueueTransfer = useCallback(
    (t: { op: Transfer["op"]; from: string; to: string; name: string; route: string; total: number }) => {
      const item: Transfer = {
        id: uid("tx"),
        moved: 0,
        speed: 0,
        state: "queued",
        startedAt: Date.now(),
        updatedAt: Date.now(),
        ...t,
      };
      setTransfers((all) => [item, ...all]);
      queueRef.current = queueRef.current.then(async () => {
        setTransfers((all) =>
          all.map((x) => (x.id === item.id ? { ...x, state: "running", updatedAt: Date.now() } : x)),
        );
        try {
          await api.startTransfer(item.id, item.from, item.to, item.op);
          setTransfers((all) =>
            all.map((x) =>
              x.id === item.id ? { ...x, state: "done", moved: x.total || x.moved, speed: 0 } : x,
            ),
          );
        } catch (e) {
          setTransfers((all) =>
            all.map((x) => (x.id === item.id ? { ...x, state: "error", error: String(e) } : x)),
          );
          setError(String(e));
        }
      });
      return queueRef.current;
    },
    [],
  );

  /* ── file operations ──────────────────────────────────────── */

  const visible = useMemo(
    () =>
      entries.filter((e) => {
        if (!prefs.showHidden && e.name.startsWith(".")) return false;
        if (query && !e.name.toLowerCase().includes(query.toLowerCase())) return false;
        return true;
      }),
    [entries, prefs.showHidden, query],
  );

  const selectedEntries = useCallback(
    () => entries.filter((e) => selected.includes(e.path)),
    [entries, selected],
  );

  const openEntry = useCallback(
    async (entry: DirEntry) => {
      if (entry.isDir) {
        navigate(entry.source === "gdrive" ? baseName(entry.path) : entry.path, entry.name);
        return;
      }
      const kind = viewerKind(entry.ext);
      if (kind === "unknown" && prefs.systemFallback && entry.source === "local") {
        try {
          await api.openSystem(entry.path);
        } catch (e) {
          setError(String(e));
        }
        return;
      }
      if (kind === "unknown" && entry.source === "gdrive") {
        setError("Copy this file to a local folder first, then open it.");
        return;
      }
      pushTab({
        id: uid("tab"),
        kind: "preview",
        title: entry.name,
        path: entry.path,
        source: entry.source,
        accountId: entry.accountId ?? undefined,
        history: [],
        historyIndex: -1,
      });
    },
    [navigate, prefs.systemFallback, pushTab],
  );

  const paste = useCallback(async () => {
    if (!clipboard || !active || active.kind !== "files") return;
    const destSource: SourceKind = active.source === "gdrive" ? "gdrive" : "local";
    if (destSource === "gdrive" && !active.accountId) {
      setError("Open a Google account folder before pasting.");
      return;
    }
    for (const item of clipboard.items) {
      const dest =
        destSource === "gdrive" && active.accountId
          ? driveDestPath(active.accountId, active.folderId, item.name)
          : joinPath(active.path || "", item.name);
      const cut = clipboard.mode === "cut";
      void enqueueTransfer({
        op: transferOp(item, destSource, cut),
        from: item.path,
        to: dest,
        name: item.name,
        route: transferRoute(item, destSource, active.accountId, cut),
        total: item.size,
      });
    }
    if (clipboard.mode === "cut") setClipboard(null);
    await queueRef.current;
    await refreshFiles();
  }, [active, clipboard, enqueueTransfer, refreshFiles]);

  const copyToLocal = useCallback(
    (items: DirEntry[]) => {
      const fallback = places.find((p) => p.kind === "downloads")?.path || places[0]?.path || "";
      setPrompt({
        title: "Copy to local folder",
        label: "Destination folder",
        value: settings.torrentDownloadDir || fallback,
        okLabel: "Copy",
        onOk: async (dir) => {
          for (const item of items) {
            if (item.isDir) {
              setError(`Folders are copied one file at a time — skipped ${item.name}.`);
              continue;
            }
            void enqueueTransfer({
              op: item.source === "gdrive" ? "download" : "copy",
              from: item.path,
              to: joinPath(dir, item.name),
              name: item.name,
              route: item.source === "gdrive" ? "Drive → Local" : "Local → Local",
              total: item.size,
            });
          }
          openToolTab("transfers", "Transfers");
        },
      });
    },
    [enqueueTransfer, openToolTab, places, settings.torrentDownloadDir],
  );

  const removeSelected = useCallback(() => {
    const items = selectedEntries().filter((e) => e.source === "local");
    if (!items.length) {
      setError("Nothing local selected — Drive items cannot be deleted from Depot.");
      return;
    }
    const run = async () => {
      setBusy(true);
      try {
        for (const item of items) {
          if (prefs.useTrash) await api.trash(item.path);
          else await api.remove(item.path);
        }
        await refreshFiles();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    };
    if (!prefs.confirmDelete) {
      void run();
      return;
    }
    setPrompt({
      title: prefs.useTrash
        ? `Move ${items.length} item${items.length > 1 ? "s" : ""} to Trash?`
        : `Type DELETE to permanently remove ${items.length} item${items.length > 1 ? "s" : ""}`,
      label: items.map((i) => i.name).join(", "),
      value: "",
      okLabel: prefs.useTrash ? "Move to Trash" : "Delete",
      danger: true,
      requireExact: prefs.useTrash ? undefined : "DELETE",
      onOk: run,
    });
  }, [prefs.confirmDelete, prefs.useTrash, refreshFiles, selectedEntries]);

  const renameSelected = useCallback(() => {
    const item = selectedEntries()[0];
    if (!item || item.source !== "local") return;
    setPrompt({
      title: "Rename",
      label: item.path,
      value: item.name,
      okLabel: "Rename",
      onOk: async (name) => {
        if (!name || name === item.name) return;
        const parent = (await api.parent(item.path)) || "";
        try {
          await api.rename(item.path, joinPath(parent, name));
          await refreshFiles();
        } catch (e) {
          setError(String(e));
        }
      },
    });
  }, [refreshFiles, selectedEntries]);

  const duplicateSelected = useCallback(async () => {
    const items = selectedEntries().filter((e) => e.source === "local");
    if (!items.length) return;
    setBusy(true);
    try {
      for (const item of items) {
        const parent = (await api.parent(item.path)) || "";
        await api.copy(item.path, joinPath(parent, duplicateName(item.name)));
      }
      await refreshFiles();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [refreshFiles, selectedEntries]);

  const newFolder = useCallback(() => {
    if (!active || active.kind !== "files") {
      setError("Create folders in a file location.");
      return;
    }
    if (active.source === "gdrive") {
      if (!active.accountId) {
        setError("Open a Google account before creating a folder.");
        return;
      }
      const accountId = active.accountId;
      const folderId = active.folderId;
      setPrompt({
        title: "New folder",
        label: active.title,
        value: "Untitled folder",
        okLabel: "Create",
        onOk: async (name) => {
          try {
            await api.mkdirDrive(accountId, folderId === "root" ? undefined : folderId, name);
            await refreshFiles();
          } catch (e) {
            setError(String(e));
          }
        },
      });
      return;
    }
    setPrompt({
      title: "New folder",
      label: active.path,
      value: "Untitled folder",
      okLabel: "Create",
      onOk: async (name) => {
        try {
          await api.mkdir(joinPath(active.path || "", name));
          await refreshFiles();
        } catch (e) {
          setError(String(e));
        }
      },
    });
  }, [active, refreshFiles]);

  /* ── keyboard ─────────────────────────────────────────────── */

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      const typing = el.tagName === "INPUT" || el.tagName === "TEXTAREA";
      const meta = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();

      // Inside the code workspace, Monaco and the terminal own the keyboard;
      // the file-manager shortcuts would otherwise steal ⌘W, ⌘R and friends.
      if (el.closest?.(".code-workspace") && e.key !== "Escape") return;

      if (e.key === "Escape") {
        if ((el as HTMLElement).closest?.(".player.is-full")) return;
        setMenu(null);
        setPrompt(null);
        setQuickLook(null);
        return;
      }
      if (!typing && key === "f" && !meta) {
        if ((el as HTMLElement).closest?.(".player")) return;
        const collapsed = prefs.sidebarOpen || prefs.inspectorOpen;
        setPrefs((p) => ({ ...p, sidebarOpen: !collapsed, inspectorOpen: !collapsed }));
        return;
      }
      if (meta && key === "t") {
        e.preventDefault();
        openLocal(active?.kind === "files" && active.source === "local" ? active.path || "" : places[0]?.path || "");
        return;
      }
      if (meta && key === "w") {
        e.preventDefault();
        if (active) closeTab(active.id);
        return;
      }
      if (meta && key === "l") {
        e.preventDefault();
        if (activeKind === "web") {
          const input = document.querySelector(".web-bar .input") as HTMLInputElement | null;
          input?.focus();
          input?.select();
          return;
        }
        openWebTab();
        return;
      }
      if (meta && key === "r") {
        e.preventDefault();
        void refreshFiles();
        return;
      }
      if (typing || activeKind !== "files") return;

      if (meta && key === "c") {
        setClipboard({ mode: "copy", items: selectedEntries() });
      } else if (meta && key === "x") {
        setClipboard({ mode: "cut", items: selectedEntries() });
      } else if (meta && key === "v") {
        void paste();
      } else if (meta && key === "a") {
        e.preventDefault();
        setSelected(visible.map((v) => v.path));
      } else if (key === "backspace" && meta) {
        removeSelected();
      } else if (key === "backspace") {
        void goUp();
      } else if (key === "enter") {
        const one = selectedEntries()[0];
        if (one) void openEntry(one);
      } else if (e.key === " ") {
        e.preventDefault();
        const one = selectedEntries()[0];
        if (one && !one.isDir) setQuickLook(one);
      } else if (e.key === "ArrowLeft" && meta) {
        stepHistory(-1);
      } else if (e.key === "ArrowRight" && meta) {
        stepHistory(1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("resize", close);
    };
  }, [menu]);

  /* ── derived view data ────────────────────────────────────── */

  const localPlaces = places.filter((p) => p.kind !== "volume");
  const volumes = places.filter((p) => p.kind === "volume");
  const editorTabs = tabs.filter((t) => t.kind === "editor");
  const selection = selectedEntries();
  const primary = selection[0] ?? null;
  const crumbs =
    active?.kind === "files" && active.source === "local" && active.path
      ? active.path.split(/[/\\]/).filter(Boolean)
      : [];
  const runningTransfers = transfers.filter((t) => t.state === "running" || t.state === "queued");
  const activeTorrents = torrents.filter((t) => t.progress < 1);
  const torrentSpeed = torrents.reduce((sum, t) => sum + t.downloadSpeed, 0);

  const tabIcon = (t: Tab): IconName => {
    if (t.kind === "files") return t.source === "gdrive" ? "cloud" : "folder";
    if (t.kind === "preview") return chromeIconFor({ isDir: false, ext: extOf(t.title) });
    if (t.kind === "editor") return "code";
    if (t.kind === "web") return "globe";
    if (t.kind === "app" && t.app) return SOCIAL_APPS[t.app].icon;
    if (t.kind === "torrents") return "magnet";
    if (t.kind === "drives") return "cloud";
    if (t.kind === "transfers") return "arrows";
    return "gear";
  };

  const contextRows: Array<{ k: string; v: string }> = (() => {
    switch (activeKind) {
      case "web":
        return [
          { k: "URL", v: active?.url || "" },
          { k: "Engine", v: "Native child webview" },
          { k: "Session", v: "System webview cookies" },
        ];
      case "app":
        return [
          { k: "App", v: active?.title || "" },
          { k: "Engine", v: "Native child webview" },
          { k: "Session", v: "Persistent system webview cookies" },
          { k: "Current page", v: active?.url || "" },
        ];
      case "torrents":
        return [
          { k: "Engine", v: "librqbit" },
          { k: "Active", v: String(activeTorrents.length) },
          { k: "Down", v: formatSpeed(torrentSpeed) },
          { k: "Save to", v: settings.torrentDownloadDir || "System Downloads" },
        ];
      case "drives":
        return [
          { k: "Accounts", v: `${accounts.length} connected` },
          { k: "Volumes", v: String(volumes.length) },
          { k: "Auth", v: "Browser sign-in · PKCE" },
        ];
      case "transfers":
        return [
          { k: "Active", v: String(transfers.filter((t) => t.state === "running").length) },
          { k: "Queued", v: String(transfers.filter((t) => t.state === "queued").length) },
          { k: "Done", v: String(transfers.filter((t) => t.state === "done").length) },
          { k: "Failed", v: String(transfers.filter((t) => t.state === "error").length) },
        ];
      case "settings":
        return [
          { k: "Bundle", v: "com.depot.files" },
          { k: "Shell", v: "Tauri 2 · React 19" },
          { k: "Config", v: "app data · settings.json" },
        ];
      case "editor":
        return [
          { k: "Workspace", v: active?.path || "" },
          { k: "Unsaved", v: String(editorDirty[active?.id || ""] ?? 0) },
          { k: "Editor", v: "Monaco" },
          { k: "Terminal", v: "Login shell on a pty" },
          { k: "Save", v: "⌘/Ctrl S · ⇧ for all" },
        ];
      case "preview":
        return [
          { k: "Name", v: active?.title || "" },
          { k: "Source", v: active?.source === "gdrive" ? "Google Drive (cached)" : "Local" },
          { k: "Kind", v: kindLabel({ isDir: false, ext: extOf(active?.title || "") }) },
          { k: "Path", v: active?.path || "" },
        ];
      default:
        return [];
    }
  })();

  const webChrome = activeKind === "web" || activeKind === "app";
  const panelToggles = (
    <div className="panel-toggles">
      <button
        className={prefs.sidebarOpen ? "btn btn-ghost btn-icon on" : "btn btn-ghost btn-icon"}
        aria-label="Toggle sidebar"
        title="Toggle sidebar"
        onClick={() => setPref("sidebarOpen", !prefs.sidebarOpen)}
      >
        <Icon name="panelLeft" size={17} />
      </button>
      <button
        className={prefs.inspectorOpen ? "btn btn-ghost btn-icon on" : "btn btn-ghost btn-icon"}
        aria-label="Toggle inspector"
        title="Toggle inspector"
        onClick={() => setPref("inspectorOpen", !prefs.inspectorOpen)}
      >
        <Icon name="panelRight" size={17} />
      </button>
      <button
        className="btn btn-ghost btn-icon"
        aria-label="Focus mode"
        title="Focus mode (F)"
        onClick={() => {
          const collapsed = prefs.sidebarOpen || prefs.inspectorOpen;
          setPrefs((p) => ({ ...p, sidebarOpen: !collapsed, inspectorOpen: !collapsed }));
        }}
      >
        <Icon name="expand" size={17} />
      </button>
    </div>
  );

  /* ── render ───────────────────────────────────────────────── */

  return (
    <div className="app">
      {/* One bar: the window's own traffic lights sit inside this row (overlay
          title bar), so the app never stacks two headers. */}
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          Depot
        </div>
        <div className="tagline" data-tauri-drag-region>
          Your files, wherever they live
        </div>
        <div className="spacer" data-tauri-drag-region />
        <button
          className="theme-toggle"
          title="Toggle theme"
          onClick={() => setPref("theme", prefs.theme === "dark" ? "light" : "dark")}
        >
          <Icon name={prefs.theme === "dark" ? "moon" : "sun"} size={14} />
          <span>{prefs.theme === "dark" ? "Dark" : "Light"}</span>
        </button>
      </header>

      <div className="body">
        {prefs.sidebarOpen && (
        <nav className="sidebar">
          <SideGroup label="Places" hint="Local">
            {localPlaces.map((p) => (
              <SideItem
                key={p.path}
                icon={PLACE_ICONS[p.kind] ?? "folder"}
                label={p.name}
                on={activeKind === "files" && active?.source === "local" && active.path === p.path}
                onClick={() => {
                  if (active?.kind === "files" && active.source === "local") navigate(p.path, p.name);
                  else openLocal(p.path, p.name);
                }}
              />
            ))}
          </SideGroup>

          <SideGroup label="Volumes" hint={String(volumes.length)}>
            {volumes.map((p) => (
              <SideItem
                key={p.path}
                icon="disk"
                label={p.name}
                on={activeKind === "files" && active?.path === p.path}
                onClick={() => {
                  if (active?.kind === "files" && active.source === "local") navigate(p.path, p.name);
                  else openLocal(p.path, p.name);
                }}
              />
            ))}
            {!volumes.length && <div className="side-item text-muted">No extra volumes</div>}
          </SideGroup>

          <SideGroup label="Cloud drives" hint={String(accounts.length)}>
            {accounts.map((a) => (
              <SideItem
                key={a.id}
                icon="cloud"
                label={a.email}
                badge="Drive"
                on={activeKind === "files" && active?.accountId === a.id}
                onClick={() => openDrive(a)}
              />
            ))}
            <SideItem icon="bucket" label="Connections" onClick={() => openToolTab("drives", "Connections")} on={activeKind === "drives"} />
          </SideGroup>

          <SideGroup label="Social apps" hint="2">
            <SideItem
              icon="facebook"
              label="Facebook"
              badge="App"
              tone="facebook"
              on={activeKind === "app" && active?.app === "facebook"}
              onClick={() => openSocialApp("facebook")}
            />
            <SideItem
              icon="instagram"
              label="Instagram"
              badge="App"
              tone="instagram"
              on={activeKind === "app" && active?.app === "instagram"}
              onClick={() => openSocialApp("instagram")}
            />
          </SideGroup>

          <SideGroup label="Workspace" hint={editorTabs.length ? String(editorTabs.length) : "Code"}>
            <SideItem
              icon="folderOpen"
              label="Open folder…"
              onClick={() =>
                void pickWorkspace(
                  active?.kind === "editor"
                    ? active.path
                    : active?.kind === "files" && active.source === "local"
                      ? active.path
                      : places[0]?.path,
                )
              }
            />
            <SideItem
              icon="code"
              label="Edit this folder"
              on={activeKind === "editor"}
              onClick={() =>
                openEditor(
                  active?.kind === "files" && active.source === "local" && active.path
                    ? active.path
                    : places[0]?.path || "",
                )
              }
            />
            {editorTabs.map((t) => (
              <SideItem
                key={t.id}
                icon="code"
                label={t.title}
                badge={editorDirty[t.id] ? String(editorDirty[t.id]) : ""}
                on={active?.id === t.id}
                onClick={() => setActiveId(t.id)}
              />
            ))}
          </SideGroup>

          <SideGroup label="Activity" hint="">
            <SideItem
              icon="magnet"
              label="Torrents"
              badge={activeTorrents.length ? String(activeTorrents.length) : ""}
              on={activeKind === "torrents"}
              onClick={() => openToolTab("torrents", "Downloads")}
            />
            <SideItem
              icon="arrows"
              label="Transfers"
              badge={runningTransfers.length ? String(runningTransfers.length) : ""}
              on={activeKind === "transfers"}
              onClick={() => openToolTab("transfers", "Transfers")}
            />
            <SideItem icon="globe" label="Open website" onClick={() => openWebTab()} />
            <SideItem icon="gear" label="Settings" on={activeKind === "settings"} onClick={() => openToolTab("settings", "Settings")} />
          </SideGroup>
        </nav>
        )}

        <div className="workspace">
          <div className="tabstrip">
            {tabs.map((t) => (
              <div
                key={t.id}
                className={`${t.id === active?.id ? "tab on" : "tab"}${t.kind === "app" && t.app ? ` tab-app ${t.app}` : ""}`}
                onClick={() => setActiveId(t.id)}
              >
                <Icon name={tabIcon(t)} size={15} />
                <span className="title">{t.title}</span>
                <button
                  className="close"
                  aria-label={`Close ${t.title}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    closeTab(t.id);
                  }}
                >
                  <Icon name="close" size={13} />
                </button>
              </div>
            ))}
            <button
              className="tab-add"
              aria-label="New tab"
              onClick={() =>
                openLocal(
                  active?.kind === "files" && active.source === "local"
                    ? active.path || ""
                    : places[0]?.path || "",
                )
              }
            >
              <Icon name="plus" />
            </button>
          </div>

          {!webChrome && (
          <div className="toolbar">
            <div className="nav-btns">
              <button
                className="btn btn-ghost btn-icon"
                aria-label="Back"
                disabled={!active || active.historyIndex <= 0}
                onClick={() => stepHistory(-1)}
              >
                <Icon name="back" size={17} />
              </button>
              <button
                className="btn btn-ghost btn-icon"
                aria-label="Forward"
                disabled={!active || active.historyIndex >= active.history.length - 1}
                onClick={() => stepHistory(1)}
              >
                <Icon name="forward" size={17} />
              </button>
              <button
                className="btn btn-ghost btn-icon"
                aria-label="Up"
                disabled={activeKind !== "files"}
                onClick={() => void goUp()}
              >
                <Icon name="up" size={17} />
              </button>
            </div>

            <div className="crumbs">
              <div className="track">
                {activeKind === "files" && active?.source === "local" ? (
                  crumbs.map((c, i) => (
                    <span key={`${c}-${i}`}>
                      {i > 0 && <span className="sep"> › </span>}
                      <button
                        className={i === crumbs.length - 1 ? "crumb last" : "crumb"}
                        onClick={() => {
                          const sepChar = active.path?.includes("\\") ? "\\" : "/";
                          const prefix = active.path?.startsWith("/") ? "/" : "";
                          navigate(prefix + crumbs.slice(0, i + 1).join(sepChar), c);
                        }}
                      >
                        {c}
                      </button>
                    </span>
                  ))
                ) : (
                  <span>{active?.kind === "web" ? active.url : active?.title}</span>
                )}
              </div>
            </div>

            {activeKind === "files" && (
              <>
                <div className="seg">
                  <button
                    className={prefs.view === "grid" ? "seg-opt on" : "seg-opt"}
                    onClick={() => setPref("view", "grid")}
                  >
                    Grid
                  </button>
                  <button
                    className={prefs.view === "list" ? "seg-opt on" : "seg-opt"}
                    onClick={() => setPref("view", "list")}
                  >
                    List
                  </button>
                </div>
                <input
                  className="input"
                  style={{ width: 232 }}
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Search this place"
                />
                <button className="btn btn-secondary" onClick={newFolder}>
                  New folder
                </button>
              </>
            )}
            {activeKind !== "files" && active && (
              <span className="tag tag-accent-2">
                {activeKind === "torrents"
                  ? `${activeTorrents.length} active`
                  : activeKind === "transfers"
                    ? `${runningTransfers.length} in flight`
                    : activeKind === "drives"
                      ? `${accounts.length} accounts`
                      : activeKind === "settings"
                        ? "Depot 0.1.0"
                        : activeKind === "editor"
                          ? `${editorDirty[active.id] ?? 0} unsaved`
                          : kindLabel({ isDir: false, ext: extOf(active.title) })}
              </span>
            )}
            {panelToggles}
          </div>
          )}

          <div className="stage">
            <main
              className={
                activeKind === "preview" ||
                activeKind === "editor" ||
                activeKind === "web" ||
                activeKind === "app"
                  ? "content flush"
                  : "content"
              }
            >
              {activeKind === "files" && (
                <>
                  <div className="content-head">
                    <div className="heading">{query ? "Search results" : active?.title}</div>
                    <div className="text-muted">
                      {query
                        ? `${visible.length} of ${entries.length} match “${query}”`
                        : `${visible.length} item${visible.length === 1 ? "" : "s"}${
                            active?.source === "gdrive" ? " · Google Drive" : ""
                          }`}
                    </div>
                  </div>

                  {!visible.length && (
                    <div className="empty">{busy ? "Loading…" : "Nothing here"}</div>
                  )}

                  {prefs.view === "grid" ? (
                    <div className="grid">
                      {visible.map((f) => (
                        <button
                          key={f.path}
                          className={selected.includes(f.path) ? "file-card sel" : "file-card"}
                          onClick={(e) => selectEntry(e, f)}
                          onDoubleClick={() => void openEntry(f)}
                          onContextMenu={(e) => {
                            e.preventDefault();
                            if (!selected.includes(f.path)) setSelected([f.path]);
                            setMenu({ x: e.clientX, y: e.clientY, entry: f });
                          }}
                        >
                          <div className="thumb">
                            {f.source === "local" && viewerKind(f.ext) === "image" ? (
                              <img src={fileUrl(f.path)} alt="" loading="lazy" />
                            ) : (
                              <FileIcon kind={iconFor(f)} size={46} />
                            )}
                            <span className="ext">{f.isDir ? "DIR" : f.ext.toUpperCase()}</span>
                          </div>
                          <div className="name">{f.name}</div>
                          <div className="meta">
                            <span>{f.isDir ? "—" : formatBytes(f.size)}</span>
                            <span className={f.source === "local" ? "src" : "src src-remote"}>
                              {f.source === "local" ? "Local" : "Drive"}
                            </span>
                          </div>
                        </button>
                      ))}
                    </div>
                  ) : (
                    <table className="table">
                      <thead>
                        <tr>
                          <th>Name</th>
                          <th>Kind</th>
                          <th style={{ textAlign: "right" }}>Size</th>
                          <th>Modified</th>
                          <th>Source</th>
                        </tr>
                      </thead>
                      <tbody>
                        {visible.map((f) => (
                          <tr
                            key={f.path}
                            className={selected.includes(f.path) ? "sel" : ""}
                            style={{ cursor: "pointer" }}
                            onClick={(e) => selectEntry(e, f)}
                            onDoubleClick={() => void openEntry(f)}
                            onContextMenu={(e) => {
                              e.preventDefault();
                              if (!selected.includes(f.path)) setSelected([f.path]);
                              setMenu({ x: e.clientX, y: e.clientY, entry: f });
                            }}
                          >
                            <td className="name-cell">
                              <FileIcon kind={iconFor(f)} size={18} />
                              {f.name}
                            </td>
                            <td>{kindLabel(f)}</td>
                            <td style={{ textAlign: "right" }}>{f.isDir ? "—" : formatBytes(f.size)}</td>
                            <td>{formatDate(f.modified)}</td>
                            <td>{f.source === "local" ? "Local" : "Drive"}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </>
              )}

              {activeKind === "preview" && active && (
                <PreviewPane
                  key={active.id}
                  title={active.title}
                  path={active.path || ""}
                  source={active.source ?? "local"}
                  parked={!!prompt || !!quickLook}
                  onEdit={
                    active.source === "gdrive" || !active.path
                      ? undefined
                      : () => openEditor(parentDir(active.path || ""), active.path)
                  }
                  onError={setError}
                />
              )}

              {/* Editor tabs stay mounted while another tab is on screen: an
                  unsaved buffer or a running shell must survive a tab click. */}
              {editorTabs.map((t) => (
                <div
                  key={t.id}
                  className="editor-host"
                  style={{ display: t.id === active?.id ? undefined : "none" }}
                >
                  <Suspense fallback={<div className="empty">Loading editor…</div>}>
                  <CodeEditor
                    root={t.path || ""}
                    initialFile={t.file}
                    openRequest={editorRequests[t.id]}
                    theme={prefs.theme}
                    visible={t.id === active?.id && !prompt && !quickLook}
                    showHidden={prefs.showHidden}
                    onError={setError}
                    onDirtyCount={(count) =>
                      setEditorDirty((all) => (all[t.id] === count ? all : { ...all, [t.id]: count }))
                    }
                    onPrompt={(options) => setPrompt({ ...options })}
                    onOpenFolder={() => void pickWorkspace(t.path)}
                    onChangeFolder={() => void rerootEditor(t.id, t.path)}
                  />
                  </Suspense>
                </div>
              ))}

              {activeKind === "web" && active && (
                <WebPane
                  key={active.id}
                  label={active.id}
                  url={active.url || ""}
                  parked={!!prompt || !!quickLook}
                  chrome={panelToggles}
                  onUrl={(url) => patchTab(active.id, { url, title: url.replace(/^https?:\/\//, "") })}
                  onError={setError}
                />
              )}

              {activeKind === "app" && active?.app && (
                <WebPane
                  key={active.id}
                  label={active.id}
                  url={active.url || SOCIAL_APPS[active.app].url}
                  app={active.app}
                  parked={!!prompt || !!quickLook}
                  chrome={panelToggles}
                  onUrl={(url) => patchTab(active.id, { url })}
                  onError={setError}
                />
              )}

              {activeKind === "torrents" && (
                <div className="stack">
                  <form
                    style={{ display: "flex", alignItems: "flex-end", gap: 10, flexWrap: "wrap" }}
                    onSubmit={async (e) => {
                      e.preventDefault();
                      setBusy(true);
                      try {
                        await api.addTorrent(magnet.trim());
                        setMagnet("");
                        setTorrents(await api.torrents());
                      } catch (err) {
                        setError(String(err));
                      } finally {
                        setBusy(false);
                      }
                    }}
                  >
                    <div className="field" style={{ flex: 1, minWidth: 260 }}>
                      <label>Magnet link, .torrent URL or local file</label>
                      <input
                        className="input"
                        value={magnet}
                        onChange={(e) => setMagnet(e.target.value)}
                        placeholder="magnet:?xt=urn:btih:…"
                      />
                    </div>
                    <button className="btn btn-primary" type="submit" disabled={!magnet.trim()}>
                      Add download
                    </button>
                  </form>

                  <div className="notice">
                    <Icon name="warn" size={18} />
                    <span>
                      Downloads only for content you hold the rights to. Depot ships no search or
                      indexer — you supply the link.
                    </span>
                  </div>

                  <div className="stack" style={{ gap: 12 }}>
                    {!torrents.length && <div className="empty">No downloads yet</div>}
                    {torrents.map((t) => {
                      const pct = Math.round(t.progress * 100);
                      const done = t.progress >= 1;
                      return (
                        <div className="row-card" key={t.id}>
                          <div className="top">
                            <span className="name">{t.name}</span>
                            <span className={done ? "tag tag-accent-2" : "tag tag-accent"}>
                              {t.error ? "Error" : done ? "Complete" : t.state}
                            </span>
                          </div>
                          <div className="progress">
                            <i
                              style={{
                                width: `${pct}%`,
                                background: done ? "var(--color-accent-2)" : "var(--color-accent)",
                              }}
                            />
                          </div>
                          <div className="facts">
                            <span>{pct}%</span>
                            <span>
                              {formatBytes(t.downloaded)} of {formatBytes(t.total)}
                            </span>
                            <span>↓ {formatSpeed(t.downloadSpeed)}</span>
                            {t.error && <span>{t.error}</span>}
                            <div className="spacer" />
                            <button className="link" onClick={() => void api.pauseTorrent(t.id)}>
                              Pause
                            </button>
                            <button className="link" onClick={() => void api.resumeTorrent(t.id)}>
                              Resume
                            </button>
                            <button className="link" onClick={() => openLocal(t.outputFolder, baseName(t.outputFolder))}>
                              Show folder
                            </button>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}

              {activeKind === "drives" && (
                <Connections
                  accounts={accounts}
                  driveQuota={driveQuota}
                  places={places.filter((place) => place.kind === "volume" || place.kind === "home")}
                  settings={settings}
                  volumeUsage={volumeUsage}
                  onConfigure={() => openToolTab("settings", "Settings")}
                  onConnectGoogle={async () => {
                    setError("");
                    await api.saveSettings(settings);
                    await api.connectDrive();
                    await loadPlaces();
                  }}
                  onDisconnectGoogle={async (accountId) => {
                    await api.disconnectDrive(accountId);
                    await loadPlaces();
                  }}
                  onOpenGoogle={openDrive}
                  onOpenLocal={(place) => openLocal(place.path, place.name)}
                  onOpenWeb={openWebTab}
                  onOpenSocial={openSocialApp}
                  onError={setError}
                />
              )}

              {activeKind === "transfers" && (
                <div className="stack">
                  <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
                    <div className="heading" style={{ fontSize: 26 }}>
                      Transfers
                    </div>
                    <div className="spacer" />
                    <button
                      className="btn btn-secondary"
                      onClick={() => setTransfers((all) => all.filter((t) => t.state !== "done"))}
                    >
                      Clear finished
                    </button>
                  </div>
                  {!transfers.length && <div className="empty">No copies or downloads yet</div>}
                  <div className="stack" style={{ gap: 12 }}>
                    {transfers.map((t) => {
                      const pct = t.total ? Math.min(100, Math.round((t.moved / t.total) * 100)) : t.state === "done" ? 100 : 0;
                      const color =
                        t.state === "error"
                          ? "var(--color-neutral-400)"
                          : t.state === "done"
                            ? "var(--color-accent-2)"
                            : "var(--color-accent)";
                      return (
                        <div className="row-card" key={t.id}>
                          <div className="top">
                            <span className="name">{t.name}</span>
                            <span className="tag tag-outline">{t.route}</span>
                          </div>
                          <div className="progress">
                            <i style={{ width: `${pct}%`, background: color }} />
                          </div>
                          <div className="facts">
                            <span>{pct}%</span>
                            <span>
                              {formatBytes(t.moved)}
                              {t.total ? ` of ${formatBytes(t.total)}` : ""}
                            </span>
                            <span>{t.state === "running" ? formatSpeed(t.speed) : "—"}</span>
                            <span>{t.error ? t.error : t.state}</span>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}

              {activeKind === "settings" && (
                <div className="stack settings-page">
                  <div>
                    <div className="settings-title-row">
                      <div>
                        <div className="heading">Provider credentials</div>
                        <div className="text-muted">Saved in Depot's app-data settings, never in the repository.</div>
                      </div>
                      <button
                        className="btn btn-primary"
                        onClick={async () => {
                          try {
                            await api.saveSettings(settings);
                            setError("");
                          } catch (e) {
                            setError(String(e));
                          }
                        }}
                      >
                        Save provider settings
                      </button>
                    </div>
                    <div className="provider-settings-grid">
                      <div className="settings-provider">
                        <ProviderSettingsHeading mark="G" tone="google" title="Google Drive" />
                        <div className="field">
                          <label>Client ID</label>
                          <input
                            className="input"
                            value={settings.googleClientId}
                            onChange={(e) => setSettings({ ...settings, googleClientId: e.target.value })}
                            placeholder="…apps.googleusercontent.com"
                          />
                        </div>
                        <div className="field">
                          <label>Client secret</label>
                          <input
                            className="input"
                            type="password"
                            value={settings.googleClientSecret}
                            onChange={(e) => setSettings({ ...settings, googleClientSecret: e.target.value })}
                          />
                        </div>
                        <p>Create a Desktop OAuth client, enable Drive API, then add <code>http://127.0.0.1:17843/callback</code>.</p>
                      </div>

                      <div className="settings-provider">
                        <ProviderSettingsHeading mark="M" tone="microsoft" title="OneDrive" />
                        <div className="field">
                          <label>Application (client) ID</label>
                          <input
                            className="input"
                            value={settings.oneDriveClientId}
                            onChange={(e) => setSettings({ ...settings, oneDriveClientId: e.target.value })}
                          />
                        </div>
                        <div className="field">
                          <label>Client secret</label>
                          <input
                            className="input"
                            type="password"
                            value={settings.oneDriveClientSecret}
                            onChange={(e) => setSettings({ ...settings, oneDriveClientSecret: e.target.value })}
                          />
                        </div>
                        <p>Register a desktop/public client in Microsoft Entra. Native OneDrive file browsing is the next adapter.</p>
                      </div>

                      <div className="settings-provider">
                        <ProviderSettingsHeading mark="D" tone="dropbox" title="Dropbox" />
                        <div className="field">
                          <label>App key / client ID</label>
                          <input
                            className="input"
                            value={settings.dropboxClientId}
                            onChange={(e) => setSettings({ ...settings, dropboxClientId: e.target.value })}
                          />
                        </div>
                        <div className="field">
                          <label>App secret</label>
                          <input
                            className="input"
                            type="password"
                            value={settings.dropboxClientSecret}
                            onChange={(e) => setSettings({ ...settings, dropboxClientSecret: e.target.value })}
                          />
                        </div>
                        <p>Create a scoped Dropbox app with file read access. Native Dropbox file browsing is the next adapter.</p>
                      </div>

                      <div className="settings-provider settings-provider-wide">
                        <ProviderSettingsHeading icon="bucket" title="Amazon S3 / S3-compatible" />
                        <div className="s3-settings-grid">
                          <div className="field">
                            <label>Endpoint (optional)</label>
                            <input
                              className="input"
                              value={settings.s3Endpoint}
                              onChange={(e) => setSettings({ ...settings, s3Endpoint: e.target.value })}
                              placeholder="https://s3.amazonaws.com"
                            />
                          </div>
                          <div className="field">
                            <label>Region</label>
                            <input
                              className="input"
                              value={settings.s3Region}
                              onChange={(e) => setSettings({ ...settings, s3Region: e.target.value })}
                              placeholder="us-east-1"
                            />
                          </div>
                          <div className="field">
                            <label>Bucket</label>
                            <input
                              className="input"
                              value={settings.s3Bucket}
                              onChange={(e) => setSettings({ ...settings, s3Bucket: e.target.value })}
                            />
                          </div>
                          <div className="field">
                            <label>Access key ID</label>
                            <input
                              className="input"
                              value={settings.s3AccessKeyId}
                              onChange={(e) => setSettings({ ...settings, s3AccessKeyId: e.target.value })}
                            />
                          </div>
                          <div className="field">
                            <label>Secret access key</label>
                            <input
                              className="input"
                              type="password"
                              value={settings.s3SecretAccessKey}
                              onChange={(e) => setSettings({ ...settings, s3SecretAccessKey: e.target.value })}
                            />
                          </div>
                        </div>
                        <p>Prefer a least-privilege IAM user restricted to the selected bucket. S3 does not use social OAuth.</p>
                      </div>
                    </div>
                  </div>

                  <div>
                    <div className="heading" style={{ fontSize: 24, marginBottom: 14 }}>
                      Places
                    </div>
                    <div className="settings-section">
                      <div className="field">
                        <label>Torrent download folder</label>
                        <input
                          className="input"
                          value={settings.torrentDownloadDir}
                          onChange={(e) =>
                            setSettings({ ...settings, torrentDownloadDir: e.target.value })
                          }
                          placeholder="Leave empty for the system Downloads folder"
                        />
                      </div>
                    </div>
                  </div>

                  <div>
                    <div className="heading" style={{ fontSize: 24, marginBottom: 14 }}>
                      Behaviour
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                      <Toggle
                        label="Show hidden files"
                        on={prefs.showHidden}
                        onClick={() => setPref("showHidden", !prefs.showHidden)}
                      />
                      <Toggle
                        label="Move to Trash instead of deleting"
                        on={prefs.useTrash}
                        onClick={() => setPref("useTrash", !prefs.useTrash)}
                      />
                      <Toggle
                        label="Ask before removing files"
                        on={prefs.confirmDelete}
                        onClick={() => setPref("confirmDelete", !prefs.confirmDelete)}
                      />
                      <Toggle
                        label="Hand unsupported codecs to the system player"
                        on={prefs.systemFallback}
                        onClick={() => setPref("systemFallback", !prefs.systemFallback)}
                      />
                    </div>
                  </div>
                </div>
              )}
            </main>

            {prefs.inspectorOpen && (
            <aside className="inspector">
              <div className="inspector-label">{activeKind === "files" ? "Details" : active?.title}</div>

              {activeKind === "files" && primary && (
                <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
                  <button
                    className="inspector-hero"
                    onClick={() => !primary.isDir && setQuickLook(primary)}
                    title={primary.isDir ? primary.name : "Quick look (Space)"}
                  >
                    {primary.source === "local" && viewerKind(primary.ext) === "image" ? (
                      <img src={fileUrl(primary.path)} alt={primary.name} />
                    ) : (
                      <FileIcon kind={iconFor(primary)} size={62} />
                    )}
                  </button>
                  <div>
                    <div className="heading" style={{ fontSize: 17, wordBreak: "break-word" }}>
                      {primary.name}
                    </div>
                    <div className="text-muted" style={{ fontSize: 13, marginTop: 6 }}>
                      {kindLabel(primary)} · {primary.isDir ? "—" : formatBytes(primary.size)}
                    </div>
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                    <Row k="Kind" v={kindLabel(primary)} />
                    <Row k="Size" v={primary.isDir ? "—" : formatBytes(primary.size)} />
                    <Row k="Modified" v={formatDate(primary.modified)} />
                    <Row k="Source" v={primary.source === "local" ? "Local disk" : "Google Drive"} />
                    <Row k="Path" v={primary.path} />
                    {selection.length > 1 && <Row k="Selected" v={`${selection.length} items`} />}
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
                    <button className="btn btn-primary btn-block" onClick={() => void openEntry(primary)}>
                      {primary.isDir ? "Open folder" : "Open in tab"}
                    </button>
                    {!primary.isDir && (
                      <button className="btn btn-secondary btn-block" onClick={() => setQuickLook(primary)}>
                        Quick look
                      </button>
                    )}
                    {primary.source === "local" && (
                      <button
                        className="btn btn-secondary btn-block"
                        onClick={() =>
                          primary.isDir
                            ? openEditor(primary.path)
                            : openEditor(active?.path || parentDir(primary.path), primary.path)
                        }
                      >
                        {primary.isDir ? "Open folder in editor" : "Edit code"}
                      </button>
                    )}
                    {!primary.isDir && primary.source === "local" && (
                      <OpenWithMenu path={primary.path} onError={setError} variant="bar" />
                    )}
                    <button className="btn btn-secondary btn-block" onClick={() => copyToLocal(selection)}>
                      Copy to local…
                    </button>
                    {primary.source === "local" && (
                      <button
                        className="btn btn-secondary btn-block"
                        onClick={() => void api.reveal(primary.path).catch((e) => setError(String(e)))}
                      >
                        Reveal in {REVEAL_IN}
                      </button>
                    )}
                  </div>
                </div>
              )}

              {activeKind === "files" && !primary && (
                <div className="empty" style={{ padding: "20px 0" }}>
                  Select an item to inspect it
                </div>
              )}

              {activeKind !== "files" && (
                <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  {contextRows.map((r) => (
                    <Row key={r.k} k={r.k} v={r.v} />
                  ))}
                </div>
              )}

              <div className="inspector-app-section">
                <div className="inspector-section-label">Social apps</div>
                <button
                  className={activeKind === "app" && active?.app === "facebook" ? "inspector-app on" : "inspector-app"}
                  onClick={() => openSocialApp("facebook")}
                >
                  <span className="inspector-app-mark facebook"><Icon name="facebook" size={17} /></span>
                  <span><strong>Facebook</strong><small>Open app tab</small></span>
                  <Icon name="forward" size={15} />
                </button>
                <button
                  className={activeKind === "app" && active?.app === "instagram" ? "inspector-app on" : "inspector-app"}
                  onClick={() => openSocialApp("instagram")}
                >
                  <span className="inspector-app-mark instagram"><Icon name="instagram" size={17} /></span>
                  <span><strong>Instagram</strong><small>Open app tab</small></span>
                  <Icon name="forward" size={15} />
                </button>
              </div>
            </aside>
            )}
          </div>
        </div>
      </div>

      <footer className="statusbar">
        <span>
          {busy
            ? "Working…"
            : activeKind === "files"
              ? `${visible.length} items shown${selection.length ? ` · ${selection.length} selected` : ""}`
              : active?.title}
        </span>
        {usage && (
          <span>
            {formatBytes(usage.free)} free of {formatBytes(usage.total)}
          </span>
        )}
        {clipboard && (
          <span>
            Clipboard: {clipboard.mode} {clipboard.items.length}
          </span>
        )}
        <div className="spacer" />
        {!!transfers.length && (
          <button className="link" onClick={() => openToolTab("transfers", "Transfers")}>
            {runningTransfers.length
              ? `${runningTransfers.length} transfer${runningTransfers.length > 1 ? "s" : ""} running`
              : `${transfers.length} transfers`}
          </button>
        )}
        {!!torrents.length && (
          <button className="link" onClick={() => openToolTab("torrents", "Downloads")}>
            {activeTorrents.length} torrent{activeTorrents.length === 1 ? "" : "s"} · ↓{" "}
            {formatSpeed(torrentSpeed)}
          </button>
        )}
        {error && <span className="err" title={error}>{error}</span>}
      </footer>

      {menu && (
        <div className="ctx" style={{ left: menu.x, top: menu.y }} onClick={(e) => e.stopPropagation()}>
          <button onClick={() => { setMenu(null); if (menu.entry) void openEntry(menu.entry); }}>
            {menu.entry?.isDir ? "Open folder" : "Open in tab"}
          </button>
          <button
            disabled={!menu.entry || menu.entry.isDir}
            onClick={() => {
              const entry = menu.entry;
              setMenu(null);
              if (entry && !entry.isDir) setQuickLook(entry);
            }}
          >
            Quick look — Space
          </button>
          <button
            disabled={!menu.entry || menu.entry.source !== "local"}
            onClick={() => {
              const entry = menu.entry;
              setMenu(null);
              if (!entry || entry.source !== "local") return;
              if (entry.isDir) openEditor(entry.path);
              else openEditor(active?.path || parentDir(entry.path), entry.path);
            }}
          >
            {menu.entry?.isDir ? "Open folder in editor" : "Edit code"}
          </button>
          <button
            disabled={!menu.entry?.isDir || menu.entry.source !== "local"}
            onClick={() => {
              const entry = menu.entry;
              setMenu(null);
              if (entry?.isDir && entry.source === "local") openEditor(entry.path, undefined, true);
            }}
          >
            Open folder in a new editor tab
          </button>
          {menu.entry && !menu.entry.isDir && menu.entry.source === "local" && (
            <OpenWithMenu
              path={menu.entry.path}
              onError={setError}
              onPicked={() => setMenu(null)}
            />
          )}
          <hr />
          <button
            onClick={() => {
              setMenu(null);
              setPrefs((p) => ({ ...p, inspectorOpen: true }));
            }}
          >
            Get Info
          </button>
          <hr />
          <button onClick={() => { setMenu(null); setClipboard({ mode: "copy", items: selection }); }}>
            Copy
          </button>
          <button onClick={() => { setMenu(null); setClipboard({ mode: "cut", items: selection }); }}>
            Cut
          </button>
          <button disabled={!clipboard} onClick={() => { setMenu(null); void paste(); }}>
            Paste
          </button>
          <button onClick={() => { setMenu(null); copyToLocal(selection); }}>Copy to local…</button>
          <hr />
          <button disabled={selection.length !== 1} onClick={() => { setMenu(null); renameSelected(); }}>
            Rename
          </button>
          <button
            disabled={!selection.length || selection.some((e) => e.source !== "local")}
            onClick={() => {
              setMenu(null);
              void duplicateSelected();
            }}
          >
            Duplicate
          </button>
          <button
            disabled={!menu.entry || menu.entry.source !== "local"}
            onClick={() => {
              setMenu(null);
              if (menu.entry) void api.reveal(menu.entry.path).catch((e) => setError(String(e)));
            }}
          >
            Reveal in {REVEAL_IN}
          </button>
          <hr />
          <button className="btn-danger" onClick={() => { setMenu(null); removeSelected(); }}>
            {prefs.useTrash ? "Move to Trash" : "Delete"}
          </button>
        </div>
      )}

      {quickLook && (
        <div className="ql-backdrop" onClick={() => setQuickLook(null)}>
          <div className="ql" onClick={(e) => e.stopPropagation()}>
            <div className="ql-head">
              <FileIcon kind={iconFor(quickLook)} size={18} />
              <div className="ql-title">{quickLook.name}</div>
              <span className="tag tag-neutral">{kindLabel(quickLook)}</span>
              <span className="text-muted" style={{ fontSize: 12.5 }}>
                {formatBytes(quickLook.size)}
              </span>
              <div className="spacer" />
              <button
                className="btn btn-secondary"
                onClick={() => {
                  const entry = quickLook;
                  setQuickLook(null);
                  void openEntry(entry);
                }}
              >
                Open in tab
              </button>
              {quickLook.source === "local" && (
                <OpenWithMenu path={quickLook.path} onError={setError} variant="bar" />
              )}
              <button className="btn btn-primary" onClick={() => setQuickLook(null)}>
                Close
              </button>
            </div>
            <div className="ql-body">
              <PreviewPane
                key={quickLook.path}
                title={quickLook.name}
                path={quickLook.path}
                source={quickLook.source}
                onError={setError}
              />
            </div>
          </div>
        </div>
      )}

      {prompt && (
        <div className="dialog-backdrop" onClick={() => setPrompt(null)}>
          <form
            className="dialog"
            onClick={(e) => e.stopPropagation()}
            onSubmit={(e) => {
              e.preventDefault();
              if (prompt.requireExact && prompt.value !== prompt.requireExact) return;
              const fn = prompt.onOk;
              const value = prompt.value;
              setPrompt(null);
              void fn(value);
            }}
          >
            <div className="dialog-title">{prompt.title}</div>
            {prompt.label && <div className="dialog-body">{prompt.label}</div>}
            <input
              className="input"
              autoFocus
              value={prompt.value}
              onChange={(e) => setPrompt({ ...prompt, value: e.target.value })}
            />
            <div className="dialog-actions">
              <button className="btn btn-secondary" type="button" onClick={() => setPrompt(null)}>
                Cancel
              </button>
              <button
                className={prompt.danger ? "btn btn-primary btn-danger" : "btn btn-primary"}
                type="submit"
                disabled={!!prompt.requireExact && prompt.value !== prompt.requireExact}
              >
                {prompt.okLabel}
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );

  function selectEntry(e: ReactMouseEvent, f: DirEntry) {
    if (e.metaKey || e.ctrlKey) {
      setSelected((s) => (s.includes(f.path) ? s.filter((p) => p !== f.path) : [...s, f.path]));
      setAnchor(f.path);
    } else if (e.shiftKey && anchor) {
      const from = visible.findIndex((v) => v.path === anchor);
      const to = visible.findIndex((v) => v.path === f.path);
      if (from >= 0 && to >= 0) {
        const [a, b] = from < to ? [from, to] : [to, from];
        setSelected(visible.slice(a, b + 1).map((v) => v.path));
      }
    } else {
      setSelected([f.path]);
      setAnchor(f.path);
    }
  }
}

/* ── small pieces ───────────────────────────────────────────── */

function SideGroup({ label, hint, children }: { label: string; hint: string; children: ReactNode }) {
  return (
    <div className="side-group">
      <div className="side-head">
        <span>{label}</span>
        <span>{hint}</span>
      </div>
      <div className="side-items">{children}</div>
    </div>
  );
}

function SideItem({
  icon,
  label,
  badge,
  tone,
  on,
  onClick,
}: {
  icon: IconName;
  label: string;
  badge?: string;
  tone?: "facebook" | "instagram";
  on?: boolean;
  onClick?: () => void;
}) {
  return (
    <button className={`${on ? "side-item on" : "side-item"}${tone ? ` ${tone}` : ""}`} onClick={onClick}>
      <Icon name={icon} />
      <span className="label">{label}</span>
      {badge && <span className="badge">{badge}</span>}
    </button>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="kv">
      <span>{k}</span>
      <span>{v}</span>
    </div>
  );
}

function Toggle({ label, on, onClick }: { label: string; on: boolean; onClick: () => void }) {
  return (
    <button className="toggle-row" onClick={onClick} aria-pressed={on}>
      <span className="label">{label}</span>
      <span className={on ? "switch on" : "switch"}>
        <i />
      </span>
    </button>
  );
}

/**
 * Resolves a viewable URL for a file. Drive files are cached locally first, so
 * the same viewer works for both sources.
 */
function usePreviewSource(path: string, source: SourceKind, title: string, onError: (m: string) => void) {
  const [src, setSrc] = useState("");
  const [text, setText] = useState("");
  const report = useRef(onError);
  report.current = onError;

  useEffect(() => {
    if (!path) return;
    let cancelled = false;
    setSrc("");
    setText("");
    (async () => {
      try {
        const local = source === "gdrive" ? await api.cacheDrive(path, title) : path;
        if (cancelled) return;
        if (viewerKind(extOf(title)) === "text") setText(await api.readText(local));
        else setSrc(fileUrl(local));
      } catch (e) {
        if (!cancelled) report.current(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [path, source, title]);

  return { src, text };
}

/** Media, image, PDF and text viewer with the design's own transport controls. */
function PreviewPane({
  title,
  path,
  source,
  parked = false,
  onEdit,
  onError,
}: {
  title: string;
  path: string;
  source: SourceKind;
  parked?: boolean;
  /** Hands a text file to the code workspace; absent for Drive files. */
  onEdit?: () => void;
  onError: (message: string) => void;
}) {
  const ext = extOf(title);
  const kind = viewerKind(ext);
  const local = source !== "gdrive";
  const { src, text } = usePreviewSource(path, source, title, onError);

  if (kind === "video" || kind === "audio") {
    return (
      <MediaPlayer
        title={title}
        path={path}
        source={source}
        ext={ext}
        kind={kind}
        parked={parked}
        onError={onError}
      />
    );
  }

  if (kind === "spreadsheet" || kind === "document" || kind === "slides") {
    return <OfficePreview title={title} path={path} source={source} onError={onError} />;
  }

  return (
    <div className="doc">
      <div className="doc-head">
        <div className="heading">{title}</div>
        <div className="spacer" />
        {kind === "text" && onEdit && (
          <button className="btn btn-secondary" onClick={onEdit}>
            <Icon name="edit" size={15} />
            Edit
          </button>
        )}
        {local && <OpenWithMenu path={path} onError={onError} variant="bar" />}
      </div>
      <div className="doc-frame">
        {kind === "image" && src && <img src={src} alt={title} />}
        {kind === "pdf" && src && <iframe title={title} src={src} />}
        {kind === "text" && <pre>{text}</pre>}
        {kind === "unknown" && (
          <div className="empty">
            No built-in preview for this file type
            {local && (
              <div style={{ marginTop: 12 }}>
                <OpenWithMenu path={path} onError={onError} variant="bar" />
              </div>
            )}
          </div>
        )}
        {kind !== "text" && kind !== "unknown" && !src && <div className="empty">Loading…</div>}
      </div>
    </div>
  );
}

/**
 * A browser or dedicated web-app tab. The page runs in a real child webview
 * parked over this pane, so `X-Frame-Options` and `frame-ancestors` do not
 * decide what can open. App mode keeps the URL fixed behind branded chrome.
 */
function WebPane({
  label,
  url,
  app,
  parked,
  chrome,
  onUrl,
  onError,
}: {
  label: string;
  url: string;
  app?: SocialAppKind;
  parked: boolean;
  chrome?: ReactNode;
  onUrl: (url: string) => void;
  onError: (message: string) => void;
}) {
  const appConfig = app ? SOCIAL_APPS[app] : null;
  const holder = useRef<HTMLDivElement>(null);
  const [address, setAddress] = useState(url);
  const callbacks = useRef({ onUrl, onError });
  callbacks.current = { onUrl, onError };
  const addressRef = useRef(address);
  addressRef.current = address;
  const initialUrl = useRef(url);
  const editing = useRef(false);
  const pending = useRef<string | null>(null);

  const bounds = () => {
    const rect = holder.current?.getBoundingClientRect();
    return rect ? ([rect.left, rect.top, rect.width, rect.height] as const) : null;
  };

  useEffect(() => {
    const el = holder.current;
    if (!el) return;
    let alive = true;
    const box = el.getBoundingClientRect();
    api
      .webOpen(label, initialUrl.current, box.left, box.top, box.width, box.height)
      .then((resolved) => {
        if (!alive) return;
        setAddress(resolved);
        callbacks.current.onUrl(resolved);
      })
      .catch((e) => callbacks.current.onError(String(e)));

    const push = () => {
      const b = bounds();
      if (b) void api.webBounds(label, ...b).catch(() => undefined);
    };
    const observer = new ResizeObserver(push);
    observer.observe(el);
    window.addEventListener("resize", push);
    return () => {
      alive = false;
      observer.disconnect();
      window.removeEventListener("resize", push);
      void api.webHide(label).catch(() => undefined);
    };
  }, [label]);

  useEffect(() => {
    if (parked) {
      void api.webHide(label).catch(() => undefined);
      return;
    }
    const b = bounds();
    if (b) void api.webBounds(label, ...b).catch(() => undefined);
  }, [parked, label]);

  // Mirror in-page navigation into the bar, but never while the user is typing
  // and never while a typed Go is still landing (that used to snap back to archive.org).
  useEffect(() => {
    const id = setInterval(() => {
      if (editing.current) return;
      api
        .webUrl(label)
        .then((current) => {
          if (!current || current === addressRef.current) return;
          if (pending.current) {
            if (webUrlHost(current) === webUrlHost(pending.current)) {
              pending.current = null;
            } else {
              return;
            }
          }
          setAddress(current);
          callbacks.current.onUrl(current);
        })
        .catch(() => undefined);
    }, 1500);
    return () => clearInterval(id);
  }, [label]);

  const go = (raw: string) => {
    const target = normalizeWebUrl(raw);
    if (!target) return;
    editing.current = false;
    pending.current = target;
    addressRef.current = target;
    setAddress(target);
    window.setTimeout(() => {
      if (pending.current === target) pending.current = null;
    }, 10000);
    api
      .webNavigate(label, target)
      .then((resolved) => {
        pending.current = resolved;
        addressRef.current = resolved;
        setAddress(resolved);
        callbacks.current.onUrl(resolved);
      })
      .catch((e) => {
        pending.current = null;
        callbacks.current.onError(String(e));
      });
  };

  return (
    <div className={app ? `web web-app web-app-${app}` : "web"}>
      <form
        className={app ? "web-bar app-bar" : "web-bar"}
        onSubmit={(e) => {
          e.preventDefault();
          if (!app) go(address);
        }}
      >
        <button
          className="btn btn-ghost btn-icon"
          type="button"
          aria-label="Back"
          onClick={() => void api.webHistory(label, "back")}
        >
          <Icon name="back" size={17} />
        </button>
        <button
          className="btn btn-ghost btn-icon"
          type="button"
          aria-label="Forward"
          onClick={() => void api.webHistory(label, "forward")}
        >
          <Icon name="forward" size={17} />
        </button>
        <button
          className="btn btn-ghost btn-icon"
          type="button"
          aria-label="Reload"
          onClick={() => void api.webHistory(label, "reload")}
        >
          <Icon name="reload" size={17} />
        </button>
        {appConfig ? (
          <>
            <div className="app-identity">
              <span className={`app-identity-mark ${app}`}>
                <Icon name={appConfig.icon} size={14} />
              </span>
              <strong>{appConfig.title}</strong>
            </div>
            <div className="spacer" />
            <span className="app-host">{webUrlHost(address)}</span>
          </>
        ) : (
          <>
            <input
              className="input"
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              onFocus={(e) => {
                editing.current = true;
                e.currentTarget.select();
              }}
              onBlur={() => {
                editing.current = false;
              }}
              placeholder="Type a URL"
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
            />
            <button className="btn btn-secondary" type="submit">
              Go
            </button>
          </>
        )}
        <button
          className="btn btn-ghost"
          type="button"
          title={app ? "Open in the system browser" : "Open in browser"}
          onClick={() => void api.openSystem(address).catch((e) => onError(String(e)))}
        >
          {app ? "Open" : "Browser"}
        </button>
        {chrome}
      </form>
      <div className="web-holder" ref={holder} />
    </div>
  );
}
