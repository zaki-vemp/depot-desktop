use crate::files::DirEntry;
use crate::state::{AppState, DriveAccount, Settings};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

const OAUTH_PORT: u16 = 17843;
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive https://www.googleapis.com/auth/userinfo.email";

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct UserInfo {
    email: Option<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn random_verifier() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    (0..64)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

async fn wait_for_code() -> Result<String, String> {
    let listener = TcpListener::bind(("127.0.0.1", OAUTH_PORT))
        .await
        .map_err(|e| format!("Could not bind OAuth port {OAUTH_PORT}: {e}"))?;

    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(180), listener.accept())
        .await
        .map_err(|_| "Timed out waiting for Google sign-in".to_string())?
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first = req.lines().next().unwrap_or("");
    let path = first.split_whitespace().nth(1).unwrap_or("");
    let url = format!("http://127.0.0.1{path}");
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
    let pairs: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

    let html = if pairs.contains_key("code") {
        "<html><body style='font-family:sans-serif;padding:40px'><h2>Depot is connected</h2><p>You can close this tab and return to the app.</p></body></html>"
    } else {
        "<html><body style='font-family:sans-serif;padding:40px'><h2>Sign-in failed</h2><p>Return to Depot and try again.</p></body></html>"
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(resp.as_bytes()).await;

    if let Some(err) = pairs.get("error") {
        return Err(format!("Google OAuth error: {err}"));
    }
    pairs
        .get("code")
        .cloned()
        .ok_or_else(|| "Google did not return an authorization code".into())
}

async fn exchange_token(
    client: &reqwest::Client,
    settings: &Settings,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("client_id", settings.google_client_id.as_str()),
        ("client_secret", settings.google_client_secret.as_str()),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", "http://127.0.0.1:17843/callback"),
    ];
    let res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("Token exchange failed: {}", res.text().await.unwrap_or_default()));
    }
    res.json().await.map_err(|e| e.to_string())
}

async fn refresh_token(
    client: &reqwest::Client,
    settings: &Settings,
    account: &DriveAccount,
) -> Result<TokenResponse, String> {
    let params = [
        ("client_id", settings.google_client_id.as_str()),
        ("client_secret", settings.google_client_secret.as_str()),
        ("refresh_token", account.refresh_token.as_str()),
        ("grant_type", "refresh_token"),
    ];
    let res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("Token refresh failed: {}", res.text().await.unwrap_or_default()));
    }
    res.json().await.map_err(|e| e.to_string())
}

async fn ensure_token(
    client: &reqwest::Client,
    state: &AppState,
    account_id: &str,
) -> Result<String, String> {
    let inner = state.inner.lock().await;
    let idx = inner
        .settings
        .accounts
        .iter()
        .position(|a| a.id == account_id)
        .ok_or_else(|| "Google account not found".to_string())?;
    let needs_refresh = inner.settings.accounts[idx].expires_at < now_secs() + 60;
    if needs_refresh {
        let account = inner.settings.accounts[idx].clone();
        let settings = inner.settings.clone();
        drop(inner);
        let tokens = refresh_token(client, &settings, &account).await?;
        let mut inner = state.inner.lock().await;
        if let Some(acc) = inner.settings.accounts.iter_mut().find(|a| a.id == account_id) {
            acc.access_token = tokens.access_token.clone();
            if let Some(rt) = tokens.refresh_token {
                acc.refresh_token = rt;
            }
            acc.expires_at = now_secs() + tokens.expires_in.unwrap_or(3600);
        }
        state.save_settings(&inner.settings)?;
        Ok(tokens.access_token)
    } else {
        Ok(inner.settings.accounts[idx].access_token.clone())
    }
}

