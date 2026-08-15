mod drive;
mod files;
mod media;
mod office;
mod openwith;
mod state;
mod torrents;
mod transfers;
mod vlc;
mod web;

use files::{DirEntry, DiskUsage};
use state::{AppState, PublicAccount, PublicSettings, Settings};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
fn get_places() -> Result<Vec<files::Place>, String> {
    files::places()
}

#[tauri::command]
fn get_home() -> Result<String, String> {
    Ok(files::home_dir()?.display().to_string())
}

#[tauri::command]
fn list_dir(path: String) -> Result<Vec<DirEntry>, String> {
    files::list_dir(&path)
}

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    files::read_text(&path, 2 * 1024 * 1024)
}

#[tauri::command]
fn mkdir(path: String) -> Result<(), String> {
    files::mkdir(&path)
}

#[tauri::command]
fn rename_path(from: String, to: String) -> Result<(), String> {
    files::rename_path(&from, &to)
}

#[tauri::command]
fn remove_path(path: String) -> Result<(), String> {
    files::remove_path(&path)
}

#[tauri::command]
fn copy_path(from: String, to: String) -> Result<(), String> {
    files::copy_path(&from, &to)
}

#[tauri::command]
fn move_path(from: String, to: String) -> Result<(), String> {
    files::move_path(&from, &to)
}

#[tauri::command]
fn trash_path(path: String) -> Result<(), String> {
    files::trash_path(&path)
}

#[tauri::command]
fn disk_usage(path: String) -> Result<DiskUsage, String> {
    files::disk_usage(&path)
}

#[tauri::command]
fn parent_path(path: String) -> Option<String> {
    files::parent_path(&path)
}

