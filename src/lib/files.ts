import type { DirEntry, ViewerKind } from "../types";
import type { FileKind, IconName } from "./icons";

const VIDEO = new Set([
  "mp4",
  "webm",
  "ogg",
  "ogv",
  "mov",
  "m4v",
  "mkv",
  "avi",
  "wmv",
  "flv",
  "f4v",
  "ts",
  "m2ts",
  "mts",
  "mpg",
  "mpeg",
  "m2v",
  "3gp",
  "3g2",
  "vob",
  "asf",
  "divx",
]);
const AUDIO = new Set(["mp3", "wav", "flac", "aac", "m4a", "oga", "opus"]);
const IMAGE = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif"]);
const PDF = new Set(["pdf"]);
const SPREADSHEET = new Set(["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "numbers"]);
const DOCUMENT = new Set(["doc", "docx", "odt", "rtf", "rtfd", "pages", "tex", "epub"]);
const SLIDES = new Set(["ppt", "pptx", "odp", "key"]);
const TEXT = new Set([
  "txt",
  "md",
  "json",
  "ts",
  "tsx",
  "js",
  "jsx",
  "rs",
  "py",
  "toml",
  "yml",
  "yaml",
  "xml",
  "html",
  "css",
  "log",
  "ini",
  "sh",
]);
const CODE = new Set(["ts", "tsx", "js", "jsx", "rs", "py", "sh", "md", "json"]);
const ARCHIVE = new Set(["zip", "tar", "gz", "7z", "rar", "dmg", "iso"]);

/** Codecs a webview reliably decodes; everything else goes to the system player. */
const WEB_SAFE_VIDEO = new Set(["mp4", "m4v", "webm", "ogg", "ogv", "mov"]);

export function viewerKind(ext: string): ViewerKind {
  const e = ext.toLowerCase();
  if (VIDEO.has(e)) return "video";
  if (AUDIO.has(e)) return "audio";
  if (IMAGE.has(e)) return "image";
  if (PDF.has(e)) return "pdf";
  if (SPREADSHEET.has(e)) return "spreadsheet";
  if (SLIDES.has(e)) return "slides";
  if (DOCUMENT.has(e)) return "document";
  if (TEXT.has(e)) return "text";
  return "unknown";
}

export function playsInWebview(ext: string) {
  return WEB_SAFE_VIDEO.has(ext.toLowerCase());
}

export function extOf(name: string) {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

/** Which colour-coded mark the file list draws for an entry. */
export function iconFor(entry: Pick<DirEntry, "isDir" | "ext">): FileKind {
  if (entry.isDir) return "folder";
  const e = entry.ext.toLowerCase();
  if (CODE.has(e)) return "code";
  if (ARCHIVE.has(e)) return "archive";
  switch (viewerKind(e)) {
    case "video":
      return "video";
    case "audio":
      return "audio";
    case "image":
      return "image";
    case "pdf":
      return "pdf";
    case "spreadsheet":
      return "sheet";
    case "slides":
      return "slides";
    case "document":
      return "doc";
    case "text":
      return "text";
    default:
      return "file";
  }
}

/** Same entry as a thin chrome stroke icon — used in tab strips and menus. */
const CHROME_FOR_KIND: Record<FileKind, IconName> = {
  folder: "folder",
  image: "image",
  video: "film",
  audio: "music",
  pdf: "doc",
  doc: "doc",
  sheet: "table",
  slides: "slides",
  code: "code",
  text: "doc",
  archive: "file",
  disk: "disk",
  file: "file",
};

export function chromeIconFor(entry: Pick<DirEntry, "isDir" | "ext">): IconName {
  return CHROME_FOR_KIND[iconFor(entry)];
}

export function kindLabel(entry: Pick<DirEntry, "isDir" | "ext">) {
  if (entry.isDir) return "Folder";
  const e = entry.ext.toLowerCase();
  if (!e) return "File";
  switch (viewerKind(e)) {
    case "video":
      return `${e.toUpperCase()} video`;
    case "audio":
      return "Audio";
    case "image":
      return `${e.toUpperCase()} image`;
    case "pdf":
      return "PDF document";
    case "spreadsheet":
      return `${e.toUpperCase()} spreadsheet`;
    case "document":
      return `${e.toUpperCase()} document`;
    case "slides":
      return `${e.toUpperCase()} presentation`;
    case "text":
      return CODE.has(e) ? "Source file" : "Text";
    default:
      return ARCHIVE.has(e) ? "Archive" : `${e.toUpperCase()} file`;
  }
}

export function formatBytes(n: number) {
  if (!n) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

export function formatSpeed(bytesPerSecond: number) {
  if (!bytesPerSecond) return "—";
  return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatDate(ts: number | null) {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleString(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatDuration(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) return "00:00";
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}
