use crate::drive;
use crate::files;
use crate::state::AppState;
use futures_util::StreamExt;
use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CHUNK: usize = 512 * 1024;
const TICK: Duration = Duration::from_millis(200);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub id: String,
    pub moved: u64,
    pub total: u64,
    pub state: String,
    pub error: Option<String>,
}

/// Emits `transfer` events at most every 200 ms so the UI stays smooth.
struct Reporter {
    app: AppHandle,
    id: String,
    total: u64,
    moved: u64,
    last: Instant,
}

impl Reporter {
    fn new(app: &AppHandle, id: &str, total: u64) -> Self {
        Self {
            app: app.clone(),
            id: id.to_string(),
            total,
            moved: 0,
            last: Instant::now() - TICK,
        }
    }

    fn add(&mut self, n: u64) {
        self.moved += n;
        if self.last.elapsed() >= TICK {
            self.emit("running");
        }
    }

    fn emit(&mut self, state: &str) {
        self.last = Instant::now();
        let _ = self.app.emit(
            "transfer",
            TransferProgress {
                id: self.id.clone(),
                moved: self.moved,
                total: self.total,
                state: state.to_string(),
                error: None,
            },
        );
    }
}

pub async fn run(
    app: AppHandle,
    state: &AppState,
    id: String,
    from: String,
    to: String,
    op: String,
) -> Result<(), String> {
    let is_move = op == "move";
    match op.as_str() {
        "download" => drive_to_local(&app, state, &id, &from, &to, false).await,
        "copy" | "upload" | "move" => copy_any(&app, state, &id, &from, &to, is_move).await,
        other => Err(format!("Unknown transfer type: {other}")),
    }
}

async fn copy_any(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    from: &str,
    to: &str,
    is_move: bool,
) -> Result<(), String> {
    match (drive::is_gdrive_path(from), drive::is_gdrive_path(to)) {
        (false, false) => copy_local(app, id, from, to, is_move).await,
        (true, false) => drive_to_local(app, state, id, from, to, is_move).await,
        (false, true) => local_to_drive(app, state, id, from, to, is_move).await,
        (true, true) => drive_to_drive(app, state, id, from, to, is_move).await,
    }
}

async fn total_size(path: PathBuf) -> u64 {
    tokio::task::spawn_blocking(move || {
        if path.is_dir() {
            walkdir::WalkDir::new(&path)
                .into_iter()
                .flatten()
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        } else {
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        }
    })
    .await
    .unwrap_or(0)
}

/// Paths of every file under `root`, relative to it.
async fn file_list(root: PathBuf) -> Vec<PathBuf> {
    tokio::task::spawn_blocking(move || {
        walkdir::WalkDir::new(&root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.path().strip_prefix(&root).ok().map(|p| p.to_path_buf()))
            .collect()
    })
    .await
    .unwrap_or_default()
}

async fn copy_file(src: &PathBuf, dest: &PathBuf, rep: &mut Reporter) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut input = tokio::fs::File::open(src).await.map_err(|e| e.to_string())?;
    let mut output = tokio::fs::File::create(dest)
        .await
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = input.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        output
            .write_all(&buf[..n])
            .await
            .map_err(|e| e.to_string())?;
        rep.add(n as u64);
    }
    output.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn copy_local(
    app: &AppHandle,
    id: &str,
    from: &str,
    to: &str,
    is_move: bool,
) -> Result<(), String> {
    let src = PathBuf::from(from);
    let dest = PathBuf::from(to);
    if !src.exists() {
        return Err(format!("Source does not exist: {from}"));
    }
    if src == dest {
        return Err("Source and destination are the same".into());
    }

    let total = total_size(src.clone()).await;
    let mut rep = Reporter::new(app, id, total);

    // Same-volume moves are a rename: no bytes travel.
    if is_move && tokio::fs::rename(&src, &dest).await.is_ok() {
        rep.moved = total;
        rep.emit("done");
        return Ok(());
    }

    if src.is_dir() {
        tokio::fs::create_dir_all(&dest)
            .await
            .map_err(|e| e.to_string())?;
        for rel in file_list(src.clone()).await {
            copy_file(&src.join(&rel), &dest.join(&rel), &mut rep).await?;
        }
    } else {
        copy_file(&src, &dest, &mut rep).await?;
    }

    if is_move {
        files::remove_path(from)?;
    }
    rep.moved = rep.moved.max(total);
    rep.emit("done");
    Ok(())
}

