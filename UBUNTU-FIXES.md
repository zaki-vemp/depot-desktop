# Ubuntu / Linux Build & Runtime Fixes

This document records the two issues hit while running this repo on Ubuntu 24.04,
the exact errors that triggered them, their root causes, and the fixes applied.
All fixes are cross-platform: they activate only on Linux and are no-ops on
Windows and macOS.

Environment where the issues were reproduced:

| Component | Value |
|---|---|
| OS | Ubuntu 24.04 |
| Display server | X11 |
| GPU | NVIDIA Quadro K4200 (Kepler) |
| Driver | nvidia-driver-470 (legacy branch, 470.256.02) |
| WebKitGTK | 2.52.3 |
| Rust | 1.95.0 |
| Disk | 219 GB root partition, **99% full at the time** |

---

## Issue 1 — Build fails: `ld terminated with signal 7 [Bus error]`

### When it happened

Running `npm run tauri dev` — compilation succeeded but the final **linking step
crashed**:

```text
$ npm run tauri dev

     Running DevCommand (`cargo  run --no-default-features --color always --`)
   Compiling depot v0.1.0 (/home/vemp-zaki/Downloads/depot-desktop/src-tauri)
error: linking with `cc` failed: exit status: 1
  = note: some arguments are omitted. use `--verbose` to show all linker arguments
  = note: collect2: fatal error: ld terminated with signal 7 [Bus error], core dumped
          compilation terminated.

error: could not compile `depot` (lib) due to 1 previous error
```

### Root cause

**The disk was 99% full — only 4.1 GB free:**

```text
$ df -h /
Filesystem      Size  Used Avail Use% Mounted on
/dev/sda3       219G  204G  4.1G  99% /
```

The linker writes its output file through memory-mapped I/O. When the disk runs
out of space mid-write, the kernel raises **SIGBUS (signal 7)** and kills `ld`.
This is an environmental failure, not a code or OS bug — it can happen on any
platform when free space runs out during linking.

A Tauri debug build with this dependency tree (librqbit, reqwest, GTK,
webkit2gtk, …) had already consumed **8.9 GB** inside `src-tauri/target/`, so
the final link had no room left. Additionally, `~/.npm` had accumulated **12 GB**
of package cache.

### Fixes applied

**1. Reclaimed disk space (environment cleanup, no repo change):**

```bash
cd src-tauri && cargo clean        # removed 8.9 GiB of stale/partial artifacts
npm cache clean --force            # removed ~12 GB of regenerable npm cache
```

Result: **4.1 GB → 33 GB free**.

**2. `src-tauri/Cargo.toml`** — leaner debug/release profiles so future builds
need far less disk (and link faster) on every platform:

```toml
[profile.dev]
debug = "line-tables-only"

[profile.release]
debug = "line-tables-only"
strip = "debuginfo"
```

Why this is safe:

- `debug = "line-tables-only"` keeps line numbers in panic backtraces, so
  debugging still works — only variable/type debug info is dropped.
- Cargo profiles are platform-independent: identical behaviour on Windows,
  macOS and Linux.
- Measured effect on this machine: a full clean build now occupies
  **2.9 GB instead of 8.9 GB** in `target/`.

### Verification

Clean rebuild after the fixes:

```text
   Compiling webkit2gtk v2.0.2
   Compiling tao v0.35.3
   Compiling muda v0.19.3
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 52s
```

Both link outputs were produced successfully:

```text
target/debug/depot                127 MB  (main executable)
target/debug/deps/libdepot_lib.so  72 MB  (the cdylib that previously crashed)
```

---

## Issue 2 — Blank webview: `Failed to create GBM buffer ... Permission denied`

### When it happened

After the build succeeded, `npm run tauri dev` launched the window but the log
flooded with:

```text
pci id for fd 21: 10de:11b4, driver (null)
pci id for fd 22: 10de:11b4, driver (null)
pci id for fd 23: 10de:11b4, driver (null)
KMS: DRM_IOCTL_MODE_CREATE_DUMB failed: Permission denied
KMS: DRM_IOCTL_MODE_CREATE_DUMB failed: Permission denied
Failed to create GBM buffer of size 1440x900: Permission denied
KMS: DRM_IOCTL_MODE_CREATE_DUMB failed: Permission denied
KMS: DRM_IOCTL_MODE_CREATE_DUMB failed: Permission denied
Failed to create GBM buffer of size 1440x900: Permission denied
```

