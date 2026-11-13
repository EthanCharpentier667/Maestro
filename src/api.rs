//! HTTP API + SSE + embedded web UI.

use crate::agents::{parse_agent, parse_cell, render_agent, render_cell};
use crate::app::AppRef;
use crate::model::*;
use crate::{chat, claude, engine, projects};
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
        .route("/api/chats", get(chats).post(chat_create))
        .route("/api/chats/{id}", get(chat_get).delete(chat_delete).put(chat_rename))
        .route("/api/chats/{id}/messages", post(chat_send))
        .route("/api/chats/{id}/cancel", post(chat_cancel))
        .route("/api/chats/{id}/reset", post(chat_reset))
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
        "source": {"mode": if s.repo_mode() { "repo" } else { "project" }, "repos": s.repo_list(), "trigger_label": s.trigger_label, "mirror": s.repo_mode() && s.has_project()},
        "assistant": {"agent": s.assistant_agent, "available": app.lib.agent(&s.assistant_agent).is_some(), "busy": chat::busy_map(&app)},
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
    let new = projects::save_settings(&app, &old, new)?;
    Ok(Json(json!(new.redacted())))
}

async fn test_github(AxState(app): AxState<AppRef>) -> R {
    let gh = app.github().await?;
    let login = gh.viewer().await?;
    let s = app.db.settings();
    let mut out = json!({"login": login, "source": if s.repo_mode() { "repo" } else { "project" }});
    if s.repo_mode() {
        out["repos"] = json!(projects::check_repos(&gh, &s.repo_list()).await);
    }
    if s.has_project() {
        let p = app.project_info(&gh, true).await?;
        let cols: Vec<&String> = p.options.iter().map(|(_, n)| n).collect();
        let missing: Vec<&str> = [&s.col_trigger, &s.col_in_progress, &s.col_review, &s.col_done]
            .iter()
            .filter(|c| !c.is_empty() && p.option_id(c).is_none())
            .map(|c| c.as_str())
            .collect();
        out[if s.repo_mode() { "mirror_project" } else { "project" }] = json!({"title": p.title, "url": p.url, "columns": cols, "missing_columns": missing});
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

async fn project_switch(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    projects::switch(&app, b["key"].as_str().unwrap_or("")).await?;
    Ok(Json(json!({"ok": true})))
}

/// `{kind: "repo", repos: [...], name?}` or `{owner, number, title?}` (a GitHub Project board).
async fn project_add(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    let key = if b["kind"].as_str() == Some("repo") || b.get("repos").is_some() {
        let repos: Vec<String> = match &b["repos"] {
            Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            Value::String(s) => parse_repos(s),
            _ => vec![],
        };
        projects::add_repo_project(&app, &repos, b["name"].as_str(), None).await?
    } else {
        projects::add_board_project(&app, b["owner"].as_str().unwrap_or(""), b["number"].as_i64().unwrap_or(0), b["title"].as_str()).await?
    };
    Ok(Json(json!({"ok": true, "key": key})))
}

async fn project_remove(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    projects::remove(&app, b["key"].as_str().unwrap_or(""))?;
    Ok(Json(json!({"ok": true})))
}

async fn project_rename(AxState(app): AxState<AppRef>, Json(b): Json<Value>) -> R {
    projects::rename(&app, b["key"].as_str().unwrap_or(""), b["name"].as_str().unwrap_or(""))?;
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

// ───────────────────────────── assistant chats

async fn chats(AxState(app): AxState<AppRef>) -> R {
    Ok(Json(json!({"chats": app.db.chats(), "busy": chat::busy_map(&app)})))
}

async fn chat_create(AxState(app): AxState<AppRef>, body: Option<Json<Value>>) -> R {
    let title = body.as_ref().and_then(|b| b["title"].as_str()).filter(|t| !t.trim().is_empty()).unwrap_or(chat::NEW_TITLE).to_string();
    let c = app.db.create_chat(&title)?;
    app.emit(Event::Chats);
    Ok(Json(json!(c)))
}

async fn chat_get(AxState(app): AxState<AppRef>, Path(id): Path<i64>) -> R {
    let c = app.db.chat(id).ok_or_else(|| anyhow::anyhow!("chat not found"))?;
    let run = app.chat_runs.lock().get(&id).copied();
    Ok(Json(json!({"chat": c, "messages": app.db.chat_messages(id), "run_id": run})))
}

async fn chat_delete(AxState(app): AxState<AppRef>, Path(id): Path<i64>) -> R {
    chat::cancel(&app, id);
    app.db.delete_chat(id);
    app.emit(Event::Chats);
    Ok(Json(json!({"ok": true})))
}

async fn chat_rename(AxState(app): AxState<AppRef>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let t = b["title"].as_str().unwrap_or("").trim();
    if t.is_empty() { return Err(anyhow::anyhow!("title is empty").into()); }
    app.db.update_chat(id, Some(t), None, 0.0);
    app.emit(Event::Chats);
    Ok(Json(json!({"ok": true})))
}

async fn chat_send(AxState(app): AxState<AppRef>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (msg, run) = chat::send(&app, id, b["text"].as_str().unwrap_or("")).await?;
    Ok(Json(json!({"message": msg, "run_id": run})))
}

async fn chat_cancel(AxState(app): AxState<AppRef>, Path(id): Path<i64>) -> R {
    Ok(Json(json!({"ok": chat::cancel(&app, id)})))
}

async fn chat_reset(AxState(app): AxState<AppRef>, Path(id): Path<i64>) -> R {
    app.db.reset_chat_session(id);
    app.db.add_chat_message(id, "system", "New conversation context: the assistant does not remember the messages above.", None, 0.0)?;
    Ok(Json(json!({"ok": true})))
}