async fn drive_to_local(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    from: &str,
    to: &str,
    is_move: bool,
) -> Result<(), String> {
    let meta = drive::file_meta(state, from).await?;
    if meta.is_dir() {
        let dest = PathBuf::from(to);
        tokio::fs::create_dir_all(&dest)
            .await
            .map_err(|e| e.to_string())?;
        let mut rep = Reporter::new(app, id, 0);
        download_drive_tree(state, &meta.account_id, &meta.id, &dest, &mut rep).await?;
        if is_move {
            drive::trash_file(state, from).await?;
        }
        rep.emit("done");
        return Ok(());
    }

    let content = drive::open_content(state, from).await?;
    let mut dest = PathBuf::from(to);
    if dest.file_name().and_then(|n| n.to_str()) != Some(&content.download_name) {
        if dest.extension().is_none() && content.download_name.contains('.') {
            dest.set_file_name(&content.download_name);
        }
    }
    let total = content.response.content_length().unwrap_or(content.meta.size);
    let mut rep = Reporter::new(app, id, total);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut file = tokio::fs::File::create(&dest)
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = content.response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        rep.add(chunk.len() as u64);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    if is_move {
        drive::trash_file(state, from).await?;
    }
    rep.emit("done");
    Ok(())
}

async fn download_drive_tree(
    state: &AppState,
    account_id: &str,
    folder_id: &str,
    dest: &PathBuf,
    rep: &mut Reporter,
) -> Result<(), String> {
    let children = drive::list_files(state, account_id, Some(folder_id.to_string())).await?;
    for child in children {
        let next = dest.join(&child.name);
        if child.is_dir {
            tokio::fs::create_dir_all(&next)
                .await
                .map_err(|e| e.to_string())?;
            let (_, id) = drive::parse_gdrive_path(&child.path)?;
            Box::pin(download_drive_tree(state, account_id, &id, &next, rep)).await?;
        } else {
            let content = drive::open_content(state, &child.path).await?;
            let mut file_dest = next;
            if file_dest.file_name().and_then(|n| n.to_str()) != Some(&content.download_name) {
                file_dest.set_file_name(&content.download_name);
            }
            if let Some(parent) = file_dest.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            let mut file = tokio::fs::File::create(&file_dest)
                .await
                .map_err(|e| e.to_string())?;
            let mut stream = content.response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| e.to_string())?;
                file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                rep.add(chunk.len() as u64);
            }
            file.flush().await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

async fn local_to_drive(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    from: &str,
    to: &str,
    is_move: bool,
) -> Result<(), String> {
    let src = PathBuf::from(from);
    if !src.exists() {
        return Err(format!("Source does not exist: {from}"));
    }
    let (account_id, parent_id, name) = drive::parse_gdrive_dest(to)?;
    let total = total_size(src.clone()).await;
    let mut rep = Reporter::new(app, id, total);
    if src.is_dir() {
        upload_local_tree(state, &src, &account_id, &parent_id, &name, &mut rep).await?;
    } else {
        upload_local_file(state, &src, &account_id, &parent_id, &name, &mut rep).await?;
    }
    if is_move {
        files::remove_path(from)?;
    }
    rep.moved = rep.moved.max(total);
    rep.emit("done");
    Ok(())
}

async fn upload_local_tree(
    state: &AppState,
    src: &PathBuf,
    account_id: &str,
    parent_id: &str,
    name: &str,
    rep: &mut Reporter,
) -> Result<(), String> {
    let folder_id = drive::create_folder(state, account_id, parent_id, name).await?;
    let mut dirs = tokio::fs::read_dir(src).await.map_err(|e| e.to_string())?;
    while let Some(entry) = dirs.next_entry().await.map_err(|e| e.to_string())? {
        let path = entry.path();
        let child_name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            Box::pin(upload_local_tree(
                state,
                &path,
                account_id,
                &folder_id,
                &child_name,
                rep,
            ))
            .await?;
        } else {
            upload_local_file(state, &path, account_id, &folder_id, &child_name, rep).await?;
        }
    }
    Ok(())
}

async fn upload_local_file(
    state: &AppState,
    src: &PathBuf,
    account_id: &str,
    parent_id: &str,
    name: &str,
    rep: &mut Reporter,
) -> Result<(), String> {
    let size = tokio::fs::metadata(src)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let mime = drive::guess_mime(name);
    let uri = drive::begin_upload(state, account_id, parent_id, name, mime, Some(size)).await?;
    if size == 0 {
        drive::put_upload_chunk(&uri, 0, 0, &[]).await?;
        return Ok(());
    }
    let mut input = tokio::fs::File::open(src).await.map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; CHUNK];
    let mut pos = 0u64;
    loop {
        let n = input.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        drive::put_upload_chunk(&uri, pos, size, &buf[..n]).await?;
        pos += n as u64;
        rep.add(n as u64);
    }
    Ok(())
}

