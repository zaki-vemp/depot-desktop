//! Undo for agent runs.
//!
//! Before an agent is allowed to touch the workspace a checkpoint is taken;
//! afterwards the workspace is compared against it, and each file the agent
//! changed can be kept or put back. This is what makes it safe to let a CLI
//! edit files unattended.
//!
//! In a git repository the checkpoint is a real tree object, written through a
//! *temporary* index so neither the user's index nor their working tree is
//! disturbed. That gets .gitignore handling for free — `node_modules` and
//! `target` are skipped because git already knows to skip them — and git only
//! hashes what actually changed, so a second checkpoint on a large repo is
//! cheap. Outside a repository it falls back to a bounded content snapshot.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::process::Command;

/// Caps for the non-git fallback, so checkpointing a huge folder cannot hang.
const MAX_SNAPSHOT_FILES: usize = 4000;
const MAX_SNAPSHOT_BYTES: usize = 24 * 1024 * 1024;
const MAX_SNAPSHOT_FILE_BYTES: u64 = 1024 * 1024;
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    "vendor",
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    /// Workspace-relative, forward-slashed.
    pub path: String,
    pub abs_path: String,
    pub name: String,
    /// `modified` | `added` | `deleted`
    pub kind: String,
    /// False when the checkpoint has no baseline for this path, so Revert
    /// cannot be offered honestly.
    pub revertible: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointInfo {
    pub id: String,
    /// `git` or `snapshot`.
    pub mode: String,
    /// Set when the snapshot fallback hit its cap; revert coverage is partial.
    pub truncated: bool,
}

enum Baseline {
    /// Tree object written through a throwaway index.
    Git { tree: String },
    Snapshot {
        files: HashMap<String, Vec<u8>>,
        truncated: bool,
    },
}

struct Checkpoint {
    root: PathBuf,
    baseline: Baseline,
}

type Registry = Arc<Mutex<HashMap<String, Checkpoint>>>;

fn store() -> &'static Registry {
    static STORE: OnceLock<Registry> = OnceLock::new();
    STORE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn git_cmd(root: &Path, index: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

async fn git(root: &Path, index: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let out = git_cmd(root, index)
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = git_cmd(root, None)
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

fn temp_index_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "depot-ckpt-index-{}-{}-{}",
        std::process::id(),
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

/// Writes the current working tree into a tree object without touching the
/// user's index. Returns the tree oid.
async fn write_tree(root: &Path) -> Result<String, String> {
    let index = temp_index_path("w");
    let _ = std::fs::remove_file(&index);
    // `add -A` against an empty temp index stages everything git would track,
    // honouring .gitignore, and leaves the real index alone.
    let result = async {
        git(root, Some(&index), &["add", "-A", "."]).await?;
        let tree = git(root, Some(&index), &["write-tree"]).await?;
        Ok::<String, String>(tree.trim().to_string())
    }
    .await;
    let _ = std::fs::remove_file(&index);
    result
}

fn is_probably_text(bytes: &[u8]) -> bool {
    !bytes.iter().take(8192).any(|b| *b == 0)
}

fn snapshot(root: &Path) -> (HashMap<String, Vec<u8>>, bool) {
    let mut files = HashMap::new();
    let mut total = 0usize;
    let mut truncated = false;

    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !e.file_type().is_dir()
                || !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref())
        });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        if files.len() >= MAX_SNAPSHOT_FILES || total >= MAX_SNAPSHOT_BYTES {
            truncated = true;
            break;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > MAX_SNAPSHOT_FILE_BYTES {
            truncated = true;
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else { continue };
        if !is_probably_text(&bytes) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else { continue };
        total += bytes.len();
        files.insert(rel.to_string_lossy().replace('\\', "/"), bytes);
    }

    (files, truncated)
}

