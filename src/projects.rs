//! Maestro projects (profiles): what is watched, switching between them, and the
//! GitHub-side setup (labels, project boards). Shared by the HTTP API and the
//! assistant's MCP tools.

use crate::app::AppRef;
use crate::engine;
use crate::github::Github;
use crate::model::*;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

/// Make `key` the active project. Only one project runs at a time: the engine is paused
/// and the switch is refused while agents are working.
pub async fn activate(app: &AppRef, mut list: Vec<ProjectProfile>, key: &str) -> Result<()> {
    anyhow::ensure!(app.busy.lock().is_empty(), "Agents are still running — pause the engine and wait for them to finish before switching project");
    let target = list.iter().find(|p| p.key == key).cloned().ok_or_else(|| anyhow::anyhow!("unknown project '{key}'"))?;
    let mut s = app.db.settings();
    s.apply_profile(&target);
    s.engine_enabled = false;
    let s = s.with_defaults();
    if let Some(p) = list.iter_mut().find(|p| p.key == key) {
        *p = s.to_profile(&target.name);
    }
    app.db.save_projects(&list);
    app.db.save_settings(&s)?;
    *app.project.lock() = None;
    *app.last_sync.lock() = None;
    app.wake.notify_one();
    let app2 = app.clone();
    tokio::spawn(async move {
        if let Err(e) = engine::sync_once(&app2).await {
            *app2.last_sync.lock() = Some((crate::db::now(), Some(e.to_string())));
            app2.notice("warn", format!("sync: {e}"));
        }
    });
    Ok(())
}

fn current_list(app: &AppRef) -> Vec<ProjectProfile> {
    let title = app.project.lock().as_ref().map(|p| p.title.clone());
    app.db.projects(title.as_deref())
}

/// Create the labels Maestro uses on a repository (best effort, idempotent).
pub async fn ensure_repo_labels(gh: &Github, s: &Settings, repo: &str) -> Result<()> {
    gh.ensure_label(repo, &s.trigger_label, "7c6cff", "Maestro picks this issue up").await?;
    for (col, color) in [(&s.col_in_progress, "fbca04"), (&s.col_review, "d876e3"), (&s.col_done, "0e8a16"), (&s.col_blocked, "bfd4f2")] {
        if !col.trim().is_empty() {
            gh.ensure_label(repo, &status_label(col), color, &format!("Maestro: {col}")).await?;
        }
    }
    Ok(())
}

/// Check every repository is reachable; returns a report per repo.
pub async fn check_repos(gh: &Github, repos: &[String]) -> Vec<Value> {
    let mut out = vec![];
    for r in repos {
        out.push(match gh.repo_info(r).await {
            Ok(v) => json!({"repo": v["full_name"].as_str().unwrap_or(r), "ok": true, "private": v["private"],
                "push": v["permissions"]["push"].as_bool().unwrap_or(false), "default_branch": v["default_branch"], "issues": v["has_issues"]}),
            Err(e) => json!({"repo": r, "ok": false, "error": format!("{e:#}")}),
        });
    }
    out
}

/// Add (or re-activate) a repo-mode project watching `repos` (the first one is the primary).
pub async fn add_repo_project(app: &AppRef, repos: &[String], name: Option<&str>, mirror: Option<(String, i64)>) -> Result<String> {
    let repos = parse_repos(&repos.join("\n"));
    anyhow::ensure!(!repos.is_empty(), "give at least one repository as owner/name");
    let gh = app.github().await?;
    // real names (case) and access check
    let mut names = vec![];
    for r in &repos {
        let v = gh.repo_info(r).await.with_context(|| format!("repository {r} not found or not accessible with this token"))?;
        names.push(v["full_name"].as_str().unwrap_or(r).to_string());
    }
    let key = repo_key(&names[0]);
    let mut list = current_list(app);
    if !list.iter().any(|p| p.key == key) {
        let mut s = Settings::default();
        s.source = "repo".into();
        s.repos = names.join("\n");
        if let Some((o, n)) = &mirror {
            s.project_owner = o.clone();
            s.project_number = *n;
        }
        let s = s.with_defaults();
        for r in &names {
            if let Err(e) = ensure_repo_labels(&gh, &s, r).await {
                app.notice("warn", format!("{r}: cannot create labels: {e:#}"));
            }
        }
        let display = name.filter(|n| !n.trim().is_empty()).map(String::from).unwrap_or_else(|| names[0].split('/').nth(1).unwrap_or(&names[0]).to_string());
        list.push(s.to_profile(&display));
    }
    activate(app, list, &key).await?;
    Ok(key)
}

