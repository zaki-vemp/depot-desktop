//! Native child webviews for web tabs.
//!
//! An iframe cannot show a site that sends `X-Frame-Options` / `frame-ancestors`.
//! A real webview is not a frame, so it loads anything a browser loads. Each web
//! tab owns one child webview parked over the content pane; the frontend keeps
//! its bounds in sync with the React layout.

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};

use crate::webview_layout;

fn parse(url: &str) -> Result<url::Url, String> {
    let candidate = if url.contains("://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    url::Url::parse(&candidate).map_err(|e| format!("Invalid URL: {e}"))
}

pub fn open(
    app: &AppHandle,
    label: &str,
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<String, String> {
    let target = parse(url)?;
    if let Some(webview) = app.get_webview(label) {
        // The tab already has a page: re-showing it must not reload and lose state.
        // Explicit navigation goes through `navigate`.
        let landed = webview.url().map(|u| u.to_string()).ok();
        set_bounds(app, label, x, y, width, height)?;
        return Ok(landed.unwrap_or_else(|| target.to_string()));
    }
    let window = app
        .get_window("main")
        .ok_or_else(|| "Main window is gone".to_string())?;
    window
        .add_child(
            tauri::webview::WebviewBuilder::new(label, WebviewUrl::External(target.clone())),
            LogicalPosition::new(x, y),
            LogicalSize::new(width.max(1.0), height.max(1.0)),
        )
        .map_err(|e| e.to_string())?;
    webview_layout::adopt(app, label)?;
    set_bounds(app, label, x, y, width, height)?;
    Ok(target.to_string())
}

pub fn set_bounds(
    app: &AppHandle,
    label: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if app.get_webview(label).is_none() {
        return Ok(());
    }
    webview_layout::place(app, label, x, y, width, height)
}

pub fn hide(app: &AppHandle, label: &str) -> Result<(), String> {
    if app.get_webview(label).is_none() {
        return Ok(());
    }
    webview_layout::park(app, label)
}

pub fn close(app: &AppHandle, label: &str) -> Result<(), String> {
    if let Some(webview) = app.get_webview(label) {
        webview.close().map_err(|e| e.to_string())?;
    }
    webview_layout::forget(app, label);
    Ok(())
}

pub fn navigate(app: &AppHandle, label: &str, url: &str) -> Result<String, String> {
    let target = parse(url)?;
    let href = target.to_string();
    let webview = app
        .get_webview(label)
        .ok_or_else(|| "This web tab has no webview".to_string())?;
    // Child webviews on macOS often no-op `navigate`; drive the page directly too.
    let _ = webview.navigate(target.clone());
    let js = format!(
        "window.location.assign({})",
        serde_json::to_string(&href).map_err(|e| e.to_string())?
    );
    let _ = webview.eval(&js);
    Ok(href)
}

/// `back`, `forward` and `reload` run in the page itself.
pub fn history(app: &AppHandle, label: &str, action: &str) -> Result<(), String> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| "This web tab has no webview".to_string())?;
    let script = match action {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        other => return Err(format!("Unknown history action: {other}")),
    };
    webview.eval(script).map_err(|e| e.to_string())
}

pub fn current_url(app: &AppHandle, label: &str) -> Result<String, String> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| "This web tab has no webview".to_string())?;
    webview
        .url()
        .map(|u| u.to_string())
        .map_err(|e| e.to_string())
}