pub async fn connect_google(state: &AppState) -> Result<DriveAccount, String> {
    let settings = state.inner.lock().await.settings.clone();
    if settings.google_client_id.is_empty() || settings.google_client_secret.is_empty() {
        return Err("Add a Google OAuth client ID and secret in Settings first".into());
    }

    let verifier = random_verifier();
    let challenge = challenge(&verifier);
    let auth = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=select_account%20consent&include_granted_scopes=true&code_challenge={}&code_challenge_method=S256",
        urlencoding::encode(&settings.google_client_id),
        urlencoding::encode("http://127.0.0.1:17843/callback"),
        urlencoding::encode(DRIVE_SCOPE),
        challenge
    );

    tauri_plugin_opener::open_url(&auth, None::<&str>).map_err(|e| e.to_string())?;
    let code = wait_for_code().await?;
    let client = reqwest::Client::new();
    let tokens = exchange_token(&client, &settings, &code, &verifier).await?;
    let user: UserInfo = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let mut inner = state.inner.lock().await;
    let email = user.email.unwrap_or_else(|| "Google Drive".into());
    let existing = inner
        .settings
        .accounts
        .iter()
        .position(|account| account.email.eq_ignore_ascii_case(&email));
    let account = DriveAccount {
        id: existing
            .map(|index| inner.settings.accounts[index].id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        email,
        access_token: tokens.access_token,
        refresh_token: tokens
            .refresh_token
            .filter(|token| !token.is_empty())
            .or_else(|| existing.map(|index| inner.settings.accounts[index].refresh_token.clone()))
            .unwrap_or_default(),
        expires_at: now_secs() + tokens.expires_in.unwrap_or(3600),
    };
    if let Some(index) = existing {
        inner.settings.accounts[index] = account.clone();
    } else {
        inner.settings.accounts.push(account.clone());
    }
    state.save_settings(&inner.settings)?;
    Ok(account)
}

pub async fn disconnect(state: &AppState, account_id: &str) -> Result<(), String> {
    let mut inner = state.inner.lock().await;
    inner.settings.accounts.retain(|a| a.id != account_id);
    state.save_settings(&inner.settings)
}

#[derive(Deserialize)]
struct DriveList {
    files: Option<Vec<DriveFile>>,
}

#[derive(Deserialize)]
struct DriveFile {
    id: String,
    name: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
    size: Option<String>,
    #[serde(rename = "modifiedTime")]
    modified_time: Option<String>,
}

pub async fn list_files(
    state: &AppState,
    account_id: &str,
    folder_id: Option<String>,
) -> Result<Vec<DirEntry>, String> {
    let client = reqwest::Client::new();
    let token = ensure_token(&client, state, account_id).await?;
    let parent = folder_id.unwrap_or_else(|| "root".into());
    let q = format!("'{parent}' in parents and trashed = false");
    let url = format!(
        "https://www.googleapis.com/drive/v3/files?pageSize=1000&fields=files(id,name,mimeType,size,modifiedTime)&q={}",
        urlencoding::encode(&q)
    );
    let res = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("Drive list failed: {}", res.text().await.unwrap_or_default()));
    }
    let body: DriveList = res.json().await.map_err(|e| e.to_string())?;
    let mut items: Vec<DirEntry> = body
        .files
        .unwrap_or_default()
        .into_iter()
        .map(|f| {
            let is_dir = f.mime_type == "application/vnd.google-apps.folder";
            let ext = if is_dir {
                String::new()
            } else {
                std::path::Path::new(&f.name)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
            };
            let modified = f.modified_time.and_then(|t| {
                chrono::DateTime::parse_from_rfc3339(&t)
                    .ok()
                    .map(|d| d.timestamp())
            });
            DirEntry {
                name: f.name,
                path: format!("gdrive://{account_id}/{}", f.id),
                is_dir,
                size: f.size.and_then(|s| s.parse().ok()).unwrap_or(0),
                modified,
                ext,
                source: "gdrive".into(),
                mime_type: Some(f.mime_type),
                account_id: Some(account_id.to_string()),
            }
        })
        .collect();
    items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(items)
}

pub fn is_gdrive_path(path: &str) -> bool {
    path.starts_with("gdrive://")
}

