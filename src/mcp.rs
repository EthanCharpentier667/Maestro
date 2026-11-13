//! Built-in MCP server (Streamable HTTP, JSON responses) exposing ticket tools.
//!
//! Agents get it automatically through `--mcp-config` (the URL carries the
//! ticket they work on). The assistant gets the same server with `scope=assistant`,
//! which adds the admin tools (status, ticket actions, engine, projects, GitHub
//! repositories and boards, agent definitions). You can also plug it into your
//! own Claude Code (add `?scope=assistant` to the URL for the admin tools):
//!   claude mcp add --transport http maestro http://127.0.0.1:7878/mcp \
//!     --header "Authorization: Bearer <token from Settings>"

use crate::agents::Agent;
use crate::app::{App, AppRef};
use crate::engine::{self, NewTicket};
use crate::projects;
use crate::model::*;
use anyhow::{anyhow, bail, Context, Result};
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

fn tools(assistant: bool) -> Value {
    let mut v = ticket_tools();
    if assistant {
        v.as_array_mut().unwrap().extend(admin_tools().as_array().unwrap().iter().cloned());
    }
    v
}

fn ticket_tools() -> Value {
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
          "start": {"type": "boolean", "description": "queue it right away (trigger label / column). Default: true outside a ticket run"},
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
    /// assistant scope: admin tools enabled
    assistant: bool,
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
                start: args["start"].as_bool().unwrap_or(c.ticket.is_none() || s.mcp_auto_queue),
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
        _ if c.assistant => admin_call(app, c, name, args).await,
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
            "instructions": if c.assistant { "Maestro control plane: tickets, agents, engine, projects, GitHub repositories and boards. Read before you write; confirm destructive or outward-facing actions with the user." } else { "Tickets of the Maestro board. Create follow-up tickets instead of doing out-of-scope work." }
        })),
        "ping" => rpc_ok(&id, json!({})),
        "tools/list" => rpc_ok(&id, json!({"tools": tools(c.assistant)})),
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
        assistant: q.get("scope").map(|s| s == "assistant").unwrap_or(false),
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

// ───────────────────────────── assistant (admin) tools

