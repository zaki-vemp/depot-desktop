/* Two icon families.

   `Icon`  — thin monochrome strokes for chrome (toolbar, sidebar, tabs, HUD).
   `FileIcon` — filled, colour-coded folder and document marks for the file
   list, the way a desktop file explorer draws them. Each mark is tinted by a
   `--ft-*` token so the type is readable before the name is. */

type Paths = string | string[];

export const ICON = {
  home: [
    "M3.2 10.9 12 4l8.8 6.9",
    "M5.6 9.3v10.1a1.4 1.4 0 0 0 1.4 1.4h10a1.4 1.4 0 0 0 1.4-1.4V9.3",
    "M9.8 20.8v-6.1h4.4v6.1",
  ],
  folder: [
    "M3.2 6.8A2.3 2.3 0 0 1 5.5 4.5h3.6c.5 0 1 .17 1.4.48l1.7 1.32h6.3a2.3 2.3 0 0 1 2.3 2.3v8.6a2.3 2.3 0 0 1-2.3 2.3H5.5a2.3 2.3 0 0 1-2.3-2.3z",
  ],
  download: ["M12 3.6v11.6", "m7.4 10.8 4.6 4.4 4.6-4.4", "M4.6 20.4h14.8"],
  image: [
    "M4.4 4.4h15.2a1.6 1.6 0 0 1 1.6 1.6v12a1.6 1.6 0 0 1-1.6 1.6H4.4A1.6 1.6 0 0 1 2.8 18V6a1.6 1.6 0 0 1 1.6-1.6z",
    "m2.9 16.4 4.3-4.2a1.6 1.6 0 0 1 2.2 0l3.1 3.1 2.1-2a1.6 1.6 0 0 1 2.2 0l4.3 4.2",
    "M9.4 9.2a1.15 1.15 0 1 1-2.3 0 1.15 1.15 0 0 1 2.3 0",
  ],
  music: ["M9.2 17.6V6.4l9.6-2.1v11.2", "M9.2 17.6a2.9 2.9 0 1 1-5.8 0 2.9 2.9 0 0 1 5.8 0", "M18.8 15.5a2.9 2.9 0 1 1-5.8 0 2.9 2.9 0 0 1 5.8 0"],
  film: [
    "M3.6 4.6h16.8a1 1 0 0 1 1 1v12.8a1 1 0 0 1-1 1H3.6a1 1 0 0 1-1-1V5.6a1 1 0 0 1 1-1z",
    "M7.4 4.6v14.8",
    "M16.6 4.6v14.8",
    "M2.6 9.4h4.8",
    "M16.6 9.4h4.8",
    "M2.6 14.6h4.8",
    "M16.6 14.6h4.8",
  ],
  disk: [
    "M3 12.6h18",
    "M4.7 12.6 6.5 6.4a1.6 1.6 0 0 1 1.54-1.15h7.92A1.6 1.6 0 0 1 17.5 6.4l1.8 6.2",
    "M3 12.6v4.8a1.7 1.7 0 0 0 1.7 1.7h14.6a1.7 1.7 0 0 0 1.7-1.7v-4.8",
    "M6.6 16.1h.02",
    "M10 16.1h.02",
  ],
  cloud: ["M6.6 18.6a4.3 4.3 0 0 1 .5-8.57 5.8 5.8 0 0 1 11.2 1.02 3.8 3.8 0 0 1-.4 7.55z"],
  net: ["M12 3.4a8.6 8.6 0 1 0 0 17.2 8.6 8.6 0 0 0 0-17.2", "M3.6 12h16.8", "M12 3.4c2.9 3.4 2.9 13.8 0 17.2", "M12 3.4c-2.9 3.4-2.9 13.8 0 17.2"],
  bucket: ["M4.4 6.6h15.2l-1.9 12.2a1 1 0 0 1-1 .85H7.3a1 1 0 0 1-1-.85z", "M4.4 6.6c0-1.6 3.4-2.9 7.6-2.9s7.6 1.3 7.6 2.9"],
  magnet: ["M6.2 4.6h3.4v7.5a2.4 2.4 0 0 0 4.8 0V4.6h3.4v7.5a5.8 5.8 0 0 1-11.6 0z", "M6.2 8.7h3.4", "M14.4 8.7h3.4"],
  arrows: ["M4 8.6h13", "m13.6 5.2 3.4 3.4-3.4 3.4", "M20 15.4H7", "m10.4 12-3.4 3.4 3.4 3.4"],
  gear: [
    "M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4",
    "M19.3 14.9a1.5 1.5 0 0 0 .3 1.66l.05.05a1.85 1.85 0 1 1-2.62 2.62l-.05-.05a1.5 1.5 0 0 0-2.53 1.06v.16a1.85 1.85 0 1 1-3.7 0v-.09a1.5 1.5 0 0 0-2.6-1.02l-.05.05a1.85 1.85 0 1 1-2.62-2.62l.05-.05a1.5 1.5 0 0 0-1.02-2.56h-.16a1.85 1.85 0 1 1 0-3.7h.09A1.5 1.5 0 0 0 5.5 7.87l-.05-.05a1.85 1.85 0 1 1 2.62-2.62l.05.05a1.5 1.5 0 0 0 1.66.3h.07A1.5 1.5 0 0 0 10.76 4.2v-.16a1.85 1.85 0 1 1 3.7 0v.09a1.5 1.5 0 0 0 2.56 1.02l.05-.05a1.85 1.85 0 1 1 2.62 2.62l-.05.05a1.5 1.5 0 0 0-.3 1.66v.07a1.5 1.5 0 0 0 1.37.9h.16a1.85 1.85 0 1 1 0 3.7h-.09a1.5 1.5 0 0 0-1.38.9z",
  ],
  globe: ["M12 3.4a8.6 8.6 0 1 0 0 17.2 8.6 8.6 0 0 0 0-17.2", "M3.9 9.2h16.2", "M3.9 14.8h16.2", "M12 3.4c2.5 3.5 2.5 13.7 0 17.2", "M12 3.4c-2.5 3.5-2.5 13.7 0 17.2"],
  facebook: ["M18 2.6h-3a5 5 0 0 0-5 5v3H7v4h3v7.8h4V14.6h3.4l.6-4H14V7.6a1 1 0 0 1 1-1h3z"],
  instagram: [
    "M7.4 2.6h9.2a4.8 4.8 0 0 1 4.8 4.8v9.2a4.8 4.8 0 0 1-4.8 4.8H7.4a4.8 4.8 0 0 1-4.8-4.8V7.4a4.8 4.8 0 0 1 4.8-4.8z",
    "M16.4 6.6h.02",
    "M15.8 12a3.8 3.8 0 1 1-7.6 0 3.8 3.8 0 0 1 7.6 0",
  ],
  doc: ["M13.6 3H7.6a2.2 2.2 0 0 0-2.2 2.2v13.6A2.2 2.2 0 0 0 7.6 21h8.8a2.2 2.2 0 0 0 2.2-2.2V7.8z", "M13.6 3v4.8h5", "M8.8 13h6.4", "M8.8 16.6h4.4"],
  table: ["M3.6 5.4h16.8v13.2H3.6z", "M3.6 10.2h16.8", "M9.2 5.4v13.2", "M14.8 5.4v13.2"],
  slides: ["M3.6 5.6h16.8v11.2H3.6z", "M8 3.2h8", "M9.4 20.8 12 16.8l2.6 4"],
  file: ["M13.6 3H7.6a2.2 2.2 0 0 0-2.2 2.2v13.6A2.2 2.2 0 0 0 7.6 21h8.8a2.2 2.2 0 0 0 2.2-2.2V7.8z", "M13.6 3v4.8h5"],
  code: ["m9.4 17.6-5.6-5.6 5.6-5.6", "m14.6 6.4 5.6 5.6-5.6 5.6"],
  back: "m14.6 18.4-6.4-6.4 6.4-6.4",
  forward: "m9.4 5.6 6.4 6.4-6.4 6.4",
  up: ["M12 19.4V5", "m5.4 11.6 6.6-6.6 6.6 6.6"],
  plus: ["M12 5.2v13.6", "M5.2 12h13.6"],
  close: ["M17.8 6.2 6.2 17.8", "M6.2 6.2l11.6 11.6"],
  warn: ["M12 9.4v4.2", "M12 17.4h.02", "M10.4 4.1 2.1 18a1.9 1.9 0 0 0 1.6 2.85h16.6A1.9 1.9 0 0 0 21.9 18L13.6 4.1a1.9 1.9 0 0 0-3.2 0z"],
  sun: [
    "M12 4.2V2.4",
    "M12 21.6v-1.8",
    "M19.8 12h1.8",
    "M2.4 12h1.8",
    "m17.5 6.5 1.3-1.3",
    "M5.2 18.8l1.3-1.3",
    "m17.5 17.5 1.3 1.3",
    "M5.2 5.2l1.3 1.3",
    "M15.8 12a3.8 3.8 0 1 1-7.6 0 3.8 3.8 0 0 1 7.6 0",
  ],
  moon: ["M12 3.4a6 6 0 0 0 8.6 8.6A8.6 8.6 0 1 1 12 3.4"],
  panelLeft: ["M4.4 4.4h15.2a1.4 1.4 0 0 1 1.4 1.4v12.4a1.4 1.4 0 0 1-1.4 1.4H4.4A1.4 1.4 0 0 1 3 18.2V5.8a1.4 1.4 0 0 1 1.4-1.4z", "M9.4 4.4v15.2"],
  panelRight: ["M4.4 4.4h15.2a1.4 1.4 0 0 1 1.4 1.4v12.4a1.4 1.4 0 0 1-1.4 1.4H4.4A1.4 1.4 0 0 1 3 18.2V5.8a1.4 1.4 0 0 1 1.4-1.4z", "M14.6 4.4v15.2"],
  expand: ["M8.6 3.4H3.4v5.2", "m3.4 3.4 6 6", "M15.4 3.4h5.2v5.2", "m20.6 3.4-6 6", "M8.6 20.6H3.4v-5.2", "m3.4 20.6 6-6", "M15.4 20.6h5.2v-5.2", "m20.6 20.6-6-6"],
  play: "M8.4 5.2v13.6L19 12z",
  pause: ["M7.6 5.2h2.8v13.6H7.6z", "M13.6 5.2h2.8v13.6h-2.8z"],
  skipBack: ["M5.2 5.2v13.6", "M18.8 5.2 9 12l9.8 6.8z"],
  skipFwd: ["M18.8 5.2v13.6", "M5.2 5.2 15 12l-9.8 6.8z"],
  volume: ["M4.4 9.8v4.4h3.8l4.8 3.8V6L8.2 9.8z", "M16.4 9.2a4 4 0 0 1 0 5.6", "M18.9 6.9a7.4 7.4 0 0 1 0 10.2"],
  volumeOff: ["M4.4 9.8v4.4h3.8l4.8 3.8V6L8.2 9.8z", "M21.4 9.6 16.2 14.8", "M16.2 9.6l5.2 5.2"],
  captions: ["M3.6 5.6h16.8v12.8H3.6z", "M8.6 10.4h2.4", "M13 10.4h2.4", "M8.6 14h6.8"],
  loop: ["M3.4 12a8.6 8.6 0 0 1 13.9-6.8l2.4 1.9", "M20.6 3.4v4.2h-4.2", "M20.6 12a8.6 8.6 0 0 1-13.9 6.8l-2.4-1.9", "M3.4 20.6v-4.2h4.2"],
  pip: ["M3.4 5.2h17.2v13.6H3.4z", "M12.6 11.2h6.2v5.6h-6.2z"],
  collapse: ["M9.4 3.4v5.2H4.2", "m3.4 3.4 5.2 5.2", "M14.6 20.6v-5.2h5.2", "m20.6 20.6-5.2-5.2"],
  eye: ["M2.4 12s3.7-7 9.6-7 9.6 7 9.6 7-3.7 7-9.6 7-9.6-7-9.6-7", "M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0"],
  reload: ["M3.4 12a8.6 8.6 0 0 1 14.6-6.2l2.6 2.4", "M20.6 3.6v4.6h-4.6", "M20.6 12a8.6 8.6 0 0 1-14.6 6.2L3.4 15.8", "M3.4 20.4v-4.6H8"],
  search: ["M15.4 10.6a4.8 4.8 0 1 1-9.6 0 4.8 4.8 0 0 1 9.6 0", "m14.2 14.2 5 5"],
  grid: ["M4.2 4.2h6v6h-6z", "M13.8 4.2h6v6h-6z", "M4.2 13.8h6v6h-6z", "M13.8 13.8h6v6h-6z"],
  list: ["M8.6 6.4h11.8", "M8.6 12h11.8", "M8.6 17.6h11.8", "M4.2 6.4h.02", "M4.2 12h.02", "M4.2 17.6h.02"],
  trash: ["M3.8 6.4h16.4", "M8.6 6.4V4.8a1.4 1.4 0 0 1 1.4-1.4h4a1.4 1.4 0 0 1 1.4 1.4v1.6", "M6.2 6.4v13a1.8 1.8 0 0 0 1.8 1.8h8a1.8 1.8 0 0 0 1.8-1.8v-13", "M10.2 10.6v6", "M13.8 10.6v6"],
  folderPlus: [
    "M3.2 6.8A2.3 2.3 0 0 1 5.5 4.5h3.6c.5 0 1 .17 1.4.48l1.7 1.32h6.3a2.3 2.3 0 0 1 2.3 2.3v8.6a2.3 2.3 0 0 1-2.3 2.3H5.5a2.3 2.3 0 0 1-2.3-2.3z",
    "M12 10.2v5.6",
    "M9.2 13h5.6",
  ],
  folderOpen: [
    "M3.2 6.8A2.3 2.3 0 0 1 5.5 4.5h3.6c.5 0 1 .17 1.4.48l1.7 1.32h6.3a2.3 2.3 0 0 1 2.3 2.3v1.2",
    "M3.2 9.9h17.3l-2 8.1a1.4 1.4 0 0 1-1.36 1.05H4.6a1.4 1.4 0 0 1-1.4-1.4z",
  ],
  sparkle: [
    "M12 3.2 13.9 9l5.8 1.9-5.8 1.9L12 18.6l-1.9-5.8L4.3 10.9 10.1 9z",
    "M18.4 3.2l.7 2.1 2.1.7-2.1.7-.7 2.1-.7-2.1-2.1-.7 2.1-.7z",
  ],
  check: "m4.8 12.6 4.6 4.6 9.8-10.4",
  chevronDown: "m6.4 9.4 5.6 5.6 5.6-5.6",
  chevronRight: "m9.4 6.4 5.6 5.6-5.6 5.6",
  terminal: ["M4.4 5.2h15.2a1.4 1.4 0 0 1 1.4 1.4v10.8a1.4 1.4 0 0 1-1.4 1.4H4.4A1.4 1.4 0 0 1 3 17.4V6.6a1.4 1.4 0 0 1 1.4-1.4z", "m7 9.6 2.8 2.4L7 14.4", "M12.6 15h4.2"],
  save: [
    "M18.4 20.6H5.6a2.2 2.2 0 0 1-2.2-2.2V5.6a2.2 2.2 0 0 1 2.2-2.2h9.6l5.4 5.4v9.6a2.2 2.2 0 0 1-2.2 2.2z",
    "M7.6 20.6v-6.4h8.8v6.4",
    "M7.6 3.4v4.8h6.4",
  ],
  edit: [
    "M11.6 4.6H5.4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-6.2",
    "M17.9 3.6a2.1 2.1 0 0 1 3 3L12.4 15.1l-4 1 1-4z",
  ],
  desktop: [
    "M3.6 4.6h16.8a1.4 1.4 0 0 1 1.4 1.4v8.6a1.4 1.4 0 0 1-1.4 1.4H3.6a1.4 1.4 0 0 1-1.4-1.4V6a1.4 1.4 0 0 1 1.4-1.4z",
    "M8.4 19.4h7.2",
    "M12 16v3.4",
  ],
  documents: [
    "M13.4 3.4H7.8a2.2 2.2 0 0 0-2.2 2.2v13a2.2 2.2 0 0 0 2.2 2.2h8.4a2.2 2.2 0 0 0 2.2-2.2V8.2z",
    "M13.4 3.4v4.8h5",
    "M9 13.4h6",
    "M9 16.8h4",
  ],
} satisfies Record<string, Paths>;

