use librqbit::api::{Api, ApiTorrentListOpts, TorrentIdOrHash};
use librqbit::{AddTorrent, Session};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

use crate::state::AppState;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TorrentInfo {
    pub id: usize,
    pub name: String,
    pub progress: f64,
    pub downloaded: u64,
    pub total: u64,
    pub download_speed: f64,
    pub state: String,
    pub error: Option<String>,
    pub output_folder: String,
}

pub async fn ensure_session(state: &AppState) -> Result<Arc<Session>, String> {
    let mut inner = state.inner.lock().await;
    if let Some(session) = &inner.session {
        return Ok(session.clone());
    }
    let dir = if inner.settings.torrent_download_dir.is_empty() {
        dirs::download_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| "Could not resolve download folder".to_string())?
    } else {
        PathBuf::from(&inner.settings.torrent_download_dir)
    };
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let session = Session::new(dir)
        .await
        .map_err(|e| format!("Could not start torrent engine: {e}"))?;
    inner.session = Some(session.clone());
    Ok(session)
}

fn api_for(session: Arc<Session>) -> Api {
    Api::new(session, None)
}

pub async fn add_magnet(state: &AppState, magnet: &str) -> Result<String, String> {
    if !(magnet.starts_with("magnet:")
        || magnet.ends_with(".torrent")
        || magnet.starts_with("http"))
    {
        return Err("Provide a magnet link, torrent URL, or .torrent path".into());
    }
    let session = ensure_session(state).await?;
    let added = session
        .add_torrent(AddTorrent::from_url(magnet), None)
        .await
        .map_err(|e| e.to_string())?;
    match added.into_handle() {
        Some(handle) => Ok(handle.name().unwrap_or_else(|| "Torrent".into())),
        None => Ok("Torrent already in session".into()),
    }
}

pub async fn list_torrents(state: &AppState) -> Result<Vec<TorrentInfo>, String> {
    let session = {
        let inner = state.inner.lock().await;
        match &inner.session {
            Some(s) => s.clone(),
            None => return Ok(vec![]),
        }
    };
    let api = api_for(session);
    let listed = api.api_torrent_list_ext(ApiTorrentListOpts { with_stats: true });
    Ok(listed
        .torrents
        .into_iter()
        .map(|t| {
            let stats = t.stats;
            let total = stats.as_ref().map(|s| s.total_bytes).unwrap_or(1).max(1);
            let downloaded = stats.as_ref().map(|s| s.progress_bytes).unwrap_or(0);
            let speed = stats
                .as_ref()
                .and_then(|s| s.live.as_ref())
                .map(|l| l.download_speed.mbps * 1024.0 * 1024.0)
                .unwrap_or(0.0);
            TorrentInfo {
                id: t.id.unwrap_or(0),
                name: t.name.unwrap_or_else(|| "Torrent".into()),
                progress: downloaded as f64 / total as f64,
                downloaded,
                total,
                download_speed: speed,
                state: stats
                    .as_ref()
                    .map(|s| format!("{:?}", s.state))
                    .unwrap_or_else(|| "unknown".into()),
                error: stats.and_then(|s| s.error),
                output_folder: t.output_folder,
            }
        })
        .collect())
}

pub async fn pause_torrent(state: &AppState, id: usize) -> Result<(), String> {
    let session = ensure_session(state).await?;
    api_for(session)
        .api_torrent_action_pause(TorrentIdOrHash::Id(id))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub async fn resume_torrent(state: &AppState, id: usize) -> Result<(), String> {
    let session = ensure_session(state).await?;
    api_for(session)
        .api_torrent_action_start(TorrentIdOrHash::Id(id))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