pub fn parse_gdrive_path(path: &str) -> Result<(String, String), String> {
    let rest = path
        .strip_prefix("gdrive://")
        .ok_or_else(|| "Not a Google Drive path".to_string())?;
    let (account_id, file_id) = rest
        .split_once('/')
        .ok_or_else(|| "Invalid Google Drive path".to_string())?;
    Ok((account_id.into(), file_id.into()))
}

/// Destination for an upload/copy: `gdrive://{account}/{parentFolder}/{name}`.
pub fn parse_gdrive_dest(path: &str) -> Result<(String, String, String), String> {
    let rest = path
        .strip_prefix("gdrive://")
        .ok_or_else(|| "Not a Google Drive path".to_string())?;
    let mut parts = rest.splitn(3, '/');
    let account_id = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Invalid Google Drive destination".to_string())?;
    let parent_id = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Invalid Google Drive destination".to_string())?;
    let raw_name = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Drive destination is missing a file name".to_string())?;
    let name = urlencoding::decode(raw_name)
        .map_err(|e| e.to_string())?
        .into_owned();
    Ok((account_id.into(), parent_id.into(), name))
}

async fn authed(state: &AppState, account_id: &str) -> Result<(reqwest::Client, String), String> {
    let client = reqwest::Client::new();
    let token = ensure_token(&client, state, account_id).await?;
    Ok((client, token))
}

async fn api_error(action: &str, res: reqwest::Response) -> String {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if body.is_empty() {
        format!("{action} failed: {status}")
    } else {
        format!("{action} failed: {status} {body}")
    }
}

#[derive(Clone)]
pub struct DriveMeta {
    pub account_id: String,
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub parents: Vec<String>,
}

impl DriveMeta {
    pub fn is_dir(&self) -> bool {
        self.mime_type == "application/vnd.google-apps.folder"
    }

    pub fn is_google_doc(&self) -> bool {
        self.mime_type.starts_with("application/vnd.google-apps.") && !self.is_dir()
    }
}

#[derive(Deserialize)]
struct DriveMetaBody {
    id: String,
    name: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
    size: Option<String>,
    parents: Option<Vec<String>>,
}

pub async fn file_meta(state: &AppState, path: &str) -> Result<DriveMeta, String> {
    let (account_id, file_id) = parse_gdrive_path(path)?;
    let (client, token) = authed(state, &account_id).await?;
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{file_id}?fields=id,name,mimeType,size,parents"
    );
    let res = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(api_error("Drive metadata", res).await);
    }
    let body: DriveMetaBody = res.json().await.map_err(|e| e.to_string())?;
    Ok(DriveMeta {
        account_id,
        id: body.id,
        name: body.name,
        mime_type: body.mime_type,
        size: body.size.and_then(|s| s.parse().ok()).unwrap_or(0),
        parents: body.parents.unwrap_or_default(),
    })
}

fn export_target(mime: &str) -> Option<(&'static str, &'static str)> {
    match mime {
        "application/vnd.google-apps.document" => Some((
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        )),
        "application/vnd.google-apps.spreadsheet" => Some((
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
        )),
        "application/vnd.google-apps.presentation" => Some((
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "pptx",
        )),
        "application/vnd.google-apps.drawing" => Some(("application/pdf", "pdf")),
        _ => None,
    }
}

pub fn with_export_ext(name: &str, ext: &str) -> String {
    let current = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    if current.as_deref() == Some(ext) {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    }
}

pub struct OpenContent {
    pub response: reqwest::Response,
    pub meta: DriveMeta,
    pub content_mime: String,
    pub download_name: String,
}

