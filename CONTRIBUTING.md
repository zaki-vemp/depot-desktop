# Contributing to Depot

Thanks for taking a look. Issues, ideas and pull requests are all welcome.

## Getting set up

You need Node.js 18+, a stable Rust toolchain from [rustup](https://rustup.rs), and — on
Linux — the Tauri system packages listed in the [README](README.md#requirements). The first
Linux compile vendors libvlc into `src-tauri/vlc-runtime` so in-app video works without a
system VLC install.

```bash
npm install
```

```bash
npm run tauri dev
```

The first Rust build takes a few minutes. After that, frontend edits hot-reload and Rust
edits trigger an incremental rebuild.

Running `npm run dev` alone starts Vite without the Rust side. The UI renders, but every IPC
call fails — useful only for pure styling work.

## Project layout

| Path | What lives there |
|---|---|
| `src/App.tsx` | The whole shell: tabs, sidebar, toolbar, file panes, inspector, dialogs |
| `src/App.css` | Design tokens and every component class, light and dark |
| `src/api.ts` | Typed wrappers over the Tauri commands — the only place IPC is declared |
| `src/types.ts` | TypeScript mirrors of the Rust payload structs |
| `src/lib/` | Extension mapping, formatting helpers, the icon set |
| `src/views/` | Media player, Office preview, Open-with, Connections |
| `src-tauri/src/` | Rust commands, one module per domain (files, drive, transfers, …) |
| `design/` | The Claude Design project the interface came from — reference, not built |

Adding a command means touching three places: the Rust module, the `generate_handler!` list
in `src-tauri/src/lib.rs`, and the wrapper in `src/api.ts` (plus `src/types.ts` if it carries
a new payload).

## Before you open a pull request

Both checks have to pass:

```bash
npx tsc --noEmit
```

```bash
cd src-tauri && cargo check
```

CI runs the same two on macOS, Windows and Linux, plus a frontend build.

Then, please:

- Keep the change focused — one concern per PR
- Match the surrounding style rather than reformatting untouched code
- Test on at least one OS and say which one in the PR description
- Include a screenshot for anything that changes the interface
- Update the README if you change behaviour, shortcuts, or setup steps

## Style

- **TypeScript** — no `any` in new code, prefer explicit prop types, keep components in the
  file that uses them unless they are shared.
- **CSS** — use the existing tokens (`--color-*`, `--space-*`, `--radius-*`, `--ft-*`). No
  hardcoded hex outside the token blocks and brand marks, so both themes keep working.
- **Rust** — commands return `Result<T, String>`; long-running work streams progress through
  events rather than blocking the IPC call.
- **Comments** — explain why, not what. Match the density already in the file.

## Reporting bugs

Open an issue with your OS and version, the Depot version, what you did, what happened, and
what you expected. Console output from `npm run tauri dev` helps a lot.

Security problems go to [SECURITY.md](SECURITY.md), not to a public issue.

## License

By contributing you agree that your work is licensed under the [MIT License](LICENSE).
