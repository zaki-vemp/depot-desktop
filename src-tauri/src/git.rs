//! Git source control for the code workspace.
//!
//! Everything goes through the `git` binary rather than a linked libgit2. It
//! is what the user already has configured — credentials, hooks, includes,
//! worktrees, LFS — and it keeps the build free of another C dependency.
//! Absence of git is not an error: the panel just reports that there is no
//! repository here.

use serde::Serialize;
use std::path::Path;
use tokio::process::Command;

/// One path in one state. A file staged *and* modified again appears twice —
/// once with `staged: true`, once without — which is what the two lists in the
/// panel show.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitFile {
    /// Repo-relative, always forward-slashed (git's own form).
    pub path: String,
    /// Absolute path, so the frontend can open it without rejoining.
    pub abs_path: String,
    pub name: String,
    /// `modified` | `added` | `deleted` | `renamed` | `copied` | `untracked` | `conflicted`
    pub kind: String,
    pub staged: bool,
    /// Previous path for renames and copies.
    pub orig_path: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitRepo {
    pub root: String,
    pub branch: String,
    /// Commits ahead of / behind the upstream, when there is one.
    pub ahead: u32,
    pub behind: u32,
    pub upstream: Option<String>,
    pub staged: Vec<GitFile>,
    pub unstaged: Vec<GitFile>,
}

/// Spawns git without flashing a console window on Windows.
fn git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root);
    // Never prompt for credentials from a GUI subprocess — it would hang
    // forever with nowhere to type.
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_OPTIONAL_LOCKS", "0");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd
}

async fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(root)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("git is not available: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Raw bytes, for `git show` of a blob that may not be UTF-8.
async fn run_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = git(root)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("git is not available: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}

fn kind_for(code: char) -> &'static str {
    match code {
        'M' => "modified",
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        'U' => "conflicted",
        '?' => "untracked",
        'T' => "modified", // type change, e.g. file → symlink
        _ => "modified",
    }
}

fn absolute(root: &str, rel: &str) -> String {
    Path::new(root)
        .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR).as_str())
        .display()
        .to_string()
}

fn file_entry(root: &str, rel: &str, code: char, staged: bool, orig: Option<String>) -> GitFile {
    GitFile {
        name: rel.rsplit('/').next().unwrap_or(rel).to_string(),
        abs_path: absolute(root, rel),
        path: rel.to_string(),
        kind: kind_for(code).to_string(),
        staged,
        orig_path: orig,
    }
}

/// Parses `git status --porcelain=v1 -z`.
///
/// `-z` matters: it turns off git's path quoting, so names with spaces,
/// quotes or non-ASCII bytes arrive verbatim instead of C-escaped.
fn parse_status(root: &str, raw: &str) -> (Vec<GitFile>, Vec<GitFile>) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    let mut fields = raw.split('\0');
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let mut chars = entry.chars();
        let x = chars.next().unwrap_or(' ');
        let y = chars.next().unwrap_or(' ');
        let path = entry[3..].to_string();

        // Rename and copy entries are followed by the original path.
        let orig = if x == 'R' || x == 'C' {
            fields.next().map(str::to_string)
        } else {
            None
        };

        if x == '?' && y == '?' {
            unstaged.push(file_entry(root, &path, '?', false, None));
            continue;
        }
        if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
            unstaged.push(file_entry(root, &path, 'U', false, orig));
            continue;
        }
        if x != ' ' {
            staged.push(file_entry(root, &path, x, true, orig.clone()));
        }
        if y != ' ' {
            unstaged.push(file_entry(root, &path, y, false, orig));
        }
    }

    (staged, unstaged)
}

