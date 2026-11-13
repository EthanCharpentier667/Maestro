//! Built-in MCP server (Streamable HTTP, JSON responses) exposing ticket tools.
//!
//! Agents get it automatically through `--mcp-config` (the URL carries the
//! ticket they work on). You can also plug it into your own Claude Code:
//!   claude mcp add --transport http maestro http://127.0.0.1:7878/mcp \
//!     --header "Authorization: Bearer <token from Settings>"

use crate::agents::Agent;
use crate::app::{App, AppRef};
use crate::engine::{self, NewTicket};
use crate::model::*;
use anyhow::{anyhow, bail, Result};
use axum::extract::{Query, State as AxState};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

/// Max tickets one ticket's agents may create (guards against runaway loops).
const MAX_CREATED_PER_TICKET: usize = 10;
const PROTOCOL: &str = "2025-06-18";

/// Write the `--mcp-config` file for an agent run, or None if the agent opted out.
pub fn config_for(app: &App, ticket: i64, agent: &Agent) -> Result<Option<PathBuf>> {
    if agent.meta.maestro_tools == Some(false) {
        return Ok(None);
    }
    let dir = app.data.join("plugins");
    std::fs::create_dir_all(&dir)?;
    let f = dir.join(format!("mcp-{ticket}-{}.json", agent.meta.name));
    let cfg = json!({"mcpServers": {"maestro": {
        "type": "http",
        "url": format!("{}/mcp?ticket={ticket}&agent={}", app.base_url, agent.meta.name),
        "headers": {"Authorization": format!("Bearer {}", app.db.mcp_token())}
    }}});
    std::fs::write(&f, serde_json::to_string_pretty(&cfg)?)?;
    Ok(Some(f))
}

fn tools() -> Value {
    let num = json!({"type": "integer", "description": "issue number"});
    let repo = json!({"type": "string", "description": "owner/name; defaults to the repository of the current ticket"});
    json!([
      {"name": "list_tickets", "description": "List tickets on the board with their Maestro state, assignee and dependencies.",
       "inputSchema": {"type": "object", "properties": {
          "state": {"type": "string", "description": "filter by state (backlog, triage, ready, blocked, running, reviewing, awaiting_merge, done, needs_human, paused, split)"},
          "query": {"type": "string", "description": "case-insensitive search in titles"}}}},
      {"name": "get_ticket", "description": "Get one ticket: body, state, assignee, dependencies, PR.",
       "inputSchema": {"type": "object", "properties": {"number": num, "repo": repo}, "required": ["number"]}},
      {"name": "list_agents", "description": "List agents and cells a ticket can be assigned to.",
       "inputSchema": {"type": "object", "properties": {}}},
      {"name": "create_ticket", "description": "Create a GitHub issue on the project board, e.g. a follow-up, a bug found while working, or out-of-scope work. Write a clear title and a body with acceptance criteria.",
       "inputSchema": {"type": "object", "properties": {
          "title": {"type": "string"}, "body": {"type": "string", "description": "markdown, with acceptance criteria"},
          "assignee": {"type": "string", "description": "agent or cell name (optional, the orchestrator decides otherwise)"},
          "helpers": {"type": "array", "items": {"type": "string"}},
          "depends_on": {"type": "array", "items": {"type": "integer"}, "description": "issue numbers that must be done first"},
          "repo": repo}, "required": ["title", "body"]}},
      {"name": "comment_ticket", "description": "Post a comment on a ticket's GitHub issue.",
       "inputSchema": {"type": "object", "properties": {"number": num, "body": {"type": "string"}, "repo": repo}, "required": ["number", "body"]}},
      {"name": "close_ticket", "description": "Close a ticket's GitHub issue (e.g. duplicate or no longer needed). You cannot close the ticket you are working on: Maestro does that when its PR is merged.",
       "inputSchema": {"type": "object", "properties": {"number": num, "reason": {"type": "string", "enum": ["completed", "not_planned"]},
          "comment": {"type": "string", "description": "why it is closed"}, "repo": repo}, "required": ["number", "comment"]}}
    ])
}

fn brief(t: &Ticket, app: &App) -> Value {
    json!({
        "number": t.number, "repo": t.repo, "title": t.title, "state": t.state, "assignee": t.agent, "helpers": t.helpers,
        "depends_on": t.deps.iter().filter_map(|d| app.db.ticket(*d)).map(|d| d.number).collect::<Vec<_>>(),
        "github_column": t.gh_status, "pr": t.pr_url, "url": t.url
    })
}

struct Caller {
    ticket: Option<Ticket>,
    agent: String,
}

fn find(app: &App, c: &Caller, args: &Value) -> Result<Ticket> {
    let n = args["number"].as_i64().ok_or_else(|| anyhow!("`number` is required"))?;
    if let Some(r) = args["repo"].as_str().filter(|r| !r.is_empty()) {
        return app.db.ticket_by_number(r, n).ok_or_else(|| anyhow!("#{n} not found in {r}"));
    }
    if let Some(t) = c.ticket.as_ref().and_then(|t| app.db.ticket_by_number(&t.repo, n)) {
        return Ok(t);
    }
    let matches: Vec<Ticket> = app.db.tickets().into_iter().filter(|t| t.number == n).collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => bail!("#{n} is not on the board"),
        _ => bail!("#{n} exists in several repositories, pass `repo`"),
    }
}

