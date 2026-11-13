//! The orchestration engine:
//!  - sync loop   : GitHub ⇄ local tickets (polling, no public webhook needed). Two sources:
//!                  repo mode (issues of the watched repos, status in `maestro:*` labels, optional
//!                  Project mirror) or project mode (a Project V2 board and its Status field)
//!  - scheduler   : drives every ticket through the state machine
//!  - triage      : orchestrator agent routes / splits / asks for clarification
//!  - worker      : specialist agent codes in an isolated worktree → PR
//!  - review      : reviewer agent approves or sends feedback (loop)

use crate::agents::{Agent, Team};
use crate::app::{App, AppRef};
use crate::claude::{self, Request};
use crate::db::SyncedItem;
use crate::git;
use crate::github::Github;
use crate::model::*;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// ───────────────────────────── helpers

fn dep_refs(body: &str, default_repo: &str) -> Vec<(String, i64)> {
    let line_re = regex::Regex::new(r"(?im)^.*\b(depends on|blocked by|requires|after)\b.*$").unwrap();
    let ref_re = regex::Regex::new(r"([\w.-]+/[\w.-]+)?#(\d+)").unwrap();
    let mut out = vec![];
    for m in line_re.find_iter(body) {
        for c in ref_re.captures_iter(m.as_str()) {
            let repo = c.get(1).map(|x| x.as_str().to_string()).unwrap_or_else(|| default_repo.to_string());
            if let Ok(n) = c[2].parse() {
                out.push((repo, n));
            }
        }
    }
    out
}

fn parent_ref(body: &str) -> Option<i64> {
    let re = regex::Regex::new(r"(?i)part of #(\d+)").unwrap();
    re.captures(body).and_then(|c| c[1].parse().ok())
}

/// Reflect a Maestro step on GitHub: a column of the Project (project mode), or a
/// `maestro:*` label / the trigger label (repo mode, plus the mirror Project if any).
async fn set_gh_status(app: &App, gh: &Github, t: &Ticket, col: &str) {
    if col.trim().is_empty() || t.gh_status.eq_ignore_ascii_case(col) {
        return;
    }
    let s = app.db.settings();
    let r = if s.repo_mode() { set_repo_status(app, gh, &s, t, Some(col)).await } else { set_project_status(app, gh, &t.item_id, col).await };
    match r {
        Ok(_) => {
            let _ = app.db.set_field(t.id, "gh_status", col);
            app.emit_ticket(t.id);
        }
        Err(e) => app.notice("warn", format!("#{}: cannot set status '{col}': {e:#}", t.number)),
    }
}

/// Remove every Maestro status from the issue (reopen / back to backlog).
async fn clear_gh_status(app: &App, gh: &Github, t: &Ticket) {
    let s = app.db.settings();
    if s.repo_mode() {
        if let Err(e) = set_repo_status(app, gh, &s, t, None).await {
            app.notice("warn", format!("#{}: cannot clear labels: {e:#}", t.number));
        }
    }
    let _ = app.db.set_field(t.id, "gh_status", "");
    app.emit_ticket(t.id);
}

async fn set_project_status(app: &App, gh: &Github, item_id: &str, col: &str) -> Result<()> {
    let p = app.project_info(gh, false).await?;
    gh.set_status(&p, item_id, col).await
}

/// repo mode: swap the `maestro:*` status label (or the trigger label for the trigger column).
async fn set_repo_status(app: &App, gh: &Github, s: &Settings, t: &Ticket, col: Option<&str>) -> Result<()> {
    let want = col.map(|c| if c.eq_ignore_ascii_case(&s.col_trigger) { s.trigger_label.clone() } else { status_label(c) });
    let is_status = |l: &str| l.to_lowercase().starts_with(STATUS_LABEL_PREFIX) || l.eq_ignore_ascii_case(&s.trigger_label);
    // keep the trigger label while working (it says "Maestro owns this issue"); drop it only on clear
    for l in t.labels.iter().filter(|l| is_status(l)) {
        let keep = want.as_deref().map(|w| l.eq_ignore_ascii_case(w) || l.eq_ignore_ascii_case(&s.trigger_label)).unwrap_or(false);
        if !keep {
            gh.remove_label(&t.repo, t.number, l).await?;
        }
    }
    let mut labels: Vec<String> = t
        .labels
        .iter()
        .filter(|l| !is_status(l) || (want.is_some() && l.eq_ignore_ascii_case(&s.trigger_label)))
        .cloned()
        .collect();
    if let Some(w) = &want {
        if !labels.iter().any(|l| l.eq_ignore_ascii_case(w)) {
            gh.add_labels(&t.repo, t.number, &[w.clone()]).await?;
            labels.push(w.clone());
        }
    }
    app.db.set_field(t.id, "labels", serde_json::to_string(&labels)?)?;
    // optional mirror board
    if let (Some(c), true) = (col, s.has_project()) {
        if !t.issue_node_id.is_empty() {
            let mirror = async {
                let p = app.project_info(gh, false).await?;
                let item = gh.add_to_project(&p.id, &t.issue_node_id).await?;
                gh.set_status(&p, &item, c).await
            };
            if let Err(e) = mirror.await {
                app.notice("warn", format!("#{}: mirror project: {e:#}", t.number));
            }
        }
    }
    Ok(())
}

async fn gh_comment(app: &App, gh: &Github, t: &Ticket, body: &str) {
    if let Err(e) = gh.comment(&t.repo, t.number, body).await {
        app.notice("warn", format!("#{}: comment failed: {e}", t.number));
    }
}

fn deps_done(app: &App, t: &Ticket) -> bool {
    t.deps.iter().all(|d| app.db.ticket(*d).map(|x| x.state == State::Done).unwrap_or(true))
}