export type IconName = keyof typeof ICON;

function toArray(p: Paths) {
  return Array.isArray(p) ? p : [p];
}

export function Icon({
  name,
  size = 16,
  strokeWidth = 1.7,
  className,
}: {
  name: IconName;
  size?: number;
  strokeWidth?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {toArray(ICON[name]).map((d, i) => (
        <path key={i} d={d} />
      ))}
    </svg>
  );
}

/* ── file marks ─────────────────────────────────────────────────────────── */

export type FileKind =
  | "folder"
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "doc"
  | "sheet"
  | "slides"
  | "code"
  | "text"
  | "archive"
  | "disk"
  | "file";

/** Solid two-tone folder: back panel with the tab, front panel over it. */
const FOLDER_BACK =
  "M2.4 6.6A2.4 2.4 0 0 1 4.8 4.2h3.85c.53 0 1.04.17 1.46.49l1.72 1.29h7.39a2.4 2.4 0 0 1 2.4 2.4V10H2.4z";
const FOLDER_FRONT = "M2.4 9.2h19.2v8.4a2.4 2.4 0 0 1-2.4 2.4H4.8a2.4 2.4 0 0 1-2.4-2.4z";

/** Sheet of paper with a turned-down corner — the base every document sits on. */
const PAGE_BODY =
  "M5.6 4.7A2.2 2.2 0 0 1 7.8 2.5h5.05L18.4 8.1v11.2a2.2 2.2 0 0 1-2.2 2.2H7.8a2.2 2.2 0 0 1-2.2-2.2z";
