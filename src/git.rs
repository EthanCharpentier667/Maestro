//! Git plumbing: one clone per repo, one worktree + branch per ticket.
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

/// Local clone for a repo (user-provided path, or managed clone in data dir).
pub async fn ensure_repo(app: &App, token: &str, repo: &str) -> Result<PathBuf> {
    let lock = app.repo_lock(repo);
    let _g = lock.lock().await;
    ensure_repo_unlocked(app, token, repo).await
}

async fn ensure_repo_unlocked(app: &App, token: &str, repo: &str) -> Result<PathBuf> {
    let s = app.db.settings();
    if let Some(p) = s.repo_path(repo) {
        let p = PathBuf::from(p);
        if !p.join(".git").exists() {
            bail!("{} is not a git repository", p.display());
        }
        return Ok(p);
    }
    let dir = app.data.join("repos").join(slug(repo));
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(dir.parent().unwrap())?;
        let parent = dir.parent().unwrap().to_path_buf();
        git(&parent, &["clone", "--filter=blob:none", &auth_url(token, repo), dir.to_str().unwrap()], &s).await?;
        // don't keep the token in the stored remote
        git(&dir, &["remote", "set-url", "origin", &format!("{}/{repo}.git", git_base())], &s).await?;
    }
    Ok(dir)
}

pub struct Workspace {
    pub path: PathBuf,
    pub branch: String,
    pub base_ref: String,
}

pub async fn prepare(app: &App, token: &str, t: &Ticket, base: &str) -> Result<Workspace> {
    let s = app.db.settings();
    let lock = app.repo_lock(&t.repo);
    let _g = lock.lock().await;
    let repo_dir = ensure_repo_unlocked(app, token, &t.repo).await?;
    let base_ref = format!("refs/maestro/base/{base}");
    git(&repo_dir, &["fetch", "--quiet", &auth_url(token, &t.repo), &format!("+refs/heads/{base}:{base_ref}")], &s).await?;
    let branch = t.branch.clone().unwrap_or_else(|| format!("maestro/{}-{}", t.number, title_slug(&t.title)));
    let wt = app.data.join("worktrees").join(slug(&t.repo)).join(t.number.to_string());

    if wt.join(".git").exists() {
        // Re-run (retry / review feedback): keep the branch and its history.
        let _ = git(&wt, &["checkout", "--quiet", &branch], &s).await;
    } else {
        std::fs::create_dir_all(wt.parent().unwrap())?;
        let _ = git(&repo_dir, &["worktree", "prune"], &s).await;
        // Does the branch already exist on GitHub (previous run)? Then continue it.
        let remote_has = git(&repo_dir, &["fetch", "--quiet", &auth_url(token, &t.repo), &format!("+refs/heads/{branch}:refs/maestro/remote/{branch}")], &s)
            .await
            .is_ok();
        let start = if remote_has { format!("refs/maestro/remote/{branch}") } else { base_ref.clone() };
        git(&repo_dir, &["worktree", "add", "--force", "-B", &branch, wt.to_str().unwrap(), &start], &s).await?;
    }
    Ok(Workspace { path: wt, branch, base_ref })
}

/// Commit anything the agent left uncommitted; return number of commits ahead of base.
pub async fn finalize(app: &App, ws: &Workspace, msg: &str) -> Result<i64> {
    let s = app.db.settings();
    let dirty = git(&ws.path, &["status", "--porcelain"], &s).await?;
    if !dirty.is_empty() {
        git(&ws.path, &["add", "-A"], &s).await?;
        git(&ws.path, &["commit", "--quiet", "-m", msg], &s).await?;
    }
    let n = git(&ws.path, &["rev-list", "--count", &format!("{}..HEAD", ws.base_ref)], &s).await?;
    Ok(n.parse().unwrap_or(0))
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

pub async fn cleanup(app: &App, t: &Ticket) {
    let s = app.db.settings();
    let lock = app.repo_lock(&t.repo);
    let _g = lock.lock().await;
    let wt = app.data.join("worktrees").join(slug(&t.repo)).join(t.number.to_string());
    if wt.exists() {
        let repo_dir = match s.repo_path(&t.repo) {
            Some(p) => PathBuf::from(p),
            None => app.data.join("repos").join(slug(&t.repo)),
        };
        let _ = git(&repo_dir, &["worktree", "remove", "--force", wt.to_str().unwrap()], &s).await;
        let _ = std::fs::remove_dir_all(&wt);
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
