//! Local video playback through libvlc.
//!
//! VLC paints into a dedicated child webview parked over the player surface so
//! it cannot cover Depot's HTML chrome. The frontend keeps that surface aligned
//! with the React layout, the same way web tabs work.

use serde::Serialize;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};

const SURFACE: &str = "depot-vlc";
const OFFSCREEN: f64 = -20000.0;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VlcInfo {
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VlcStatus {
    pub token: String,
    pub path: String,
    pub playing: bool,
    pub ended: bool,
    pub time_ms: i64,
    pub length_ms: i64,
    pub volume: i32,
    pub muted: bool,
    pub rate: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VlcTrack {
    pub id: String,
    pub label: String,
    pub kind: String,
}

#[allow(dead_code)]
struct Api {
    _core: Option<libloading::Library>,
    _lib: libloading::Library,
    new: unsafe extern "C" fn(c_int, *const *const c_char) -> *mut c_void,
    media_new_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    media_release: unsafe extern "C" fn(*mut c_void),
    media_player_new: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    media_player_set_media: unsafe extern "C" fn(*mut c_void, *mut c_void),
    media_player_play: unsafe extern "C" fn(*mut c_void) -> c_int,
    media_player_set_pause: unsafe extern "C" fn(*mut c_void, c_int),
    media_player_stop: unsafe extern "C" fn(*mut c_void),
    media_player_is_playing: unsafe extern "C" fn(*mut c_void) -> c_int,
    media_player_get_time: unsafe extern "C" fn(*mut c_void) -> i64,
    media_player_set_time: unsafe extern "C" fn(*mut c_void, i64),
    media_player_get_length: unsafe extern "C" fn(*mut c_void) -> i64,
    media_player_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    media_player_set_rate: unsafe extern "C" fn(*mut c_void, f32) -> c_int,
    media_player_get_rate: unsafe extern "C" fn(*mut c_void) -> f32,
    audio_set_volume: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    audio_get_volume: unsafe extern "C" fn(*mut c_void) -> c_int,
    audio_set_mute: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    audio_get_mute: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_nsobject: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    set_hwnd: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    set_xwindow: Option<unsafe extern "C" fn(*mut c_void, u32)>,
    video_set_spu: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    video_get_spu_description: unsafe extern "C" fn(*mut c_void) -> *mut TrackDesc,
    track_description_list_release: unsafe extern "C" fn(*mut TrackDesc),
    add_slave: unsafe extern "C" fn(*mut c_void, c_int, *const c_char, bool) -> c_int,
}

#[repr(C)]
struct TrackDesc {
    id: c_int,
    name: *const c_char,
    next: *mut TrackDesc,
}

struct Engine {
    instance: *mut c_void,
    player: *mut c_void,
    bound: bool,
    token: String,
    path: String,
    pending: Option<String>,
}

unsafe impl Send for Engine {}

fn engine() -> &'static Mutex<Engine> {
    static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();
    ENGINE.get_or_init(|| {
        Mutex::new(Engine {
            instance: std::ptr::null_mut(),
            player: std::ptr::null_mut(),
            bound: false,
            token: String::new(),
            path: String::new(),
            pending: None,
        })
    })
}

fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    match API.get_or_init(load_api) {
        Ok(api) => Ok(api),
        Err(e) => Err(e.clone()),
    }
}

pub fn available() -> VlcInfo {
    match locate_libvlc() {
        Some((lib, plugins)) => VlcInfo {
            available: true,
            version: Some("libvlc".into()),
            message: format!("Using {} (plugins {})", lib.display(), plugins.display()),
        },
        None => VlcInfo {
            available: false,
            version: None,
            message: "Install VLC Media Player to play local video in Depot.".into(),
        },
    }
}

pub fn open(
    app: &AppHandle,
    token: String,
    path: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err("Video file is missing".into());
    }
    let loaded = api()?;
    ensure_surface(app, x, y, width, height)?;
    let needs_bind;
    {
        let mut eng = engine().lock().map_err(|e| e.to_string())?;
        ensure_player(loaded, &mut eng)?;
        eng.token = token;
        eng.path = path.clone();
        needs_bind = !eng.bound;
        if eng.bound {
            start_media(loaded, &mut eng, &src)?;
        } else {
            eng.pending = Some(path);
        }
    }
    if needs_bind {
        bind_output(app)?;
    }
    Ok(())
}