pub async fn create(root: String) -> Result<CheckpointInfo, String> {
    let dir = PathBuf::from(&root);
    if !dir.is_dir() {
        return Err(format!("{root} is not a folder"));
    }

    let id = format!(
        "ckpt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );

    let in_repo = git(&dir, None, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|s| s.trim() == "true")
        .unwrap_or(false);

    let (baseline, mode, truncated) = if in_repo {
        match write_tree(&dir).await {
            Ok(tree) => (Baseline::Git { tree }, "git", false),
            Err(_) => {
                let (files, truncated) = snapshot(&dir);
                (Baseline::Snapshot { files, truncated }, "snapshot", truncated)
            }
        }
    } else {
        let (files, truncated) = snapshot(&dir);
        (Baseline::Snapshot { files, truncated }, "snapshot", truncated)
    };

    store().lock().unwrap().insert(
        id.clone(),
        Checkpoint {
            root: dir,
            baseline,
        },
    );

    Ok(CheckpointInfo {
        id,
        mode: mode.into(),
        truncated,
    })
}

fn absolute(root: &Path, rel: &str) -> String {
    root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR).as_str())
        .display()
        .to_string()
}

fn changed_entry(root: &Path, rel: &str, kind: &str, revertible: bool) -> ChangedFile {
    ChangedFile {
        name: rel.rsplit('/').next().unwrap_or(rel).to_string(),
        abs_path: absolute(root, rel),
        path: rel.to_string(),
        kind: kind.to_string(),
        revertible,
    }
}

/// Everything that differs from the checkpoint right now.
pub async fn changes(id: String) -> Result<Vec<ChangedFile>, String> {
    let (root, tree, snap, truncated) = {
        let all = store().lock().unwrap();
        let ck = all.get(&id).ok_or("That checkpoint is gone")?;
        match &ck.baseline {
            Baseline::Git { tree } => (ck.root.clone(), Some(tree.clone()), None, false),
            Baseline::Snapshot { files, truncated } => {
                (ck.root.clone(), None, Some(files.clone()), *truncated)
            }
        }
    };

    let mut out = Vec::new();

    if let Some(tree) = tree {
        // Compare the checkpoint tree with a tree of the workspace as it is
        // now. Diffing two trees (rather than tree-vs-worktree) is what makes
        // files the agent newly created show up as additions.
        let now = write_tree(&root).await?;
        let raw = git(
            &root,
            None,
            &["diff-tree", "-r", "-z", "--name-status", "--no-renames", &tree, &now],
        )
        .await?;

        let mut fields = raw.split('\0').filter(|f| !f.is_empty());
        while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
            let kind = match status.chars().next().unwrap_or('M') {
                'A' => "added",
                'D' => "deleted",
                _ => "modified",
            };
            out.push(changed_entry(&root, path, kind, true));
        }
    } else if let Some(before) = snap {
        let (after, _) = snapshot(&root);
        for (path, old) in &before {
            match after.get(path) {
                None => out.push(changed_entry(&root, path, "deleted", true)),
                Some(new) if new != old => out.push(changed_entry(&root, path, "modified", true)),
                Some(_) => {}
            }
        }
        for path in after.keys() {
            if !before.contains_key(path) {
                // If the snapshot hit its cap, "absent from the baseline" does
                // not prove the agent created it — the file may simply never
                // have been captured. Reverting would delete the user's own
                // file, so it is offered as keep-only.
                out.push(changed_entry(&root, path, "added", !truncated));
            }
        }
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The file's contents at the checkpoint. Empty when it did not exist then,
/// which is what a diff of a newly created file should show on the left.
pub async fn original(id: String, path: String) -> Result<String, String> {
    let (root, tree, snap) = {
        let all = store().lock().unwrap();
        let ck = all.get(&id).ok_or("That checkpoint is gone")?;
        match &ck.baseline {
            Baseline::Git { tree } => (ck.root.clone(), Some(tree.clone()), None),
            Baseline::Snapshot { files, .. } => (
                ck.root.clone(),
                None,
                Some(files.get(&path).cloned().unwrap_or_default()),
            ),
        }
    };

    if let Some(tree) = tree {
        let spec = format!("{tree}:{path}");
        return Ok(git_bytes(&root, &["show", &spec])
            .await
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default());
    }
    Ok(String::from_utf8_lossy(&snap.unwrap_or_default()).into_owned())
}

/// Puts the named paths back to their checkpoint state. A file the agent
/// created is deleted; one it deleted is restored.
pub async fn revert(id: String, paths: Vec<String>) -> Result<(), String> {
    let (root, tree, snap, truncated) = {
        let all = store().lock().unwrap();
        let ck = all.get(&id).ok_or("That checkpoint is gone")?;
        match &ck.baseline {
            Baseline::Git { tree } => (ck.root.clone(), Some(tree.clone()), None, false),
            Baseline::Snapshot { files, truncated } => {
                (ck.root.clone(), None, Some(files.clone()), *truncated)
            }
        }
    };

    for path in paths {
        let target = PathBuf::from(absolute(&root, &path));

        // Existed at the checkpoint? Restore it. Otherwise the agent created
        // it, so undoing means removing it.
        let existed_before: Option<Vec<u8>> = if let Some(tree) = &tree {
            let spec = format!("{tree}:{path}");
            git_bytes(&root, &["show", &spec]).await.ok()
        } else {
            snap.as_ref().and_then(|s| s.get(&path).cloned())
        };

        match existed_before {
            Some(bytes) => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
            }
            None => {
                // Deleting is only safe when the baseline is complete. With a
                // truncated snapshot we cannot tell "the agent created this"
                // from "we never captured it", and guessing wrong destroys the
                // user's file.
                if truncated {
                    return Err(format!(
                        "Cannot undo {path}: this checkpoint is incomplete, so Depot cannot prove the file is new. Delete it yourself if you want it gone."
                    ));
                }
                if target.exists() {
                    std::fs::remove_file(&target).map_err(|e| e.to_string())?;
                }
            }
        }
    }

    Ok(())
}

