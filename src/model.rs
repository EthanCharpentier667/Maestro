//! Core data types + the ticket state machine (single source of truth,
//! also served to the UI so the diagram always matches the engine).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// On the GitHub board but not in the trigger column yet.
    Backlog,
    /// Picked up, waiting for the orchestrator to analyse / route it.
    Triage,
    /// Orchestrator split it into sub-tickets; waits for them.
    Split,
    /// Has unfinished dependencies.
    Blocked,
    /// Routed to an agent, waiting for a free worker slot.
    Ready,
    /// A specialist agent is working on it in its own git worktree.
    Running,
    /// PR opened, reviewer agent is checking it.
    Reviewing,
    /// Reviewer approved (or review disabled) — waiting for a human merge.
    AwaitingMerge,
    /// PR merged / issue closed.
    Done,
    /// Agent failed too many times, or needs clarification.
    NeedsHuman,
    /// Paused / cancelled by the user.
    Paused,
}

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Backlog => "backlog",
            State::Triage => "triage",
            State::Split => "split",
            State::Blocked => "blocked",
            State::Ready => "ready",
            State::Running => "running",
            State::Reviewing => "reviewing",
            State::AwaitingMerge => "awaiting_merge",
            State::Done => "done",
            State::NeedsHuman => "needs_human",
            State::Paused => "paused",
        }
    }
    pub fn parse(s: &str) -> State {
        match s {
            "triage" => State::Triage,
            "split" => State::Split,
            "blocked" => State::Blocked,
            "ready" => State::Ready,
            "running" => State::Running,
            "reviewing" => State::Reviewing,
            "awaiting_merge" => State::AwaitingMerge,
            "done" => State::Done,
            "needs_human" => State::NeedsHuman,
            "paused" => State::Paused,
            _ => State::Backlog,
        }
    }
    pub const ALL: [State; 11] = [
        State::Backlog,
        State::Triage,
        State::Split,
        State::Blocked,
        State::Ready,
        State::Running,
        State::Reviewing,
        State::AwaitingMerge,
        State::Done,
        State::NeedsHuman,
        State::Paused,
    ];
}

/// Allowed transitions: (from, to, trigger label). The engine refuses any
/// transition that is not listed here.
pub const TRANSITIONS: &[(State, State, &str)] = &[
    (State::Backlog, State::Triage, "moved to trigger column"),
    (State::Backlog, State::Ready, "created by orchestrator"),
    (State::Backlog, State::Blocked, "created by orchestrator, deps pending"),
    (State::Triage, State::Ready, "routed to agent"),
    (State::Triage, State::Blocked, "routed, deps pending"),
    (State::Triage, State::Split, "split into sub-tickets"),
    (State::Triage, State::NeedsHuman, "unclear ticket"),
    (State::Split, State::Done, "all sub-tickets done"),
    (State::Blocked, State::Ready, "deps done"),
    (State::Ready, State::Running, "worker slot free"),
    (State::Running, State::Reviewing, "PR opened"),
    (State::Running, State::AwaitingMerge, "PR opened (no review)"),
    (State::Running, State::Ready, "failed, retry"),
    (State::Running, State::NeedsHuman, "failed / no changes"),
    (State::Reviewing, State::AwaitingMerge, "review approved"),
    (State::Reviewing, State::Running, "changes requested"),
    (State::Reviewing, State::NeedsHuman, "review failed"),
    (State::AwaitingMerge, State::Done, "PR merged / issue closed"),
    (State::NeedsHuman, State::Triage, "human: retriage"),
    (State::NeedsHuman, State::Ready, "human: retry"),
    (State::Paused, State::Triage, "resume"),
];