fn roster(app: &App) -> String {
    let mut out = app
        .lib
        .workers()
        .iter()
        .map(|a| format!("- agent `{}` — {} (labels: {})", a.meta.name, a.meta.description, a.meta.labels.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    let cells = app.lib.cells();
    if !cells.is_empty() {
        out.push_str("\n\nCells (groups of agents working together on one ticket; assign by cell name):\n");
        for c in cells {
            out.push_str(&format!(
                "- cell `{}` ({}: lead {}, members {}) — {}\n",
                c.meta.name,
                if c.meta.mode == "pipeline" { "pipeline" } else { "delegate" },
                c.meta.lead,
                c.meta.members.join(", "),
                c.meta.description
            ));
        }
    }
    out
}

fn label_agent(app: &App, t: &Ticket) -> Option<String> {
    let workers = app.lib.workers();
    for l in &t.labels {
        if let Some(n) = l.strip_prefix("agent:").or_else(|| l.strip_prefix("cell:")) {
            if app.lib.is_assignable(n) {
                return Some(n.to_string());
            }
        }
    }
    for c in app.lib.cells() {
        if t.labels.iter().any(|l| c.meta.labels.iter().any(|x| x.eq_ignore_ascii_case(l))) {
            return Some(c.meta.name);
        }
    }
    for a in &workers {
        if t.labels.iter().any(|l| a.meta.labels.iter().any(|x| x.eq_ignore_ascii_case(l))) {
            return Some(a.meta.name.clone());
        }
    }
    None
}

// ───────────────────────────── sync

/// repo mode: open issues of every watched repo, recently closed ones we track, and any
/// tracked ticket not seen (closed long ago, or living in another repo) fetched one by one.
async fn repo_items(app: &AppRef, gh: &Github, s: &Settings) -> Result<(Vec<SyncedItem>, Vec<i64>)> {
    let key = s.project_key();
    let since = (chrono::Utc::now() - chrono::Duration::days(21)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut items: Vec<SyncedItem> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut push = |mut it: SyncedItem, items: &mut Vec<SyncedItem>| {
        if seen.insert((it.repo.to_lowercase(), it.number)) {
            it.item_id = format!("{key}|{}", it.issue_node_id);
            it.gh_status = s.status_from_labels(&it.labels);
            items.push(it);
        }
    };
    let repos = s.repo_list();
    let mut failed = vec![];
    for repo in &repos {
        let open = match gh.repo_issues(repo, "open", None, 1000).await {
            Ok(v) => v,
            Err(e) => {
                // one unreachable repository must not stop the others from syncing
                app.notice("warn", format!("sync: issues of {repo}: {e:#}"));
                failed.push(format!("{repo}: {e:#}"));
                continue;
            }
        };
        for it in open {
            push(it, &mut items);
        }
        for it in gh.repo_issues(repo, "closed", Some(&since), 300).await.unwrap_or_default() {
            if app.db.ticket_by_number(repo, it.number).is_some() {
                push(it, &mut items);
            }
        }
    }
    if !repos.is_empty() && failed.len() == repos.len() {
        anyhow::bail!("{}", failed.join("; "));
    }
    let mut gone = vec![];
    for t in app.db.tickets() {
        if t.state == State::Done || items.iter().any(|i| i.repo.eq_ignore_ascii_case(&t.repo) && i.number == t.number) {
            continue;
        }
        match gh.issue(&t.repo, t.number).await {
            Ok(Some(it)) => push(it, &mut items),
            Ok(None) => gone.push(t.id),
            Err(e) => app.notice("warn", format!("sync {}#{}: {e:#}", t.repo, t.number)),
        }
    }
    Ok((items, gone))
}

pub async fn sync_once(app: &AppRef) -> Result<usize> {
    let s = app.db.settings();
    anyhow::ensure!(s.is_configured(), "No project configured: add repositories (or a GitHub Project) in Settings");
    let gh = app.github().await?;
    let (items, gone) = if s.repo_mode() {
        if s.has_project() {
            // refresh the mirror board's metadata (columns) — failure is not fatal
            if let Err(e) = app.project_info(&gh, true).await {
                app.notice("warn", format!("mirror project: {e:#}"));
            }
        } else {
            *app.project.lock() = None;
        }
        repo_items(app, &gh, &s).await?
    } else {
        let project = app.project_info(&gh, true).await?;
        (gh.items(&project.id, &s.status_field).await?, vec![])
    };
    let n = items.len();
    let mut keep = vec![];
    let mut fresh: Vec<i64> = vec![];
    for it in &items {
        keep.push(it.item_id.clone());
        let (t, is_new) = app.db.upsert_synced(it)?;
        if is_new {
            fresh.push(t.id);
        }
        app.emit(Event::Ticket { ticket: t });
    }
    // dependencies + parent links parsed from bodies (additive)
    for it in &items {
        let Some(t) = app.db.ticket_by_number(&it.repo, it.number) else { continue };
        for (repo, num) in dep_refs(&it.body, &it.repo) {
            if let Some(d) = app.db.ticket_by_number(&repo, num) {
                let _ = app.db.add_dep(t.id, d.id);
            }
        }
        if t.parent.is_none() {
            if let Some(p) = parent_ref(&it.body).and_then(|n| app.db.ticket_by_number(&it.repo, n)) {
                let _ = app.db.set_field(t.id, "parent", p.id);
            }
        }
    }
    if s.repo_mode() {
        for id in gone {
            app.db.delete_ticket(id);
            app.emit(Event::TicketRemoved { id });
        }
    } else {
        for id in app.db.remove_missing(&keep) {
            app.emit(Event::TicketRemoved { id });
        }
    }
    // state reactions to GitHub changes
    for t in app.db.tickets() {
        if t.gh_state == "CLOSED" && t.state != State::Done {
            if app.busy.lock().contains(&t.id) {
                continue;
            }
            app.transition(t.id, State::Done, "issue closed on GitHub");
            if s.repo_mode() && t.agent.is_some() {
                set_gh_status(app, &gh, &t, &s.col_done).await;
            }
            git::cleanup(app, &t).await;
            continue;
        }
        if t.state == State::Backlog && t.gh_state == "OPEN" && t.gh_status.eq_ignore_ascii_case(&s.col_trigger) {
            let why = if s.repo_mode() { format!("labelled '{}'", s.trigger_label) } else { format!("moved to '{}'", s.col_trigger) };
            app.transition(t.id, State::Triage, &why);
        }
        if t.state == State::AwaitingMerge {
            if let Some(pr) = t.pr_number {
                match gh.pr_state(&t.repo, pr).await.as_deref() {
                    Ok("merged") => {
                        app.transition(t.id, State::Done, "PR merged");
                        set_gh_status(app, &gh, &t, &s.col_done).await;
                        git::cleanup(app, &t).await;
                    }
                    Ok("closed") => {
                        app.transition(t.id, State::NeedsHuman, "PR closed without merge");
                    }
                    _ => {}
                }
            }
        }
    }
    let _ = fresh;
    *app.last_sync.lock() = Some((crate::db::now(), None));
    app.wake.notify_one();
    Ok(n)
}

pub async fn sync_loop(app: AppRef) {
    loop {
        let s = app.db.settings();
        if s.is_configured() {
            if let Err(e) = sync_once(&app).await {
                *app.last_sync.lock() = Some((crate::db::now(), Some(e.to_string())));
                app.notice("warn", format!("sync: {e}"));
            }
        }
        tokio::time::sleep(Duration::from_secs(s.poll_seconds.max(10))).await;
    }
}

// ───────────────────────────── scheduler

pub async fn scheduler_loop(app: AppRef) {
    loop {
        tick(&app).await;
        tokio::select! {
            _ = app.wake.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(5)) => {},
        }
    }
}

async fn tick(app: &AppRef) {
    let s = app.db.settings();
    let tickets = app.db.tickets();
    let by_id: HashMap<i64, &Ticket> = tickets.iter().map(|t| (t.id, t)).collect();

    // cheap, deterministic transitions (always on)
    for t in &tickets {
        match t.state {
            State::Blocked if deps_done(app, t) => {
                app.transition(t.id, State::Ready, "dependencies done");
            }
            State::Ready if !deps_done(app, t) => {
                app.transition(t.id, State::Blocked, "waiting for dependencies");
            }
            State::Split => {
                let kids: Vec<_> = tickets.iter().filter(|c| c.parent == Some(t.id)).collect();
                if !kids.is_empty() && kids.iter().all(|c| c.state == State::Done) {
                    app.transition(t.id, State::Done, "all sub-tickets done");
                    if let Ok(gh) = app.github().await {
                        set_gh_status(app, &gh, t, &s.col_done).await;
                        gh_comment(app, &gh, t, "**Maestro**: all sub-tickets are done.").await;
                        let _ = gh.set_issue_state(&t.repo, t.number, "closed", "completed").await;
                    }
                }
            }
            _ => {}
        }
    }
    let _ = by_id;

    if !s.engine_enabled {
        return;
    }
    let mut free = s.max_parallel.saturating_sub(app.busy.lock().len());
    // triage first (cheap & unblocks work), then ready tickets, oldest first
    let mut queue: Vec<&Ticket> = tickets.iter().filter(|t| t.state == State::Triage).collect();
    queue.extend(tickets.iter().filter(|t| t.state == State::Ready && deps_done(app, t)));
    for t in queue {
        if free == 0 {
            break;
        }
        if !app.busy.lock().insert(t.id) {
            continue;
        }
        free -= 1;
        let app2 = Arc::clone(app);
        let id = t.id;
        let st = t.state;
        tokio::spawn(async move {
            let r = if st == State::Triage { triage(&app2, id).await } else { work(&app2, id).await };
            if let Err(e) = r {
                app2.notice("error", format!("ticket {id}: {e:#}"));
                if let Some(t) = app2.db.ticket(id) {
                    let _ = app2.db.set_field(id, "note", format!("{e:#}"));
                    if t.state == State::Running || t.state == State::Triage || t.state == State::Reviewing {
                        app2.transition(id, State::NeedsHuman, "engine error");
                    }
                }
            }
            app2.busy.lock().remove(&id);
            app2.wake.notify_one();
        });
    }
}

// ───────────────────────────── triage (orchestrator)

fn triage_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "decision": {"type": "string", "enum": ["assign", "split", "clarify"]},
        "agent": {"type": "string", "description": "agent or cell name when decision=assign"},
        "helpers": {"type": "array", "items": {"type": "string"}, "description": "optional extra agents that work with the assignee"},
        "reason": {"type": "string"},
        "question": {"type": "string", "description": "question for the human when decision=clarify"},
        "subtasks": {"type": "array", "items": {"type": "object", "properties": {
            "key": {"type": "string"}, "title": {"type": "string"}, "body": {"type": "string"},
            "agent": {"type": "string", "description": "agent or cell name"},
            "helpers": {"type": "array", "items": {"type": "string"}},
            "depends_on": {"type": "array", "items": {"type": "string"}}
        }, "required": ["key", "title", "body", "agent"]}}
      },
      "required": ["decision", "reason"]
    })
}