/// Opens a Drive file for reading. Google Docs/Sheets/Slides are exported.
pub async fn open_content(state: &AppState, path: &str) -> Result<OpenContent, String> {
    let meta = file_meta(state, path).await?;
    if meta.is_dir() {
        return Err("Cannot download a Drive folder as a single file".into());
    }
    let (client, token) = authed(state, &meta.account_id).await?;
    let (url, content_mime, download_name) = if meta.is_google_doc() {
        let (export_mime, ext) = export_target(&meta.mime_type).ok_or_else(|| {
            format!(
                "Google file \"{}\" cannot be copied out of Drive in this format",
                meta.name
            )
        })?;
        (
            format!(
                "https://www.googleapis.com/drive/v3/files/{}/export?mimeType={}",
                meta.id,
                urlencoding::encode(export_mime)
            ),
            export_mime.to_string(),
            with_export_ext(&meta.name, ext),
        )
    } else {
        (
            format!(
                "https://www.googleapis.com/drive/v3/files/{}?alt=media",
                meta.id
            ),
            meta.mime_type.clone(),
            meta.name.clone(),
        )
    };
    let res = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(api_error("Download", res).await);
    }
    Ok(OpenContent {
        response: res,
        meta,
        content_mime,
        download_name,
    })
}

/// Opens a Drive file for reading. The caller streams the body, so transfers can
/// report byte-level progress.
pub async fn open_download(state: &AppState, path: &str) -> Result<reqwest::Response, String> {
    Ok(open_content(state, path).await?.response)
}

#[derive(Deserialize)]
struct CreatedFile {
    id: String,
}

pub async fn create_folder(
    state: &AppState,
    account_id: &str,
    parent_id: &str,
    name: &str,
) -> Result<String, String> {
    let (client, token) = authed(state, account_id).await?;
    let body = serde_json::json!({
        "name": name,
        "mimeType": "application/vnd.google-apps.folder",
        "parents": [parent_id],
    });
    let res = client
        .post("https://www.googleapis.com/drive/v3/files")
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(api_error("Create folder", res).await);
    }
    let created: CreatedFile = res.json().await.map_err(|e| e.to_string())?;
    Ok(created.id)
}

pub async fn copy_within_account(
    state: &AppState,
    path: &str,
    parent_id: &str,
    name: &str,
) -> Result<String, String> {
    let meta = file_meta(state, path).await?;
    let (client, token) = authed(state, &meta.account_id).await?;
    let body = serde_json::json!({
        "name": name,
        "parents": [parent_id],
    });
    let url = format!("https://www.googleapis.com/drive/v3/files/{}/copy", meta.id);
    let res = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(api_error("Drive copy", res).await);
    }
    let created: CreatedFile = res.json().await.map_err(|e| e.to_string())?;
    Ok(created.id)
}

pub async fn move_within_account(
    state: &AppState,
    path: &str,
    new_parent: &str,
    name: &str,
) -> Result<(), String> {
    let meta = file_meta(state, path).await?;
    let (client, token) = authed(state, &meta.account_id).await?;
    let remove = meta.parents.join(",");
    let mut url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?addParents={}&supportsAllDrives=true",
        meta.id,
        urlencoding::encode(new_parent)
    );
    if !remove.is_empty() {
        url.push_str("&removeParents=");
        url.push_str(&urlencoding::encode(&remove));
    }
    let body = serde_json::json!({ "name": name });
    let res = client
        .patch(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(api_error("Drive move", res).await);
    }
    Ok(())
}

pub async fn trash_file(state: &AppState, path: &str) -> Result<(), String> {
    let (account_id, file_id) = parse_gdrive_path(path)?;
    let (client, token) = authed(state, &account_id).await?;
    let url = format!("https://www.googleapis.com/drive/v3/files/{file_id}");
    let res = client
        .patch(&url)
        .bearer_auth(&token)
        .json(&serde_json::json!({ "trashed": true }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(api_error("Drive trash", res).await);
    }
    Ok(())
}

pub fn guess_mime(name: &str) -> &'static str {
    match std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("txt") | Some("md") => "text/plain",
        Some("html") | Some("htm") => "text/html",
        Some("json") => "application/json",
        Some("csv") => "text/csv",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("zip") => "application/zip",
        Some("docx") => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        _ => "application/octet-stream",
    }
}