const PAGE_FOLD = "M12.85 2.5 18.4 8.1h-3.85a1.7 1.7 0 0 1-1.7-1.7z";

/** Glyphs live inside the page, roughly x 8–16, y 11.5–19. */
const GLYPH: Partial<Record<FileKind, { stroke?: string[]; fill?: string[] }>> = {
  text: { stroke: ["M8.6 12.6h6.8", "M8.6 15.4h6.8", "M8.6 18.2h4.2"] },
  doc: { stroke: ["M8.6 12.6h6.8", "M8.6 15.4h6.8", "M8.6 18.2h4.2"] },
  pdf: {
    fill: ["M7.9 13.9h8.2a.9.9 0 0 1 .9.9v3.4a.9.9 0 0 1-.9.9H7.9a.9.9 0 0 1-.9-.9v-3.4a.9.9 0 0 1 .9-.9z"],
    stroke: [],
  },
  sheet: { stroke: ["M8.4 12.5h7.2v6.4H8.4z", "M8.4 14.9h7.2", "M8.4 17h7.2", "M11.4 12.5v6.4"] },
  slides: { stroke: ["M8.4 12.6h7.2v5.6H8.4z"], fill: ["M10.1 16.9h1.1v-2.3h-1.1zM12.05 16.9h1.1v-3.4h-1.1zM14 16.9h1.1v-1.5H14z"] },
  code: { stroke: ["m10.7 12.9-2.5 2.9 2.5 2.9", "m13.5 12.9 2.5 2.9-2.5 2.9"] },
  video: { fill: ["M10.1 12.8v6l5.1-3z"] },
  audio: {
    stroke: ["M10.6 17.7v-5.1l5-1.15v5.1"],
    fill: [
      "M9.15 16.35a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3",
      "M14.15 15.2a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3",
    ],
  },
  image: {
    fill: [
      "M8.1 18.7 10.8 15l1.85 2.1 1.7-2 2.15 3.6z",
      "M10.35 13.3a1 1 0 1 1-2 0 1 1 0 0 1 2 0",
    ],
  },
  archive: {
    stroke: ["M12 11.6v1.3", "M12 14.2v1.3"],
    fill: ["M10.6 16.6h2.8a.9.9 0 0 1 .9.9v2.1a.9.9 0 0 1-.9.9h-2.8a.9.9 0 0 1-.9-.9v-2.1a.9.9 0 0 1 .9-.9z"],
  },
};

