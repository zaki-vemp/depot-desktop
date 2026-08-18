//! Dragging files *out* of Depot and into another application.
//!
//! A webview can only offer what HTML drag-and-drop offers — text — so the
//! browser drag is cancelled in the front end and a native one is started here
//! instead. The receiver then sees real files: a drop on Files, Finder or
//! Explorer copies them, a drop on a mail client attaches them.
//!
//! On Windows and macOS the `drag` crate does the platform work. GTK is done
//! here because the crate hands the paths over as `file://{path}`, which loses
//! every name containing a space or any other character a URI has to escape.

use std::path::PathBuf;
use tauri::WebviewWindow;

/// Starts the drag for `paths`. It ends when the user drops or lets go, so the
/// call only reports whether the gesture could be handed to the platform.
pub fn start(window: &WebviewWindow, paths: Vec<PathBuf>) -> Result<(), String> {
    let handle = window.clone();
    window
        .run_on_main_thread(move || begin(&handle, paths))
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn begin(window: &WebviewWindow, paths: Vec<PathBuf>) {
    use gtk::prelude::*;
    use gtk::gdk;
    use std::cell::RefCell;
    use std::rc::Rc;

    let Ok(gtk_window) = window.gtk_window() else {
        return;
    };
    let uris: Vec<String> = paths
        .iter()
        .map(|path| crate::clipboard::file_uri(path))
        .collect();

    gtk_window.drag_source_set(
        gdk::ModifierType::BUTTON1_MASK,
        &[],
        gdk::DragAction::COPY | gdk::DragAction::MOVE,
    );
    gtk_window.drag_source_add_uri_targets();

    let handlers = Rc::new(RefCell::new(Vec::new()));
    handlers.borrow_mut().push(gtk_window.connect_drag_data_get(
        move |_, _, selection, _, _| {
            let list: Vec<&str> = uris.iter().map(String::as_str).collect();
            selection.set_uris(&list);
        },
    ));

    // The window is a drag source for this gesture only: left in place, the
    // handler would answer every later drag inside the webview with these same
    // paths.
    let pending = handlers.clone();
    let end = gtk_window.connect_drag_end(move |source, _| {
        source.drag_source_unset();
        for handler in pending.borrow_mut().drain(..) {
            source.disconnect(handler);
        }
    });
    handlers.borrow_mut().push(end);

    if let Some(targets) = gtk_window.drag_source_get_target_list() {
        // `-1, -1` means "start from where the pointer is", which is where the
        // press that the front end reported still has the button down.
        gtk_window.drag_begin_with_coordinates(
            &targets,
            gdk::DragAction::COPY,
            gdk::ffi::GDK_BUTTON_PRIMARY as i32,
            None,
            -1,
            -1,
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn begin(window: &WebviewWindow, paths: Vec<PathBuf>) {
    /// What the cursor carries during the drag. The app icon is the one image
    /// every platform build already ships.
    const PREVIEW: &[u8] = include_bytes!("../icons/32x32.png");

    let _ = drag::start_drag(
        window,
        drag::DragItem::Files(paths),
        drag::Image::Raw(PREVIEW.to_vec()),
        |_result, _cursor| {},
        drag::Options::default(),
    );
}
