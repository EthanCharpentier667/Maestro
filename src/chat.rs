//! The Assistant: a conversation with the "maestro" agent, a Claude Code session
//! resumed turn after turn (`claude --resume`). It has no repository to edit; it
//! acts through the Maestro MCP tools in assistant scope (tickets, engine,
//! projects, GitHub repositories and boards, agent definitions).

use crate::app::{App, AppRef};
use crate::claude::{self, Request};
use crate::model::*;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;

pub const NEW_TITLE: &str = "New chat";

/// chat id -> running run id, for the UI.
pub fn busy_map(app: &App) -> Value {
    json!(app.chat_runs.lock().iter().map(|(c, r)| (c.to_string(), json!(*r))).collect::<serde_json::Map<String, Value>>())
}

pub fn cancel(app: &App, chat_id: i64) -> bool {
    let run = app.chat_runs.lock().get(&chat_id).copied();
    match run.and_then(|r| app.cancels.lock().remove(&r)) {
        Some(tx) => tx.send(()).is_ok(),
        None => false,
    }
}

/// Short snapshot of the live state, prepended to every turn so the assistant never
/// reasons on stale numbers (it can still call `maestro_status` for details).
fn snapshot(app: &App) -> String {
    let s = app.db.settings();
    let ts = app.db.tickets();
    let count = |st: State| ts.iter().filter(|t| t.state == st).count();
    let project = app
        .db
        .projects(app.project.lock().as_ref().map(|p| p.title.as_str()))
        .into_iter()
        .find(|p| p.key == s.project_key())
        .map(|p| p.name)
        .unwrap_or_else(|| "(none)".into());
    let source = if !s.is_configured() {
        "not configured".to_string()
    } else if s.repo_mode() {
        format!("repos {} (trigger label `{}`){}", s.repo_list().join(", "), s.trigger_label, if s.has_project() { format!(", mirror board {}/#{}", s.project_owner, s.project_number) } else { String::new() })
    } else {
        format!("GitHub Project {}/#{}", s.project_owner, s.project_number)
    };
    format!(
        "[Maestro live state, {}] active project: {project} — {source}; engine {}; {} tickets: {} backlog, {} triage, {} ready/blocked, {} running, {} in review/awaiting merge, {} need a human, {} done; {} agent runs in progress.",
        chrono::Local::now().format("%Y-%m-%d %H:%M"),
        if s.engine_enabled { "RUNNING" } else { "PAUSED" },
        ts.len(),
        count(State::Backlog),
        count(State::Triage),
        count(State::Ready) + count(State::Blocked),
        count(State::Running),
        count(State::Reviewing) + count(State::AwaitingMerge),
        count(State::NeedsHuman),
        count(State::Done),
        app.busy.lock().len(),
    )
}

/// `--mcp-config` for the assistant: the Maestro server in assistant scope.
fn mcp_config(app: &App, agent: &str) -> Result<std::path::PathBuf> {
    let dir = app.data.join("plugins");
    std::fs::create_dir_all(&dir)?;
    let f = dir.join(format!("mcp-assistant-{agent}.json"));
    let cfg = json!({"mcpServers": {"maestro": {
        "type": "http",
        "url": format!("{}/mcp?scope=assistant&agent={agent}", app.base_url),
        "headers": {"Authorization": format!("Bearer {}", app.db.mcp_token())}
    }}});
    std::fs::write(&f, serde_json::to_string_pretty(&cfg)?)?;
    Ok(f)
}