pub fn set_bounds(app: &AppHandle, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    if app.get_webview(SURFACE).is_none() {
        return Ok(());
    }
    place(app, x, y, width, height)
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    pause()?;
    if let Some(webview) = app.get_webview(SURFACE) {
        webview
            .set_position(LogicalPosition::new(OFFSCREEN, OFFSCREEN))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn close(app: &AppHandle, token: String) -> Result<(), String> {
    {
        let mut eng = engine().lock().map_err(|e| e.to_string())?;
        if !token.is_empty() && eng.token != token {
            return Ok(());
        }
        if let Ok(api) = api() {
            if !eng.player.is_null() {
                unsafe { (api.media_player_stop)(eng.player) };
            }
        }
        eng.token.clear();
        eng.path.clear();
        eng.pending = None;
    }
    hide(app)
}

pub fn toggle() -> Result<(), String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    let api = api()?;
    unsafe {
        if (api.media_player_is_playing)(eng.player) != 0 {
            (api.media_player_set_pause)(eng.player, 1);
        } else {
            let _ = (api.media_player_play)(eng.player);
        }
    }
    Ok(())
}

pub fn play() -> Result<(), String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    let api = api()?;
    unsafe {
        let _ = (api.media_player_play)(eng.player);
    }
    Ok(())
}

pub fn pause() -> Result<(), String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    let api = api()?;
    unsafe { (api.media_player_set_pause)(eng.player, 1) };
    Ok(())
}

pub fn seek(ms: f64) -> Result<(), String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    let api = api()?;
    unsafe { (api.media_player_set_time)(eng.player, ms.max(0.0) as i64) };
    Ok(())
}

pub fn set_volume(volume: f64) -> Result<(), String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    let api = api()?;
    let v = (volume * 100.0).round().clamp(0.0, 100.0) as c_int;
    unsafe {
        let _ = (api.audio_set_volume)(eng.player, v);
    }
    Ok(())
}

pub fn set_rate(rate: f64) -> Result<(), String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    let api = api()?;
    unsafe {
        let _ = (api.media_player_set_rate)(eng.player, rate as f32);
    }
    Ok(())
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    let api = api()?;
    unsafe {
        let _ = (api.audio_set_mute)(eng.player, i32::from(muted));
    }
    Ok(())
}

pub fn status() -> Result<VlcStatus, String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(VlcStatus {
            token: eng.token.clone(),
            path: eng.path.clone(),
            playing: false,
            ended: false,
            time_ms: 0,
            length_ms: 0,
            volume: 100,
            muted: false,
            rate: 1.0,
        });
    }
    let api = api()?;
    let state = unsafe { (api.media_player_get_state)(eng.player) };
    Ok(VlcStatus {
        token: eng.token.clone(),
        path: eng.path.clone(),
        playing: unsafe { (api.media_player_is_playing)(eng.player) != 0 },
        ended: state == 6,
        time_ms: unsafe { (api.media_player_get_time)(eng.player) }.max(0),
        length_ms: unsafe { (api.media_player_get_length)(eng.player) }.max(0),
        volume: unsafe { (api.audio_get_volume)(eng.player) }.clamp(0, 100),
        muted: unsafe { (api.audio_get_mute)(eng.player) > 0 },
        rate: unsafe { (api.media_player_get_rate)(eng.player) },
    })
}

pub fn tracks() -> Result<Vec<VlcTrack>, String> {
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(Vec::new());
    }
    let api = api()?;
    let mut out = Vec::new();
    unsafe {
        let mut desc = (api.video_get_spu_description)(eng.player);
        let head = desc;
        while !desc.is_null() {
            let id = (*desc).id;
            if id >= 0 {
                let label = if (*desc).name.is_null() {
                    format!("Track {id}")
                } else {
                    CStr::from_ptr((*desc).name).to_string_lossy().into_owned()
                };
                out.push(VlcTrack {
                    id: format!("spu:{id}"),
                    label,
                    kind: "embedded".into(),
                });
            }
            desc = (*desc).next;
        }
        if !head.is_null() {
            (api.track_description_list_release)(head);
        }
    }
    Ok(out)
}

