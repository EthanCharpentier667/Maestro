//! HTTP API + SSE + embedded web UI.

use crate::agents::{parse_agent, render_agent};
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
        .route("/api/tickets/{id}", get(ticket))
        .route("/api/tickets/{id}/{action}", post(ticket_action))
        .route("/api/runs", get(runs))
        .route("/api/runs/{id}/logs", get(logs))
        .route("/api/runs/{id}/cancel", post(cancel_run))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/settings/test-github", post(test_github))
        .route("/api/settings/test-claude", post(test_claude))
        .route("/api/sync", post(sync))
        .route("/api/engine", post(engine_toggle))
        .route("/api/agents", get(agents))
        .route("/api/agents/{name}", put(put_agent).delete(del_agent))
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
    let last_sync = app.last_sync.lock().clone();
    Ok(Json(json!({
        "tickets": app.db.tickets(),
        "agents": app.lib.agents(),
        "skills": app.lib.skills().iter().map(|s| json!({"name": s.name, "description": s.description})).collect::<Vec<_>>(),
        "settings": s.redacted(),
        "env_secrets": Settings::env_secrets(),
        "project": project,
        "stats": app.db.stats(),
        "statemachine": sm_json(),
        "runs": app.db.runs(None, 60),
        "busy": busy,
        "last_sync": last_sync.map(|(ts, err)| json!({"ts": ts, "error": err})),
        "claude_version": claude::version(&s).await,
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