/// Store the user's message and start the assistant's turn in the background.
/// Returns the stored message and the run id (live logs arrive over SSE).
pub async fn send(app: &AppRef, chat_id: i64, text: &str) -> Result<(ChatMessage, i64)> {
    let text = text.trim().to_string();
    anyhow::ensure!(!text.is_empty(), "empty message");
    let chat = app.db.chat(chat_id).context("chat not found")?;
    anyhow::ensure!(!app.chat_runs.lock().contains_key(&chat_id), "the assistant is still answering in this chat");
    let s = app.db.settings();
    let agent = app
        .lib
        .agent(&s.assistant_agent)
        .ok_or_else(|| anyhow!("assistant agent '{}' not found (Agents, cells, skills)", s.assistant_agent))?;

    let msg = app.db.add_chat_message(chat_id, "user", &text, None, 0.0)?;
    app.emit(Event::Chat { message: msg.clone() });
    if chat.title == NEW_TITLE {
        let mut t: String = text.lines().next().unwrap_or("").chars().take(60).collect();
        if text.chars().count() > 60 {
            t.push('…');
        }
        app.db.update_chat(chat_id, Some(&t), None, 0.0);
        app.emit(Event::Chats);
    }
    let run = app.db.create_run(None, "chat", &agent.meta.name)?;
    app.emit(Event::Run { run: run.clone() });
    app.chat_runs.lock().insert(chat_id, run.id);
    app.emit(Event::ChatState { chat_id, run_id: Some(run.id) });

    let app2 = Arc::clone(app);
    let run_id = run.id;
    tokio::spawn(async move {
        let r = turn(&app2, chat_id, run_id, &agent, &text, chat.session_id.clone()).await;
        if let Err(e) = r {
            if let Ok(m) = app2.db.add_chat_message(chat_id, "error", &format!("{e:#}"), Some(run_id), 0.0) {
                app2.emit(Event::Chat { message: m });
            }
            if let Some(r) = app2.db.finish_run(run_id, "failed", 0.0, 0, None, &format!("{e:#}")) {
                app2.emit(Event::Run { run: r });
            }
        }
        app2.chat_runs.lock().remove(&chat_id);
        app2.emit(Event::ChatState { chat_id, run_id: None });
        app2.emit(Event::Chats);
    });
    Ok((msg, run.id))
}

async fn turn(app: &AppRef, chat_id: i64, run_id: i64, agent: &crate::agents::Agent, text: &str, session: Option<String>) -> Result<()> {
    // A stable working directory: Claude Code keys its sessions by cwd.
    let cwd = app.data.join("assistant");
    std::fs::create_dir_all(&cwd)?;
    let plugin = app.lib.build_plugin(agent)?;
    let mcp = mcp_config(app, &agent.meta.name)?;
    let first = session.is_none();
    let mut prompt = snapshot(app);
    if first {
        prompt.push_str("\n\nThis is the start of a conversation in the Maestro web UI (Assistant view). Load the `maestro-guide` skill before answering anything about Maestro.");
    }
    prompt.push_str("\n\n");
    prompt.push_str(text);

    let req = |resume: Option<String>, prompt: String| Request {
        run_id,
        cwd: &cwd,
        agent,
        prompt,
        json_schema: None,
        plugin_dir: plugin.clone(),
        tools: None,
        read_only: true,
        subagents: None,
        mcp: Some(mcp.clone()),
        resume,
    };
    let mut out = claude::run(app, req(session.clone(), prompt.clone())).await?;
    if !out.ok && !out.cancelled && session.is_some() {
        // the session may be gone (other machine, cleaned up): start a fresh one
        app.log(run_id, "system", "resume failed, starting a new session");
        let fresh = format!("{}\n\n(Note: the previous conversation context was lost; ask the user if you need it.)", prompt);
        out = claude::run(app, req(None, fresh)).await?;
    }
    let status = if out.cancelled { "cancelled" } else if out.ok { "success" } else { "failed" };
    if let Some(r) = app.db.finish_run(run_id, status, out.cost, out.turns, out.session_id.as_deref(), &out.text) {
        app.emit(Event::Run { run: r });
    }
    let (role, body) = if out.cancelled {
        ("error", "Stopped.".to_string())
    } else if out.ok {
        ("assistant", if out.text.trim().is_empty() { "(no answer)".into() } else { out.text.clone() })
    } else {
        ("error", if out.text.trim().is_empty() { "The assistant failed (see Runs & logs).".into() } else { out.text.clone() })
    };
    let m = app.db.add_chat_message(chat_id, role, &body, Some(run_id), out.cost)?;
    app.db.update_chat(chat_id, None, out.session_id.as_deref().filter(|_| out.ok), out.cost);
    app.emit(Event::Chat { message: m });
    Ok(())
}