async fn call(app: &AppRef, c: &Caller, name: &str, args: &Value) -> Result<Value> {
    match name {
        "list_tickets" => {
            let q = args["query"].as_str().unwrap_or("").to_lowercase();
            let st = args["state"].as_str().unwrap_or("");
            Ok(json!(app
                .db
                .tickets()
                .iter()
                .filter(|t| (st.is_empty() || t.state.as_str() == st) && (q.is_empty() || t.title.to_lowercase().contains(&q)))
                .map(|t| brief(t, app))
                .collect::<Vec<_>>()))
        }
        "get_ticket" => {
            let t = find(app, c, args)?;
            let mut v = brief(&t, app);
            v["body"] = json!(t.body);
            v["note"] = json!(t.note);
            Ok(v)
        }
        "list_agents" => Ok(json!({
            "agents": app.lib.workers().iter().map(|a| json!({"name": a.meta.name, "description": a.meta.description})).collect::<Vec<_>>(),
            "cells": app.lib.cells().iter().map(|x| json!({"name": x.meta.name, "description": x.meta.description, "mode": x.meta.mode, "lead": x.meta.lead, "members": x.meta.members})).collect::<Vec<_>>(),
        })),
        "create_ticket" => {
            if let Some(t) = &c.ticket {
                let mut counts = app.mcp_created.lock();
                let n = counts.entry(t.id).or_insert(0);
                if *n >= MAX_CREATED_PER_TICKET {
                    bail!("limit reached: {MAX_CREATED_PER_TICKET} tickets created while working on #{}", t.number);
                }
                *n += 1;
            }
            let repo = args["repo"].as_str().filter(|r| !r.is_empty()).map(String::from).or_else(|| c.ticket.as_ref().map(|t| t.repo.clone()));
            let repo = repo.ok_or_else(|| anyhow!("`repo` is required"))?;
            let mut deps = vec![];
            for n in args["depends_on"].as_array().cloned().unwrap_or_default() {
                let t = find(app, c, &json!({"number": n, "repo": repo}))?;
                deps.push(t.id);
            }
            let gh = app.github().await?;
            let s = app.db.settings();
            let created_by = match &c.ticket {
                Some(t) => format!("agent `{}` while working on #{}", c.agent, t.number),
                None => c.agent.clone(),
            };
            let t = engine::create_one(app, &gh, NewTicket {
                repo,
                title: args["title"].as_str().unwrap_or("").into(),
                body: args["body"].as_str().unwrap_or("").into(),
                assignee: args["assignee"].as_str().map(String::from),
                helpers: args["helpers"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                deps,
                related: c.ticket.as_ref().map(|t| t.id),
                start: c.ticket.is_none() || s.mcp_auto_queue,
                created_by,
            })
            .await?;
            app.notice("info", format!("{} created #{} {}", c.agent, t.number, t.title));
            Ok(json!({"created": brief(&t, app), "queued": t.state != State::Backlog}))
        }
        "comment_ticket" => {
            let t = find(app, c, args)?;
            let body = args["body"].as_str().unwrap_or("").trim();
            anyhow::ensure!(!body.is_empty(), "empty comment");
            let gh = app.github().await?;
            gh.comment(&t.repo, t.number, &format!("{body}\n\n_- {} (via Maestro)_", c.agent)).await?;
            Ok(json!({"ok": true}))
        }
        "close_ticket" => {
            let t = find(app, c, args)?;
            if c.ticket.as_ref().map(|x| x.id == t.id).unwrap_or(false) {
                bail!("you cannot close the ticket you are working on");
            }
            anyhow::ensure!(!app.busy.lock().contains(&t.id), "#{} is being worked on right now", t.number);
            let gh = app.github().await?;
            let comment = format!("{}\n\n_- closed by {} (via Maestro)_", args["comment"].as_str().unwrap_or(""), c.agent);
            engine::close_ticket(app, &gh, t.id, args["reason"].as_str().unwrap_or("completed"), Some(&comment)).await?;
            Ok(json!({"ok": true}))
        }
        _ => bail!("unknown tool {name}"),
    }
}

fn rpc_ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}
fn rpc_err(id: &Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

async fn handle_one(app: &AppRef, c: &Caller, m: &Value) -> Option<Value> {
    let id = m.get("id").cloned();
    let method = m["method"].as_str().unwrap_or("");
    let id = id?; // notifications get no response
    Some(match method {
        "initialize" => rpc_ok(&id, json!({
            "protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "maestro", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Tickets of the Maestro board (GitHub Project). Create follow-up tickets instead of doing out-of-scope work."
        })),
        "ping" => rpc_ok(&id, json!({})),
        "tools/list" => rpc_ok(&id, json!({"tools": tools()})),
        "tools/call" => {
            let name = m["params"]["name"].as_str().unwrap_or("");
            let args = m["params"]["arguments"].clone();
            let args = if args.is_null() { json!({}) } else { args };
            match call(app, c, name, &args).await {
                Ok(v) => rpc_ok(&id, json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}], "isError": false})),
                Err(e) => rpc_ok(&id, json!({"content": [{"type": "text", "text": format!("Error: {e:#}")}], "isError": true})),
            }
        }
        _ => rpc_err(&id, -32601, "method not found"),
    })
}

pub async fn post(AxState(app): AxState<AppRef>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let token = app.db.mcp_token();
    let given = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from)
        .or_else(|| q.get("token").cloned());
    if given.as_deref() != Some(token.as_str()) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid token"}))).into_response();
    }
    let caller = Caller {
        ticket: q.get("ticket").and_then(|t| t.parse().ok()).and_then(|id| app.db.ticket(id)),
        agent: q.get("agent").cloned().unwrap_or_else(|| "external client".into()),
    };
    if let Some(batch) = body.as_array() {
        let mut out = vec![];
        for m in batch {
            if let Some(r) = handle_one(&app, &caller, m).await {
                out.push(r);
            }
        }
        if out.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        return Json(Value::Array(out)).into_response();
    }
    match handle_one(&app, &caller, &body).await {
        Some(r) => Json(r).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// No server-initiated stream.
pub async fn get() -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}