#[tauri::command]
fn open_in_system(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_in_dir(path: String) -> Result<(), String> {
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn preview_office(path: String) -> Result<office::OfficePreview, String> {
    office::preview(path)
}

#[tauri::command]
fn list_open_with(path: String) -> Result<Vec<openwith::OpenApp>, String> {
    openwith::list_apps(path)
}

#[tauri::command]
fn open_with_app(path: String, app: String) -> Result<(), String> {
    openwith::open_with(path, app)
}

#[tauri::command]
fn pick_open_with(path: String) -> Result<(), String> {
    openwith::pick_and_open(&path)
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<PublicSettings, String> {
    let s = state.inner.lock().await.settings.clone();
    Ok(PublicSettings {
        google_client_id: s.google_client_id,
        google_client_secret: s.google_client_secret,
        one_drive_client_id: s.one_drive_client_id,
        one_drive_client_secret: s.one_drive_client_secret,
        dropbox_client_id: s.dropbox_client_id,
        dropbox_client_secret: s.dropbox_client_secret,
        s3_endpoint: s.s3_endpoint,
        s3_region: s.s3_region,
        s3_bucket: s.s3_bucket,
        s3_access_key_id: s.s3_access_key_id,
        s3_secret_access_key: s.s3_secret_access_key,
        torrent_download_dir: s.torrent_download_dir,
    })
}

#[tauri::command]
async fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    let mut inner = state.inner.lock().await;
    let accounts = inner.settings.accounts.clone();
    inner.settings.google_client_id = settings.google_client_id;
    inner.settings.google_client_secret = settings.google_client_secret;
    inner.settings.one_drive_client_id = settings.one_drive_client_id;
    inner.settings.one_drive_client_secret = settings.one_drive_client_secret;
    inner.settings.dropbox_client_id = settings.dropbox_client_id;
    inner.settings.dropbox_client_secret = settings.dropbox_client_secret;
    inner.settings.s3_endpoint = settings.s3_endpoint;
    inner.settings.s3_region = settings.s3_region;
    inner.settings.s3_bucket = settings.s3_bucket;
    inner.settings.s3_access_key_id = settings.s3_access_key_id;
    inner.settings.s3_secret_access_key = settings.s3_secret_access_key;
    inner.settings.torrent_download_dir = settings.torrent_download_dir;
    inner.settings.accounts = accounts;
    state.save_settings(&inner.settings)
}

#[tauri::command]
async fn list_drive_accounts(state: State<'_, AppState>) -> Result<Vec<PublicAccount>, String> {
    let inner = state.inner.lock().await;
    Ok(inner
        .settings
        .accounts
        .iter()
        .map(|a| PublicAccount {
            id: a.id.clone(),
            email: a.email.clone(),
        })
        .collect())
}

#[tauri::command]
async fn connect_google_drive(state: State<'_, AppState>) -> Result<PublicAccount, String> {
    let account = drive::connect_google(&state).await?;
    Ok(PublicAccount {
        id: account.id,
        email: account.email,
    })
}

#[tauri::command]
async fn disconnect_google_drive(state: State<'_, AppState>, account_id: String) -> Result<(), String> {
    drive::disconnect(&state, &account_id).await
}

#[tauri::command]
async fn list_drive(state: State<'_, AppState>, account_id: String, folder_id: Option<String>) -> Result<Vec<DirEntry>, String> {
    drive::list_files(&state, &account_id, folder_id).await
}

#[tauri::command]
async fn download_drive_file(state: State<'_, AppState>, path: String, dest: String) -> Result<String, String> {
    drive::download_file(&state, &path, &dest).await
}

#[tauri::command]
async fn cache_drive_file(state: State<'_, AppState>, path: String, name: String) -> Result<String, String> {
    drive::cache_file(&state, &path, &name).await
}

#[tauri::command]
async fn drive_quota(state: State<'_, AppState>, account_id: String) -> Result<DiskUsage, String> {
    drive::quota(&state, &account_id).await
}

#[tauri::command]
async fn mkdir_drive(
    state: State<'_, AppState>,
    account_id: String,
    folder_id: Option<String>,
    name: String,
) -> Result<String, String> {
    let parent = folder_id
        .filter(|id| !id.is_empty() && id != "root")
        .unwrap_or_else(|| "root".into());
    drive::create_folder(&state, &account_id, &parent, &name).await
}

#[tauri::command]
async fn start_transfer(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    from: String,
    to: String,
    op: String,
) -> Result<(), String> {
    transfers::run(app, &state, id, from, to, op).await
}

#[tauri::command]
fn web_open(
    app: AppHandle,
    label: String,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<String, String> {
    web::open(&app, &label, &url, x, y, width, height)
}

#[tauri::command]
fn web_bounds(
    app: AppHandle,
    label: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    web::set_bounds(&app, &label, x, y, width, height)
}

#[tauri::command]
fn web_hide(app: AppHandle, label: String) -> Result<(), String> {
    web::hide(&app, &label)
}

#[tauri::command]
fn web_close(app: AppHandle, label: String) -> Result<(), String> {
    web::close(&app, &label)
}

#[tauri::command]
fn web_navigate(app: AppHandle, label: String, url: String) -> Result<String, String> {
    web::navigate(&app, &label, &url)
}

#[tauri::command]
fn web_history(app: AppHandle, label: String, action: String) -> Result<(), String> {
    web::history(&app, &label, &action)
}

#[tauri::command]
fn web_url(app: AppHandle, label: String) -> Result<String, String> {
    web::current_url(&app, &label)
}

#[tauri::command]
fn vlc_available() -> vlc::VlcInfo {
    vlc::available()
}

#[tauri::command]
fn vlc_open(
    app: AppHandle,
    token: String,
    path: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    vlc::open(&app, token, path, x, y, width, height)
}

#[tauri::command]
fn vlc_bounds(app: AppHandle, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    vlc::set_bounds(&app, x, y, width, height)
}

#[tauri::command]
fn vlc_hide(app: AppHandle) -> Result<(), String> {
    vlc::hide(&app)
}

#[tauri::command]
fn vlc_close(app: AppHandle, token: String) -> Result<(), String> {
    vlc::close(&app, token)
}

#[tauri::command]
fn vlc_toggle() -> Result<(), String> {
    vlc::toggle()
}

#[tauri::command]
fn vlc_play() -> Result<(), String> {
    vlc::play()
}

#[tauri::command]
fn vlc_pause() -> Result<(), String> {
    vlc::pause()
}

#[tauri::command]
fn vlc_seek(ms: f64) -> Result<(), String> {
    vlc::seek(ms)
}

#[tauri::command]
fn vlc_set_volume(volume: f64) -> Result<(), String> {
    vlc::set_volume(volume)
}

#[tauri::command]
fn vlc_set_rate(rate: f64) -> Result<(), String> {
    vlc::set_rate(rate)
}

#[tauri::command]
fn vlc_set_mute(muted: bool) -> Result<(), String> {
    vlc::set_mute(muted)
}

#[tauri::command]
fn vlc_status() -> Result<vlc::VlcStatus, String> {
    vlc::status()
}

#[tauri::command]
fn vlc_tracks() -> Result<Vec<vlc::VlcTrack>, String> {
    vlc::tracks()
}

#[tauri::command]
fn vlc_set_subtitle(id: Option<String>) -> Result<(), String> {
    vlc::set_subtitle(id)
}

#[tauri::command]
async fn list_subtitles(path: String) -> Result<Vec<media::SubtitleTrack>, String> {
    media::list_subtitles(path).await
}

#[tauri::command]
async fn subtitle_vtt(state: State<'_, AppState>, path: String, track_id: String) -> Result<String, String> {
    media::subtitle_vtt(&state.app_dir, path, track_id).await
}

#[tauri::command]
async fn add_torrent(state: State<'_, AppState>, magnet: String) -> Result<String, String> {
    torrents::add_magnet(&state, &magnet).await
}

#[tauri::command]
async fn list_torrents(state: State<'_, AppState>) -> Result<Vec<torrents::TorrentInfo>, String> {
    torrents::list_torrents(&state).await
}

#[tauri::command]
async fn pause_torrent(state: State<'_, AppState>, id: usize) -> Result<(), String> {
    torrents::pause_torrent(&state, id).await
}

#[tauri::command]
async fn resume_torrent(state: State<'_, AppState>, id: usize) -> Result<(), String> {
    torrents::resume_torrent(&state, id).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
            app.manage(AppState::new(dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_places,
            get_home,
            list_dir,
            read_text_file,
            mkdir,
            rename_path,
            remove_path,
            trash_path,
            disk_usage,
            copy_path,
            move_path,
            parent_path,
            open_in_system,
            reveal_in_dir,
            preview_office,
            list_open_with,
            open_with_app,
            pick_open_with,
            get_settings,
            save_settings,
            list_drive_accounts,
            connect_google_drive,
            disconnect_google_drive,
            list_drive,
            download_drive_file,
            cache_drive_file,
            drive_quota,
            mkdir_drive,
            start_transfer,
            web_open,
            web_bounds,
            web_hide,
            web_close,
            web_navigate,
            web_history,
            web_url,
            vlc_available,
            vlc_open,
            vlc_bounds,
            vlc_hide,
            vlc_close,
            vlc_toggle,
            vlc_play,
            vlc_pause,
            vlc_seek,
            vlc_set_volume,
            vlc_set_rate,
            vlc_set_mute,
            vlc_status,
            vlc_tracks,
            vlc_set_subtitle,
            list_subtitles,
            subtitle_vtt,
            add_torrent,
            list_torrents,
            pause_torrent,
            resume_torrent
        ])
        .run(tauri::generate_context!())
        .expect("error while running Depot");
}