The webview renders blank/black when this happens.

### Root cause

Since **WebKitGTK 2.40**, the default renderer allocates its buffers through
the GPU via **GBM/DMA-BUF**. The **NVIDIA proprietary 470 driver** (the last
branch supporting Kepler-generation cards like the Quadro K4200) does not
implement the GBM/DMA-BUF interfaces WebKit needs, so every buffer allocation
is denied (`driver (null)` in the `pci id` lines is the giveaway: GLVND cannot
match the device to a usable driver path).

This is a well-known compatibility gap between WebKitGTK ≥ 2.40 and the NVIDIA
legacy driver series, documented across the Tauri, WebKit and Electron issue
trackers. The workaround is to opt WebKitGTK back onto its classic renderer with
`WEBKIT_DISABLE_DMABUF_RENDERER=1`.

### Fix applied

**`src-tauri/src/main.rs`** — set the environment variable programmatically at
startup, before the Tauri app is created:

```rust
#[cfg(target_os = "linux")]
{
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}
```

Why it is written this way:

- `#[cfg(target_os = "linux")]` — the code is **compiled out entirely** on
  Windows and macOS, which use WebView2 and WKWebView (not WebKitGTK), so those
  platforms cannot be affected.
- The `if ... is_none()` guard means a developer can still override the
  behaviour explicitly, e.g. `WEBKIT_DISABLE_DMABUF_RENDERER=0 npm run tauri dev`
  to test the DMA-BUF path on a machine where it works.
- Setting it in code (rather than a shell export or `.desktop` file) means the
  fix ships with the app for every user and every launch method.

**`README.md`** — added a *Linux troubleshooting* subsection under
*Development* explaining the automatic env var, the failure mode, how to
override it, and that the remaining `libEGL warning` lines are harmless.

### Verification

1. Incremental rebuild: `Finished dev profile target(s) in 1.85s`.
2. Ran the binary for 12 seconds with output captured — **zero** occurrences of
   `GBM buffer` / `DRM_IOCTL_MODE_CREATE_DUMB` / `KMS` errors (previously
   dozens), and the app stayed alive until the timeout killed it.
3. Took an X11 screenshot while the app was running and analysed its pixels:

   ```text
   GBM error count in run log : 0
   unique colors in screenshot: 1323
   luminance mean/stdev       : 227.0 / 72.0 (range 13–255)
   verdict                    : RENDERED CONTENT (rich colors)
   ```

   A blank/failed webview would be a near-uniform image; instead the window
   shows the real UI (light file pane + dark sidebar/text).

### Remaining harmless messages

These lines may still appear in the console on NVIDIA 470 + X11 and can be
ignored — they are GLVND probing noise present in virtually every OpenGL
application on this driver generation:

```text
libEGL warning: pci id for fd 21: 10de:11b4, driver (null)
libEGL warning: egl: failed to create dri2 screen
libEGL warning: DRI2: failed to create screen
```

---

## Summary of all file changes

| File | Change | Reason |
|---|---|---|
| `src-tauri/Cargo.toml` | Added `[profile.dev]` / `[profile.release]` with `debug = "line-tables-only"` (+ `strip = "debuginfo"` in release) | Halve build artifact size on all platforms to avoid disk-exhaustion link failures (Issue 1) |
| `src-tauri/src/main.rs` | Set `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Linux unless already set | WebKitGTK DMA-BUF renderer is incompatible with NVIDIA legacy 470 driver → blank webview (Issue 2) |
| `README.md` | New *Linux troubleshooting* note under Development | Document the automatic workaround and override for contributors |
| `UBUNTU-FIXES.md` | This document | Record errors, causes and fixes |
| (environment, not repo) | `cargo clean` + `npm cache clean --force` | Freed 29 GB so the build could link successfully (Issue 1) |

## Prevention tips

- Keep **≥ 10 GB free** on the drive holding `src-tauri/target/`; Rust debug
  builds fail in confusing ways (including SIGBUS link crashes) when the disk
  fills up. Check with `df -h` occasionally.
- Periodically reclaim caches: `cargo clean` in projects you are not actively
  hacking on, `npm cache clean --force`, and old snap revisions
  (`snap list --all`).
- If a Linux machine with an NVIDIA legacy driver shows a blank Tauri window,
  check the console output for `Failed to create GBM buffer` before suspecting
  app code.