/// Add (or re-activate) a project-mode project on a GitHub Project V2 board.
pub async fn add_board_project(app: &AppRef, owner: &str, number: i64, title: Option<&str>) -> Result<String> {
    let key = project_key(owner, number);
    anyhow::ensure!(!key.is_empty(), "owner and project number are required");
    let mut list = current_list(app);
    if !list.iter().any(|p| p.key == key) {
        let mut s = Settings::default();
        s.source = "project".into();
        s.project_owner = owner.trim().into();
        s.project_number = number;
        let s = s.with_defaults();
        let gh = app.github().await?;
        let info = gh.project(owner, number, &s.status_field).await?;
        let name = title.filter(|t| !t.is_empty()).unwrap_or(&info.title).to_string();
        list.push(s.to_profile(&name));
    }
    activate(app, list, &key).await?;
    Ok(key)
}

pub async fn switch(app: &AppRef, key: &str) -> Result<()> {
    let list = current_list(app);
    activate(app, list, key).await
}

pub fn remove(app: &AppRef, key: &str) -> Result<()> {
    if key == app.db.settings().project_key() {
        bail!("Switch to another project before removing this one");
    }
    app.db.delete_project(key);
    Ok(())
}

pub fn rename(app: &AppRef, key: &str, name: &str) -> Result<()> {
    anyhow::ensure!(!name.trim().is_empty(), "name is empty");
    let mut list = app.db.projects(None);
    let p = list.iter_mut().find(|p| p.key == key).context("unknown project")?;
    p.name = name.trim().into();
    app.db.save_projects(&list);
    Ok(())
}

/// Persist new settings; when the active project's identity changed (other primary repo,
/// other board), keep its tickets under the new key.
pub fn save_settings(app: &AppRef, old: &Settings, new: Settings) -> Result<Settings> {
    let new = new.with_defaults();
    app.db.save_settings(&new)?;
    let (ok, nk) = (old.project_key(), new.project_key());
    if !ok.is_empty() && !nk.is_empty() && ok != nk {
        app.db.rekey_project(&ok, &nk);
    }
    if new.project_owner != old.project_owner || new.project_number != old.project_number || new.status_field != old.status_field || new.source != old.source {
        *app.project.lock() = None;
    }
    if new.repos != old.repos || new.trigger_label != old.trigger_label {
        // new repos get the labels too
        let app2 = app.clone();
        let s2 = new.clone();
        tokio::spawn(async move {
            if let Ok(gh) = app2.github().await {
                for r in s2.repo_list() {
                    let _ = ensure_repo_labels(&gh, &s2, &r).await;
                }
            }
        });
    }
    app.wake.notify_one();
    Ok(new)
}

/// Create a GitHub Project V2 ready for Maestro: Status columns completed, repositories
/// linked. `attach`: "mirror" (board for the active repo-mode project), "switch" (new
/// project-mode Maestro project) or "none".
pub async fn create_board(app: &AppRef, owner: &str, title: &str, link: &[String], attach: &str) -> Result<Value> {
    let gh = app.github().await?;
    let (number, url, id) = gh.create_project(owner, title).await?;
    let defaults = Settings::default().with_defaults();
    let mut notes = vec![];
    match gh.project(owner, number, &defaults.status_field).await {
        Ok(info) => {
            let wanted = vec![defaults.col_trigger.clone(), defaults.col_in_progress.clone(), defaults.col_review.clone(), defaults.col_done.clone()];
            match gh.ensure_status_options(&info, &wanted).await {
                Ok(added) if !added.is_empty() => notes.push(format!("added Status columns: {}", added.join(", "))),
                Ok(_) => {}
                Err(e) => notes.push(format!("could not add Status columns ({e:#}); add 'In Review' by hand")),
            }
        }
        Err(e) => notes.push(format!("could not read the Status field: {e:#}")),
    }
    for r in link {
        if let Err(e) = gh.link_project_repo(&id, r).await {
            notes.push(format!("link {r}: {e:#}"));
        }
    }
    match attach {
        "mirror" => {
            let old = app.db.settings();
            anyhow::ensure!(old.repo_mode(), "the active project is not in repo mode: use attach = switch");
            let mut new = old.clone();
            new.project_owner = owner.into();
            new.project_number = number;
            save_settings(app, &old, new)?;
            notes.push("set as the mirror board of the active project".into());
        }
        "switch" => {
            add_board_project(app, owner, number, Some(title)).await?;
            notes.push("added as a Maestro project and switched to it (engine paused)".into());
        }
        _ => {}
    }
    Ok(json!({"number": number, "url": url, "owner": owner, "notes": notes}))
}