pub fn can_transition(from: State, to: State) -> bool {
    if from == to {
        return true;
    }
    // Pausing, closing on GitHub and manual reset are always allowed.
    if to == State::Paused || to == State::Done || to == State::Backlog {
        return true;
    }
    TRANSITIONS.iter().any(|(f, t, _)| *f == from && *t == to)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub id: i64,
    pub item_id: String,
    pub issue_node_id: String,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub labels: Vec<String>,
    pub gh_status: String,
    pub gh_state: String,
    pub state: State,
    /// Agent or cell name.
    pub agent: Option<String>,
    /// Extra agents working with the assignee on this ticket.
    pub helpers: Vec<String>,
    pub parent: Option<i64>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub attempts: i64,
    pub review_rounds: i64,
    pub feedback: Option<String>,
    pub note: Option<String>,
    pub cost_usd: f64,
    pub deps: Vec<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: i64,
    pub ticket_id: Option<i64>,
    pub kind: String,
    pub agent: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub cost_usd: f64,
    pub turns: i64,
    pub session_id: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub run_id: i64,
    pub ts: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    // --- GitHub
    pub github_auth: String, // "token" | "gh_cli"
    pub github_token: String,
    pub project_owner: String,
    pub project_number: i64,
    pub status_field: String,
    pub col_trigger: String,
    pub col_in_progress: String,
    pub col_review: String,
    pub col_done: String,
    pub col_blocked: String,
    pub poll_seconds: u64,
    // --- Claude Code
    pub claude_bin: String,
    pub claude_auth: String, // "account" | "oauth_token" | "api_key"
    pub claude_oauth_token: String,
    pub anthropic_api_key: String,
    pub permission_mode: String,
    pub default_model: String,
    pub orchestrator_agent: String,
    pub reviewer_agent: String,
    pub max_parallel: usize,
    pub max_attempts: i64,
    pub max_review_rounds: i64,
    pub max_budget_usd: f64,
    pub review_enabled: bool,
    pub auto_merge: bool,
    pub always_triage: bool,
    pub engine_enabled: bool,
    /// Tickets created by agents through MCP are queued right away (otherwise: backlog).
    pub mcp_auto_queue: bool,
    // --- Git
    pub base_branch: String,
    pub repo_paths: String, // "owner/repo=/abs/path" one per line
    pub git_author_name: String,
    pub git_author_email: String,
}

impl Settings {
    pub fn with_defaults(mut self) -> Self {
        fn d(s: &mut String, v: &str) {
            if s.trim().is_empty() {
                *s = v.to_string();
            }
        }
        d(&mut self.github_auth, "token");
        d(&mut self.status_field, "Status");
        d(&mut self.col_trigger, "Todo");
        d(&mut self.col_in_progress, "In Progress");
        d(&mut self.col_review, "In Review");
        d(&mut self.col_done, "Done");
        d(&mut self.claude_bin, "claude");
        d(&mut self.claude_auth, "account");
        d(&mut self.permission_mode, "acceptEdits");
        d(&mut self.orchestrator_agent, "orchestrator");
        d(&mut self.reviewer_agent, "reviewer");
        d(&mut self.git_author_name, "Maestro Bot");
        d(&mut self.git_author_email, "maestro@localhost");
        if self.poll_seconds == 0 {
            self.poll_seconds = 30;
        }
        if self.max_parallel == 0 {
            self.max_parallel = 2;
        }
        if self.max_attempts == 0 {
            self.max_attempts = 2;
        }
        if self.max_review_rounds == 0 {
            self.max_review_rounds = 2;
        }
        self
    }

    /// Copy with secrets masked, for the UI.
    pub fn redacted(&self) -> Settings {
        let mut s = self.clone();
        let m = |v: &mut String| {
            if !v.is_empty() {
                *v = "••••••".into();
            }
        };
        m(&mut s.github_token);
        m(&mut s.claude_oauth_token);
        m(&mut s.anthropic_api_key);
        s
    }

    /// Secrets provided by the environment (.env) win over the database and are
    /// never persisted. Returns the settings field names that come from the env.
    pub fn apply_env(&mut self) -> Vec<&'static str> {
        let mut from_env = vec![];
        let mut take = |field: &'static str, vars: &[&str], dst: &mut String| {
            if let Some(v) = vars.iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())) {
                *dst = v.trim().to_string();
                from_env.push(field);
            }
        };
        take("github_token", &["MAESTRO_GITHUB_TOKEN"], &mut self.github_token);
        take("claude_oauth_token", &["MAESTRO_CLAUDE_OAUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"], &mut self.claude_oauth_token);
        take("anthropic_api_key", &["MAESTRO_ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY"], &mut self.anthropic_api_key);
        from_env
    }

    pub fn env_secrets() -> Vec<&'static str> {
        Settings::default().apply_env()
    }

    pub fn repo_path(&self, repo: &str) -> Option<String> {
        self.repo_paths.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim().eq_ignore_ascii_case(repo)).then(|| v.trim().to_string())
        })
    }
}

/// Events pushed to the UI over SSE.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Ticket { ticket: Ticket },
    TicketRemoved { id: i64 },
    Transition { ticket_id: i64, from: State, to: State, reason: String, ts: String },
    Run { run: Run },
    Log { line: LogLine },
    Notice { level: String, message: String },
}