fn plan_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "summary": {"type": "string"},
        "tickets": {"type": "array", "items": {"type": "object", "properties": {
            "key": {"type": "string"}, "title": {"type": "string"}, "body": {"type": "string"},
            "agent": {"type": "string", "description": "agent or cell name"},
            "helpers": {"type": "array", "items": {"type": "string"}},
            "depends_on": {"type": "array", "items": {"type": "string"}}
        }, "required": ["key", "title", "body", "agent"]}}
      },
      "required": ["tickets"]
    })
}

async fn orchestrator_cwd(app: &App, repo: &str) -> std::path::PathBuf {
    if repo.is_empty() {
        return app.data.clone();
    }
    if let Ok(gh) = app.github().await {
        if let Ok(p) = git::ensure_repo(app, gh.token(), repo).await {
            return p;
        }
    }
    app.data.clone()
}

fn orchestrator(app: &App) -> Result<Agent> {
    let s = app.db.settings();
    app.lib
        .agent(&s.orchestrator_agent)
        .ok_or_else(|| anyhow!("orchestrator agent '{}' not found", s.orchestrator_agent))
}

async fn triage(app: &AppRef, id: i64) -> Result<()> {
    let s = app.db.settings();
    let t = app.db.ticket(id).context("ticket gone")?;
    let gh = app.github().await?;

    // Fast path: explicit routing by label.
    if !s.always_triage {
        if let Some(agent) = label_agent(app, &t) {
            app.db.set_field(id, "agent", &agent)?;
            let to = if deps_done(app, &t) { State::Ready } else { State::Blocked };
            app.transition(id, to, &format!("label → {agent}"));
            return Ok(());
        }
    }

    let orch = orchestrator(app)?;
    let run = app.db.create_run(Some(id), "triage", &orch.meta.name)?;
    app.emit(Event::Run { run: run.clone() });
    let comments = gh.issue_comments(&t.repo, t.number).await.unwrap_or_default();
    let prompt = format!(
        "You are triaging a ticket from the GitHub Project board.\n\n## Team (worker agents)\n{}\n\n## Ticket {}#{}: {}\nLabels: {}\n\n{}\n\n## Comments\n{}\n\n\
         The current directory is the repository; you may inspect it (read-only) to judge scope.\n\
         Decide:\n- `assign`: it fits a single focused PR → give `agent` (an agent or a cell). Use a cell, or add `helpers`, when the PR needs several specialities working together.\n\
         - `split`: too big or spans several specialities → give 2-8 `subtasks` (each one PR-sized, with `agent` and `depends_on` keys of other subtasks).\n\
         - `clarify`: the ticket is too ambiguous to act on → give a precise `question`.\n\
         Answer ONLY with the JSON object.",
        roster(app), t.repo, t.number, t.title, t.labels.join(", "), t.body,
        comments.iter().rev().take(10).rev().map(|(u, b)| format!("**{u}**: {b}")).collect::<Vec<_>>().join("\n\n")
    );
    let cwd = orchestrator_cwd(app, &t.repo).await;
    let plugin = app.lib.build_plugin(&orch)?;
    let out = claude::run(app, Request { run_id: run.id, cwd: &cwd, agent: &orch, prompt, json_schema: Some(triage_schema()), plugin_dir: plugin, tools: None, read_only: true, subagents: None, mcp: None, resume: None }).await?;
    app.db.add_cost(id, out.cost);
    let fin = app.db.finish_run(run.id, if out.ok { "success" } else { "failed" }, out.cost, out.turns, out.session_id.as_deref(), &out.text);
    if let Some(r) = fin {
        app.emit(Event::Run { run: r });
    }
    if out.cancelled {
        app.transition(id, State::Paused, "cancelled");
        return Ok(());
    }
    let d = out.structured.filter(|_| out.ok).ok_or_else(|| anyhow!("orchestrator returned no decision: {}", out.text))?;
    let reason = d["reason"].as_str().unwrap_or("").to_string();
    let workers = app.lib.workers();
    let _ = &workers;
    let valid = |n: &str| app.lib.is_assignable(n);

    match d["decision"].as_str().unwrap_or("") {
        "assign" => {
            let agent = d["agent"].as_str().unwrap_or("");
            anyhow::ensure!(valid(agent), "orchestrator picked unknown agent '{agent}'");
            app.db.set_field(id, "agent", agent)?;
            let helpers = valid_helpers(app, &d["helpers"]);
            app.db.set_field(id, "helpers", serde_json::to_string(&helpers)?)?;
            let to = if deps_done(app, &t) { State::Ready } else { State::Blocked };
            app.transition(id, to, &format!("orchestrator → {agent}"));
            gh_comment(app, &gh, &t, &format!("🎼 **Maestro** — routed to **`{agent}`**\n\n> {reason}")).await;
        }
        "split" => {
            let subs = d["subtasks"].as_array().cloned().unwrap_or_default();
            anyhow::ensure!(!subs.is_empty(), "split without subtasks");
            let created = create_tickets(app, &gh, &t.repo, &subs, Some(&t), true).await?;
            app.transition(id, State::Split, &format!("split into {} sub-tickets", created.len()));
            set_gh_status(app, &gh, &t, &s.col_in_progress).await;
            let list = created.iter().map(|(n, a)| format!("- #{n} → `{a}`")).collect::<Vec<_>>().join("\n");
            gh_comment(app, &gh, &t, &format!("🎼 **Maestro** — split into sub-tickets:\n\n{list}\n\n> {reason}")).await;
        }
        _ => {
            let q = d["question"].as_str().unwrap_or(&reason).to_string();
            app.db.set_field(id, "note", format!("Question: {q}"))?;
            app.transition(id, State::NeedsHuman, "orchestrator needs clarification");
            gh_comment(app, &gh, &t, &format!("🎼 **Maestro** — I need a clarification before routing this:\n\n{q}\n\n_Answer in a comment, then hit **Retriage** in Maestro (or {})._", s.trigger_hint())).await;
        }
    }
    Ok(())
}