pub fn set_subtitle(id: Option<String>) -> Result<(), String> {
    let api = api()?;
    let eng = engine().lock().map_err(|e| e.to_string())?;
    if eng.player.is_null() {
        return Ok(());
    }
    match id {
        None => {
            unsafe {
                let _ = (api.video_set_spu)(eng.player, -1);
            }
        }
        Some(id) => {
            if let Some(spu) = id.strip_prefix("spu:") {
                let idx: c_int = spu.parse().map_err(|_| "Invalid subtitle track".to_string())?;
                unsafe {
                    let _ = (api.video_set_spu)(eng.player, idx);
                }
            } else if let Some(path) = id.strip_prefix("file:") {
                let uri = file_uri(Path::new(path))?;
                let c = CString::new(uri).map_err(|e| e.to_string())?;
                unsafe {
                    let _ = (api.add_slave)(eng.player, 0, c.as_ptr(), true);
                }
            } else {
                return Err("Unknown subtitle track".into());
            }
        }
    }
    Ok(())
}

fn ensure_player(api: &Api, eng: &mut Engine) -> Result<(), String> {
    if eng.instance.is_null() {
        eng.instance = new_instance(api)?;
    }
    if eng.player.is_null() {
        eng.player = unsafe { (api.media_player_new)(eng.instance) };
        if eng.player.is_null() {
            return Err("libvlc could not create a media player".into());
        }
        eng.bound = false;
    }
    Ok(())
}

