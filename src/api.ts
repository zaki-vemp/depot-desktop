import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSettings,
  DirEntry,
  DiskUsage,
  DriveAccount,
  Place,
  SubtitleTrack,
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
  mkdir: (path: string) => invoke<void>("mkdir", { path }),
  rename: (from: string, to: string) =>
    invoke<void>("rename_path", { from, to }),
  remove: (path: string) => invoke<void>("remove_path", { path }),
  trash: (path: string) => invoke<void>("trash_path", { path }),
  copy: (from: string, to: string) => invoke<void>("copy_path", { from, to }),
  move: (from: string, to: string) => invoke<void>("move_path", { from, to }),
  parent: (path: string) => invoke<string | null>("parent_path", { path }),
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
  addTorrent: (magnet: string) => invoke<string>("add_torrent", { magnet }),
  torrents: () => invoke<TorrentInfo[]>("list_torrents"),
  pauseTorrent: (id: number) => invoke<void>("pause_torrent", { id }),
  resumeTorrent: (id: number) => invoke<void>("resume_torrent", { id }),
};

export function onTransfer(handler: (e: TransferEvent) => void) {
  return listen<TransferEvent>("transfer", (event) => handler(event.payload));
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

export function driveDestPath(accountId: string, folderId: string | undefined, name: string) {
  return `gdrive://${accountId}/${folderId || "root"}/${encodeURIComponent(name)}`;
}