/**
 * Colour-coded mark for a directory entry. Colour comes from `--ft-*` tokens so
 * both themes can retune the palette without touching the geometry.
 */
export function FileIcon({
  kind,
  size = 20,
  className,
}: {
  kind: FileKind;
  size?: number;
  className?: string;
}) {
  const cls = ["file-icon", `fi-${kind}`, className].filter(Boolean).join(" ");
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    className: cls,
    "aria-hidden": true as const,
  };

  if (kind === "folder") {
    return (
      <svg {...common} fill="currentColor">
        <path d={FOLDER_BACK} opacity="0.55" />
        <path d={FOLDER_FRONT} />
      </svg>
    );
  }

  if (kind === "disk") {
    return (
      <svg {...common} fill="currentColor">
        <path d="M3.6 5.4h16.8a1.8 1.8 0 0 1 1.8 1.8v9.6a1.8 1.8 0 0 1-1.8 1.8H3.6a1.8 1.8 0 0 1-1.8-1.8V7.2a1.8 1.8 0 0 1 1.8-1.8z" opacity="0.55" />
        <path d="M6.4 14.4h6.2v1.6H6.4zM17 14.2a1.1 1.1 0 1 1 0 2.2 1.1 1.1 0 0 1 0-2.2" />
      </svg>
    );
  }

  const glyph = GLYPH[kind];
  return (
    <svg {...common} fill="none">
      <path d={PAGE_BODY} fill="currentColor" fillOpacity="0.14" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
      <path d={PAGE_FOLD} fill="currentColor" fillOpacity="0.42" />
      {glyph?.fill?.map((d, i) => (
        <path key={`f${i}`} d={d} fill="currentColor" />
      ))}
      {glyph?.stroke?.map((d, i) => (
        <path
          key={`s${i}`}
          d={d}
          stroke="currentColor"
          strokeWidth="1.3"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ))}
      {kind === "pdf" && (
        <text
          x="12"
          y="17.55"
          textAnchor="middle"
          fontSize="3.9"
          fontWeight="700"
          letterSpacing="0.15"
          fill="var(--color-bg)"
          stroke="none"
        >
          PDF
        </text>
      )}
    </svg>
  );
}
