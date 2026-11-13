//! HTTP API + SSE + embedded web UI.

use crate::agents::{parse_agent, parse_cell, render_agent, render_cell};
use crate::app::AppRef;
use crate::model::*;
use crate::{claude, engine};
use axum::extract::{Path, State as AxState};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

type R = Result<Json<Value>, ApiErr>;

pub struct ApiErr(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiErr {
    fn from(e: E) -> Self {
        ApiErr(e.into())
    }
}
impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({"error": format!("{:#}", self.0)}))).into_response()
    }
}

const INDEX: &str = include_str!("../web/index.html");
const JS: &str = include_str!("../web/app.js");
const CSS: &str = include_str!("../web/style.css");

pub fn router(app: AppRef) -> Router {
    Router::new()
        .route("/", get(|| async { ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX) }))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], JS) }))
        .route("/style.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS) }))
        .route("/api/state", get(state))
        .route("/api/events", get(events))
        .route("/api/statemachine", get(statemachine))
        .route("/mcp", post(crate::mcp::post).get(crate::mcp::get))
        .route("/api/tickets", post(create_ticket))
        .route("/api/tickets/{id}", get(ticket))
        .route("/api/tickets/{id}/{action}", post(ticket_action))
        .route("/api/runs", get(runs))
        .route("/api/runs/{id}/logs", get(logs))
        .route("/api/runs/{id}/cancel", post(cancel_run))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/settings/test-github", post(test_github))
        .route("/api/settings/test-claude", post(test_claude))
        .route("/api/projects", get(projects))
        .route("/api/projects/switch", post(project_switch))
        .route("/api/projects/add", post(project_add))
        .route("/api/projects/remove", post(project_remove))
        .route("/api/projects/rename", post(project_rename))
        .route("/api/github/projects", get(gh_projects))
        .route("/api/github/repos", get(gh_repos))
        .route("/api/sync", post(sync))
        .route("/api/engine", post(engine_toggle))
        .route("/api/agents", get(agents))
        .route("/api/agents/{name}", put(put_agent).delete(del_agent))
        .route("/api/cells", get(cells))
        .route("/api/cells/{name}", put(put_cell).delete(del_cell))
        .route("/api/skills", get(skills))
        .route("/api/skills/{name}", put(put_skill).delete(del_skill))
        .route("/api/plan", post(plan))
        .route("/api/plan/apply", post(plan_apply))
        .with_state(app)
}

fn sm_json() -> Value {
    json!({
        "states": State::ALL.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        "transitions": TRANSITIONS.iter().map(|(f,t,l)| json!({"from": f.as_str(), "to": t.as_str(), "label": l})).collect::<Vec<_>>()
    })
}

async fn state(AxState(app): AxState<AppRef>) -> R {
    let s = app.db.settings();
    let busy: Vec<i64> = app.busy.lock().iter().copied().collect();
    let project = app.project.lock().clone();
    let title = project.as_ref().map(|p| p.title.clone());
    let last_sync = app.last_sync.lock().clone();
    Ok(Json(json!({
        "tickets": app.db.tickets(),
        "agents": app.lib.agents(),
        "cells": app.lib.cells(),
        "skills": app.lib.skills().iter().map(|s| json!({"name": s.name, "description": s.description})).collect::<Vec<_>>(),
        "settings": s.redacted(),
        "env_secrets": Settings::env_secrets(),
        "project": project,
        "projects": app.db.projects(title.as_deref()),
        "active_project": s.project_key(),
        "stats": app.db.stats(),
        "statemachine": sm_json(),
        "runs": app.db.runs(None, 60),
        "busy": busy,
        "last_sync": last_sync.map(|(ts, err)| json!({"ts": ts, "error": err})),
        "claude_version": claude::version(&s).await,
        "mcp": {"url": format!("{}/mcp", app.base_url), "token": app.db.mcp_token()},
    })))
}

async fn events(AxState(app): AxState<AppRef>) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = app.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|e| e.ok()).map(|e| Ok(SseEvent::default().data(serde_json::to_string(&e).unwrap_or_default())));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn statemachine() -> Json<Value> {
    Json(sm_json())
}

