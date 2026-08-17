#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Linux + NVIDIA proprietary drivers (especially the legacy 470 branch)
    // cannot provide the GBM/DMA-BUF buffers that WebKitGTK >= 2.40 uses by
    // default. That logs "Failed to create GBM buffer ... Permission denied"
    // and renders a blank webview. Falling back to the classic renderer fixes
    // it. This is a no-op on Windows and macOS (WebView2 / WKWebView are used
    // there instead of WebKitGTK). Users can still override it deliberately,
    // e.g. WEBKIT_DISABLE_DMABUF_RENDERER=0 to re-enable.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    depot_lib::run()
}