async fn drive_to_drive(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    from: &str,
    to: &str,
    is_move: bool,
) -> Result<(), String> {
    let meta = drive::file_meta(state, from).await?;
    let (dest_account, dest_parent, dest_name) = drive::parse_gdrive_dest(to)?;
    if meta.id == dest_parent {
        return Err("Cannot paste a folder into itself".into());
    }

    if meta.account_id == dest_account {
        let mut rep = Reporter::new(app, id, meta.size.max(1));
        if is_move {
            drive::move_within_account(state, from, &dest_parent, &dest_name).await?;
        } else if meta.is_dir() {
            copy_drive_folder_same_account(
                state,
                &meta.account_id,
                &meta.id,
                &dest_parent,
                &dest_name,
                &mut rep,
            )
            .await?;
        } else {
            drive::copy_within_account(state, from, &dest_parent, &dest_name).await?;
        }
        rep.moved = rep.total;
        rep.emit("done");
        return Ok(());
    }

    let mut rep = Reporter::new(app, id, meta.size);
    if meta.is_dir() {
        copy_drive_tree_cross_account(
            state,
            &meta.account_id,
            &meta.id,
            &dest_account,
            &dest_parent,
            &dest_name,
            &mut rep,
        )
        .await?;
    } else {
        stream_drive_file_to_drive(state, from, &dest_account, &dest_parent, &dest_name, &mut rep)
            .await?;
    }
    if is_move {
        drive::trash_file(state, from).await?;
    }
    rep.emit("done");
    Ok(())
}

async fn copy_drive_folder_same_account(
    state: &AppState,
    account_id: &str,
    folder_id: &str,
    dest_parent: &str,
    name: &str,
    rep: &mut Reporter,
) -> Result<(), String> {
    let created = drive::create_folder(state, account_id, dest_parent, name).await?;
    let children = drive::list_files(state, account_id, Some(folder_id.to_string())).await?;
    for child in children {
        if child.is_dir {
            let (_, id) = drive::parse_gdrive_path(&child.path)?;
            Box::pin(copy_drive_folder_same_account(
                state,
                account_id,
                &id,
                &created,
                &child.name,
                rep,
            ))
            .await?;
        } else {
            drive::copy_within_account(state, &child.path, &created, &child.name).await?;
            rep.add(child.size.max(1));
        }
    }
    Ok(())
}

async fn copy_drive_tree_cross_account(
    state: &AppState,
    from_account: &str,
    folder_id: &str,
    dest_account: &str,
    dest_parent: &str,
    name: &str,
    rep: &mut Reporter,
) -> Result<(), String> {
    let created = drive::create_folder(state, dest_account, dest_parent, name).await?;
    let children = drive::list_files(state, from_account, Some(folder_id.to_string())).await?;
    for child in children {
        if child.is_dir {
            let (_, id) = drive::parse_gdrive_path(&child.path)?;
            Box::pin(copy_drive_tree_cross_account(
                state,
                from_account,
                &id,
                dest_account,
                &created,
                &child.name,
                rep,
            ))
            .await?;
        } else {
            stream_drive_file_to_drive(
                state,
                &child.path,
                dest_account,
                &created,
                &child.name,
                rep,
            )
            .await?;
        }
    }
    Ok(())
}

async fn stream_drive_file_to_drive(
    state: &AppState,
    from: &str,
    dest_account: &str,
    dest_parent: &str,
    dest_name: &str,
    rep: &mut Reporter,
) -> Result<(), String> {
    let content = drive::open_content(state, from).await?;
    let name = if dest_name == content.meta.name {
        content.download_name.clone()
    } else {
        dest_name.to_string()
    };
    let mime = content.content_mime.clone();
    let known = content.response.content_length().or({
        if content.meta.size > 0 {
            Some(content.meta.size)
        } else {
            None
        }
    });
    let uri = drive::begin_upload(
        state,
        dest_account,
        dest_parent,
        &name,
        &mime,
        known,
    )
    .await?;

    let mut stream = content.response.bytes_stream();
    if let Some(total) = known {
        if total == 0 {
            drive::put_upload_chunk(&uri, 0, 0, &[]).await?;
            return Ok(());
        }
        let mut pos = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            drive::put_upload_chunk(&uri, pos, total, &chunk).await?;
            pos += chunk.len() as u64;
            rep.add(chunk.len() as u64);
        }
        return Ok(());
    }

    let mut pos = 0u64;
    let mut pending: Option<Vec<u8>> = None;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?.to_vec();
        if let Some(prev) = pending.take() {
            drive::put_upload_chunk_unknown(&uri, pos, &prev, false).await?;
            pos += prev.len() as u64;
            rep.add(prev.len() as u64);
        }
        pending = Some(chunk);
    }
    if let Some(last) = pending {
        if last.is_empty() && pos == 0 {
            drive::put_upload_chunk(&uri, 0, 0, &[]).await?;
        } else {
            drive::put_upload_chunk_unknown(&uri, pos, &last, true).await?;
            rep.add(last.len() as u64);
        }
    } else {
        drive::put_upload_chunk(&uri, 0, 0, &[]).await?;
    }
    Ok(())
}
