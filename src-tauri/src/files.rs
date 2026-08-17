use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<i64>,
    pub ext: String,
    pub source: String,
    pub mime_type: Option<String>,
    pub account_id: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    pub path: String,
    pub kind: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub total: u64,
    pub free: u64,
    pub mount: String,
}

pub fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "Could not resolve home directory".into())
}

pub fn places() -> Result<Vec<Place>, String> {
    let home = home_dir()?;
    let mut out = vec![Place {
        name: "Home".into(),
        path: home.display().to_string(),
        kind: "home".into(),
    }];

    for (name, path) in [
        ("Desktop", dirs::desktop_dir()),
        ("Documents", dirs::document_dir()),
        ("Downloads", dirs::download_dir()),
        ("Pictures", dirs::picture_dir()),
        ("Music", dirs::audio_dir()),
        ("Movies", dirs::video_dir()),
    ] {
        if let Some(p) = path {
            if p.exists() {
                out.push(Place {
                    name: name.into(),
                    path: p.display().to_string(),
                    kind: name.to_lowercase(),
                });
            }
        }
    }

    for vol in volumes() {
        if !out.iter().any(|p| p.path == vol.path) {
            out.push(vol);
        }
    }

    Ok(out)
}

fn volumes() -> Vec<Place> {
    let mut vols = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = fs::read_dir("/Volumes") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    vols.push(Place {
                        name: entry.file_name().to_string_lossy().into(),
                        path: path.display().to_string(),
                        kind: "volume".into(),
                    });
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for root in ["/mnt", "/media", "/run/media"] {
            collect_mounts(root, &mut vols);
        }
        vols.push(Place {
            name: "Root".into(),
            path: "/".into(),
            kind: "volume".into(),
        });
    }

    #[cfg(windows)]
    {
        for letter in b'A'..=b'Z' {
            let path = format!("{}:\\", letter as char);
            if Path::new(&path).exists() {
                vols.push(Place {
                    name: format!("{}:", letter as char),
                    path,
                    kind: "volume".into(),
                });
            }
        }
    }

    vols
}

#[cfg(target_os = "linux")]
fn collect_mounts(root: &str, vols: &mut Vec<Place>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if root == "/run/media" {
                if let Ok(users) = fs::read_dir(&path) {
                    for user_mount in users.flatten() {
                        let p = user_mount.path();
                        if p.is_dir() {
                            vols.push(Place {
                                name: user_mount.file_name().to_string_lossy().into(),
                                path: p.display().to_string(),
                                kind: "volume".into(),
                            });
                        }
                    }
                }
            } else {
                vols.push(Place {
                    name: entry.file_name().to_string_lossy().into(),
                    path: path.display().to_string(),
                    kind: "volume".into(),
                });
            }
        }
    }
}

pub fn list_dir(path: &str) -> Result<Vec<DirEntry>, String> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err("Not a directory".into());
    }

    let mut items = Vec::new();
    let entries = fs::read_dir(path).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let p = entry.path();
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(p.is_dir());
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = meta.and_then(|m| m.modified().ok()).and_then(|t| {
            t.duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });
        let name = entry.file_name().to_string_lossy().into_owned();
        let ext = if is_dir {
            String::new()
        } else {
            p.extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };
        items.push(DirEntry {
            name,
            path: p.display().to_string(),
            is_dir,
            size,
            modified,
            ext,
            source: "local".into(),
            mime_type: None,
            account_id: None,
        });
    }

    items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(items)
}

pub fn read_text(path: &str, max_bytes: usize) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > max_bytes {
        return Err(format!("File is larger than {} bytes", max_bytes));
    }
    String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8 text".into())
}

/// Writes editor buffers back to disk. Parent directories are created so
/// "new file" in a folder the tree only just showed still lands.
pub fn write_text(path: &str, contents: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_dir() {
        return Err("Path is a directory".into());
    }
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(p, contents).map_err(|e| e.to_string())
}

