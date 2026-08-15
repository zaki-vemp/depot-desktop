# Depot

**A fast, cross-platform desktop file explorer that also speaks cloud.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8DB.svg)](https://tauri.app)
[![React 19](https://img.shields.io/badge/React-19-61DAFB.svg)](https://react.dev)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)](#install)

Depot is a local file manager first: tabs, breadcrumbs, grid and list views, copy/cut/paste,
transfers with real progress. On top of that it connects **multiple Google Drive accounts**,
opens **video / audio / image / PDF / Office / text** files in tabs, browses the web in
**native child webviews**, and downloads **magnet / `.torrent`** links you already have the
right to obtain.

Built with **Tauri 2 + React 19 + TypeScript + Rust** — a small binary against the system
webview, not a bundled Chromium.

<!-- Screenshots: drop them in docs/ and reference them here, e.g.
![Depot, light theme](docs/screenshot-light.png)
![Depot, dark theme](docs/screenshot-dark.png) -->


---

## Contents

- [Features](#features)
- [Install](#install)
- [First run](#first-run)
- [Operations](#operations)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Where Depot stores things](#where-depot-stores-things)
- [Privacy and network activity](#privacy-and-network-activity)
- [Architecture](#architecture)
- [Development](#development)
- [Status and roadmap](#status-and-roadmap)
- [Contributing](#contributing)
- [License](#license)

---

## Features

### Files

- Tabbed browsing with per-tab back / forward history and clickable breadcrumbs
- Grid and list views, colour-coded file-type marks, live thumbnails for local images
- Places sidebar (Home, Desktop, Documents, Downloads, Pictures, Music, Movies) and volumes
  (`/Volumes` on macOS, drive letters on Windows, `/media` `/mnt` `/run/media` on Linux)
- Click, ⌘/Ctrl-click and Shift-click selection, right-click context menu
- New folder, rename, copy, cut, paste, reveal in Finder/Explorer, open with…
- Delete to Trash by default; permanent delete sits behind a typed `DELETE` confirmation
- Filter box, hidden-file toggle, folders-first sorting
- Inspector panel: kind, size, modified, source, full path, per-item actions
- Status bar with item counts, selection counts and free space on the current volume

### Transfers

- Copies, moves and Drive downloads stream in 512 KB chunks from Rust and report real byte counts
- Same-volume moves take the rename fast path; folders copy file by file
- Transfers tab shows progress, throughput, state and errors; the queue runs serially

### Previews and players

- Video and audio player with scrubbing, volume, rate, loop, picture-in-picture and subtitles
- Local video plays through **libvlc** (from your installed VLC), so MKV / AVI / HEVC and other
  codecs a webview cannot decode play without converting
- Images, PDFs and text/code files render in-tab; Quick Look with <kbd>Space</kbd>
- Office previews, read-only: `.xlsx` `.xlsm` `.xls` `.xlsb` `.ods` `.csv` `.tsv` as sheets,
  `.docx` `.odt` `.pptx` `.odp` as extracted text

### Cloud drives

- Multiple **Google Drive** accounts side by side, browsable like a local folder
- Copy and paste between Drive accounts and local folders; downloads run through the transfer queue
- Drive quota shown per account
- **OneDrive**, **Dropbox** and **S3-compatible** storage: credential setup and web access today,
  native list/download adapters still to come

### Web and app tabs

- Website tabs run in a real native child webview, so sites that refuse framing still load
- Dedicated **Facebook** and **Instagram** app tabs — singleton, branded, no address bar,
  with cookies and page state that survive tab switches

### Torrents

- Magnet links and `.torrent` URLs via [rqbit](https://github.com/ikatson/rqbit)
- Pause / resume, per-torrent progress and speed, configurable download directory
- **No search, no indexer** — you supply the link

### Interface

- Off-white light theme and graphite dark theme, toggled in the title bar and remembered
- Collapsible sidebar and inspector (<kbd>F</kbd> hides both for a full-width pane)
- Native UI font stack; no web fonts, no network calls to render the interface

---

## Install

### Requirements

| | |
|---|---|
| Node.js | 18 or newer |
| Rust | stable toolchain via [rustup](https://rustup.rs) |
| Webview | WebView2 (Windows), WebKitGTK (Linux), system WebKit (macOS) |
| VLC | optional — [VLC Media Player](https://www.videolan.org/vlc/) for in-app video. Depot loads `libvlc` from the installed app; it is not bundled |

Linux also needs the Tauri system packages (Debian / Ubuntu):

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### Run from source

```bash
git clone https://github.com/zaki-vemp/depot-desktop.git
```

```bash
cd depot-desktop && npm install
```

```bash
npm run tauri dev
```

The first Rust build takes a few minutes; later runs are incremental.

### Build installers

```bash
npm run tauri build
```

Output lands in `src-tauri/target/release/bundle/`:

| Platform | Artifacts |
|---|---|
| macOS | `.app`, `.dmg` |
| Windows | `.msi`, `.exe` (NSIS) |
| Linux | `.deb`, `.AppImage` |

Builds are unsigned. Distributing them outside your own machine means code-signing and
notarising them yourself.

---

## First run

### Google Drive

1. Open the [Google Cloud Console](https://console.cloud.google.com/) and create a project
2. Enable the **Google Drive API**
3. **Credentials → Create OAuth client → Desktop app**
4. Add the redirect URI `http://127.0.0.1:17843/callback`
5. Paste the Client ID and Client secret into **Settings**, then save
6. Open **Connections → Sign in with Google**; repeat and pick another account to add more

Sign-in uses the system browser with PKCE. Depot never sees your Google password, and the
tokens land in the app-data directory — never in this repository.

### Other providers

**Settings** stores credentials for the adapters that are still being built:

- **OneDrive** — Microsoft Entra application (client) ID and client secret
- **Dropbox** — scoped app key/client ID and app secret
- **S3-compatible** — endpoint (optional for AWS), region, bucket, access key ID, secret access key

Until their native adapters ship, OneDrive and Dropbox open as ordinary website tabs.

### Torrent downloads

Set a download directory in **Settings** (defaults to your system Downloads folder), then paste
a magnet or `.torrent` URL in the **Torrents** tab.

---

## Operations

### File operations

| Operation | How | Notes |
|---|---|---|
| Open | Double-click, or <kbd>Enter</kbd> | Folders navigate in place; files open a preview tab |
| Quick look | <kbd>Space</kbd> | Overlay preview, no tab |
| New folder | Toolbar **New folder** | Created in the current directory |
| Rename | Context menu → Rename | In-place dialog |
| Copy / cut / paste | <kbd>⌘/Ctrl</kbd> + <kbd>C</kbd> / <kbd>X</kbd> / <kbd>V</kbd> | Cross-source: local ↔ Drive |
| Delete | <kbd>⌘/Ctrl</kbd> + <kbd>Backspace</kbd> | Trash by default; permanent delete needs a typed `DELETE` |
| Reveal | Context menu → Reveal | Opens Finder / Explorer / file manager at the item |
| Open with | Context menu → Open with | Lists installed handlers; falls back to the system picker |
| Filter | Toolbar search box | Filters the current listing by name |
| Show hidden | Settings toggle | Dotfiles and system-hidden entries |

Every operation runs as a Rust command over Tauri IPC — the UI never touches the filesystem
directly. Long-running work (copy, move, Drive download) becomes a tracked transfer with
progress events instead of a blocking call.

### Transfers

Queued serially, streamed in 512 KB chunks, progress emitted on a `transfer` event. Moves
inside one volume use `rename` and finish instantly; cross-volume moves copy then remove.
Failures stop that transfer only and surface the error in the Transfers tab.

### Torrents

`add_torrent` accepts a magnet URI or a `.torrent` URL, the librqbit session runs in-process,
and progress polls into the Torrents tab. Only download content you hold the rights to —
open data, freely licensed software, your own backups, public-domain media. Depot ships no
search engine or indexer.

---

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| <kbd>⌘/Ctrl</kbd> + <kbd>T</kbd> | New folder tab |
| <kbd>⌘/Ctrl</kbd> + <kbd>W</kbd> | Close tab |
| <kbd>⌘/Ctrl</kbd> + <kbd>L</kbd> | New web tab (or focus the address bar in one) |
| <kbd>⌘/Ctrl</kbd> + <kbd>R</kbd> | Refresh the listing |
| <kbd>⌘/Ctrl</kbd> + <kbd>C</kbd> / <kbd>X</kbd> / <kbd>V</kbd> | Copy / cut / paste |
| <kbd>⌘/Ctrl</kbd> + <kbd>A</kbd> | Select all |
| <kbd>⌘/Ctrl</kbd> + <kbd>Backspace</kbd> | Delete selection |
| <kbd>Backspace</kbd> | Parent folder |
| <kbd>Enter</kbd> | Open selection |
| <kbd>Space</kbd> | Quick look |
| <kbd>⌘/Ctrl</kbd> + <kbd>←</kbd> / <kbd>→</kbd> | Back / forward in tab history |
| <kbd>F</kbd> | Toggle sidebar and inspector together |
| <kbd>Esc</kbd> | Close menu, dialog or quick look |

---

## Where Depot stores things

Everything lives in the OS app-data directory for `com.depot.files`:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/com.depot.files/` |
| Windows | `%APPDATA%\com.depot.files\` |
| Linux | `~/.local/share/com.depot.files/` |

| File | Contents |
|---|---|
| `settings.json` | Provider credentials, torrent directory, UI preferences |
| `cache/` | Drive files fetched for preview |

Drive OAuth tokens are stored alongside the settings, and website cookies stay in the system
webview's own profile. Nothing is written into the repository.

---

## Privacy and network activity

- **No telemetry, no analytics, no auto-update pings.** Depot makes no network call you did not ask for.
- Google Drive traffic goes to Google's APIs with your own OAuth client.
- Website and social app tabs are real webviews — they talk to those sites directly, exactly as a browser would.
- Torrent traffic goes to the swarm for the link you pasted.
- The interface loads no remote fonts, scripts or styles.

---

## Architecture

```
src/                     React 19 + TypeScript frontend
├─ App.tsx               Shell: tabs, sidebar, toolbar, file panes, inspector, dialogs
├─ App.css               Design tokens and every component class (light + dark themes)
├─ api.ts                Typed wrappers over the Tauri IPC commands
├─ types.ts              Shared types mirroring the Rust structs
├─ lib/
│  ├─ files.ts           Extension → viewer/icon/kind mapping, byte and date formatting
│  └─ icons.tsx          Chrome stroke icons + colour-coded file marks
└─ views/                MediaPlayer, OfficePreview, OpenWith, Connections

src-tauri/src/           Rust backend
├─ lib.rs                Command registration, app state wiring, window setup
├─ files.rs              Listing, places, volumes, disk usage, trash, rename, open/reveal
├─ transfers.rs          Chunked copy/move/download with progress events
├─ drive.rs              Google OAuth (PKCE, loopback :17843) and Drive REST calls
├─ media.rs              Playback preparation, subtitle discovery, VTT conversion
├─ vlc.rs                libvlc loading and player control
├─ office.rs             Spreadsheet and document text extraction
├─ openwith.rs           Installed application handlers per platform
├─ web.rs                Native child webviews for website and app tabs
├─ torrents.rs           librqbit session, add/pause/resume/list
└─ state.rs              Settings model, load/save, cache directory

design/                  The Claude Design project the interface came from (reference only)
```

The frontend calls **57 Rust commands** across seven groups: filesystem, settings, Google
Drive, transfers, web tabs, VLC playback, and torrents. `src/api.ts` is the single place
where the IPC surface is declared, and `src/types.ts` mirrors the Rust payloads.

---

## Development

```bash
npm run tauri dev
```

```bash
npx tsc --noEmit
```

```bash
cd src-tauri && cargo check
```

| Script | Does |
|---|---|
| `npm run dev` | Vite dev server alone on :1420 (no Rust, IPC calls will fail) |
| `npm run build` | Typecheck then build the frontend into `dist/` |
| `npm run tauri dev` | Full app with hot reload |
| `npm run tauri build` | Production installers |
| `npm run tauri icon src-tauri/app-icon.png` | Regenerate every platform icon from the source PNG |

Both checks — `tsc --noEmit` and `cargo check` — must pass before a pull request. See
[CONTRIBUTING.md](CONTRIBUTING.md).

---

## Status and roadmap

**Working today:** local browsing and file operations, transfers with progress, previews and
players, Office previews, Google Drive (multi-account), website and social app tabs,
torrent downloads, light/dark themes.

**Next up:**

- [ ] Native OneDrive adapter (credentials UI already in place)
- [ ] Native Dropbox adapter
- [ ] Native S3-compatible adapter
- [ ] Upload path for Drive (local → cloud) alongside download
- [ ] Recursive search across a folder tree
- [ ] Signed and notarised release builds

---

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the dev
setup, project layout and PR checklist. Security reports go through
[SECURITY.md](SECURITY.md), not public issues.

---

## License

[MIT](LICENSE) © 2026 Syed Zakiuddin

Depot bundles no third-party binaries. `libvlc` is loaded from your own VLC install at
runtime (VLC is LGPL-2.1+ / GPL-2.0+ and remains separate), and Rust and npm dependencies keep
their own licenses.
