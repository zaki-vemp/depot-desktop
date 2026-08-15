# design/ — vendored design source

Harbor's interface came from a Claude Design project. That project lives behind
the owner's claude.ai login, so anyone else — a teammate, a fresh agent session,
CI — cannot fetch it. **This folder is a frozen copy so the design is readable
from the repo alone.**

Imported 2026-08-14 from
`https://claude.ai/design/p/57774789-9fc5-4f8f-a1f8-73b3245904b7`
(read-only; nothing has ever been written back to the design project).

## Files

| Path | What it is |
| --- | --- |
| `Harbor Organic.dc.html` | The interface mock: full layout, every pane, and the dark-theme token block. Placeholder data. |
| `_ds/organic-…/styles.css` | **The design system.** Tokens, ramps, type, radii, shadows, component classes. The source of truth for the look. |
| `_ds/organic-…/readme.md` | The system's own written guidance — direction, colour and type rules, component table, do / don't. Read this before changing any visual decision. |
| `_ds/organic-…/_ds_bundle.js` | Component bundle for the mock. Effectively empty — this system is plain CSS. |
| `support.js` | The design-canvas runtime that renders `.dc.html` (`<x-dc>`, `sc-for`, `sc-if`, `{{ }}` bindings). |

The directory layout matches the design project exactly, because the mock
references `./support.js` and `_ds/organic-…/styles.css` by relative path.

## Viewing the mock

It needs to be served over HTTP — `file://` will not resolve the relative
scripts:

```bash
npx serve design
```

Then open `Harbor Organic.dc.html`. The runtime pulls React from unpkg and the
stylesheet pulls Caprasimo/Figtree from Google Fonts, so **rendering the mock
needs network**. Verified working this way.

## How it maps into the app

| Design | Code |
| --- | --- |
| `_ds/organic-…/styles.css` tokens and component classes | [`src/App.css`](../src/App.css) — copied, then extended with app layout |
| The `[data-theme="dark"]` block inside `Harbor Organic.dc.html` | The dark block in `src/App.css` |
| Icon paths in the mock's `ICON` map | [`src/lib/icons.tsx`](../src/lib/icons.tsx) |
| Mock layout and panes | [`src/App.tsx`](../src/App.tsx) |

The mock's content is placeholder — invented files, drives, torrents and
percentages. Everything the app shows is real data from the Rust backend; only
the visual language came across.

## Which file wins

`src/App.css` is what ships and what you edit. This folder is a **reference
snapshot**, not a build input — nothing imports from it.

If the design project changes and you want the new version:

1. Re-read the files from the design project (`DesignSync` `get_file`, or the
   download button in the claude.ai design UI).
2. Overwrite the copies here so the snapshot stays honest.
3. Port the token block at the top of `styles.css` into `src/App.css` — that
   block is where the look lives; the app layout below it is Harbor's own and
   should be left alone.
