use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::UNIX_EPOCH;
use tokio::process::Command;

const SIDECAR_SUBS: &[&str] = &["vtt", "srt", "ass", "ssa"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub id: String,
    pub label: String,
    pub language: Option<String>,
    pub kind: String,
}

pub async fn list_subtitles(path: String) -> Result<Vec<SubtitleTrack>, String> {
    let src = PathBuf::from(&path);
    let mut tracks = Vec::new();

    if let Some(parent) = src.parent() {
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if let Ok(entries) = fs::read_dir(parent) {
            let mut sidecars: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.file_stem()
                            .and_then(|s| s.to_str())
                            .map(|name| name == stem || name.starts_with(&format!("{stem}.")))
                            .unwrap_or(false)
                        && p.extension()
                            .and_then(|e| e.to_str())
                            .map(|e| SIDECAR_SUBS.contains(&e.to_ascii_lowercase().as_str()))
                            .unwrap_or(false)
                })
                .collect();
            sidecars.sort();
            for p in sidecars {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("Subtitles");
                let lang = language_from_name(name, stem);
                tracks.push(SubtitleTrack {
                    id: format!("file:{}", p.display()),
                    label: lang
                        .as_deref()
                        .map(|l| format!("File · {l}"))
                        .unwrap_or_else(|| name.to_string()),
                    language: lang,
                    kind: "sidecar".into(),
                });
            }
        }
    }

    if let Some(probe) = find_tool("ffprobe") {
        if let Ok(embedded) = embedded_subs(&probe, &src).await {
            tracks.extend(embedded);
        }
    }

    Ok(tracks)
}

pub async fn subtitle_vtt(app_dir: &Path, video_path: String, track_id: String) -> Result<String, String> {
    let cache = media_cache(app_dir)?;
    if let Some(path) = track_id.strip_prefix("file:") {
        let src = PathBuf::from(path);
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "vtt" {
            return Ok(src.display().to_string());
        }
        if ext == "srt" {
            let dest = cache.join(format!("{}.vtt", file_key(&src)?));
            if !dest.is_file() {
                let body = fs::read_to_string(&src).map_err(|e| e.to_string())?;
                fs::write(&dest, srt_to_vtt(&body)).map_err(|e| e.to_string())?;
            }
            return Ok(dest.display().to_string());
        }
        let dest = cache.join(format!("{}.vtt", file_key(&src)?));
        let ff = find_tool("ffmpeg").ok_or_else(|| "ffmpeg is required to convert this subtitle file".to_string())?;
        run_ffmpeg(
            &ff,
            [
                "-y",
                "-i",
                &src.display().to_string(),
                dest.to_str().ok_or("bad path")?,
            ],
        )
        .await?;
        return Ok(dest.display().to_string());
    }

    if let Some(index) = track_id.strip_prefix("stream:") {
        let video = PathBuf::from(&video_path);
        let dest = cache.join(format!("{}-s{index}.vtt", file_key(&video)?));
        if dest.is_file() {
            return Ok(dest.display().to_string());
        }
        let ff = find_tool("ffmpeg").ok_or_else(|| "ffmpeg is required to extract embedded subtitles".to_string())?;
        let map = format!("0:{index}");
        run_ffmpeg(
            &ff,
            [
                "-y",
                "-i",
                &video.display().to_string(),
                "-map",
                &map,
                dest.to_str().ok_or("bad path")?,
            ],
        )
        .await?;
        return Ok(dest.display().to_string());
    }

    Err("Unknown subtitle track".into())
}

fn media_cache(app_dir: &Path) -> Result<PathBuf, String> {
    let dir = app_dir.join("media-cache");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn file_key(path: &Path) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut h = Sha256::new();
    h.update(b"harbor-vtt-v1");
    h.update(path.to_string_lossy().as_bytes());
    h.update(meta.len().to_le_bytes());
    h.update(mtime.to_le_bytes());
    Ok(h
        .finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect())
}

async fn run_ffmpeg(ffmpeg: &Path, args: impl IntoIterator<Item = &str>) -> Result<(), String> {
    let collected: Vec<&str> = args.into_iter().collect();
    let output = Command::new(ffmpeg)
        .args(&collected)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Could not run ffmpeg: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        let tail = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        Err(if tail.is_empty() {
            "ffmpeg failed".into()
        } else {
            tail
        })
    }
}

async fn embedded_subs(ffprobe: &Path, src: &Path) -> Result<Vec<SubtitleTrack>, String> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            &src.display().to_string(),
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    let mut tracks = Vec::new();
    if let Some(streams) = json.get("streams").and_then(|s| s.as_array()) {
        for stream in streams {
            if stream.get("codec_type").and_then(|v| v.as_str()) != Some("subtitle") {
                continue;
            }
            let index = stream.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
            let codec = stream.get("codec_name").and_then(|v| v.as_str()).unwrap_or("sub");
            let tags = stream.get("tags").cloned().unwrap_or(serde_json::Value::Null);
            let lang = tags
                .get("language")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && *s != "und")
                .map(|s| s.to_string());
            let title = tags
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let label = match (title, lang.clone()) {
                (Some(t), Some(l)) => format!("{t} ({l})"),
                (Some(t), None) => t,
                (None, Some(l)) => format!("Embedded · {l}"),
                (None, None) => format!("Embedded · {codec}"),
            };
            tracks.push(SubtitleTrack {
                id: format!("stream:{index}"),
                label,
                language: lang,
                kind: "embedded".into(),
            });
        }
    }
    Ok(tracks)
}

fn language_from_name(name: &str, stem: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem_l = stem.to_ascii_lowercase();
    let without_ext = lower.rsplit_once('.').map(|(a, _)| a).unwrap_or(&lower);
    let rest = without_ext
        .strip_prefix(&stem_l)
        .unwrap_or(without_ext)
        .trim_matches('.');
    if rest.is_empty() {
        return None;
    }
    Some(rest.replace('.', " · "))
}

fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path.split(sep) {
            let cand = PathBuf::from(dir).join(&exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    for dir in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
        "/usr/bin",
    ] {
        let cand = PathBuf::from(dir).join(&exe);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

pub fn srt_to_vtt(input: &str) -> String {
    let body = input.trim_start_matches('\u{feff}');
    let mut out = String::from("WEBVTT\n\n");
    for block in body.split("\n\n") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let mut i = 0;
        if lines[0].chars().all(|c| c.is_ascii_digit()) {
            i = 1;
        }
        if i >= lines.len() {
            continue;
        }
        let timing = lines[i].replace(',', ".");
        out.push_str(&timing);
        out.push('\n');
        for line in &lines[i + 1..] {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::srt_to_vtt;

    #[test]
    fn converts_srt_commas() {
        let vtt = srt_to_vtt("1\n00:00:01,000 --> 00:00:02,500\nHello\n");
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:01.000 --> 00:00:02.500"));
        assert!(vtt.contains("Hello"));
    }
}