fn admin_tools() -> Value {
    let num = json!({"type": "integer", "description": "issue number"});
    let repo = json!({"type": "string", "description": "owner/name (optional when the number is unique on the board)"});
    let strs = json!({"type": "array", "items": {"type": "string"}});
    json!([
      {"name": "maestro_status", "description": "Overview of Maestro right now: active project and source, engine on/off, ticket counts per state, tickets needing a human (with their note), running agents, recent failures, last sync, cost.",
       "inputSchema": {"type": "object", "properties": {}}},
      {"name": "ticket_details", "description": "Everything about one ticket: body, state, assignee, helpers, dependencies (with their state), PR, note, reviewer feedback, state history and runs.",
       "inputSchema": {"type": "object", "properties": {"number": num, "repo": repo}, "required": ["number"]}},
      {"name": "ticket_action", "description": "Drive a ticket through the state machine. start (queue a backlog ticket), retriage (orchestrator decides again), retry (re-run its agent), pause, resume, cancel (stop the current run), reset (back to backlog), assign (needs agent), helpers (needs helpers), deps (needs depends_on), close (needs comment, optional reason completed|not_planned), reopen, merge (squash-merge its PR).",
       "inputSchema": {"type": "object", "properties": {"number": num, "repo": repo,
          "action": {"type": "string", "enum": ["start", "retriage", "retry", "pause", "resume", "cancel", "reset", "assign", "helpers", "deps", "close", "reopen", "merge"]},
          "agent": {"type": "string"}, "helpers": strs, "depends_on": {"type": "array", "items": {"type": "integer"}},
          "comment": {"type": "string"}, "reason": {"type": "string", "enum": ["completed", "not_planned"]}}, "required": ["number", "action"]}},
      {"name": "create_tickets", "description": "Create a dependency graph of tickets in one call (like the Planner). Each item: key, title, body (markdown with acceptance criteria), agent (agent or cell), optional helpers, depends_on (keys of earlier items). Issues are created in order with 'Depends on #n' links.",
       "inputSchema": {"type": "object", "properties": {"repo": {"type": "string", "description": "owner/name; default: primary repository of the active project"},
          "start": {"type": "boolean", "description": "queue them immediately (default true)"},
          "tickets": {"type": "array", "items": {"type": "object", "properties": {"key": {"type": "string"}, "title": {"type": "string"}, "body": {"type": "string"},
             "agent": {"type": "string"}, "helpers": strs, "depends_on": strs}, "required": ["key", "title", "body", "agent"]}}}, "required": ["tickets"]}},
      {"name": "list_runs", "description": "Recent agent runs (triage, work, review, plan, chat) with status, cost and summary.",
       "inputSchema": {"type": "object", "properties": {"number": {"type": "integer", "description": "only runs of this ticket"}, "repo": repo,
          "status": {"type": "string", "description": "running | success | failed | cancelled | interrupted"}, "limit": {"type": "integer"}}}},
      {"name": "get_run_log", "description": "The log of a run: what the agent said, which tools it used, errors, final result. Use it to explain a failure.",
       "inputSchema": {"type": "object", "properties": {"run_id": {"type": "integer"}, "tail": {"type": "integer", "description": "last N lines (default 80)"}}, "required": ["run_id"]}},
      {"name": "engine_control", "description": "Start or pause the engine (pausing lets running agents finish their step) and/or sync GitHub now.",
       "inputSchema": {"type": "object", "properties": {"enabled": {"type": "boolean"}, "sync": {"type": "boolean"}}}},
      {"name": "list_projects", "description": "Maestro projects (each watches repositories or a GitHub Project board) and which one is active.",
       "inputSchema": {"type": "object", "properties": {}}},
      {"name": "switch_project", "description": "Make another Maestro project active. Pauses the engine; refused while agents are running.",
       "inputSchema": {"type": "object", "properties": {"key": {"type": "string"}}, "required": ["key"]}},
      {"name": "add_project", "description": "Add a Maestro project and switch to it (engine paused). kind=repo: watch the issues of `repos` (first = primary), Maestro labels are created on them. kind=project: use an existing GitHub Project board (owner + number).",
       "inputSchema": {"type": "object", "properties": {"kind": {"type": "string", "enum": ["repo", "project"]}, "repos": strs, "name": {"type": "string"},
          "owner": {"type": "string"}, "number": {"type": "integer"}}, "required": ["kind"]}},
      {"name": "configure_project", "description": "Change settings of the active project and execution. Only the fields you pass change. repos replaces the whole list (keep the primary first).",
       "inputSchema": {"type": "object", "properties": {"repos": strs, "trigger_label": {"type": "string"}, "base_branch": {"type": "string"},
          "mirror_owner": {"type": "string", "description": "repo mode: GitHub Project mirrored from the labels ('' to remove)"}, "mirror_number": {"type": "integer"},
          "max_parallel": {"type": "integer"}, "max_attempts": {"type": "integer"}, "max_review_rounds": {"type": "integer"},
          "review_enabled": {"type": "boolean"}, "auto_merge": {"type": "boolean"}, "always_triage": {"type": "boolean"}, "mcp_auto_queue": {"type": "boolean"}}}},
      {"name": "github_list_repos", "description": "Repositories the GitHub token can push to (most recently pushed first).",
       "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}}},
      {"name": "github_repo_info", "description": "Check repositories: exist, private, default branch, can the token push.",
       "inputSchema": {"type": "object", "properties": {"repos": strs}, "required": ["repos"]}},
      {"name": "github_create_repo", "description": "Create a GitHub repository (for the token's user, or an organization via owner). Optionally add it to the active repo-mode project's watched repositories.",
       "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "owner": {"type": "string", "description": "organization; empty = your user"},
          "private": {"type": "boolean", "description": "default true"}, "description": {"type": "string"}, "auto_init": {"type": "boolean", "description": "create with a README so it has a main branch (default true)"},
          "watch": {"type": "boolean", "description": "add it to the active project's repositories (default false)"}}, "required": ["name"]}},
      {"name": "github_list_projects", "description": "GitHub Project V2 boards the token can see.",
       "inputSchema": {"type": "object", "properties": {}}},
      {"name": "github_create_project", "description": "Create a GitHub Project V2 board ready for Maestro (Status columns Todo / In Progress / In Review / Done), link repositories to it, and optionally attach it: mirror (board of the active repo-mode project) or switch (a new board-driven Maestro project).",
       "inputSchema": {"type": "object", "properties": {"owner": {"type": "string", "description": "user or organization login"}, "title": {"type": "string"},
          "link_repos": strs, "attach": {"type": "string", "enum": ["mirror", "switch", "none"]}}, "required": ["owner", "title"]}},
      {"name": "get_definition", "description": "Read the raw markdown of an agent, a cell or a skill (front-matter + instructions). Without name: list all with descriptions.",
       "inputSchema": {"type": "object", "properties": {"kind": {"type": "string", "enum": ["agent", "cell", "skill"]}, "name": {"type": "string"}}, "required": ["kind"]}},
      {"name": "save_definition", "description": "Create or replace an agent, a cell or a skill (full raw markdown with front-matter). Agents and cells are validated; read the current one first and keep what you do not mean to change.",
       "inputSchema": {"type": "object", "properties": {"kind": {"type": "string", "enum": ["agent", "cell", "skill"]}, "name": {"type": "string"}, "content": {"type": "string"}}, "required": ["kind", "name", "content"]}}
    ])
}