/// Create the GitHub issue behind a new ticket and put it on the board. Returns the
/// synced item (item id, node id, status) ready for `upsert_synced`.
async fn open_issue(app: &App, gh: &Github, repo: &str, title: &str, body: &str, mut labels: Vec<String>, start: bool) -> Result<SyncedItem> {
    let s = app.db.settings();
    anyhow::ensure!(s.is_configured(), "No project configured (Settings → Project)");
    let col = if start { s.col_trigger.clone() } else { String::new() };
    let item_id;
    let (num, url, node) = if s.repo_mode() {
        if start {
            labels.push(s.trigger_label.clone());
        }
        let r = gh.create_issue(repo, title, body, &labels).await?;
        item_id = format!("{}|{}", s.project_key(), r.2);
        if s.has_project() {
            let mirror = async {
                let p = app.project_info(gh, false).await?;
                let item = gh.add_to_project(&p.id, &r.2).await?;
                if start {
                    gh.set_status(&p, &item, &col).await?;
                }
                anyhow::Ok(())
            };
            if let Err(e) = mirror.await {
                app.notice("warn", format!("#{}: mirror project: {e:#}", r.0));
            }
        }
        r
    } else {
        let project = app.project_info(gh, false).await?;
        let r = gh.create_issue(repo, title, body, &labels).await?;
        item_id = gh.add_to_project(&project.id, &r.2).await?;
        if start {
            let _ = gh.set_status(&project, &item_id, &col).await;
        }
        r
    };
    Ok(SyncedItem {
        item_id,
        issue_node_id: node,
        repo: repo.to_string(),
        number: num,
        title: title.to_string(),
        body: body.to_string(),
        url,
        labels,
        gh_status: col,
        gh_state: "OPEN".into(),
    })
}