pub async fn begin_upload(
    state: &AppState,
    account_id: &str,
    parent_id: &str,
    name: &str,
    mime: &str,
    size: Option<u64>,
) -> Result<String, String> {
    let (client, token) = authed(state, account_id).await?;
    let metadata = serde_json::json!({
        "name": name,
        "parents": [parent_id],
    });
    let mut req = client
        .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable")
        .bearer_auth(&token)
        .header("Content-Type", "application/json; charset=UTF-8")
        .header("X-Upload-Content-Type", mime);
    if let Some(n) = size {
        req = req.header("X-Upload-Content-Length", n.to_string());
    }
    let res = req.json(&metadata).send().await.map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(api_error("Start upload", res).await);
    }
    res.headers()
        .get(reqwest::header::LOCATION)
        .ok_or_else(|| "Drive did not return an upload URL".to_string())?
        .to_str()
        .map_err(|e| e.to_string())
        .map(|s| s.to_string())
}

pub async fn put_upload_chunk(
    uri: &str,
    start: u64,
    total: u64,
    chunk: &[u8],
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let end = start + chunk.len() as u64 - 1;
    let range = if chunk.is_empty() {
        format!("bytes */{total}")
    } else {
        format!("bytes {start}-{end}/{total}")
    };
    let res = client
        .put(uri)
        .header("Content-Length", chunk.len())
        .header("Content-Range", range)
        .body(chunk.to_vec())
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status().as_u16();
    if status == 200 || status == 201 || status == 308 {
        Ok(())
    } else {
        Err(api_error("Upload", res).await)
    }
}

pub async fn put_upload_chunk_unknown(
    uri: &str,
    start: u64,
    chunk: &[u8],
    last: bool,
) -> Result<(), String> {
    let end = start + chunk.len() as u64 - 1;
    let total = if last { end + 1 } else { 0 };
    if last {
        put_upload_chunk(uri, start, total, chunk).await
    } else {
        let client = reqwest::Client::new();
        let res = client
            .put(uri)
            .header("Content-Length", chunk.len())
            .header("Content-Range", format!("bytes {start}-{end}/*"))
            .body(chunk.to_vec())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        if status == 308 || status == 200 || status == 201 {
            Ok(())
        } else {
            Err(api_error("Upload", res).await)
        }
    }
}

#[derive(Deserialize)]
struct AboutResponse {
    #[serde(rename = "storageQuota")]
    storage_quota: Option<StorageQuota>,
}

#[derive(Deserialize)]
struct StorageQuota {
    limit: Option<String>,
    usage: Option<String>,
}

pub async fn quota(state: &AppState, account_id: &str) -> Result<crate::files::DiskUsage, String> {
    let client = reqwest::Client::new();
    let token = ensure_token(&client, state, account_id).await?;
    let res = client
        .get("https://www.googleapis.com/drive/v3/about?fields=storageQuota")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("Drive quota failed: {}", res.status()));
    }
    let body: AboutResponse = res.json().await.map_err(|e| e.to_string())?;
    let q = body
        .storage_quota
        .ok_or_else(|| "Drive returned no quota".to_string())?;
    let total: u64 = q.limit.and_then(|v| v.parse().ok()).unwrap_or(0);
    let used: u64 = q.usage.and_then(|v| v.parse().ok()).unwrap_or(0);
    Ok(crate::files::DiskUsage {
        total,
        free: total.saturating_sub(used),
        mount: "Google Drive".into(),
    })
}

pub async fn download_file(
    state: &AppState,
    path: &str,
    dest: &str,
) -> Result<String, String> {
    let res = open_download(state, path).await?;
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    let dest_path = PathBuf::from(dest);
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    tokio::fs::write(&dest_path, bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(dest_path.display().to_string())
}

pub async fn cache_file(state: &AppState, path: &str, name: &str) -> Result<String, String> {
    let dest = state.cache_dir().join(name);
    download_file(state, path, &dest.display().to_string()).await
}
