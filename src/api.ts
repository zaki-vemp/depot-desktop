import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import type {
  AgentChange,
  AgentDone,
  AgentEvent,
  AgentOptions,
  AppSettings,
  EngineDoctor,
  EngineStatus,
  CheckpointInfo,
  DirEntry,
  DiskUsage,
  DriveAccount,
  GitRepo,
  Place,
  SubtitleTrack,
  TermData,
  TermExit,
  TorrentInfo,
  TransferEvent,
  OpenApp,
  OfficePreview,
  VlcInfo,
  VlcStatus,
} from "./types";

export const api = {
  places: () => invoke<Place[]>("get_places"),
  home: () => invoke<string>("get_home"),
  listDir: (path: string) => invoke<DirEntry[]>("list_dir", { path }),
  readText: (path: string) => invoke<string>("read_text_file", { path }),
  writeText: (path: string, contents: string) =>
    invoke<void>("write_text_file", { path, contents }),
  createFile: (path: string) => invoke<void>("create_file", { path }),
  isTextFile: (path: string) => invoke<boolean>("is_text_file", { path }),
  mkdir: (path: string) => invoke<void>("mkdir", { path }),
  rename: (from: string, to: string) =>
    invoke<void>("rename_path", { from, to }),
  remove: (path: string) => invoke<void>("remove_path", { path }),
  trash: (path: string) => invoke<void>("trash_path", { path }),
  copy: (from: string, to: string) => invoke<void>("copy_path", { from, to }),
  move: (from: string, to: string) => invoke<void>("move_path", { from, to }),
  /** Starts a native drag, so the files can be dropped into any other app. */
  startDrag: (paths: string[]) => invoke<void>("start_file_drag", { paths }),
  /** Puts files on the system clipboard, ready to paste outside Depot. */
  clipboardCopyFiles: (paths: string[], cut: boolean) =>
    invoke<void>("clipboard_copy_files", { paths, cut }),
  parent: (path: string) => invoke<string | null>("parent_path", { path }),
  /** Native folder chooser; resolves to null when the user cancels. */
  pickFolder: async (title: string, defaultPath?: string) => {
    const picked = await openDialog({ directory: true, multiple: false, title, defaultPath });
    return typeof picked === "string" ? picked : null;
  },
  openSystem: (path: string) => invoke<void>("open_in_system", { path }),
  reveal: (path: string) => invoke<void>("reveal_in_dir", { path }),
  previewOffice: (path: string) => invoke<OfficePreview>("preview_office", { path }),
  listOpenWith: (path: string) => invoke<OpenApp[]>("list_open_with", { path }),
  openWithApp: (path: string, app: string) => invoke<void>("open_with_app", { path, app }),
  pickOpenWith: (path: string) => invoke<void>("pick_open_with", { path }),
  diskUsage: (path: string) => invoke<DiskUsage>("disk_usage", { path }),
  settings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<void>("save_settings", { settings }),
  driveAccounts: () => invoke<DriveAccount[]>("list_drive_accounts"),
  connectDrive: () => invoke<DriveAccount>("connect_google_drive"),
  disconnectDrive: (accountId: string) =>
    invoke<void>("disconnect_google_drive", { accountId }),
  listDrive: (accountId: string, folderId?: string | null) =>
    invoke<DirEntry[]>("list_drive", { accountId, folderId }),
  mkdirDrive: (accountId: string, folderId: string | null | undefined, name: string) =>
    invoke<string>("mkdir_drive", { accountId, folderId: folderId || null, name }),
  downloadDrive: (path: string, dest: string) =>
    invoke<string>("download_drive_file", { path, dest }),
  cacheDrive: (path: string, name: string) =>
    invoke<string>("cache_drive_file", { path, name }),
  driveQuota: (accountId: string) =>
    invoke<DiskUsage>("drive_quota", { accountId }),
  /** Runs a tracked copy/move/download; progress arrives on the `transfer` event. */
  startTransfer: (id: string, from: string, to: string, op: string) =>
    invoke<void>("start_transfer", { id, from, to, op }),
  /* Web tabs run in a real child webview, so sites that refuse framing still load. */
  webOpen: (label: string, url: string, x: number, y: number, width: number, height: number) =>
    invoke<string>("web_open", { label, url, x, y, width, height }),
  webBounds: (label: string, x: number, y: number, width: number, height: number) =>
    invoke<void>("web_bounds", { label, x, y, width, height }),
  webHide: (label: string) => invoke<void>("web_hide", { label }),
  webClose: (label: string) => invoke<void>("web_close", { label }),
  webNavigate: (label: string, url: string) => invoke<string>("web_navigate", { label, url }),
  webHistory: (label: string, action: "back" | "forward" | "reload") =>
    invoke<void>("web_history", { label, action }),
  webUrl: (label: string) => invoke<string>("web_url", { label }),
  vlcAvailable: () => invoke<VlcInfo>("vlc_available"),
  vlcOpen: (token: string, path: string, x: number, y: number, width: number, height: number) =>
    invoke<void>("vlc_open", { token, path, x, y, width, height }),
  vlcBounds: (x: number, y: number, width: number, height: number) =>
    invoke<void>("vlc_bounds", { x, y, width, height }),
  vlcHide: () => invoke<void>("vlc_hide"),
  vlcClose: (token: string) => invoke<void>("vlc_close", { token }),
  vlcToggle: () => invoke<void>("vlc_toggle"),
  vlcPlay: () => invoke<void>("vlc_play"),
  vlcPause: () => invoke<void>("vlc_pause"),
  vlcSeek: (ms: number) => invoke<void>("vlc_seek", { ms }),
  vlcSetVolume: (volume: number) => invoke<void>("vlc_set_volume", { volume }),
  vlcSetRate: (rate: number) => invoke<void>("vlc_set_rate", { rate }),
  vlcSetMute: (muted: boolean) => invoke<void>("vlc_set_mute", { muted }),
  vlcStatus: () => invoke<VlcStatus>("vlc_status"),
  vlcTracks: () => invoke<SubtitleTrack[]>("vlc_tracks"),
  vlcSetSubtitle: (id: string | null) => invoke<void>("vlc_set_subtitle", { id }),
  listSubtitles: (path: string) => invoke<SubtitleTrack[]>("list_subtitles", { path }),
  subtitleVtt: (path: string, trackId: string) =>
    invoke<string>("subtitle_vtt", { path, trackId }),
  /* Source control. `gitInfo` resolves to null outside a repository. */
  gitInfo: (cwd: string) => invoke<GitRepo | null>("git_info", { cwd }),
  /** Contents at a revision: `HEAD` for the last commit, `:` for the index. */
  gitShow: (root: string, rev: string, path: string) =>
    invoke<string>("git_show", { root, rev, path }),
  gitStage: (root: string, paths: string[]) => invoke<void>("git_stage", { root, paths }),
  gitUnstage: (root: string, paths: string[]) => invoke<void>("git_unstage", { root, paths }),
  gitDiscard: (root: string, paths: string[]) => invoke<void>("git_discard", { root, paths }),
  gitCommit: (root: string, message: string, amend = false) =>
    invoke<string>("git_commit", { root, message, amend }),

  /* Coding agents. Depot spawns whichever CLI the user already has signed in
     — Claude Code, Codex, and friends — and never talks to a model itself. */
  agentEngines: () => invoke<EngineStatus[]>("agent_engines"),
  /** Why an engine will not run: paths, versions, which auth vars are set. */
  agentDoctor: (engine: string) => invoke<EngineDoctor>("agent_doctor", { engine }),
  agentRun: (id: string, cwd: string, prompt: string, options: AgentOptions) =>
    invoke<void>("agent_run", { id, cwd, prompt, options }),
  agentCancel: (id: string) => invoke<void>("agent_cancel", { id }),
  /** Forgets the stored conversation so the next turn starts cold. */
  agentReset: (engine: string, cwd: string) => invoke<void>("agent_reset", { engine, cwd }),

  /* Checkpoints: what makes an agent's edits undoable. */
  checkpointCreate: (root: string) => invoke<CheckpointInfo>("checkpoint_create", { root }),
  checkpointChanges: (id: string) => invoke<AgentChange[]>("checkpoint_changes", { id }),
  checkpointOriginal: (id: string, path: string) =>
    invoke<string>("checkpoint_original", { id, path }),
  checkpointRevert: (id: string, paths: string[]) =>
    invoke<void>("checkpoint_revert", { id, paths }),
  checkpointDiscard: (id: string) => invoke<void>("checkpoint_discard", { id }),

  /* The built-in terminal: a real pty per session, driven from xterm.js. */
  termOpen: (id: string, cwd: string, cols: number, rows: number) =>
    invoke<void>("term_open", { id, cwd, cols, rows }),
  termWrite: (id: string, data: string) => invoke<void>("term_write", { id, data }),
  termResize: (id: string, cols: number, rows: number) =>
    invoke<void>("term_resize", { id, cols, rows }),
  termClose: (id: string) => invoke<void>("term_close", { id }),
  addTorrent: (magnet: string) => invoke<string>("add_torrent", { magnet }),
  torrents: () => invoke<TorrentInfo[]>("list_torrents"),
  pauseTorrent: (id: number) => invoke<void>("pause_torrent", { id }),
  resumeTorrent: (id: number) => invoke<void>("resume_torrent", { id }),
};

