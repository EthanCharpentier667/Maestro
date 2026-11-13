//! Git plumbing: every ticket gets its own clone (in `.maestro_workspaces/`) and branch,
//! so agents never step on each other or on the user's checkout.
//! The GitHub token is only passed on the command line for fetch/push and is
//! never written into git config or logs.

use crate::app::App;
use crate::model::{Settings, Ticket};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Git host. Override with MAESTRO_GIT_BASE (GitHub Enterprise, or file:// for tests).
pub fn git_base() -> String {
    std::env::var("MAESTRO_GIT_BASE").unwrap_or_else(|_| "https://github.com".into()).trim_end_matches('/').to_string()
}

pub fn auth_url(token: &str, repo: &str) -> String {
    let base = git_base();
    match base.strip_prefix("https://") {
        Some(host) => format!("https://x-access-token:{token}@{host}/{repo}.git"),
        None => format!("{base}/{repo}.git"),
    }
}

pub async fn git(dir: &Path, args: &[&str], s: &Settings) -> Result<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", &s.git_author_name)
        .env("GIT_AUTHOR_EMAIL", &s.git_author_email)
        .env("GIT_COMMITTER_NAME", &s.git_author_name)
        .env("GIT_COMMITTER_EMAIL", &s.git_author_email)
        .output()
        .await
        .context("git not found")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        // scrub tokens from error messages
        let re = regex::Regex::new(r"x-access-token:[^@]+@").unwrap();
        bail!("git {}: {}", args.first().unwrap_or(&""), re.replace_all(err.trim(), "x-access-token:***@"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn slug(repo: &str) -> String {
    repo.replace('/', "__")
}

/// Hidden folder holding one independent clone per ticket, so agents never share a
/// working tree, an index or branches (nor touch the user's own checkout).
/// Override with MAESTRO_WORKSPACES.
pub fn workspaces_root(app: &App) -> PathBuf {
    match std::env::var("MAESTRO_WORKSPACES") {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => app.data.join(".maestro_workspaces"),
    }
}

fn ticket_dir(app: &App, repo: &str, number: i64) -> PathBuf {
    workspaces_root(app).join(slug(repo)).join(number.to_string())
}

/// Local repository for `repo`: the user's checkout when a path is configured,
/// otherwise a bare mirror managed by Maestro. Only used as a (read-only) object
/// source for workspace clones, never checked out or modified.
async fn object_source(app: &App, token: &str, repo: &str, s: &Settings) -> Result<PathBuf> {
    if let Some(p) = s.repo_path(repo) {
        let p = PathBuf::from(p);
        if !p.join(".git").exists() {
            bail!("{} is not a git repository", p.display());
        }
        return Ok(p);
    }
    let dir = app.data.join("mirrors").join(format!("{}.git", slug(repo)));
    if !dir.join("HEAD").exists() {
        std::fs::create_dir_all(dir.parent().unwrap())?;
        let _ = std::fs::remove_dir_all(&dir);
        let parent = dir.parent().unwrap().to_path_buf();
        git(&parent, &["clone", "--quiet", "--bare", &auth_url(token, repo), dir.to_str().unwrap()], s).await?;
        // don't keep the token in the stored remote
        git(&dir, &["remote", "set-url", "origin", &format!("{}/{repo}.git", git_base())], s).await?;
    }
    Ok(dir)
}

/// Clone `repo` into `dest` (local clone from the object source: fast, objects are
/// hard-linked) and point `origin` at GitHub. Fresh objects are fetched from GitHub.
async fn clone_workspace(app: &App, token: &str, repo: &str, dest: &Path, s: &Settings) -> Result<()> {
    let lock = app.repo_lock(repo);
    let _g = lock.lock().await;
    let src = object_source(app, token, repo, s).await?;
    let _ = std::fs::remove_dir_all(dest); // leftover of an interrupted clone
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let parent = dest.parent().unwrap().to_path_buf();
    git(&parent, &["clone", "--quiet", "--no-checkout", src.to_str().unwrap(), dest.to_str().unwrap()], s).await?;
    git(dest, &["remote", "set-url", "origin", &format!("{}/{repo}.git", git_base())], s).await?;
    Ok(())
}

async fn fetch_base(token: &str, repo: &str, dir: &Path, base: &str, s: &Settings) -> Result<String> {
    let base_ref = format!("refs/maestro/base/{base}");
    git(dir, &["fetch", "--quiet", &auth_url(token, repo), &format!("+refs/heads/{base}:{base_ref}")], s).await?;
    Ok(base_ref)
}

/// Read-only snapshot of `base` for the orchestrator / planner, refreshed on each call.
pub async fn snapshot(app: &App, token: &str, repo: &str, base: &str) -> Result<PathBuf> {
    let s = app.db.settings();
    let dir = workspaces_root(app).join(slug(repo)).join("_base");
    let lock = app.repo_lock(&format!("{repo}#snapshot"));
    let _g = lock.lock().await;
    if !dir.join(".git").exists() {
        clone_workspace(app, token, repo, &dir, &s).await?;
    }
    let base_ref = fetch_base(token, repo, &dir, base, &s).await?;
    git(&dir, &["checkout", "--quiet", "--force", "--detach", &base_ref], &s).await?;
    let _ = git(&dir, &["clean", "-fdq"], &s).await;
    Ok(dir)
}

pub struct Workspace {
    pub path: PathBuf,
    pub branch: String,
    pub base_ref: String,
}

/// The ticket's own clone in `.maestro_workspaces/<repo>/<number>`, on its branch,
/// with the latest base fetched into `refs/maestro/base/<base>`.
pub async fn prepare(app: &App, token: &str, t: &Ticket, base: &str) -> Result<Workspace> {
    let s = app.db.settings();
    let branch = t.branch.clone().unwrap_or_else(|| format!("maestro/{}-{}", t.number, title_slug(&t.title)));
    let ws = ticket_dir(app, &t.repo, t.number);
    if !ws.join(".git").exists() {
        clone_workspace(app, token, &t.repo, &ws, &s).await?;
    }
    // From here on the clone is private to this ticket: no lock needed.
    let base_ref = fetch_base(token, &t.repo, &ws, base, &s).await?;
    let current = git(&ws, &["symbolic-ref", "--quiet", "--short", "HEAD"], &s).await.unwrap_or_default();
    let has_commit = git(&ws, &["rev-parse", "--verify", "--quiet", "HEAD"], &s).await.is_ok();
    if current != branch || !has_commit {
        // Does the branch already exist on GitHub (previous run)? Then continue it.
        let remote_has = git(&ws, &["fetch", "--quiet", &auth_url(token, &t.repo), &format!("+refs/heads/{branch}:refs/maestro/remote/{branch}")], &s)
            .await
            .is_ok();
        let local_has = git(&ws, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")], &s).await.is_ok();
        if local_has {
            git(&ws, &["checkout", "--quiet", "--force", &branch], &s).await?;
        } else {
            let start = if remote_has { format!("refs/maestro/remote/{branch}") } else { base_ref.clone() };
            git(&ws, &["checkout", "--quiet", "--force", "-B", &branch, &start], &s).await?;
        }
    }
    // Re-run (retry / review feedback / conflict fix): keep the branch and its history.
    Ok(Workspace { path: ws, branch, base_ref })
}

/// Commit anything the agent left uncommitted; return number of commits ahead of base.
pub async fn finalize(app: &App, ws: &Workspace, msg: &str) -> Result<i64> {
    let s = app.db.settings();
    if is_merging(ws).await {
        bail!("a merge is still in progress in the workspace (unresolved conflicts)");
    }
    let dirty = git(&ws.path, &["status", "--porcelain"], &s).await?;
    if !dirty.is_empty() {
        git(&ws.path, &["add", "-A"], &s).await?;
        git(&ws.path, &["commit", "--quiet", "-m", msg], &s).await?;
    }
    let n = git(&ws.path, &["rev-list", "--count", &format!("{}..HEAD", ws.base_ref)], &s).await?;
    Ok(n.parse().unwrap_or(0))
}

async fn is_merging(ws: &Workspace) -> bool {
    Command::new("git")
        .current_dir(&ws.path)
        .args(["rev-parse", "--verify", "--quiet", "MERGE_HEAD"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Refresh the base from GitHub and list the files that would conflict when merging
/// it into the branch. Empty = merges cleanly. Nothing is changed in the workspace.
pub async fn conflicts(app: &App, token: &str, repo: &str, ws: &Workspace, base: &str) -> Result<Vec<String>> {
    let s = app.db.settings();
    fetch_base(token, repo, &ws.path, base, &s).await?;
    let out = Command::new("git")
        .current_dir(&ws.path)
        .args(["merge-tree", "--write-tree", "--name-only", "--no-messages", &ws.base_ref, "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .context("git not found")?;
    match out.status.code() {
        Some(0) => Ok(vec![]),
        Some(1) => {
            let mut files: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().skip(1).map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            files.dedup();
            if files.is_empty() {
                files.push("(unknown files)".into());
            }
            Ok(files)
        }
        _ => bail!("git merge-tree failed (git >= 2.38 required): {}", String::from_utf8_lossy(&out.stderr).trim()),
    }
}

pub async fn push(app: &App, token: &str, repo: &str, ws: &Workspace) -> Result<()> {
    let s = app.db.settings();
    git(&ws.path, &["push", "--quiet", "--force", &auth_url(token, repo), &format!("HEAD:refs/heads/{}", ws.branch)], &s).await?;
    Ok(())
}

pub async fn diff_stat(app: &App, ws: &Workspace) -> String {
    let s = app.db.settings();
    git(&ws.path, &["diff", "--stat", &format!("{}...HEAD", ws.base_ref)], &s).await.unwrap_or_default()
}

/// Delete the ticket's workspace (and any worktree left by older versions).
pub async fn cleanup(app: &App, t: &Ticket) {
    let ws = ticket_dir(app, &t.repo, t.number);
    if ws.exists() {
        let _ = tokio::fs::remove_dir_all(&ws).await;
    }
    let legacy = app.data.join("worktrees").join(slug(&t.repo)).join(t.number.to_string());
    if legacy.exists() {
        let s = app.db.settings();
        let lock = app.repo_lock(&t.repo);
        let _g = lock.lock().await;
        let repo_dir = match s.repo_path(&t.repo) {
            Some(p) => PathBuf::from(p),
            None => app.data.join("repos").join(slug(&t.repo)),
        };
        let _ = git(&repo_dir, &["worktree", "remove", "--force", legacy.to_str().unwrap()], &s).await;
        let _ = std::fs::remove_dir_all(&legacy);
    }
}

fn title_slug(t: &str) -> String {
    let mut s: String = t
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').chars().take(40).collect::<String>().trim_matches('-').to_string()
}