async fn ticket(AxState(app): AxState<AppRef>, Path(id): Path<i64>) -> R {
    let t = app.db.ticket(id).ok_or_else(|| anyhow::anyhow!("not found"))?;
    Ok(Json(json!({"ticket": t, "transitions": app.db.transitions(id), "runs": app.db.runs(Some(id), 50)})))
}

async fn create_ticket(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let gh = app.github().await?;
    let strs = |k: &str| -> Vec<String> { b[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
    let t = engine::create_one(&app, &gh, engine::NewTicket {
        repo: b["repo"].as_str().unwrap_or("").trim().into(),
        title: b["title"].as_str().unwrap_or("").into(),
        body: b["body"].as_str().unwrap_or("").into(),
        assignee: b["agent"].as_str().map(String::from),
        helpers: strs("helpers"),
        deps: b["deps"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default(),
        related: None,
        start: b["start"].as_bool().unwrap_or(false),
        created_by: "Maestro UI".into(),
    })
    .await?;
    Ok(Json(json!(t)))
}

async fn ticket_action(AxState(app): AxState<AppRef>, Path((id, action)): Path<(i64, String)>, body: Option<Json<Value>>) -> R {
    let body = body.map(|b| b.0).unwrap_or(Value::Null);
    engine::action(&app, id, &action, &body).await?;
    Ok(Json(json!({"ok": true, "ticket": app.db.ticket(id)})))
}

async fn runs(AxState(app): AxState<AppRef>) -> R {
    Ok(Json(json!(app.db.runs(None, 200))))
}

async fn logs(AxState(app): AxState<AppRef>, Path(id): Path<i64>) -> R {
    Ok(Json(json!(app.db.logs(id))))
}

async fn cancel_run(AxState(app): AxState<AppRef>, Path(id): Path<i64>) -> R {
    let tx = app.cancels.lock().remove(&id);
    Ok(Json(json!({"ok": tx.map(|t| t.send(()).is_ok()).unwrap_or(false)})))
}

async fn get_settings(AxState(app): AxState<AppRef>) -> R {
    Ok(Json(json!(app.db.settings().redacted())))
}

async fn put_settings(AxState(app): AxState<AppRef>, Json(mut new): Json<Settings>) -> R {
    let old = app.db.settings();
    let keep = |n: &mut String, o: &str| {
        if n == "••••••" {
            *n = o.to_string();
        }
    };
    keep(&mut new.github_token, &old.github_token);
    keep(&mut new.claude_oauth_token, &old.claude_oauth_token);
    keep(&mut new.anthropic_api_key, &old.anthropic_api_key);
    let new = new.with_defaults();
    let project_changed = new.project_owner != old.project_owner || new.project_number != old.project_number || new.status_field != old.status_field;
    app.db.save_settings(&new)?;
    if project_changed {
        // editing the active project's owner/number re-points it; keep its tickets and name
        let (ok, nk) = (old.project_key(), new.project_key());
        if !ok.is_empty() && !nk.is_empty() && ok != nk {
            app.db.rekey_project(&ok, &nk);
        }
        *app.project.lock() = None;
    }
    app.wake.notify_one();
    Ok(Json(json!(new.redacted())))
}

async fn test_github(AxState(app): AxState<AppRef>) -> R {
    let gh = app.github().await?;
    let login = gh.viewer().await?;
    let s = app.db.settings();
    let mut out = json!({"login": login});
    if !s.project_owner.is_empty() && s.project_number > 0 {
        let p = app.project_info(&gh, true).await?;
        let cols: Vec<&String> = p.options.iter().map(|(_, n)| n).collect();
        let missing: Vec<&str> = [&s.col_trigger, &s.col_in_progress, &s.col_review, &s.col_done]
            .iter()
            .filter(|c| !c.is_empty() && p.option_id(c).is_none())
            .map(|c| c.as_str())
            .collect();
        out["project"] = json!({"title": p.title, "url": p.url, "columns": cols, "missing_columns": missing});
    }
    Ok(Json(out))
}

async fn test_claude(AxState(app): AxState<AppRef>) -> R {
    let s = app.db.settings();
    let v = claude::version(&s).await;
    let r = claude::check(&s).await?;
    Ok(Json(json!({"version": v, "result": r})))
}

async fn sync(AxState(app): AxState<AppRef>) -> R {
    let n = engine::sync_once(&app).await?;
    Ok(Json(json!({"items": n})))
}

async fn engine_toggle(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let mut s = app.db.settings();
    s.engine_enabled = b["enabled"].as_bool().unwrap_or(false);
    app.db.save_settings(&s)?;
    app.wake.notify_one();
    app.notice("info", if s.engine_enabled { "Engine started" } else { "Engine paused (running agents finish their current step)" });
    Ok(Json(json!({"enabled": s.engine_enabled})))
}

async fn agents(AxState(app): AxState<AppRef>) -> R {
    let v: Vec<Value> = app.lib.agents().iter().map(|a| json!({"agent": a, "raw": render_agent(a)})).collect();
    Ok(Json(json!(v)))
}

async fn put_agent(AxState(app): AxState<AppRef>, Path(name): Path<String>, Json(b): Json<Value>) -> R {
    let raw = b["raw"].as_str().ok_or_else(|| anyhow::anyhow!("missing raw"))?;
    let a = parse_agent(raw)?;
    app.lib.save_agent(&a, Some(&name))?;
    Ok(Json(json!(a)))
}

async fn del_agent(AxState(app): AxState<AppRef>, Path(name): Path<String>) -> R {
    app.lib.delete_agent(&name)?;
    Ok(Json(json!({"ok": true})))
}

async fn cells(AxState(app): AxState<AppRef>) -> R {
    let v: Vec<Value> = app.lib.cells().iter().map(|c| json!({"cell": c, "raw": render_cell(c)})).collect();
    Ok(Json(json!(v)))
}

async fn put_cell(AxState(app): AxState<AppRef>, Path(name): Path<String>, Json(b): Json<Value>) -> R {
    let raw = b["raw"].as_str().ok_or_else(|| anyhow::anyhow!("missing raw"))?;
    let c = parse_cell(raw)?;
    app.lib.save_cell(&c, Some(&name))?;
    Ok(Json(json!(c)))
}

async fn del_cell(AxState(app): AxState<AppRef>, Path(name): Path<String>) -> R {
    app.lib.delete_cell(&name)?;
    Ok(Json(json!({"ok": true})))
}

async fn skills(AxState(app): AxState<AppRef>) -> R {
    Ok(Json(json!(app.lib.skills())))
}

async fn put_skill(AxState(app): AxState<AppRef>, Path(name): Path<String>, Json(b): Json<Value>) -> R {
    app.lib.save_skill(&name, b["content"].as_str().unwrap_or(""))?;
    Ok(Json(json!({"ok": true})))
}

async fn del_skill(AxState(app): AxState<AppRef>, Path(name): Path<String>) -> R {
    app.lib.delete_skill(&name)?;
    Ok(Json(json!({"ok": true})))
}

async fn plan(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let goal = b["goal"].as_str().unwrap_or("").trim().to_string();
    let repo = b["repo"].as_str().unwrap_or("").trim().to_string();
    if !(!goal.is_empty()) { return Err(anyhow::anyhow!("goal is empty").into()); }
    if !(repo.contains('/')) { return Err(anyhow::anyhow!("pick a repository (owner/name)").into()); }
    // Planning can take a while: run it in the background, result arrives via SSE.
    let app2 = app.clone();
    tokio::spawn(async move {
        match engine::plan(&app2, &goal, &repo).await {
            Ok(v) => app2.emit(Event::Notice { level: "plan".into(), message: v.to_string() }),
            Err(e) => app2.notice("error", format!("planner: {e:#}")),
        }
    });
    Ok(Json(json!({"started": true})))
}

async fn plan_apply(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let repo = b["repo"].as_str().unwrap_or("").to_string();
    let tickets = b["tickets"].as_array().cloned().unwrap_or_default();
    let start = b["start"].as_bool().unwrap_or(true);
    if !(!tickets.is_empty()) { return Err(anyhow::anyhow!("no tickets").into()); }
    let gh = app.github().await?;
    let created = engine::create_tickets(&app, &gh, &repo, &tickets, None, start).await?;
    app.notice("info", format!("Created {} tickets on GitHub", created.len()));
    Ok(Json(json!({"created": created})))
}

// ───────────────────────────── projects (switcher)

async fn projects(AxState(app): AxState<AppRef>) -> R {
    let title = app.project.lock().as_ref().map(|p| p.title.clone());
    Ok(Json(json!({"projects": app.db.projects(title.as_deref()), "active": app.db.settings().project_key()})))
}

/// Make `key` the active project. Only one project runs at a time: the engine is paused
/// and the switch is refused while agents are working.
async fn activate(app: &AppRef, mut list: Vec<ProjectProfile>, key: &str) -> anyhow::Result<()> {
    anyhow::ensure!(app.busy.lock().is_empty(), "Agents are still running — pause the engine and wait for them to finish before switching project");
    let target = list.iter().find(|p| p.key == key).cloned().ok_or_else(|| anyhow::anyhow!("unknown project"))?;
    let mut s = app.db.settings();
    s.apply_profile(&target);
    s.engine_enabled = false;
    let s = s.with_defaults();
    // persist the (refreshed) outgoing profile + the target
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

async fn project_switch(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let key = b["key"].as_str().unwrap_or("").to_string();
    let title = app.project.lock().as_ref().map(|p| p.title.clone());
    let list = app.db.projects(title.as_deref());
    activate(&app, list, &key).await?;
    Ok(Json(json!({"ok": true})))
}

async fn project_add(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let owner = b["owner"].as_str().unwrap_or("").trim().to_string();
    let number = b["number"].as_i64().unwrap_or(0);
    let key = project_key(&owner, number);
    if !(!key.is_empty()) { return Err(anyhow::anyhow!("owner and project number are required").into()); }
    let title = app.project.lock().as_ref().map(|p| p.title.clone());
    let mut list = app.db.projects(title.as_deref());
    if !list.iter().any(|p| p.key == key) {
        let mut s = Settings::default().with_defaults();
        s.project_owner = owner.clone();
        s.project_number = number;
        // validate against GitHub (also resolves the real title and checks the Status field)
        let gh = app.github().await?;
        let info = gh.project(&owner, number, &s.status_field).await?;
        let name = b["title"].as_str().filter(|t| !t.is_empty()).unwrap_or(&info.title).to_string();
        list.push(s.to_profile(&name));
    }
    activate(&app, list, &key).await?;
    Ok(Json(json!({"ok": true, "key": key})))
}

async fn project_remove(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let key = b["key"].as_str().unwrap_or("");
    if !(key != app.db.settings().project_key()) { return Err(anyhow::anyhow!("Switch to another project before removing this one").into()); }
    app.db.delete_project(key);
    Ok(Json(json!({"ok": true})))
}

async fn project_rename(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let key = b["key"].as_str().unwrap_or("");
    let name = b["name"].as_str().unwrap_or("").trim();
    if !(!name.is_empty()) { return Err(anyhow::anyhow!("name is empty").into()); }
    let mut list = app.db.projects(None);
    if let Some(p) = list.iter_mut().find(|p| p.key == key) {
        p.name = name.into();
    }
    app.db.save_projects(&list);
    Ok(Json(json!({"ok": true})))
}

async fn gh_projects(AxState(app): AxState<AppRef>) -> R {
    let gh = app.github().await?;
    Ok(Json(json!(gh.list_projects().await?)))
}

async fn gh_repos(AxState(app): AxState<AppRef>) -> R {
    let gh = app.github().await?;
    Ok(Json(json!(gh.list_repos().await?)))
}
