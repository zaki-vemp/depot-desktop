export type SourceKind = "local" | "gdrive";

export interface DirEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modified: number | null;
  ext: string;
  source: SourceKind;
  mimeType?: string | null;
  accountId?: string | null;
}

export interface Place {
  name: string;
  path: string;
  kind: string;
}

export interface DriveAccount {
  id: string;
  email: string;
}

export interface AppSettings {
  googleClientId: string;
  googleClientSecret: string;
  oneDriveClientId: string;
  oneDriveClientSecret: string;
  dropboxClientId: string;
  dropboxClientSecret: string;
  s3Endpoint: string;
  s3Region: string;
  s3Bucket: string;
  s3AccessKeyId: string;
  s3SecretAccessKey: string;
  torrentDownloadDir: string;
}

export interface TorrentInfo {
  id: number;
  name: string;
  progress: number;
  downloaded: number;
  total: number;
  downloadSpeed: number;
  state: string;
  error?: string | null;
  outputFolder: string;
}

export interface DiskUsage {
  total: number;
  free: number;
  mount: string;
}

export type TransferState = "queued" | "running" | "done" | "error";

export interface Transfer {
  id: string;
  name: string;
  route: string;
  op: "copy" | "move" | "download" | "upload";
  from: string;
  to: string;
  moved: number;
  total: number;
  speed: number;
  state: TransferState;
  error?: string;
  startedAt: number;
  updatedAt: number;
}

/** Payload emitted by the Rust `transfer` event. */
export interface TransferEvent {
  id: string;
  moved: number;
  total: number;
  state: TransferState;
  error?: string | null;
}

export type TabKind =
  | "files"
  | "preview"
  | "editor"
  | "web"
  | "app"
  | "torrents"
  | "settings"
  | "drives"
  | "transfers";

export type SocialAppKind = "facebook" | "instagram";

export interface Tab {
  id: string;
  kind: TabKind;
  title: string;
  path?: string;
  source?: SourceKind;
  accountId?: string;
  folderId?: string;
  url?: string;
  app?: SocialAppKind;
  /** Editor tabs: the file to open on first mount, if the tab was opened on one. */
  file?: string;
  /** Visited locations for this tab, used by back/forward. */
  history: HistoryEntry[];
  historyIndex: number;
}

export interface HistoryEntry {
  /** Local path, or Drive folder id for `gdrive` tabs. */
  loc: string;
  title: string;
}

export interface UiPrefs {
  theme: "light" | "dark";
  view: "grid" | "list";
  showHidden: boolean;
  useTrash: boolean;
  systemFallback: boolean;
  confirmDelete: boolean;
  /** Side panels — collapsing them hands the space to viewers. */
  sidebarOpen: boolean;
  inspectorOpen: boolean;
}

export type ViewerKind =
  | "video"
  | "audio"
  | "image"
  | "pdf"
  | "text"
  | "spreadsheet"
  | "document"
  | "slides"
  | "unknown";

export interface VlcInfo {
  available: boolean;
  version?: string | null;
  message: string;
}

export interface VlcStatus {
  token: string;
  path: string;
  playing: boolean;
  ended: boolean;
  timeMs: number;
  lengthMs: number;
  volume: number;
  muted: boolean;
  rate: number;
}

export interface SubtitleTrack {
  id: string;
  label: string;
  language?: string | null;
  kind: "sidecar" | "embedded";
}

/** One chunk of pty output. `chunk` is base64-encoded raw bytes. */
export interface TermData {
  id: string;
  chunk: string;
}

export interface TermExit {
  id: string;
  code: number | null;
}

export type GitChangeKind =
  | "modified"
  | "added"
  | "deleted"
  | "renamed"
  | "copied"
  | "untracked"
  | "conflicted";

export interface GitFile {
  /** Repo-relative, forward-slashed. */
  path: string;
  absPath: string;
  name: string;
  kind: GitChangeKind;
  staged: boolean;
  origPath?: string | null;
}

export interface GitRepo {
  root: string;
  branch: string;
  ahead: number;
  behind: number;
  upstream?: string | null;
  staged: GitFile[];
  unstaged: GitFile[];
}

export interface AgentPreset {
  id: string;
  label: string;
  command: string;
  args: string[];
  /** Whether `command` resolves on PATH right now. */
  available: boolean;
  note: string;
}

export interface AgentChunk {
  id: string;
  stream: "stdout" | "stderr";
  line: string;
}

export interface AgentDone {
  id: string;
  code: number | null;
  error?: string | null;
}

export interface CheckpointInfo {
  id: string;
  /** `git` when backed by a tree object, `snapshot` outside a repository. */
  mode: "git" | "snapshot";
  truncated: boolean;
}

export interface AgentChange {
  path: string;
  absPath: string;
  name: string;
  kind: "modified" | "added" | "deleted";
  /** False when the checkpoint cannot prove what the file looked like before. */
  revertible: boolean;
}

export type ChatRole = "you" | "agent" | "system";

export interface ChatMessage {
  id: string;
  role: ChatRole;
  text: string;
  /** Set on agent turns while output is still streaming in. */
  streaming?: boolean;
  /** Files this turn changed, resolved once the run finishes. */
  changes?: AgentChange[];
  checkpoint?: string;
  failed?: boolean;
}

/** A file open in the code editor. */
export interface EditorDoc {
  path: string;
  name: string;
  /** Contents as loaded from disk — compared against the buffer for dirtiness. */
  saved: string;
  language: string;
  readonly?: boolean;
}

export interface OpenApp {
  name: string;
  path: string;
  isDefault: boolean;
}

export interface OfficeSheet {
  name: string;
  rows: string[][];
}

export interface OfficePage {
  title: string;
  body: string;
}

export interface OfficePreview {
  kind: "spreadsheet" | "document" | "slides";
  sheets: OfficeSheet[];
  pages: OfficePage[];
  truncated: boolean;
  note: string;
}
