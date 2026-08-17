//! Placement for the child webviews that back web tabs and the VLC surface.
//!
//! Windows and macOS position child webviews natively, so there `set_position`
//! and `set_size` are all this module does.
//!
//! GTK needs more. `tauri-runtime-wry` builds every child webview with
//! `build_gtk(window.default_vbox())` on Linux and the BSDs, and wry packs a
//! `GtkBox` child with expand and fill set. Two webviews therefore split the
//! window between them instead of one floating over the other, and the bounds
//! calls are silently ignored: wry only honours them when the parent is a
//! `GtkFixed` or when the webview owns an X11 child window, and neither holds on
//! that path. So on GTK this module swaps the window's vertical box for a
//! `GtkFixed`, keeps the main webview stretched across it, and moves the child
//! webviews itself.

use tauri::AppHandle;

/// Parked webviews sit far outside the window rather than being hidden, so the
/// page keeps its state and keeps running while another tab is in front.
const OFFSCREEN: f64 = -20000.0;

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
mod gtk_impl {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use gtk::prelude::*;
    use tauri::{AppHandle, Manager};

    const PARKED: i32 = super::OFFSCREEN as i32;

    thread_local! {
        /// Child webview widgets by label. GTK objects are not `Send`, so every
        /// entry is written and read on the main thread.
        static CHILDREN: RefCell<HashMap<String, gtk::Widget>> = RefCell::new(HashMap::new());
    }

    /// The `GtkFixed` that floats the child webviews over the main one,
    /// installing the layout on first use.
    fn fixed(app: &AppHandle) -> Option<gtk::Fixed> {
        let vbox = app.get_window("main")?.default_vbox().ok()?;
        if let Some(overlay) = vbox
            .children()
            .into_iter()
            .find_map(|child| child.downcast::<gtk::Overlay>().ok())
        {
            return overlay
                .children()
                .into_iter()
                .find_map(|child| child.downcast::<gtk::Fixed>().ok());
        }
        install(&vbox)
    }

    /// Rehome the main webview under a `GtkOverlay` and float a `GtkFixed` on
    /// top of it. The overlay stretches its base child to the whole window on
    /// every resize, which a `GtkFixed` on its own will not do — a `GtkFixed`
    /// hands each child exactly the size it asked for.
    fn install(vbox: &gtk::Box) -> Option<gtk::Fixed> {
        // The main webview is packed first and is only ever removed here, so it
        // is the first child.
        let main = vbox.children().into_iter().next()?;
        let overlay = gtk::Overlay::new();
        let fixed = gtk::Fixed::new();
        vbox.remove(&main);
        vbox.pack_start(&overlay, true, true, 0);
        overlay.add(&main);
        overlay.add_overlay(&fixed);
        // The `GtkFixed` spans the whole window, so without this it swallows
        // every click before the main webview sees one. Pass-through only makes
        // the layer itself transparent to input; the child webviews inside it
        // own their windows and still get their events.
        overlay.set_overlay_pass_through(&fixed, true);
        overlay.show_all();
        Some(fixed)
    }

    /// Install the `GtkFixed` before any child webview exists, so `install` can
    /// never mistake a child for the main webview.
    pub fn prepare(app: &AppHandle) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            fixed(&handle);
        });
    }

    pub fn adopt(app: &AppHandle, label: &str) -> Result<(), String> {
        let handle = app.clone();
        let label = label.to_string();
        app.run_on_main_thread(move || {
            let Some(fixed) = fixed(&handle) else {
                return;
            };
            let Some(vbox) = handle
                .get_window("main")
                .and_then(|window| window.default_vbox().ok())
            else {
                return;
            };
            // wry packed the new webview straight into the vertical box, next to
            // the `GtkOverlay` installed above — which is the only other child.
            let Some(widget) = vbox
                .children()
                .into_iter()
                .find(|child| !child.is::<gtk::Overlay>())
            else {
                return;
            };
            vbox.remove(&widget);
            fixed.put(&widget, PARKED, PARKED);
            widget.show_all();
            CHILDREN.with(|children| children.borrow_mut().insert(label, widget));
        })
        .map_err(|e| e.to_string())
    }

    pub fn place(
        app: &AppHandle,
        label: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<(), String> {
        let handle = app.clone();
        let label = label.to_string();
        app.run_on_main_thread(move || {
            let Some(fixed) = fixed(&handle) else {
                return;
            };
            CHILDREN.with(|children| {
                let children = children.borrow();
                let Some(widget) = children.get(&label) else {
                    return;
                };
                let size = (
                    width.round().max(1.0) as i32,
                    height.round().max(1.0) as i32,
                );
                if widget.size_request() != size {
                    widget.set_size_request(size.0, size.1);
                }
                fixed.move_(widget, x.round() as i32, y.round() as i32);
            });
        })
        .map_err(|e| e.to_string())
    }

    pub fn park(app: &AppHandle, label: &str) -> Result<(), String> {
        let handle = app.clone();
        let label = label.to_string();
        app.run_on_main_thread(move || {
            let Some(fixed) = fixed(&handle) else {
                return;
            };
            CHILDREN.with(|children| {
                if let Some(widget) = children.borrow().get(&label) {
                    fixed.move_(widget, PARKED, PARKED);
                }
            });
        })
        .map_err(|e| e.to_string())
    }

    pub fn forget(app: &AppHandle, label: &str) {
        let label = label.to_string();
        let _ = app.run_on_main_thread(move || {
            CHILDREN.with(|children| children.borrow_mut().remove(&label));
        });
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
mod gtk_impl {
    use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager};

    pub fn prepare(_app: &AppHandle) {}

    pub fn adopt(_app: &AppHandle, _label: &str) -> Result<(), String> {
        Ok(())
    }

    pub fn place(
        app: &AppHandle,
        label: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<(), String> {
        let Some(webview) = app.get_webview(label) else {
            return Ok(());
        };
        webview
            .set_size(LogicalSize::new(width.max(1.0), height.max(1.0)))
            .map_err(|e| e.to_string())?;
        webview
            .set_position(LogicalPosition::new(x, y))
            .map_err(|e| e.to_string())
    }

    pub fn park(app: &AppHandle, label: &str) -> Result<(), String> {
        let Some(webview) = app.get_webview(label) else {
            return Ok(());
        };
        webview
            .set_position(LogicalPosition::new(super::OFFSCREEN, super::OFFSCREEN))
            .map_err(|e| e.to_string())
    }

    pub fn forget(_app: &AppHandle, _label: &str) {}
}

/// Prepare the window for child webviews. Call once, before any tab exists.
pub fn prepare(app: &AppHandle) {
    gtk_impl::prepare(app);
}

/// Take ownership of a webview that `Window::add_child` just created. Call this
/// before the first `place`.
pub fn adopt(app: &AppHandle, label: &str) -> Result<(), String> {
    gtk_impl::adopt(app, label)
}

/// Put a webview at `x`/`y` with the given size, in logical pixels relative to
/// the window's content area.
pub fn place(
    app: &AppHandle,
    label: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    gtk_impl::place(app, label, x, y, width, height)
}

/// Park a webview offscreen so it keeps running while another tab is in front.
pub fn park(app: &AppHandle, label: &str) -> Result<(), String> {
    gtk_impl::park(app, label)
}

/// Drop the bookkeeping for a webview that has been closed.
pub fn forget(app: &AppHandle, label: &str) {
    gtk_impl::forget(app, label)
}
