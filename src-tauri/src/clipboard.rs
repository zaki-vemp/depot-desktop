//! Files on the *system* clipboard.
//!
//! Depot's own Copy/Cut only moves entries between its own tabs. This module
//! puts the same selection where the rest of the desktop can see it, so a copy
//! here and a paste in Files, Finder or Explorer is one gesture. No crate does
//! that portably — each platform wants a different flavour of file list — so
//! all three are spelled out here.
//!
//! The clipboard is served by the running process on X11 and Wayland: the
//! promise lives as long as Depot does, exactly like every other GTK app.

use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// A percent-encoded `file://` URI. A raw space is enough to make a receiving
/// app reject the whole list, so every byte outside the unreserved set — plus
/// the separators a path actually needs — goes out as `%XX`.
pub fn file_uri(path: &Path) -> String {
    #[cfg(unix)]
    let raw: Vec<u8> = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    // A Windows path is text, and a URI wants forward slashes and a leading
    // one: `C:\a b` becomes `file:///C:/a%20b`.
    #[cfg(not(unix))]
    let raw: Vec<u8> = path.to_string_lossy().replace('\\', "/").into_bytes();

    let mut uri = String::from("file://");
    if !raw.starts_with(b"/") {
        uri.push('/');
    }
    for byte in raw {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// Puts `paths` on the system clipboard as files.
///
/// `cut` asks the receiver to move rather than copy. GTK file managers and
/// Explorer honour it; Finder ignores it, because macOS has no cut-and-paste
/// for files — the paste lands as a copy there.
pub async fn copy_files(app: &AppHandle, paths: Vec<PathBuf>, cut: bool) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    // Every backend below is main-thread only: GTK asserts it, `NSPasteboard`
    // wants the UI thread, and the Win32 clipboard belongs to the thread that
    // owns the window.
    app.run_on_main_thread(move || {
        let _ = tx.send(write(&paths, cut));
    })
    .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|_| "the clipboard write never reported back".to_string())?
}

#[cfg(target_os = "linux")]
fn write(paths: &[PathBuf], cut: bool) -> Result<(), String> {
    use gtk::gdk;
    use gtk::{Clipboard, TargetEntry, TargetFlags};

    const GNOME: u32 = 0;
    const URI_LIST: u32 = 1;
    const KDE_CUT: u32 = 2;
    const TEXT: u32 = 3;

    let uris: Vec<String> = paths.iter().map(|path| file_uri(path)).collect();
    // Nautilus, Nemo, Caja, Thunar and PCManFM read both the operation and the
    // list from this one target; plain `text/uri-list` cannot say "cut".
    let gnome = format!(
        "{}\n{}",
        if cut { "cut" } else { "copy" },
        uris.join("\n")
    );
    let uri_list = format!("{}\r\n", uris.join("\r\n"));
    // Pasting into a terminal or an editor should give the paths themselves.
    let text = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let display = gdk::Display::default().ok_or("No display is available")?;
    let clipboard = Clipboard::default(&display).ok_or("No clipboard is available")?;
    let targets = vec![
        TargetEntry::new("x-special/gnome-copied-files", TargetFlags::empty(), GNOME),
        TargetEntry::new("text/uri-list", TargetFlags::empty(), URI_LIST),
        // KDE keeps the cut flag in a target of its own and takes the paths
        // from `text/uri-list`.
        TargetEntry::new(
            "application/x-kde-cutselection",
            TargetFlags::empty(),
            KDE_CUT,
        ),
        TargetEntry::new("UTF8_STRING", TargetFlags::empty(), TEXT),
        TargetEntry::new("text/plain;charset=utf-8", TargetFlags::empty(), TEXT),
    ];

    let claimed = clipboard.set_with_data(&targets, move |_, selection, info| {
        match info {
            GNOME => selection.set(
                &gdk::Atom::intern("x-special/gnome-copied-files"),
                8,
                gnome.as_bytes(),
            ),
            URI_LIST => selection.set(&gdk::Atom::intern("text/uri-list"), 8, uri_list.as_bytes()),
            KDE_CUT => selection.set(
                &gdk::Atom::intern("application/x-kde-cutselection"),
                8,
                if cut { b"1" } else { b"0" },
            ),
            _ => {
                selection.set_text(&text);
            }
        };
    });

    if claimed {
        Ok(())
    } else {
        Err("Another app is holding the clipboard".into())
    }
}

#[cfg(target_os = "windows")]
fn write(paths: &[PathBuf], cut: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::w;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, POINT};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::DROPFILES;

    // `CF_HDROP` is a `DROPFILES` header followed by the wide paths, each one
    // terminated, and one more terminator to close the list.
    let mut names: Vec<u16> = Vec::new();
    for path in paths {
        names.extend(path.as_os_str().encode_wide());
        names.push(0);
    }
    names.push(0);
    let header = std::mem::size_of::<DROPFILES>();

    unsafe {
        OpenClipboard(None).map_err(|e| e.to_string())?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;

            let drop = GlobalAlloc(GMEM_MOVEABLE, header + names.len() * 2)
                .map_err(|e| e.to_string())?;
            let base = GlobalLock(drop) as *mut u8;
            if base.is_null() {
                let _ = GlobalFree(Some(drop));
                return Err("Windows refused the clipboard allocation".into());
            }
            std::ptr::write(
                base as *mut DROPFILES,
                DROPFILES {
                    pFiles: header as u32,
                    pt: POINT { x: 0, y: 0 },
                    fNC: false.into(),
                    fWide: true.into(),
                },
            );
            std::ptr::copy_nonoverlapping(
                names.as_ptr() as *const u8,
                base.add(header),
                names.len() * 2,
            );
            let _ = GlobalUnlock(drop);
            // The clipboard owns the block from here on, so it must not be freed.
            SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(drop.0))).map_err(|e| e.to_string())?;

            // Without this, Explorer pastes a cut as a copy.
            let effect_format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
            if effect_format != 0 {
                let effect = GlobalAlloc(GMEM_MOVEABLE, 4).map_err(|e| e.to_string())?;
                let slot = GlobalLock(effect) as *mut u32;
                if slot.is_null() {
                    let _ = GlobalFree(Some(effect));
                } else {
                    // DROPEFFECT_MOVE / DROPEFFECT_COPY.
                    std::ptr::write(slot, if cut { 2 } else { 1 });
                    let _ = GlobalUnlock(effect);
                    let _ = SetClipboardData(effect_format, Some(HANDLE(effect.0)));
                }
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

#[cfg(target_os = "macos")]
fn write(paths: &[PathBuf], _cut: bool) -> Result<(), String> {
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};

    // Finder pastes whatever file URLs are on the general pasteboard; there is
    // no cut, so `cut` cannot be honoured here.
    let files: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = paths
        .iter()
        .map(|path| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            ProtocolObject::from_retained(url)
        })
        .collect();

    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    if pasteboard.writeObjects(&NSArray::from_retained_slice(&files)) {
        Ok(())
    } else {
        Err("The pasteboard refused the file list".into())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn write(_paths: &[PathBuf], _cut: bool) -> Result<(), String> {
    Err("This platform has no file clipboard".into())
}