/// Forgets a checkpoint once every change has been kept or undone.
pub fn discard(id: String) {
    store().lock().unwrap().remove(&id);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn repo(tag: &str) -> Option<Scratch> {
        let dir = std::env::temp_dir().join(format!("depot-ckpt-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if git(&dir, None, &["init", "--quiet"]).await.is_err() {
            return None;
        }
        git(&dir, None, &["config", "user.email", "t@example.com"]).await.unwrap();
        git(&dir, None, &["config", "user.name", "T"]).await.unwrap();
        Some(Scratch(dir))
    }

    fn plain(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("depot-ckpt-plain-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    async fn kinds(id: &str) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = changes(id.to_string())
            .await
            .unwrap()
            .into_iter()
            .map(|c| (c.path, c.kind))
            .collect();
        v.sort();
        v
    }

    #[tokio::test]
    async fn git_checkpoint_sees_edits_additions_and_deletions() {
        let Some(s) = repo("git").await else { return };
        let dir = &s.0;
        std::fs::write(dir.join("keep.txt"), "keep\n").unwrap();
        std::fs::write(dir.join("edit.txt"), "before\n").unwrap();
        std::fs::write(dir.join("gone.txt"), "bye\n").unwrap();
        git(dir, None, &["add", "-A"]).await.unwrap();
        git(dir, None, &["commit", "-qm", "base"]).await.unwrap();

        let ck = create(dir.display().to_string()).await.unwrap();
        assert_eq!(ck.mode, "git");
        assert!(kinds(&ck.id).await.is_empty(), "nothing changed yet");

        // Stand in for the agent.
        std::fs::write(dir.join("edit.txt"), "after\n").unwrap();
        std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();
        std::fs::remove_file(dir.join("gone.txt")).unwrap();

        assert_eq!(
            kinds(&ck.id).await,
            vec![
                ("edit.txt".into(), "modified".into()),
                ("gone.txt".into(), "deleted".into()),
                ("new.txt".into(), "added".into()),
            ]
        );

        assert_eq!(original(ck.id.clone(), "edit.txt".into()).await.unwrap(), "before\n");
        // A file that did not exist at the checkpoint has no left-hand side.
        assert_eq!(original(ck.id.clone(), "new.txt".into()).await.unwrap(), "");
    }

    #[tokio::test]
    async fn git_revert_restores_edits_and_deletions_and_removes_additions() {
        let Some(s) = repo("revert").await else { return };
        let dir = &s.0;
        std::fs::write(dir.join("edit.txt"), "before\n").unwrap();
        std::fs::write(dir.join("gone.txt"), "bye\n").unwrap();
        git(dir, None, &["add", "-A"]).await.unwrap();
        git(dir, None, &["commit", "-qm", "base"]).await.unwrap();

        let ck = create(dir.display().to_string()).await.unwrap();
        std::fs::write(dir.join("edit.txt"), "after\n").unwrap();
        std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();
        std::fs::remove_file(dir.join("gone.txt")).unwrap();

        revert(
            ck.id.clone(),
            vec!["edit.txt".into(), "new.txt".into(), "gone.txt".into()],
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(dir.join("edit.txt")).unwrap(), "before\n");
        assert_eq!(std::fs::read_to_string(dir.join("gone.txt")).unwrap(), "bye\n");
        assert!(!dir.join("new.txt").exists(), "agent's new file should be gone");
        assert!(kinds(&ck.id).await.is_empty(), "clean again after revert");
    }

    #[tokio::test]
    async fn reverting_one_file_leaves_the_others_alone() {
        let Some(s) = repo("partial").await else { return };
        let dir = &s.0;
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        git(dir, None, &["add", "-A"]).await.unwrap();
        git(dir, None, &["commit", "-qm", "base"]).await.unwrap();

        let ck = create(dir.display().to_string()).await.unwrap();
        std::fs::write(dir.join("a.txt"), "a2\n").unwrap();
        std::fs::write(dir.join("b.txt"), "b2\n").unwrap();

        revert(ck.id.clone(), vec!["a.txt".into()]).await.unwrap();

        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "a\n");
        assert_eq!(
            std::fs::read_to_string(dir.join("b.txt")).unwrap(),
            "b2\n",
            "keeping one change must not undo the other"
        );
        assert_eq!(kinds(&ck.id).await, vec![("b.txt".into(), "modified".into())]);
    }

    #[tokio::test]
    async fn checkpoint_does_not_disturb_the_users_index() {
        let Some(s) = repo("index").await else { return };
        let dir = &s.0;
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(dir, None, &["add", "-A"]).await.unwrap();
        git(dir, None, &["commit", "-qm", "base"]).await.unwrap();

        // Deliberately leave one file staged and one merely modified.
        std::fs::write(dir.join("a.txt"), "staged\n").unwrap();
        git(dir, None, &["add", "a.txt"]).await.unwrap();
        std::fs::write(dir.join("b.txt"), "untracked\n").unwrap();

        let before = git(dir, None, &["status", "--porcelain"]).await.unwrap();
        let _ = create(dir.display().to_string()).await.unwrap();
        let after = git(dir, None, &["status", "--porcelain"]).await.unwrap();

        assert_eq!(before, after, "checkpointing must not stage or unstage anything");
    }

    #[tokio::test]
    async fn ignored_paths_are_not_part_of_a_git_checkpoint() {
        let Some(s) = repo("ignored").await else { return };
        let dir = &s.0;
        std::fs::write(dir.join(".gitignore"), "node_modules/\n").unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("node_modules/big.js"), "junk\n").unwrap();
        git(dir, None, &["add", "-A"]).await.unwrap();
        git(dir, None, &["commit", "-qm", "base"]).await.unwrap();

        let ck = create(dir.display().to_string()).await.unwrap();
        std::fs::write(dir.join("node_modules/big.js"), "different junk\n").unwrap();

        assert!(
            kinds(&ck.id).await.is_empty(),
            "ignored files must not show up as agent changes"
        );
    }

    #[tokio::test]
    async fn snapshot_mode_works_outside_a_repository() {
        let s = plain("snap");
        let dir = &s.0;
        std::fs::write(dir.join("a.txt"), "before\n").unwrap();

        let ck = create(dir.display().to_string()).await.unwrap();
        assert_eq!(ck.mode, "snapshot");

        std::fs::write(dir.join("a.txt"), "after\n").unwrap();
        std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();

        assert_eq!(
            kinds(&ck.id).await,
            vec![
                ("a.txt".into(), "modified".into()),
                ("new.txt".into(), "added".into()),
            ]
        );

        revert(ck.id.clone(), vec!["a.txt".into(), "new.txt".into()])
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "before\n");
        assert!(!dir.join("new.txt").exists());
    }

    #[tokio::test]
    async fn a_discarded_checkpoint_reports_itself_gone() {
        let s = plain("discard");
        std::fs::write(s.0.join("a.txt"), "x\n").unwrap();
        let ck = create(s.0.display().to_string()).await.unwrap();
        discard(ck.id.clone());
        assert!(changes(ck.id).await.is_err());
    }
}