/// Create GitHub issues for a list of {key,title,body,agent,depends_on}.
/// Returns [(issue number, agent)].
pub async fn create_tickets(app: &App, gh: &Github, repo: &str, items: &[Value], parent: Option<&Ticket>, start: bool) -> Result<Vec<(i64, String)>> {
    let mut key_to_id: HashMap<String, i64> = HashMap::new();
    let mut key_to_num: HashMap<String, i64> = HashMap::new();
    let mut out = vec![];
    let workers = app.lib.workers();
    for it in items {
        let key = it["key"].as_str().unwrap_or("").to_string();
        let mut agent = it["agent"].as_str().unwrap_or("").to_string();
        if !app.lib.is_assignable(&agent) {
            agent = workers.first().map(|a| a.meta.name.clone()).unwrap_or_default();
        }
        let helpers = valid_helpers(app, &it["helpers"]);
        let deps: Vec<String> = it["depends_on"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let mut body = it["body"].as_str().unwrap_or("").to_string();
        let dep_nums: Vec<String> = deps.iter().filter_map(|k| key_to_num.get(k)).map(|n| format!("#{n}")).collect();
        body.push_str("\n\n---\n");
        if let Some(p) = parent {
            body.push_str(&format!("Part of #{}\n", p.number));
        }
        if !dep_nums.is_empty() {
            body.push_str(&format!("Depends on {}\n", dep_nums.join(", ")));
        }
        body.push_str(&format!("_Created by Maestro orchestrator — assigned to `{agent}`_"));
        let kind = if app.lib.cell(&agent).is_some() { "cell" } else { "agent" };
        let labels = vec![format!("{kind}:{agent}")];
        let title = it["title"].as_str().unwrap_or("Untitled").to_string();
        let item = open_issue(app, gh, repo, &title, &body, labels, start).await?;
        let num = item.number;
        let (t, _) = app.db.upsert_synced(&item)?;
        app.db.set_field(t.id, "agent", &agent)?;
        app.db.set_field(t.id, "helpers", serde_json::to_string(&helpers)?)?;
        if let Some(p) = parent {
            app.db.set_field(t.id, "parent", p.id)?;
        }
        for k in &deps {
            if let Some(d) = key_to_id.get(k) {
                app.db.add_dep(t.id, *d)?;
            }
        }
        if start {
            let t2 = app.db.ticket(t.id).unwrap();
            let to = if deps_done(app, &t2) { State::Ready } else { State::Blocked };
            app.transition(t.id, to, "created by orchestrator");
        } else {
            app.emit_ticket(t.id);
        }
        key_to_id.insert(key.clone(), t.id);
        key_to_num.insert(key, num);
        out.push((num, agent));
    }
    Ok(out)
}

/// Options for creating a single ticket (UI, MCP).
pub struct NewTicket {
    pub repo: String,
    pub title: String,
    pub body: String,
    pub assignee: Option<String>,
    pub helpers: Vec<String>,
    /// local ticket ids
    pub deps: Vec<i64>,
    pub related: Option<i64>,
    pub start: bool,
    pub created_by: String,
}

pub async fn create_one(app: &App, gh: &Github, n: NewTicket) -> Result<Ticket> {
    anyhow::ensure!(n.repo.contains('/'), "repository must be owner/name");
    anyhow::ensure!(!n.title.trim().is_empty(), "title is empty");
    let assignee = n.assignee.filter(|a| !a.is_empty());
    if let Some(a) = &assignee {
        anyhow::ensure!(app.lib.is_assignable(a), "unknown agent or cell '{a}'");
    }
    let deps: Vec<Ticket> = n.deps.iter().filter_map(|d| app.db.ticket(*d)).collect();
    let mut body = n.body.clone();
    let mut footer = vec![];
    if let Some(r) = n.related.and_then(|r| app.db.ticket(r)) {
        footer.push(format!("Related to #{}", r.number));
    }
    if !deps.is_empty() {
        footer.push(format!("Depends on {}", deps.iter().map(|d| if d.repo == n.repo { format!("#{}", d.number) } else { format!("{}#{}", d.repo, d.number) }).collect::<Vec<_>>().join(", ")));
    }
    footer.push(format!("_Created by {} via Maestro_", n.created_by));
    body.push_str("\n\n---\n");
    body.push_str(&footer.join("\n"));
    let labels: Vec<String> = assignee
        .iter()
        .map(|a| format!("{}:{a}", if app.lib.cell(a).is_some() { "cell" } else { "agent" }))
        .collect();
    let item = open_issue(app, gh, &n.repo, &n.title, &body, labels, n.start).await?;
    let (t, _) = app.db.upsert_synced(&item)?;
    if let Some(a) = &assignee {
        app.db.set_field(t.id, "agent", a)?;
    }
    app.db.set_field(t.id, "helpers", serde_json::to_string(&valid_helpers(app, &json!(n.helpers)))?)?;
    app.db.set_deps(t.id, &n.deps)?;
    if n.start {
        app.transition(t.id, State::Triage, &format!("created by {}", n.created_by));
    } else {
        app.emit_ticket(t.id);
    }
    Ok(app.db.ticket(t.id).unwrap())
}

pub async fn close_ticket(app: &App, gh: &Github, id: i64, reason: &str, comment: Option<&str>) -> Result<()> {
    let t = app.db.ticket(id).context("not found")?;
    cancel_runs(app, id);
    if let Some(c) = comment.filter(|c| !c.trim().is_empty()) {
        gh.comment(&t.repo, t.number, c).await?;
    }
    gh.set_issue_state(&t.repo, t.number, "closed", if reason == "not_planned" { "not_planned" } else { "completed" }).await?;
    if app.db.ticket(id).map(|x| x.state != State::Done).unwrap_or(false) {
        app.db.set_state(id, t.state, State::Done, "closed from Maestro")?;
        app.emit_ticket(id);
    }
    let s = app.db.settings();
    set_gh_status(app, gh, &t, &s.col_done).await;
    git::cleanup(app, &t).await;
    Ok(())
}

/// Orchestrator "planner": turn a free-form goal into a ticket DAG (preview only).
pub async fn plan(app: &AppRef, goal: &str, repo: &str) -> Result<Value> {
    let orch = orchestrator(app)?;
    let run = app.db.create_run(None, "plan", &orch.meta.name)?;
    app.emit(Event::Run { run: run.clone() });
    let existing = app
        .db
        .tickets()
        .iter()
        .filter(|t| t.state != State::Done)
        .map(|t| format!("- #{} {} [{}]", t.number, t.title, t.state.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "Plan the following goal as a set of GitHub tickets for the team.\n\n## Goal\n{goal}\n\n## Team (worker agents)\n{}\n\n## Already open tickets (avoid duplicates)\n{}\n\n\
         The current directory is the repository `{repo}` (read-only) — inspect it to make tickets concrete (files, systems).\n\
         Rules: each ticket = one focused PR for ONE agent; precise acceptance criteria in `body` (markdown); `depends_on` lists `key`s of tickets that must be merged first; order tickets so dependencies come first. Answer ONLY with the JSON object.",
        roster(app), if existing.is_empty() { "(none)".into() } else { existing }
    );
    let cwd = orchestrator_cwd(app, repo).await;
    let plugin = app.lib.build_plugin(&orch)?;
    let out = claude::run(app, Request { run_id: run.id, cwd: &cwd, agent: &orch, prompt, json_schema: Some(plan_schema()), plugin_dir: plugin, tools: None, read_only: true, subagents: None, mcp: None, resume: None }).await?;
    if let Some(r) = app.db.finish_run(run.id, if out.ok { "success" } else { "failed" }, out.cost, out.turns, out.session_id.as_deref(), &out.text) {
        app.emit(Event::Run { run: r });
    }
    let v = out.structured.filter(|_| out.ok).ok_or_else(|| anyhow!("planner failed: {}", out.text))?;
    Ok(json!({"run_id": run.id, "plan": v}))
}

fn valid_helpers(app: &App, v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .filter(|n| app.lib.workers().iter().any(|w| w.meta.name == *n))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

// ───────────────────────────── worker + review

fn review_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "approve": {"type": "boolean"},
        "summary": {"type": "string"},
        "issues": {"type": "array", "items": {"type": "string"}, "description": "blocking problems the author must fix"}
      },
      "required": ["approve", "summary"]
    })
}