/// Repository containing `cwd`, or `None` when there is no repo or no git.
pub async fn info(cwd: String) -> Result<Option<GitRepo>, String> {
    let dir = Path::new(&cwd);
    if !dir.is_dir() {
        return Ok(None);
    }

    let root = match run(dir, &["rev-parse", "--show-toplevel"]).await {
        Ok(out) => out.trim().to_string(),
        // Not a repository, or git is not installed. Both mean "no panel".
        Err(_) => return Ok(None),
    };
    if root.is_empty() {
        return Ok(None);
    }
    let root_path = Path::new(&root).to_path_buf();

    let branch = run(&root_path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .map(|b| b.trim().to_string())
        .unwrap_or_default();
    // A repository with no commits yet reports the literal "HEAD".
    let branch = if branch == "HEAD" || branch.is_empty() {
        run(&root_path, &["symbolic-ref", "--short", "HEAD"])
            .await
            .map(|b| b.trim().to_string())
            .unwrap_or_else(|_| "detached".into())
    } else {
        branch
    };

    let upstream = run(
        &root_path,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"],
    )
    .await
    .ok()
    .map(|u| u.trim().to_string())
    .filter(|u| !u.is_empty());

    let (mut ahead, mut behind) = (0, 0);
    if upstream.is_some() {
        if let Ok(counts) = run(&root_path, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]).await {
            let mut parts = counts.split_whitespace();
            behind = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            ahead = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
    }

    let raw = run(
        &root_path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let (staged, unstaged) = parse_status(&root, &raw);

    Ok(Some(GitRepo {
        root,
        branch,
        ahead,
        behind,
        upstream,
        staged,
        unstaged,
    }))
}

/// Contents of a path at a revision — `HEAD` for the last commit, `:` for the
/// index. Returns an empty string when the file does not exist there, which is
/// exactly what a diff against a newly added file should show.
pub async fn show(root: String, rev: String, path: String) -> Result<String, String> {
    let spec = if rev == ":" {
        format!(":{path}")
    } else {
        format!("{rev}:{path}")
    };
    match run_bytes(Path::new(&root), &["show", &spec]).await {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => Ok(String::new()),
    }
}

pub async fn stage(root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(Path::new(&root), &args).await.map(|_| ())
}

pub async fn unstage(root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let dir = Path::new(&root);

    // In a repo with no commits there is no HEAD to restore from, and
    // `rm --cached` is the only way to unstage. Gate it on HEAD actually being
    // missing: used as a blind fallback it would *untrack* the file instead of
    // just unstaging it, which is not what the button says it does.
    if run(dir, &["rev-parse", "--verify", "HEAD"]).await.is_err() {
        let mut fallback = vec!["rm", "--cached", "-r", "--"];
        fallback.extend(paths.iter().map(String::as_str));
        return run(dir, &fallback).await.map(|_| ());
    }

    let mut args = vec!["restore", "--staged", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(dir, &args).await.map(|_| ())
}

/// Throws away working-tree edits. Untracked files are deleted outright, so
/// the frontend confirms before calling this.
pub async fn discard(root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["checkout", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    if run(Path::new(&root), &args).await.is_ok() {
        return Ok(());
    }
    // Untracked, or no HEAD yet: remove the file instead of reverting it.
    let mut clean = vec!["clean", "-fd", "--"];
    clean.extend(paths.iter().map(String::as_str));
    run(Path::new(&root), &clean).await.map(|_| ())
}

pub async fn commit(root: String, message: String, amend: bool) -> Result<String, String> {
    if message.trim().is_empty() && !amend {
        return Err("Write a commit message first".into());
    }
    let mut args = vec!["commit", "-m", message.as_str()];
    if amend {
        args.push("--amend");
    }
    run(Path::new(&root), &args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_index_and_worktree_states() {
        // "MM" is staged *and* modified again: it belongs in both lists.
        let raw = " M a.txt\0M  b.txt\0MM c.txt\0?? d.txt\0A  e.txt\0";
        let (staged, unstaged) = parse_status("/repo", raw);

        let s: Vec<_> = staged.iter().map(|f| (f.path.as_str(), f.kind.as_str())).collect();
        let u: Vec<_> = unstaged.iter().map(|f| (f.path.as_str(), f.kind.as_str())).collect();

        assert_eq!(s, vec![("b.txt", "modified"), ("c.txt", "modified"), ("e.txt", "added")]);
        assert_eq!(
            u,
            vec![("a.txt", "modified"), ("c.txt", "modified"), ("d.txt", "untracked")]
        );
    }

    #[test]
    fn reads_the_original_path_of_a_rename() {
        let raw = "R  new.txt\0old.txt\0 M other.txt\0";
        let (staged, unstaged) = parse_status("/repo", raw);
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].kind, "renamed");
        assert_eq!(staged[0].orig_path.as_deref(), Some("old.txt"));
        // The rename's original path must not be mistaken for another entry.
        assert_eq!(unstaged.len(), 1);
        assert_eq!(unstaged[0].path, "other.txt");
    }

    #[test]
    fn keeps_paths_with_spaces_intact() {
        let raw = " M dir with spaces/a b.txt\0";
        let (_, unstaged) = parse_status("/repo", raw);
        assert_eq!(unstaged[0].path, "dir with spaces/a b.txt");
        assert_eq!(unstaged[0].name, "a b.txt");
    }

    #[test]
    fn treats_unmerged_entries_as_conflicts() {
        let raw = "UU both.txt\0AA added.txt\0";
        let (staged, unstaged) = parse_status("/repo", raw);
        assert!(staged.is_empty());
        assert_eq!(unstaged.len(), 2);
        assert!(unstaged.iter().all(|f| f.kind == "conflicted"));
    }

    /* ── against a real repository ───────────────────────────────────────
       These drive the actual `git` binary in a throwaway directory, which is
       the only way to check that the argument lists we build do what the
       buttons in the panel claim. Skipped when git is not installed. */

    struct Scratch(std::path::PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn scratch_repo(tag: &str) -> Option<Scratch> {
        let dir = std::env::temp_dir().join(format!("depot-git-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if run(&dir, &["init", "--quiet"]).await.is_err() {
            return None; // no git on this machine
        }
        run(&dir, &["config", "user.email", "test@example.com"]).await.unwrap();
        run(&dir, &["config", "user.name", "Depot Test"]).await.unwrap();
        run(&dir, &["config", "commit.gpgsign", "false"]).await.unwrap();
        Some(Scratch(dir))
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[tokio::test]
    async fn tracks_a_file_through_stage_commit_and_edit() {
        let Some(repo) = scratch_repo("flow").await else { return };
        let dir = repo.0.clone();
        let root = dir.display().to_string();

        // Untracked.
        write(&dir, "a.txt", "one\n");
        let st = info(root.clone()).await.unwrap().unwrap();
        assert_eq!(st.unstaged.len(), 1);
        assert_eq!(st.unstaged[0].kind, "untracked");
        assert!(st.staged.is_empty());

        // Staged, before any commit exists.
        stage(root.clone(), vec!["a.txt".into()]).await.unwrap();
        let st = info(root.clone()).await.unwrap().unwrap();
        assert_eq!(st.staged.len(), 1);
        assert_eq!(st.staged[0].kind, "added");

        commit(root.clone(), "first".into(), false).await.unwrap();
        let st = info(root.clone()).await.unwrap().unwrap();
        assert!(st.staged.is_empty() && st.unstaged.is_empty(), "clean after commit");

        // Edited: now modified, and HEAD still holds the original.
        write(&dir, "a.txt", "one\ntwo\n");
        let st = info(root.clone()).await.unwrap().unwrap();
        assert_eq!(st.unstaged.len(), 1);
        assert_eq!(st.unstaged[0].kind, "modified");
        assert_eq!(show(root.clone(), "HEAD".into(), "a.txt".into()).await.unwrap(), "one\n");

        // Discard puts the working tree back.
        discard(root.clone(), vec!["a.txt".into()]).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");
    }

    #[tokio::test]
    async fn unstage_keeps_the_file_tracked() {
        let Some(repo) = scratch_repo("unstage").await else { return };
        let root = repo.0.display().to_string();

        write(&repo.0, "a.txt", "one\n");
        stage(root.clone(), vec!["a.txt".into()]).await.unwrap();
        commit(root.clone(), "first".into(), false).await.unwrap();

        write(&repo.0, "a.txt", "changed\n");
        stage(root.clone(), vec!["a.txt".into()]).await.unwrap();
        unstage(root.clone(), vec!["a.txt".into()]).await.unwrap();

        let st = info(root.clone()).await.unwrap().unwrap();
        // The regression this guards: `git rm --cached` as a blind fallback
        // would report the file as deleted-from-index plus untracked.
        assert!(st.staged.is_empty(), "nothing should remain staged");
        assert_eq!(st.unstaged.len(), 1);
        assert_eq!(
            st.unstaged[0].kind, "modified",
            "still tracked, just not staged"
        );
    }

    #[tokio::test]
    async fn unstage_works_before_the_first_commit() {
        let Some(repo) = scratch_repo("empty").await else { return };
        let root = repo.0.display().to_string();

        write(&repo.0, "a.txt", "one\n");
        stage(root.clone(), vec!["a.txt".into()]).await.unwrap();
        // No HEAD yet, so this must take the `rm --cached` path.
        unstage(root.clone(), vec!["a.txt".into()]).await.unwrap();

        let st = info(root.clone()).await.unwrap().unwrap();
        assert!(st.staged.is_empty());
        assert_eq!(st.unstaged[0].kind, "untracked");
    }

    #[tokio::test]
    async fn reports_no_repository_outside_one() {
        let dir = std::env::temp_dir().join(format!("depot-git-plain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A bare directory under /tmp is not inside a repo.
        let got = info(dir.display().to_string()).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(got.is_none(), "expected None outside a repository");
    }

    #[tokio::test]
    async fn show_is_empty_for_a_path_absent_from_the_revision() {
        let Some(repo) = scratch_repo("absent").await else { return };
        let root = repo.0.display().to_string();
        write(&repo.0, "a.txt", "one\n");
        stage(root.clone(), vec!["a.txt".into()]).await.unwrap();
        commit(root.clone(), "first".into(), false).await.unwrap();

        // Never committed: the diff's left-hand side is legitimately blank.
        let out = show(root, "HEAD".into(), "never-existed.txt".into()).await.unwrap();
        assert_eq!(out, "");
    }
}
