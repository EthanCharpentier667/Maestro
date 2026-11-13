//! Shared application state.

use crate::agents::Library;
use crate::db::Db;
use crate::github::{Github, ProjectInfo};
use crate::model::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, Notify};

pub struct App {
    pub db: Db,
    pub lib: Library,
    pub data: PathBuf,
    pub events: broadcast::Sender<Event>,
    /// run_id -> cancel switch for running Claude processes
    pub cancels: Mutex<HashMap<i64, oneshot::Sender<()>>>,
    /// ticket ids currently owned by a worker task
    pub busy: Mutex<std::collections::HashSet<i64>>,
    pub project: Mutex<Option<ProjectInfo>>,
    pub wake: Notify,
    pub last_sync: Mutex<Option<(String, Option<String>)>>,
    /// One async lock per repository: serialises clone / fetch / worktree ops.
    pub repo_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// http://host:port used by agents to reach the MCP endpoint.
    pub base_url: String,
    /// ticket id -> number of tickets its agents created through MCP
    pub mcp_created: Mutex<HashMap<i64, usize>>,
}

pub type AppRef = Arc<App>;

impl App {
    pub fn repo_lock(&self, repo: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.repo_locks.lock().entry(repo.to_lowercase()).or_default().clone()
    }

    pub fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    pub fn notice(&self, level: &str, msg: impl Into<String>) {
        let message = msg.into();
        match level {
            "error" => tracing::error!("{message}"),
            "warn" => tracing::warn!("{message}"),
            _ => tracing::info!("{message}"),
        }
        self.emit(Event::Notice { level: level.into(), message });
    }

    pub fn emit_ticket(&self, id: i64) {
        if let Some(t) = self.db.ticket(id) {
            self.emit(Event::Ticket { ticket: t });
        }
    }

    pub fn log(&self, run_id: i64, kind: &str, text: &str) {
        let line = self.db.add_log(run_id, kind, text);
        self.emit(Event::Log { line });
    }

    /// Validated state transition (persists, records history, broadcasts).
    pub fn transition(&self, id: i64, to: State, reason: &str) -> bool {
        let Some(t) = self.db.ticket(id) else { return false };
        if t.state == to {
            return true;
        }
        if !can_transition(t.state, to) {
            tracing::warn!("refused transition #{} {:?} -> {:?} ({reason})", t.number, t.state, to);
            return false;
        }
        if self.db.set_state(id, t.state, to, reason).is_ok() {
            self.emit(Event::Transition { ticket_id: id, from: t.state, to, reason: reason.into(), ts: crate::db::now() });
            self.emit_ticket(id);
            self.wake.notify_one();
            true
        } else {
            false
        }
    }

    pub async fn github(&self) -> anyhow::Result<Github> {
        let s = self.db.settings();
        Ok(Github::new(crate::github::resolve_token(&s).await?))
    }

    pub async fn project_info(&self, gh: &Github, refresh: bool) -> anyhow::Result<ProjectInfo> {
        if !refresh {
            if let Some(p) = self.project.lock().clone() {
                return Ok(p);
            }
        }
        let s = self.db.settings();
        anyhow::ensure!(!s.project_owner.is_empty() && s.project_number > 0, "Configure the GitHub project (Settings → GitHub)");
        let p = gh.project(&s.project_owner, s.project_number, &s.status_field).await?;
        *self.project.lock() = Some(p.clone());
        Ok(p)
    }
}