/// Creates an empty file, refusing to clobber one that already exists.
pub fn create_file(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.exists() {
        return Err(format!("{} already exists", p.display()));
    }
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(p, "").map_err(|e| e.to_string())
}

/// Cheap probe so the editor can refuse binaries before reading them whole.
pub fn is_text_file(path: &str, sniff_bytes: usize) -> Result<bool, String> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; sniff_bytes];
    let read = file.read(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(read);
    // A NUL byte in the first few KB is the same heuristic git uses.
    Ok(!buf.contains(&0))
}

pub fn mkdir(path: &str) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| e.to_string())
}

pub fn rename_path(from: &str, to: &str) -> Result<(), String> {
    fs::rename(from, to).map_err(|e| e.to_string())
}

pub fn remove_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(|e| e.to_string())
    } else {
        fs::remove_file(p).map_err(|e| e.to_string())
    }
}

pub fn copy_path(from: &str, to: &str) -> Result<(), String> {
    let src = Path::new(from);
    let dest = Path::new(to);
    if !src.exists() {
        return Err("Source does not exist".into());
    }
    if src.is_dir() {
        copy_dir(src, dest)
    } else {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(src, dest).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.map_err(|e| e.to_string())?;
        let rel = entry.path().strip_prefix(src).map_err(|e| e.to_string())?;
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn move_path(from: &str, to: &str) -> Result<(), String> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_path(from, to)?;
            remove_path(from)
        }
    }
}

/// Moves a path to the OS trash instead of unlinking it.
pub fn trash_path(path: &str) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// Capacity of the volume that holds `path` — the deepest matching mount point wins.
pub fn disk_usage(path: &str) -> Result<DiskUsage, String> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best: Option<&sysinfo::Disk> = None;
    for disk in disks.list() {
        if target.starts_with(disk.mount_point()) {
            let deeper = best
                .map(|b| disk.mount_point().as_os_str().len() > b.mount_point().as_os_str().len())
                .unwrap_or(true);
            if deeper {
                best = Some(disk);
            }
        }
    }
    let disk = best.ok_or_else(|| format!("No volume found for {}", target.display()))?;
    Ok(DiskUsage {
        total: disk.total_space(),
        free: disk.available_space(),
        mount: disk.mount_point().display().to_string(),
    })
}

pub fn parent_path(path: &str) -> Option<String> {
    Path::new(path)
        .parent()
        .map(|p| p.display().to_string())
        .filter(|p| !p.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("depot-files-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn write_text_round_trips() {
        let path = scratch("round-trip.txt");
        write_text(path.to_str().unwrap(), "line one\nline two\n").unwrap();
        assert_eq!(
            read_text(path.to_str().unwrap(), 1024).unwrap(),
            "line one\nline two\n"
        );

        // Saving again replaces the file rather than appending to it.
        write_text(path.to_str().unwrap(), "replaced").unwrap();
        assert_eq!(read_text(path.to_str().unwrap(), 1024).unwrap(), "replaced");
    }

    #[test]
    fn write_text_creates_missing_parents() {
        let path = scratch("nested/deeper/new.rs");
        write_text(path.to_str().unwrap(), "fn main() {}").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn write_text_refuses_a_directory() {
        let dir = scratch("a-directory");
        fs::create_dir_all(&dir).unwrap();
        assert!(write_text(dir.to_str().unwrap(), "nope").is_err());
    }

    #[test]
    fn create_file_will_not_clobber() {
        let path = scratch("once.txt");
        let _ = fs::remove_file(&path);
        create_file(path.to_str().unwrap()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        assert!(create_file(path.to_str().unwrap()).is_err());
    }

    #[test]
    fn is_text_file_rejects_binaries() {
        let text = scratch("code.ts");
        write_text(text.to_str().unwrap(), "export const x = 1;\n").unwrap();
        assert!(is_text_file(text.to_str().unwrap(), 8192).unwrap());

        let binary = scratch("blob.bin");
        fs::write(&binary, [0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01, 0x02]).unwrap();
        assert!(!is_text_file(binary.to_str().unwrap(), 8192).unwrap());
    }
}