fn run_brief(r: &Run, app: &App) -> Value {
    let t = r.ticket_id.and_then(|id| app.db.ticket(id));
    json!({"run_id": r.id, "kind": r.kind, "agent": r.agent, "status": r.status, "ticket": t.map(|t| format!("{}#{}", t.repo, t.number)),
        "started_at": r.started_at, "finished_at": r.finished_at, "cost_usd": r.cost_usd, "turns": r.turns,
        "summary": r.summary.as_deref().map(|s| s.chars().take(600).collect::<String>())})
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

async fn admin_call(app: &AppRef, c: &Caller, name: &str, args: &Value) -> Result<Value> {
    match name {
        "maestro_status" => {
            let s = app.db.settings();
            let ts = app.db.tickets();
            let mut per_state = serde_json::Map::new();
            for st in State::ALL {
                let n = ts.iter().filter(|t| t.state == st).count();
                if n > 0 {
                    per_state.insert(st.as_str().into(), json!(n));
                }
            }
            let busy: Vec<i64> = app.busy.lock().iter().copied().collect();
            let title = app.project.lock().as_ref().map(|p| p.title.clone());
            let active = app.db.projects(title.as_deref()).into_iter().find(|p| p.key == s.project_key());
            let runs = app.db.runs(None, 40);
            Ok(json!({
                "project": active.map(|p| json!({"key": p.key, "name": p.name})),
                "source": if s.repo_mode() { json!({"mode": "repo", "repos": s.repo_list(), "trigger_label": s.trigger_label,
                    "mirror_board": if s.has_project() { json!(format!("{}/#{}", s.project_owner, s.project_number)) } else { Value::Null }}) }
                    else { json!({"mode": "project", "board": format!("{}/#{}", s.project_owner, s.project_number), "trigger_column": s.col_trigger}) },
                "configured": s.is_configured(),
                "engine_enabled": s.engine_enabled,
                "limits": {"max_parallel": s.max_parallel, "max_attempts": s.max_attempts, "max_review_rounds": s.max_review_rounds, "review": s.review_enabled, "auto_merge": s.auto_merge},
                "tickets_per_state": per_state,
                "needs_human": ts.iter().filter(|t| t.state == State::NeedsHuman).map(|t| json!({"number": t.number, "repo": t.repo, "title": t.title, "agent": t.agent, "note": t.note})).collect::<Vec<_>>(),
                "in_progress": ts.iter().filter(|t| busy.contains(&t.id)).map(|t| json!({"number": t.number, "repo": t.repo, "title": t.title, "state": t.state, "agent": t.agent})).collect::<Vec<_>>(),
                "awaiting_merge": ts.iter().filter(|t| t.state == State::AwaitingMerge).map(|t| json!({"number": t.number, "title": t.title, "pr": t.pr_url})).collect::<Vec<_>>(),
                "recent_failures": runs.iter().filter(|r| r.status == "failed").take(5).map(|r| run_brief(r, app)).collect::<Vec<_>>(),
                "last_sync": app.last_sync.lock().clone().map(|(ts, e)| json!({"ts": ts, "error": e})),
                "stats": app.db.stats(),
            }))
        }
        "ticket_details" => {
            let t = find(app, c, args)?;
            let deps: Vec<Value> = t.deps.iter().filter_map(|d| app.db.ticket(*d)).map(|d| json!({"number": d.number, "repo": d.repo, "title": d.title, "state": d.state})).collect();
            Ok(json!({"ticket": t, "dependencies": deps, "transitions": app.db.transitions(t.id),
                "runs": app.db.runs(Some(t.id), 20).iter().map(|r| run_brief(r, app)).collect::<Vec<_>>()}))
        }
        "ticket_action" => {
            let t = find(app, c, args)?;
            let action = args["action"].as_str().unwrap_or("");
            let mut body = json!({});
            match action {
                "assign" => body["agent"] = args["agent"].clone(),
                "helpers" => body["helpers"] = args["helpers"].clone(),
                "deps" => {
                    let mut ids = vec![];
                    for n in args["depends_on"].as_array().cloned().unwrap_or_default() {
                        ids.push(find(app, c, &json!({"number": n, "repo": t.repo}))?.id);
                    }
                    body["deps"] = json!(ids);
                }
                "close" => {
                    anyhow::ensure!(args["comment"].as_str().map(|s| !s.trim().is_empty()).unwrap_or(false), "close needs a comment saying why");
                    body["comment"] = json!(format!("{}\n\n_- closed by {} (via Maestro)_", args["comment"].as_str().unwrap_or(""), c.agent));
                    body["reason"] = args["reason"].clone();
                }
                _ => {}
            }
            engine::action(app, t.id, action, &body).await?;
            let after = app.db.ticket(t.id).context("ticket gone")?;
            Ok(json!({"ok": true, "ticket": brief(&after, app)}))
        }
        "create_tickets" => {
            let s = app.db.settings();
            let repo = args["repo"].as_str().filter(|r| !r.is_empty()).map(String::from).or_else(|| s.repo_list().first().cloned());
            let repo = repo.ok_or_else(|| anyhow!("`repo` is required (no primary repository in this project)"))?;
            let items = args["tickets"].as_array().cloned().unwrap_or_default();
            anyhow::ensure!(!items.is_empty() && items.len() <= 20, "give 1 to 20 tickets");
            let gh = app.github().await?;
            let created = engine::create_tickets(app, &gh, &repo, &items, None, args["start"].as_bool().unwrap_or(true)).await?;
            app.notice("info", format!("{} created {} tickets", c.agent, created.len()));
            Ok(json!({"created": created.iter().map(|(n, a)| json!({"number": n, "agent": a, "repo": repo})).collect::<Vec<_>>()}))
        }
        "list_runs" => {
            let limit = args["limit"].as_i64().unwrap_or(20).clamp(1, 100);
            let ticket = if args["number"].is_i64() { Some(find(app, c, args)?.id) } else { None };
            let st = args["status"].as_str().unwrap_or("");
            Ok(json!(app.db.runs(ticket, 200).iter().filter(|r| st.is_empty() || r.status == st).take(limit as usize).map(|r| run_brief(r, app)).collect::<Vec<_>>()))
        }
        "get_run_log" => {
            let id = args["run_id"].as_i64().context("run_id is required")?;
            let tail = args["tail"].as_i64().unwrap_or(80).clamp(1, 400) as usize;
            let lines = app.db.logs(id);
            let skip = lines.len().saturating_sub(tail);
            Ok(json!({"total_lines": lines.len(), "lines": lines.iter().skip(skip).map(|l| format!("[{}] {}", l.kind, l.text.chars().take(1500).collect::<String>())).collect::<Vec<_>>()}))
        }
        "engine_control" => {
            let mut out = json!({});
            if let Some(on) = args["enabled"].as_bool() {
                let mut s = app.db.settings();
                anyhow::ensure!(!on || s.is_configured(), "configure a project first");
                s.engine_enabled = on;
                app.db.save_settings(&s)?;
                app.wake.notify_one();
                app.notice("info", if on { "Engine started by the assistant" } else { "Engine paused by the assistant" });
                out["engine_enabled"] = json!(on);
            }
            if args["sync"].as_bool().unwrap_or(false) {
                out["synced_items"] = json!(engine::sync_once(app).await?);
            }
            Ok(out)
        }
        "list_projects" => {
            let title = app.project.lock().as_ref().map(|p| p.title.clone());
            Ok(json!({"active": app.db.settings().project_key(), "projects": app.db.projects(title.as_deref()).iter().map(|p| json!({
                "key": p.key, "name": p.name, "source": p.source, "repos": parse_repos(&p.repos), "board": if p.project_number > 0 { json!(format!("{}/#{}", p.project_owner, p.project_number)) } else { Value::Null }
            })).collect::<Vec<_>>()}))
        }
        "switch_project" => {
            projects::switch(app, args["key"].as_str().unwrap_or("")).await?;
            Ok(json!({"ok": true, "note": "engine paused; start it when ready"}))
        }
        "add_project" => {
            let key = if args["kind"].as_str() == Some("project") {
                projects::add_board_project(app, args["owner"].as_str().unwrap_or(""), args["number"].as_i64().unwrap_or(0), args["name"].as_str()).await?
            } else {
                projects::add_repo_project(app, &strs(&args["repos"]), args["name"].as_str(), None).await?
            };
            Ok(json!({"ok": true, "key": key, "note": "switched to it; engine paused"}))
        }
        "configure_project" => {
            let old = app.db.settings();
            let mut s = old.clone();
            if args["repos"].is_array() {
                let r = parse_repos(&strs(&args["repos"]).join("\n"));
                anyhow::ensure!(!r.is_empty() || !s.repo_mode(), "a repo-mode project needs at least one repository");
                s.repos = r.join("\n");
            }
            if let Some(v) = args["trigger_label"].as_str().filter(|v| !v.trim().is_empty()) { s.trigger_label = v.trim().into(); }
            if let Some(v) = args["base_branch"].as_str() { s.base_branch = v.trim().into(); }
            if let Some(v) = args["mirror_owner"].as_str() {
                anyhow::ensure!(s.repo_mode(), "mirror_* only applies to repo-mode projects");
                s.project_owner = v.trim().into();
                s.project_number = if v.trim().is_empty() { 0 } else { args["mirror_number"].as_i64().unwrap_or(s.project_number) };
            }
            if let Some(v) = args["max_parallel"].as_u64() { s.max_parallel = v.clamp(1, 16) as usize; }
            if let Some(v) = args["max_attempts"].as_i64() { s.max_attempts = v.clamp(1, 10); }
            if let Some(v) = args["max_review_rounds"].as_i64() { s.max_review_rounds = v.clamp(0, 10); }
            if let Some(v) = args["review_enabled"].as_bool() { s.review_enabled = v; }
            if let Some(v) = args["auto_merge"].as_bool() { s.auto_merge = v; }
            if let Some(v) = args["always_triage"].as_bool() { s.always_triage = v; }
            if let Some(v) = args["mcp_auto_queue"].as_bool() { s.mcp_auto_queue = v; }
            let new = projects::save_settings(app, &old, s)?;
            Ok(json!({"ok": true, "repos": new.repo_list(), "trigger_label": new.trigger_label, "base_branch": new.base_branch,
                "mirror": if new.has_project() { json!(format!("{}/#{}", new.project_owner, new.project_number)) } else { Value::Null },
                "max_parallel": new.max_parallel, "review_enabled": new.review_enabled, "auto_merge": new.auto_merge}))
        }
        "github_list_repos" => {
            let q = args["query"].as_str().unwrap_or("").to_lowercase();
            let gh = app.github().await?;
            Ok(json!(gh.list_repos().await?.into_iter().filter(|r| q.is_empty() || r.to_lowercase().contains(&q)).take(100).collect::<Vec<_>>()))
        }
        "github_repo_info" => {
            let gh = app.github().await?;
            Ok(json!(projects::check_repos(&gh, &strs(&args["repos"])).await))
        }
        "github_create_repo" => {
            let name = args["name"].as_str().unwrap_or("").trim();
            anyhow::ensure!(!name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch)), "invalid repository name");
            let gh = app.github().await?;
            let v = gh
                .create_repo(args["owner"].as_str(), name, args["private"].as_bool().unwrap_or(true), args["description"].as_str().unwrap_or(""), args["auto_init"].as_bool().unwrap_or(true))
                .await?;
            let full = v["full_name"].as_str().unwrap_or_default().to_string();
            let mut out = json!({"repo": full, "url": v["html_url"], "private": v["private"], "default_branch": v["default_branch"]});
            app.notice("info", format!("{} created the repository {full}", c.agent));
            if args["watch"].as_bool().unwrap_or(false) {
                let old = app.db.settings();
                if old.repo_mode() && old.is_configured() {
                    let mut s = old.clone();
                    s.repos = format!("{}\n{full}", s.repos.trim());
                    projects::save_settings(app, &old, s)?;
                    out["watched"] = json!(true);
                } else {
                    let key = projects::add_repo_project(app, &[full.clone()], None, None).await?;
                    out["watched"] = json!(true);
                    out["note"] = json!(format!("no repo-mode project was active: created project {key} and switched to it (engine paused)"));
                }
            }
            Ok(out)
        }
        "github_list_projects" => {
            let gh = app.github().await?;
            Ok(json!(gh.list_projects().await?))
        }
        "github_create_project" => {
            let owner = args["owner"].as_str().unwrap_or("").trim();
            let title = args["title"].as_str().unwrap_or("").trim();
            anyhow::ensure!(!owner.is_empty() && !title.is_empty(), "owner and title are required");
            let v = projects::create_board(app, owner, title, &strs(&args["link_repos"]), args["attach"].as_str().unwrap_or("none")).await?;
            app.notice("info", format!("{} created the GitHub project {title}", c.agent));
            Ok(v)
        }
        "get_definition" => {
            let kind = args["kind"].as_str().unwrap_or("");
            let name = args["name"].as_str().unwrap_or("");
            if name.is_empty() {
                return Ok(match kind {
                    "agent" => json!(app.lib.agents().iter().map(|a| json!({"name": a.meta.name, "role": a.meta.role, "description": a.meta.description, "skills": a.meta.skills, "labels": a.meta.labels})).collect::<Vec<_>>()),
                    "cell" => json!(app.lib.cells().iter().map(|x| json!({"name": x.meta.name, "mode": x.meta.mode, "lead": x.meta.lead, "members": x.meta.members, "description": x.meta.description})).collect::<Vec<_>>()),
                    _ => json!(app.lib.skills().iter().map(|s| json!({"name": s.name, "description": s.description})).collect::<Vec<_>>()),
                });
            }
            let raw = match kind {
                "agent" => app.lib.agent(name).map(|a| crate::agents::render_agent(&a)),
                "cell" => app.lib.cell(name).map(|x| crate::agents::render_cell(&x)),
                _ => app.lib.skills().into_iter().find(|s| s.name == name).map(|s| s.content),
            };
            Ok(json!({"kind": kind, "name": name, "content": raw.ok_or_else(|| anyhow!("{kind} '{name}' not found"))?}))
        }
        "save_definition" => {
            let kind = args["kind"].as_str().unwrap_or("");
            let name = args["name"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            anyhow::ensure!(!content.trim().is_empty(), "content is empty");
            match kind {
                "agent" => {
                    let a = crate::agents::parse_agent(content)?;
                    app.lib.save_agent(&a, Some(name))?;
                }
                "cell" => {
                    let x = crate::agents::parse_cell(content)?;
                    app.lib.save_cell(&x, Some(name))?;
                }
                "skill" => app.lib.save_skill(name, content)?,
                _ => bail!("kind must be agent, cell or skill"),
            }
            app.notice("info", format!("{} saved the {kind} '{name}'", c.agent));
            Ok(json!({"ok": true}))
        }
        _ => bail!("unknown tool {name}"),
    }
}