fn start_media(api: &Api, eng: &mut Engine, src: &Path) -> Result<(), String> {
    let c_path = CString::new(src.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let media = unsafe { (api.media_new_path)(eng.instance, c_path.as_ptr()) };
    if media.is_null() {
        return Err("libvlc could not open this file".into());
    }
    unsafe {
        (api.media_player_stop)(eng.player);
        (api.media_player_set_media)(eng.player, media);
        (api.media_release)(media);
        if (api.media_player_play)(eng.player) != 0 {
            return Err("libvlc failed to start playback".into());
        }
    }
    Ok(())
}

fn new_instance(api: &Api) -> Result<*mut c_void, String> {
    let args = [
        CString::new("--intf=dummy").unwrap(),
        CString::new("--no-video-title-show").unwrap(),
        CString::new("--quiet").unwrap(),
        CString::new("--no-osd").unwrap(),
        CString::new("--no-stats").unwrap(),
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        CString::new("--no-xlib").unwrap(),
    ];
    let ptrs: Vec<*const c_char> = args.iter().map(|s| s.as_ptr()).collect();
    let inst = unsafe { (api.new)(ptrs.len() as c_int, ptrs.as_ptr()) };
    if inst.is_null() {
        Err("libvlc_new failed. Check that VLC plugins are installed.".into())
    } else {
        Ok(inst)
    }
}

fn ensure_surface(app: &AppHandle, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    if app.get_webview(SURFACE).is_some() {
        return place(app, x, y, width, height);
    }
    let window = app
        .get_window("main")
        .ok_or_else(|| "Main window is gone".to_string())?;
    let url = url::Url::parse("about:blank").map_err(|e| e.to_string())?;
    window
        .add_child(
            tauri::webview::WebviewBuilder::new(SURFACE, WebviewUrl::External(url))
                .focused(false)
                .background_color(tauri::webview::Color(17, 17, 17, 255)),
            LogicalPosition::new(x, y),
            LogicalSize::new(width.max(1.0), height.max(1.0)),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn place(app: &AppHandle, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    let Some(webview) = app.get_webview(SURFACE) else {
        return Ok(());
    };
    webview
        .set_size(LogicalSize::new(width.max(1.0), height.max(1.0)))
        .map_err(|e| e.to_string())?;
    webview
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

fn bind_output(app: &AppHandle) -> Result<(), String> {
    let webview = app
        .get_webview(SURFACE)
        .ok_or_else(|| "VLC surface is missing".to_string())?;
    webview
        .with_webview(|platform| {
            let Ok(api) = api() else {
                return;
            };
            let Ok(mut eng) = engine().lock() else {
                return;
            };
            if eng.player.is_null() {
                return;
            }
            attach_drawable(api, eng.player, &platform);
            eng.bound = true;
            if let Some(path) = eng.pending.take() {
                let _ = start_media(api, &mut eng, Path::new(&path));
            }
        })
        .map_err(|e| e.to_string())
}

fn attach_drawable(api: &Api, player: *mut c_void, platform: &tauri::webview::PlatformWebview) {
    #[cfg(target_os = "macos")]
    if let Some(set) = api.set_nsobject {
        unsafe { set(player, platform.inner()) };
    }
    #[cfg(windows)]
    if let Some(set) = api.set_hwnd {
        if let Some(hwnd) = windows_hwnd(platform) {
            unsafe { set(player, hwnd) };
        }
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    if let Some(set) = api.set_xwindow {
        if let Some(xid) = linux_xid(platform) {
            unsafe { set(player, xid) };
        }
    }
}

#[cfg(windows)]
fn windows_hwnd(platform: &tauri::webview::PlatformWebview) -> Option<*mut c_void> {
    let controller = platform.controller();
    unsafe {
        let mut hwnd = std::mem::zeroed();
        controller.ParentWindow(&mut hwnd).ok()?;
        let raw = hwnd.0 as *mut c_void;
        (!raw.is_null()).then_some(raw)
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn linux_xid(platform: &tauri::webview::PlatformWebview) -> Option<u32> {
    let widget = platform.inner();
    let widget_ptr = widget.as_ptr() as *mut c_void;
    let window_ptr = unsafe { gtk_widget_get_window(widget_ptr) };
    if window_ptr.is_null() {
        return None;
    }
    let xid = unsafe { gdk_x11_window_get_xid(window_ptr) };
    (xid != 0).then_some(xid as u32)
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
extern "C" {
    fn gtk_widget_get_window(widget: *mut c_void) -> *mut c_void;
    fn gdk_x11_window_get_xid(window: *mut c_void) -> u64;
}

fn load_api() -> Result<Api, String> {
    let (lib_path, plugins) = locate_libvlc().ok_or_else(|| {
        "VLC is not installed. Install VLC Media Player, then reopen this tab.".to_string()
    })?;
    std::env::set_var("VLC_PLUGIN_PATH", &plugins);
    if let Some(dir) = lib_path.parent() {
        prepend_path(dir);
    }
    let core = lib_path
        .parent()
        .map(|dir| {
            let name = if cfg!(windows) {
                "libvlccore.dll"
            } else if cfg!(target_os = "macos") {
                "libvlccore.dylib"
            } else {
                "libvlccore.so.9"
            };
            dir.join(name)
        })
        .and_then(|p| unsafe { libloading::Library::new(&p).ok() });
    let lib = unsafe {
        libloading::Library::new(&lib_path).map_err(|e| format!("Could not load libvlc: {e}"))?
    };
    unsafe {
        Ok(Api {
            _core: core,
            new: *lib.get(b"libvlc_new\0").map_err(|e| e.to_string())?,
            media_new_path: *lib.get(b"libvlc_media_new_path\0").map_err(|e| e.to_string())?,
            media_release: *lib.get(b"libvlc_media_release\0").map_err(|e| e.to_string())?,
            media_player_new: *lib
                .get(b"libvlc_media_player_new\0")
                .map_err(|e| e.to_string())?,
            media_player_set_media: *lib
                .get(b"libvlc_media_player_set_media\0")
                .map_err(|e| e.to_string())?,
            media_player_play: *lib
                .get(b"libvlc_media_player_play\0")
                .map_err(|e| e.to_string())?,
            media_player_set_pause: *lib
                .get(b"libvlc_media_player_set_pause\0")
                .map_err(|e| e.to_string())?,
            media_player_stop: *lib
                .get(b"libvlc_media_player_stop\0")
                .map_err(|e| e.to_string())?,
            media_player_is_playing: *lib
                .get(b"libvlc_media_player_is_playing\0")
                .map_err(|e| e.to_string())?,
            media_player_get_time: *lib
                .get(b"libvlc_media_player_get_time\0")
                .map_err(|e| e.to_string())?,
            media_player_set_time: *lib
                .get(b"libvlc_media_player_set_time\0")
                .map_err(|e| e.to_string())?,
            media_player_get_length: *lib
                .get(b"libvlc_media_player_get_length\0")
                .map_err(|e| e.to_string())?,
            media_player_get_state: *lib
                .get(b"libvlc_media_player_get_state\0")
                .map_err(|e| e.to_string())?,
            media_player_set_rate: *lib
                .get(b"libvlc_media_player_set_rate\0")
                .map_err(|e| e.to_string())?,
            media_player_get_rate: *lib
                .get(b"libvlc_media_player_get_rate\0")
                .map_err(|e| e.to_string())?,
            audio_set_volume: *lib.get(b"libvlc_audio_set_volume\0").map_err(|e| e.to_string())?,
            audio_get_volume: *lib.get(b"libvlc_audio_get_volume\0").map_err(|e| e.to_string())?,
            audio_set_mute: *lib.get(b"libvlc_audio_set_mute\0").map_err(|e| e.to_string())?,
            audio_get_mute: *lib.get(b"libvlc_audio_get_mute\0").map_err(|e| e.to_string())?,
            set_nsobject: lib.get(b"libvlc_media_player_set_nsobject\0").ok().map(|s| *s),
            set_hwnd: lib.get(b"libvlc_media_player_set_hwnd\0").ok().map(|s| *s),
            set_xwindow: lib.get(b"libvlc_media_player_set_xwindow\0").ok().map(|s| *s),
            video_set_spu: *lib.get(b"libvlc_video_set_spu\0").map_err(|e| e.to_string())?,
            video_get_spu_description: *lib
                .get(b"libvlc_video_get_spu_description\0")
                .map_err(|e| e.to_string())?,
            track_description_list_release: *lib
                .get(b"libvlc_track_description_list_release\0")
                .map_err(|e| e.to_string())?,
            add_slave: *lib
                .get(b"libvlc_media_player_add_slave\0")
                .map_err(|e| e.to_string())?,
            _lib: lib,
        })
    }
}

fn locate_libvlc() -> Option<(PathBuf, PathBuf)> {
    let mut libs = Vec::new();
    #[cfg(target_os = "macos")]
    {
        libs.push(PathBuf::from(
            "/Applications/VLC.app/Contents/MacOS/lib/libvlc.dylib",
        ));
        libs.push(PathBuf::from("/opt/homebrew/opt/vlc/lib/libvlc.dylib"));
        libs.push(PathBuf::from("/usr/local/opt/vlc/lib/libvlc.dylib"));
    }
    #[cfg(windows)]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            libs.push(PathBuf::from(pf).join("VideoLAN/VLC/libvlc.dll"));
        }
        if let Some(pf) = std::env::var_os("ProgramFiles(x86)") {
            libs.push(PathBuf::from(pf).join("VideoLAN/VLC/libvlc.dll"));
        }
        libs.push(PathBuf::from(r"C:\Program Files\VideoLAN\VLC\libvlc.dll"));
    }
    #[cfg(unix)]
    {
        libs.push(PathBuf::from("/usr/lib/x86_64-linux-gnu/libvlc.so.5"));
        libs.push(PathBuf::from("/usr/lib/aarch64-linux-gnu/libvlc.so.5"));
        libs.push(PathBuf::from("/usr/lib/libvlc.so.5"));
        libs.push(PathBuf::from("/usr/local/lib/libvlc.so.5"));
    }
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        let libname = if cfg!(windows) {
            "libvlc.dll"
        } else if cfg!(target_os = "macos") {
            "libvlc.dylib"
        } else {
            "libvlc.so.5"
        };
        for dir in path.split(sep) {
            libs.push(PathBuf::from(dir).join(libname));
            let parent = PathBuf::from(dir);
            if parent.ends_with("bin") {
                if let Some(root) = parent.parent() {
                    libs.push(root.join("lib").join(libname));
                    libs.push(root.join(libname));
                }
            }
        }
    }
    for lib in libs {
        if !lib.is_file() {
            continue;
        }
        if let Some(plugins) = plugins_for(&lib) {
            return Some((lib, plugins));
        }
    }
    None
}

fn plugins_for(lib: &Path) -> Option<PathBuf> {
    let dir = lib.parent()?;
    let candidates = [
        dir.join("plugins"),
        dir.parent().map(|p| p.join("plugins")).unwrap_or_default(),
        dir.join("vlc/plugins"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu/vlc/plugins"),
        PathBuf::from("/usr/lib/vlc/plugins"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

fn prepend_path(dir: &Path) {
    let key = if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let extra = dir.display().to_string();
    let joined = match std::env::var(key) {
        Ok(cur) if !cur.is_empty() => format!("{extra}{sep}{cur}", sep = if cfg!(windows) { ";" } else { ":" }),
        _ => extra,
    };
    std::env::set_var(key, joined);
}

fn file_uri(path: &Path) -> Result<String, String> {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| "Subtitle path is not absolute".to_string())
}

#[cfg(test)]
mod tests {
    use super::locate_libvlc;

    #[test]
    fn finds_system_vlc_when_installed() {
        if locate_libvlc().is_none() {
            return;
        }
        let (lib, plugins) = locate_libvlc().unwrap();
        assert!(lib.is_file());
        assert!(plugins.is_dir());
    }
}
