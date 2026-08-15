use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenApp {
    pub name: String,
    pub path: String,
    pub is_default: bool,
}

pub fn list_apps(path: String) -> Result<Vec<OpenApp>, String> {
    if !Path::new(&path).exists() {
        return Err("File is missing".into());
    }
    #[cfg(target_os = "macos")]
    {
        return list_macos(&path);
    }
    #[cfg(target_os = "windows")]
    {
        return Ok(vec![OpenApp {
            name: "Choose another app…".into(),
            path: "__pick__".into(),
            is_default: false,
        }]);
    }
    #[cfg(target_os = "linux")]
    {
        return list_linux(&path);
    }
    #[allow(unreachable_code)]
    Ok(Vec::new())
}

pub fn open_with(path: String, app: String) -> Result<(), String> {
    if app == "__pick__" {
        return pick_and_open(&path);
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .args(["-a", &app, &path])
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
    }
    tauri_plugin_opener::open_path(&path, Some(app.as_str())).map_err(|e| e.to_string())
}

pub fn pick_and_open(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("osascript")
            .args([
                "-e",
                "try\nPOSIX path of (choose application as alias)\non error\n\"\"\nend try",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        let app = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if app.is_empty() {
            return Ok(());
        }
        return open_with(path.to_string(), app);
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("rundll32")
            .args(["shell32.dll,OpenAs_RunDLL", path])
            .status()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let chooser = ["zenity", "kdialog", "gio"]
            .into_iter()
            .find(|bin| Command::new("which").arg(bin).status().map(|s| s.success()).unwrap_or(false));
        if chooser == Some("zenity") {
            let out = Command::new("zenity")
                .args(["--file-selection", "--title=Open with", "--filename=/usr/bin/"])
                .output()
                .map_err(|e| e.to_string())?;
            let app = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if app.is_empty() {
                return Ok(());
            }
            Command::new(app)
                .arg(path)
                .spawn()
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        return tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string());
    }
    #[allow(unreachable_code)]
    Err("Open with is not available on this platform".into())
}

#[cfg(target_os = "macos")]
fn list_macos(path: &str) -> Result<Vec<OpenApp>, String> {
    let escaped = serde_json::to_string(path).map_err(|e| e.to_string())?;
    let script = format!(
        r#"
ObjC.import('AppKit');
const url = $.NSURL.fileURLWithPath({escaped});
const ws = $.NSWorkspace.sharedWorkspace;
const urls = ws.URLsForApplicationsToOpenURL(url);
const def = ws.URLForApplicationToOpenURL(url);
const defPath = def ? ObjC.unwrap(def.path) : '';
const out = [];
const n = urls.count;
for (let i = 0; i < n; i++) {{
  const p = ObjC.unwrap(urls.objectAtIndex(i).path);
  if (!p || p.indexOf('Depot.app') !== -1) continue;
  const parts = p.split('/');
  let name = parts[parts.length - 1] || p;
  if (name.endsWith('.app')) name = name.slice(0, -4);
  out.push({{ name, path: p, isDefault: p === defPath }});
}}
JSON.stringify(out);
"#
    );
    let output = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let parsed: Vec<OpenApp> = serde_json::from_str(raw.trim()).unwrap_or_default();
    let mut apps = parsed;
    apps.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
    apps.dedup_by(|a, b| a.path == b.path);
    apps.truncate(16);
    Ok(apps)
}

#[cfg(target_os = "linux")]
fn list_linux(path: &str) -> Result<Vec<OpenApp>, String> {
    let mime = Command::new("xdg-mime")
        .args(["query", "filetype", path])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut apps = Vec::new();
    if !mime.is_empty() {
        if let Ok(out) = Command::new("xdg-mime").args(["query", "default", &mime]).output() {
            let desktop = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !desktop.is_empty() {
                apps.push(OpenApp {
                    name: desktop.trim_end_matches(".desktop").replace('-', " "),
                    path: desktop,
                    is_default: true,
                });
            }
        }
    }
    Ok(apps)
}

#[allow(dead_code)]
fn app_name(path: &PathBuf) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