async fn work(app: &AppRef, id: i64) -> Result<()> {
    let s = app.db.settings();
    let gh = app.github().await?;
    let t0 = app.db.ticket(id).context("ticket gone")?;
    let assignee = t0.agent.clone().context("no agent assigned")?;
    let team = app.lib.team(&assignee, &t0.helpers)?;
    let agent_name = team.name.clone();
    let base = if s.base_branch.trim().is_empty() { gh.default_branch(&t0.repo).await.unwrap_or_else(|_| "main".into()) } else { s.base_branch.clone() };

    app.transition(id, State::Running, &format!("{agent_name} started"));
    set_gh_status(app, &gh, &t0, &s.col_in_progress).await;

    loop {
        let t = app.db.ticket(id).context("ticket gone")?;
        app.db.set_field(id, "attempts", t.attempts + 1)?;
        let ws = git::prepare(app, gh.token(), &t, &base).await?;
        app.db.set_field(id, "branch", &ws.branch)?;

        // ---- build the worker prompt
        let comments = gh.issue_comments(&t.repo, t.number).await.unwrap_or_default();
        let parent_ctx = t
            .parent
            .and_then(|p| app.db.ticket(p))
            .map(|p| format!("\n## Parent ticket #{}: {}\n{}\n", p.number, p.title, p.body))
            .unwrap_or_default();
        let deps_ctx = t
            .deps
            .iter()
            .filter_map(|d| app.db.ticket(*d))
            .map(|d| format!("- #{} {} ({})", d.number, d.title, d.state.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        let feedback = t.feedback.clone().map(|f| format!("\n## ⚠️ Reviewer feedback to address (round {})\n{f}\n", t.review_rounds)).unwrap_or_default();
        let prompt = format!(
            "You are working on GitHub issue {}#{} on branch `{}` (git worktree, base `{}`).\n\n# {}\n\n{}\n{}{}\n## Dependencies\n{}\n\n## Discussion\n{}\n{}\n\
             ## Instructions\n- Implement the ticket completely in this repository. Follow existing conventions.\n\
             - Build / run tests when the project has them; fix what you break.\n\
             - Commit your work with clear messages (`git add` + `git commit`). Do NOT push and do NOT open a PR — Maestro does that.\n\
             - If you find work that is out of scope (bug, follow-up, missing feature), do not do it: create a ticket with the `maestro` tools when they are available (`create_ticket`), and mention it in your summary.\n\
             - Finish with a short markdown summary of what you changed and anything a human must check.",
            t.repo, t.number, ws.branch, base, t.title, t.body, parent_ctx, "",
            if deps_ctx.is_empty() { "(none)".into() } else { deps_ctx },
            comments.iter().rev().take(10).rev().map(|(u, b)| format!("**{u}**: {b}")).collect::<Vec<_>>().join("\n\n"),
            feedback
        );

        let out = run_team(app, id, &team, &ws.path, prompt).await?;
        if out.cancelled {
            app.transition(id, State::Paused, "cancelled by user");
            return Ok(());
        }
        let t = app.db.ticket(id).context("ticket gone")?;
        if !out.ok {
            if t.attempts < s.max_attempts {
                app.transition(id, State::Ready, &format!("attempt {} failed, retrying", t.attempts));
            } else {
                app.db.set_field(id, "note", format!("Agent failed: {}", out.text.chars().take(500).collect::<String>()))?;
                app.transition(id, State::NeedsHuman, "agent failed");
                gh_comment(app, &gh, &t, &format!("🎼 **Maestro** — `{agent_name}` failed after {} attempts:\n\n```\n{}\n```", t.attempts, out.text.chars().take(1500).collect::<String>())).await;
            }
            return Ok(());
        }

        let commits = git::finalize(app, &ws, &format!("{} (#{})\n\nMaestro agent: {agent_name}", t.title, t.number)).await?;
        if commits == 0 {
            app.db.set_field(id, "note", "Agent produced no changes")?;
            app.transition(id, State::NeedsHuman, "no changes produced");
            gh_comment(app, &gh, &t, &format!("🎼 **Maestro** — `{agent_name}` finished without changing any file:\n\n{}", out.text)).await;
            return Ok(());
        }
        git::push(app, gh.token(), &t.repo, &ws).await?;
        let stat = git::diff_stat(app, &ws).await;
        let pr_body = format!(
            "Closes #{}\n\n## Summary ({agent_name})\n{}\n\n<details><summary>Diff stat</summary>\n\n```\n{}\n```\n</details>\n\n_🎼 Opened by Maestro — cost so far ${:.3}_",
            t.number, out.text, stat, t.cost_usd
        );
        let (pr, pr_url) = gh.open_pr(&t.repo, &ws.branch, &base, &format!("{} (#{})", t.title, t.number), &pr_body).await?;
        if t.pr_number.is_none() {
            gh_comment(app, &gh, &t, &format!("🎼 **Maestro** — `{agent_name}` opened {pr_url}")).await;
        }
        app.db.set_field(id, "pr_number", pr)?;
        app.db.set_field(id, "pr_url", &pr_url)?;
        app.db.set_field(id, "feedback", Option::<String>::None)?;
        let t = app.db.ticket(id).context("ticket gone")?;
        set_gh_status(app, &gh, &t, &s.col_review).await;

        if !s.review_enabled {
            app.transition(id, State::AwaitingMerge, "PR opened");
            return maybe_merge(app, &gh, id).await;
        }

        // ---- review
        app.transition(id, State::Reviewing, "PR opened, reviewing");
        let reviewer = app.lib.agent(&s.reviewer_agent).ok_or_else(|| anyhow!("reviewer agent '{}' not found", s.reviewer_agent))?;
        let rrun = app.db.create_run(Some(id), "review", &reviewer.meta.name)?;
        app.emit(Event::Run { run: rrun.clone() });
        let rprompt = format!(
            "Review the changes on this branch for issue #{} \"{}\".\n\n## Ticket\n{}\n\nInspect with `git diff {}...HEAD` and `git log {}..HEAD`, read the touched files, and verify the acceptance criteria.\n\
             Approve if it correctly and safely implements the ticket. Only list *blocking* issues. Answer ONLY with the JSON object.",
            t.number, t.title, t.body, ws.base_ref, ws.base_ref
        );
        let plugin = app.lib.build_plugin(&reviewer)?;
        let rout = claude::run(app, Request { run_id: rrun.id, cwd: &ws.path, agent: &reviewer, prompt: rprompt, json_schema: Some(review_schema()), plugin_dir: plugin, tools: None, read_only: true, subagents: None, mcp: None, resume: None }).await?;
        app.db.add_cost(id, rout.cost);
        if let Some(r) = app.db.finish_run(rrun.id, if rout.ok { "success" } else { "failed" }, rout.cost, rout.turns, rout.session_id.as_deref(), &rout.text) {
            app.emit(Event::Run { run: r });
        }
        if rout.cancelled {
            app.transition(id, State::AwaitingMerge, "review cancelled");
            return Ok(());
        }
        let Some(v) = rout.structured.filter(|_| rout.ok) else {
            app.transition(id, State::NeedsHuman, "reviewer failed");
            return Ok(());
        };
        let approve = v["approve"].as_bool().unwrap_or(false);
        let summary = v["summary"].as_str().unwrap_or("").to_string();
        let issues: Vec<String> = v["issues"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let _ = gh
            .review_comment(&t.repo, pr, &format!(
                "🔍 **Maestro review** (`{}`) — {}\n\n{}\n{}",
                reviewer.meta.name,
                if approve { "✅ approved" } else { "🔁 changes requested" },
                summary,
                issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")
            ))
            .await;
        if approve {
            app.transition(id, State::AwaitingMerge, "review approved");
            return maybe_merge(app, &gh, id).await;
        }
        let t = app.db.ticket(id).context("ticket gone")?;
        if t.review_rounds >= s.max_review_rounds {
            app.db.set_field(id, "note", format!("Review not passed after {} rounds: {summary}", t.review_rounds))?;
            app.transition(id, State::NeedsHuman, "review rounds exhausted");
            return Ok(());
        }
        app.db.set_field(id, "review_rounds", t.review_rounds + 1)?;
        app.db.set_field(id, "feedback", format!("{summary}\n\n{}", issues.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")))?;
        app.transition(id, State::Running, "changes requested");
        // loop: same worktree, agent fixes the feedback, push updates the PR
    }
}

/// Run a team on a ticket's worktree.
///  - single agent / delegate: one Claude session; members are declared as subagents.
///  - pipeline: lead then members, one session each, summaries passed along.
async fn run_team(app: &App, ticket: i64, team: &Team, cwd: &std::path::Path, prompt: String) -> Result<claude::Outcome> {
    if !team.pipeline {
        let mut prompt = prompt;
        let mut subagents = None;
        if !team.members.is_empty() {
            let mut defs = serde_json::Map::new();
            for m in &team.members {
                let mut d = json!({"description": m.meta.description, "prompt": m.prompt});
                if !m.meta.tools.is_empty() {
                    d["tools"] = json!(m.meta.tools);
                }
                if !m.meta.model.is_empty() && m.meta.model != "inherit" {
                    d["model"] = json!(m.meta.model);
                }
                defs.insert(m.meta.name.clone(), d);
            }
            let dir = app.data.join("plugins");
            std::fs::create_dir_all(&dir)?;
            let f = dir.join(format!("team-{}-{ticket}.agents.json", team.name));
            std::fs::write(&f, serde_json::to_string_pretty(&Value::Object(defs))?)?;
            subagents = Some(f);
            prompt.push_str(&format!(
                "\n\n## Your team\nYou lead `{}`. Delegate with the Task tool to these subagents (same repository), then integrate their work:\n{}\n",
                team.name,
                team.members.iter().map(|m| format!("- `{}`: {}", m.meta.name, m.meta.description)).collect::<Vec<_>>().join("\n")
            ));
        }
        if !team.instructions.trim().is_empty() {
            prompt.push_str(&format!("\n## Team instructions\n{}\n", team.instructions.trim()));
        }
        let run = app.db.create_run(Some(ticket), "work", &team.lead.meta.name)?;
        app.emit(Event::Run { run: run.clone() });
        if !team.members.is_empty() {
            app.log(run.id, "system", &format!("team {} · lead {} · members {}", team.name, team.lead.meta.name, team.members.iter().map(|m| m.meta.name.as_str()).collect::<Vec<_>>().join(", ")));
        }
        let plugin = app.lib.build_team_plugin(team)?;
        let mcp = crate::mcp::config_for(app, ticket, &team.lead)?;
        let out = claude::run(app, Request { run_id: run.id, cwd, agent: &team.lead, prompt, json_schema: None, plugin_dir: plugin, tools: None, read_only: false, subagents, mcp , resume: None }).await?;
        finish(app, ticket, run.id, &out);
        return Ok(out);
    }

    // pipeline
    let steps: Vec<&Agent> = std::iter::once(&team.lead).chain(team.members.iter()).collect();
    let n = steps.len();
    let mut total = claude::Outcome { ok: true, ..Default::default() };
    let mut summaries: Vec<String> = vec![];
    for (i, agent) in steps.into_iter().enumerate() {
        let mut p = prompt.clone();
        p.push_str(&format!("\n\n## Pipeline `{}` — step {}/{} (you are `{}`)\n", team.name, i + 1, n, agent.meta.name));
        if summaries.is_empty() {
            p.push_str("You are the first step. Do your part only; later steps will build on your commits.\n");
        } else {
            p.push_str("Previous steps (their commits are already on this branch):\n");
            p.push_str(&summaries.join("\n\n"));
            p.push('\n');
        }
        if !team.instructions.trim().is_empty() {
            p.push_str(&format!("\n## Team instructions\n{}\n", team.instructions.trim()));
        }
        let run = app.db.create_run(Some(ticket), "work", &agent.meta.name)?;
        app.emit(Event::Run { run: run.clone() });
        app.log(run.id, "system", &format!("pipeline {} · step {}/{}", team.name, i + 1, n));
        let plugin = app.lib.build_plugin(agent)?;
        let mcp = crate::mcp::config_for(app, ticket, agent)?;
        let out = claude::run(app, Request { run_id: run.id, cwd, agent, prompt: p, json_schema: None, plugin_dir: plugin, tools: None, read_only: false, subagents: None, mcp , resume: None }).await?;
        finish(app, ticket, run.id, &out);
        total.cost += out.cost;
        total.turns += out.turns;
        total.session_id = out.session_id.clone();
        summaries.push(format!("### Step {} — {}\n{}", i + 1, agent.meta.name, out.text));
        if out.cancelled || !out.ok {
            total.ok = false;
            total.cancelled = out.cancelled;
            total.text = format!("step {} ({}) failed: {}", i + 1, agent.meta.name, out.text);
            return Ok(total);
        }
    }
    total.text = summaries.join("\n\n");
    Ok(total)
}

fn finish(app: &App, ticket: i64, run_id: i64, out: &claude::Outcome) {
    app.db.add_cost(ticket, out.cost);
    let status = if out.cancelled { "cancelled" } else if out.ok { "success" } else { "failed" };
    if let Some(r) = app.db.finish_run(run_id, status, out.cost, out.turns, out.session_id.as_deref(), &out.text) {
        app.emit(Event::Run { run: r });
    }
}

async fn maybe_merge(app: &App, gh: &Github, id: i64) -> Result<()> {
    let s = app.db.settings();
    if !s.auto_merge {
        return Ok(());
    }
    let t = app.db.ticket(id).context("ticket gone")?;
    if let Some(pr) = t.pr_number {
        match gh.merge_pr(&t.repo, pr).await {
            Ok(_) => {
                app.transition(id, State::Done, "auto-merged");
                set_gh_status(app, gh, &t, &s.col_done).await;
                git::cleanup(app, &t).await;
            }
            Err(e) => app.notice("warn", format!("#{} auto-merge failed: {e}", t.number)),
        }
    }
    Ok(())
}

/// User actions from the UI.
pub async fn action(app: &AppRef, id: i64, action: &str, body: &Value) -> Result<()> {
    let t = app.db.ticket(id).context("not found")?;
    let s = app.db.settings();
    match action {
        "start" => {
            if let Ok(gh) = app.github().await {
                set_gh_status(app, &gh, &t, &s.col_trigger).await;
            }
            app.transition(id, State::Triage, "started from Maestro");
        }
        "retriage" => {
            app.db.set_field(id, "note", Option::<String>::None)?;
            if !app.transition(id, State::Triage, "human: retriage") {
                app.db.set_state(id, t.state, State::Triage, "human: retriage (forced)")?;
                app.emit_ticket(id);
            }
        }
        "retry" => {
            app.db.set_field(id, "attempts", 0)?;
            app.db.set_field(id, "note", Option::<String>::None)?;
            anyhow::ensure!(t.agent.is_some(), "assign an agent first");
            app.db.set_state(id, t.state, State::Ready, "human: retry")?;
            app.emit_ticket(id);
            app.wake.notify_one();
        }
        "pause" => {
            cancel_runs(app, id);
            app.transition(id, State::Paused, "paused by user");
        }
        "resume" => {
            let to = if t.agent.is_some() { State::Ready } else { State::Triage };
            app.db.set_state(id, t.state, to, "resumed")?;
            app.emit_ticket(id);
            app.wake.notify_one();
        }
        "cancel" => cancel_runs(app, id),
        "reset" => {
            cancel_runs(app, id);
            app.db.set_field(id, "attempts", 0)?;
            app.db.set_field(id, "review_rounds", 0)?;
            app.transition(id, State::Backlog, "reset by user");
            // otherwise the trigger label / column would pick it up again at the next sync
            if let Ok(gh) = app.github().await {
                if let Some(t) = app.db.ticket(id) {
                    clear_gh_status(app, &gh, &t).await;
                }
            }
        }
        "assign" => {
            let a = body["agent"].as_str().unwrap_or("");
            anyhow::ensure!(app.lib.is_assignable(a), "unknown agent or cell");
            app.db.set_field(id, "agent", a)?;
            if matches!(t.state, State::Backlog | State::Triage | State::NeedsHuman | State::Paused) {
                app.db.set_state(id, t.state, State::Ready, &format!("human → {a}"))?;
            }
            app.emit_ticket(id);
            app.wake.notify_one();
        }
        "helpers" => {
            let h = valid_helpers(app, &body["helpers"]);
            app.db.set_field(id, "helpers", serde_json::to_string(&h)?)?;
            app.emit_ticket(id);
        }
        "deps" => {
            let deps: Vec<i64> = body["deps"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
            app.db.set_deps(id, &deps)?;
            app.emit_ticket(id);
            app.wake.notify_one();
        }
        "close" => {
            let gh = app.github().await?;
            close_ticket(app, &gh, id, body["reason"].as_str().unwrap_or("completed"), body["comment"].as_str()).await?;
        }
        "reopen" => {
            let gh = app.github().await?;
            gh.set_issue_state(&t.repo, t.number, "open", "reopened").await?;
            app.db.set_field(id, "attempts", 0)?;
            app.db.set_field(id, "review_rounds", 0)?;
            app.db.set_state(id, t.state, State::Backlog, "reopened from Maestro")?;
            clear_gh_status(app, &gh, &t).await;
        }
        "merge" => {
            let gh = app.github().await?;
            let pr = t.pr_number.context("no PR")?;
            gh.merge_pr(&t.repo, pr).await?;
            app.transition(id, State::Done, "merged from Maestro");
            set_gh_status(app, &gh, &t, &s.col_done).await;
            git::cleanup(app, &t).await;
        }
        _ => anyhow::bail!("unknown action"),
    }
    Ok(())
}

fn cancel_runs(app: &App, ticket: i64) {
    for r in app.db.runs(Some(ticket), 20) {
        if r.status == "running" {
            if let Some(tx) = app.cancels.lock().remove(&r.id) {
                let _ = tx.send(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_deps() {
        let b = "Do X\nDepends on #12, #14 and acme/lib#3\nRelated #99";
        assert_eq!(dep_refs(b, "me/r"), vec![("me/r".into(), 12), ("me/r".into(), 14), ("acme/lib".into(), 3)]);
        assert_eq!(parent_ref("---\nPart of #7\n"), Some(7));
    }
    #[test]
    fn state_machine() {
        assert!(can_transition(State::Backlog, State::Triage));
        assert!(!can_transition(State::Backlog, State::Running));
        assert!(can_transition(State::Reviewing, State::Running));
    }
}
