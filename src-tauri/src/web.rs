//! Native child webviews for web tabs.
//!
//! An iframe cannot show a site that sends `X-Frame-Options` / `frame-ancestors`.
//! A real webview is not a frame, so it loads anything a browser loads. Each web
//! tab owns one child webview parked over the content pane; the frontend keeps
//! its bounds in sync with the React layout.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

/// Parked position for a hidden webview — offscreen instead of destroyed, so the
/// page keeps its state while another tab is in front.
const OFFSCREEN: f64 = -20000.0;

/// WKWebView's stock agent string stops at `(KHTML, like Gecko)` — it carries no
/// `Version/… Safari/…` token, which identity providers read as "embedded
/// webview" and answer with a downgraded or refused sign-in. Dropbox's
/// "Continue with Apple / Google / Microsoft" buttons sit behind exactly that
/// check. WebView2 on Windows and WebKitGTK on Linux already send a complete
/// browser agent, so only macOS needs the override.
#[cfg(target_os = "macos")]
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";

/// An OAuth popup ends its handshake with `window.close()`, which does nothing
/// in a wry window and would leave a dead window on screen. The override sends
/// the popup at a scheme nothing can load; `on_navigation` below recognises it
/// and closes the window for real. The `opener` guard keeps the override off
/// ordinary tabs, which share this script through the popup's parent webview
/// configuration on macOS.
const POPUP_CLOSE_SCHEME: &str = "depot-popup-close";
const POPUP_CLOSE_SCRIPT: &str = r#"
(function () {
  if (!window.opener) return;
  window.close = function () {
    window.location.href = "depot-popup-close://now";
  };
})();
"#;

/// Popup windows need unique labels; the counter never repeats one within a run.
static POPUP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Labels of popups still waiting for a real page, with the time they opened.
static PENDING_POPUPS: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());

/// How long a popup may sit blank before it counts as abandoned.
const POPUP_GRACE: Duration = Duration::from_secs(120);

/// Sites often pre-open a blank popup on every page load and only sometimes use
/// it, so reloading a tab would otherwise strand one hidden window per visit.
/// Anything still blank well past any plausible sign-in is dropped.
fn reap_stale_popups(app: &AppHandle) {
    let Ok(mut pending) = PENDING_POPUPS.lock() else {
        return;
    };
    pending.retain(|(label, opened)| {
        let Some(window) = app.get_webview_window(label) else {
            return false; // already gone
        };
        if window.is_visible().unwrap_or(false) {
            return false; // in use; no longer our business
        }
        if opened.elapsed() < POPUP_GRACE {
            return true;
        }
        let _ = window.close();
        false
    });
}

fn parse(url: &str) -> Result<url::Url, String> {
    let candidate = if url.contains("://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    url::Url::parse(&candidate).map_err(|e| format!("Invalid URL: {e}"))
}

/// Builds the real window behind a `window.open` call.
///
/// Sign-in providers hand their result back through `window.opener`, so the
/// popup has to be a genuine WebKit child of the tab that opened it rather than
/// another tab of our own. `window_features` carries the opener's webview
/// configuration across, which is what preserves that relationship.
///
/// It starts hidden. Dropbox — like most sign-in pages — opens its popup blank
/// while the page is still loading and only points it at the provider once the
/// user picks one, so showing every `window.open` immediately would leave an
/// empty window sitting over the app. The navigation handler reveals it as soon
/// as a real page is on its way.
fn open_popup(
    app: &AppHandle,
    url: url::Url,
    features: tauri::webview::NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    reap_stale_popups(app);
    let label = format!("popup-{}", POPUP_SEQ.fetch_add(1, Ordering::Relaxed));
    let title = url.host_str().unwrap_or("Sign in").to_string();
    let handler_app = app.clone();
    let handler_label = label.clone();

    #[allow(unused_mut)]
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(title)
        .inner_size(520.0, 700.0)
        .visible(false)
        .initialization_script(POPUP_CLOSE_SCRIPT)
        .on_navigation(move |target| {
            let app = handler_app.clone();
            let label = handler_label.clone();
            let closing = target.scheme() == POPUP_CLOSE_SCHEME;
            if !closing && !matches!(target.scheme(), "http" | "https") {
                // `about:blank` placeholder: leave the window hidden and waiting.
                return true;
            }
            // Touching the window inside the navigation callback would act on the
            // webview from within its own delegate, so hand it to the event loop.
            tauri::async_runtime::spawn(async move {
                let Some(window) = app.get_webview_window(&label) else {
                    return;
                };
                if closing {
                    let _ = window.close();
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            });
            !closing
        })
        // Applied after the defaults so a site's own width/height wins.
        .window_features(features);

    #[cfg(target_os = "macos")]
    {
        builder = builder.user_agent(USER_AGENT);
    }

    match builder.build() {
        Ok(window) => {
            if let Ok(mut pending) = PENDING_POPUPS.lock() {
                pending.push((label, Instant::now()));
            }
            NewWindowResponse::Create { window }
        }
        Err(_) => NewWindowResponse::Deny,
    }
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
    let popup_app = app.clone();

    #[allow(unused_mut)]
    let mut builder =
        tauri::webview::WebviewBuilder::new(label, WebviewUrl::External(target.clone()))
            .initialization_script(POPUP_CLOSE_SCRIPT)
            .on_new_window(move |url, features| open_popup(&popup_app, url, features));

    #[cfg(target_os = "macos")]
    {
        builder = builder.user_agent(USER_AGENT);
    }

    window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width.max(1.0), height.max(1.0)),
        )
        .map_err(|e| e.to_string())?;
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

pub fn hide(app: &AppHandle, label: &str) -> Result<(), String> {
    let Some(webview) = app.get_webview(label) else {
        return Ok(());
    };
    webview
        .set_position(LogicalPosition::new(OFFSCREEN, OFFSCREEN))
        .map_err(|e| e.to_string())
}

pub fn close(app: &AppHandle, label: &str) -> Result<(), String> {
    if let Some(webview) = app.get_webview(label) {
        webview.close().map_err(|e| e.to_string())?;
    }
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