export function onTransfer(handler: (e: TransferEvent) => void) {
  return listen<TransferEvent>("transfer", (event) => handler(event.payload));
}

/** One structured agent event (thinking, text, tool call, result), as it happens. */
export function onAgentEvent(handler: (e: AgentEvent) => void) {
  return listen<AgentEvent>("agent:event", (event) => handler(event.payload));
}


export function onAgentDone(handler: (e: AgentDone) => void) {
  return listen<AgentDone>("agent:done", (event) => handler(event.payload));
}

/** Raw pty output. `chunk` is base64 so partial UTF-8 sequences survive the hop. */
export function onTermData(handler: (e: TermData) => void) {
  return listen<TermData>("term:data", (event) => handler(event.payload));
}

export function onTermExit(handler: (e: TermExit) => void) {
  return listen<TermExit>("term:exit", (event) => handler(event.payload));
}

export function fileUrl(path: string) {
  return convertFileSrc(path);
}

export function joinPath(dir: string, name: string) {
  if (/^[A-Za-z]:\\/.test(dir) || dir.includes("\\")) {
    return `${dir.replace(/\\+$/, "")}\\${name}`;
  }
  if (dir.endsWith("/")) return `${dir}${name}`;
  return `${dir}/${name}`;
}

export function baseName(path: string) {
  return path.split(/[/\\]/).filter(Boolean).pop() || path;
}

/**
 * The containing directory, with roots kept intact on every platform:
 * `C:\Users` → `C:\` (not the drive-relative `C:`), and `/etc` → `/`.
 */
export function parentDir(path: string) {
  const windows = /^[A-Za-z]:/.test(path) || path.includes("\\");
  const sep = windows ? "\\" : "/";
  const trimmed = path.replace(/[/\\]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (cut < 0) return path;
  const head = trimmed.slice(0, cut);
  if (!head) return sep;
  if (windows && /^[A-Za-z]:$/.test(head)) return head + sep;
  return head;
}

export function driveDestPath(accountId: string, folderId: string | undefined, name: string) {
  return `gdrive://${accountId}/${folderId || "root"}/${encodeURIComponent(name)}`;
}
