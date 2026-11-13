//! SQLite persistence (WAL, single connection behind a mutex — the workload is tiny).

use crate::model::*;
use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;

pub struct Db {
    conn: Mutex<Connection>,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK (id=1), json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS tickets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id TEXT NOT NULL UNIQUE,
  issue_node_id TEXT NOT NULL DEFAULT '',
  repo TEXT NOT NULL DEFAULT '',
  number INTEGER NOT NULL DEFAULT 0,
  title TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '',
  url TEXT NOT NULL DEFAULT '',
  labels TEXT NOT NULL DEFAULT '[]',
  gh_status TEXT NOT NULL DEFAULT '',
  gh_state TEXT NOT NULL DEFAULT 'OPEN',
  state TEXT NOT NULL DEFAULT 'backlog',
  agent TEXT,
  parent INTEGER,
  branch TEXT,
  pr_number INTEGER,
  pr_url TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  review_rounds INTEGER NOT NULL DEFAULT 0,
  feedback TEXT,
  note TEXT,
  cost_usd REAL NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS deps (
  ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
  depends_on INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
  PRIMARY KEY (ticket_id, depends_on)
);
CREATE TABLE IF NOT EXISTS transitions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
  from_state TEXT NOT NULL, to_state TEXT NOT NULL, reason TEXT NOT NULL, ts TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL,
  kind TEXT NOT NULL, agent TEXT NOT NULL, status TEXT NOT NULL,
  started_at TEXT NOT NULL, finished_at TEXT,
  cost_usd REAL NOT NULL DEFAULT 0, turns INTEGER NOT NULL DEFAULT 0,
  session_id TEXT, summary TEXT
);
CREATE TABLE IF NOT EXISTS logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  ts TEXT NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS logs_run ON logs(run_id);
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT);
CREATE INDEX IF NOT EXISTS runs_ticket ON runs(ticket_id);
"#;

const TICKET_COLS: &str = "id,item_id,issue_node_id,repo,number,title,body,url,labels,gh_status,gh_state,state,agent,parent,branch,pr_number,pr_url,attempts,review_rounds,feedback,note,cost_usd,updated_at,helpers";

fn row_ticket(r: &Row) -> rusqlite::Result<Ticket> {
    let labels: String = r.get(8)?;
    let state: String = r.get(11)?;
    Ok(Ticket {
        id: r.get(0)?,
        item_id: r.get(1)?,
        issue_node_id: r.get(2)?,
        repo: r.get(3)?,
        number: r.get(4)?,
        title: r.get(5)?,
        body: r.get(6)?,
        url: r.get(7)?,
        labels: serde_json::from_str(&labels).unwrap_or_default(),
        gh_status: r.get(9)?,
        gh_state: r.get(10)?,
        state: State::parse(&state),
        agent: r.get(12)?,
        parent: r.get(13)?,
        branch: r.get(14)?,
        pr_number: r.get(15)?,
        pr_url: r.get(16)?,
        attempts: r.get(17)?,
        review_rounds: r.get(18)?,
        feedback: r.get(19)?,
        note: r.get(20)?,
        cost_usd: r.get(21)?,
        updated_at: r.get(22)?,
        helpers: serde_json::from_str(&r.get::<_, String>(23)?).unwrap_or_default(),
        deps: vec![],
    })
}

fn row_run(r: &Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        ticket_id: r.get(1)?,
        kind: r.get(2)?,
        agent: r.get(3)?,
        status: r.get(4)?,
        started_at: r.get(5)?,
        finished_at: r.get(6)?,
        cost_usd: r.get(7)?,
        turns: r.get(8)?,
        session_id: r.get(9)?,
        summary: r.get(10)?,
    })
}

/// Incoming data from a GitHub sync.
pub struct SyncedItem {
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
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        // migrations (ignore "duplicate column" on already-migrated DBs)
        let _ = conn.execute("ALTER TABLE tickets ADD COLUMN helpers TEXT NOT NULL DEFAULT '[]'", []);
        // Crash recovery: anything left "running" is retried.
        conn.execute("UPDATE runs SET status='interrupted', finished_at=?1 WHERE status='running'", [now()])?;
        conn.execute("UPDATE tickets SET state='ready' WHERE state='running'", [])?;
        conn.execute("UPDATE tickets SET state='awaiting_merge' WHERE state='reviewing'", [])?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    // ---------- settings
    pub fn settings(&self) -> Settings {
        let c = self.conn.lock();
        let json: Option<String> = c
            .query_row("SELECT json FROM settings WHERE id=1", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        let mut s = json.and_then(|j| serde_json::from_str::<Settings>(&j).ok())
            .unwrap_or_default()
            .with_defaults();
        s.apply_env();
        s
    }
    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let mut s = s.clone();
        for f in Settings::env_secrets() {
            match f {
                "github_token" => s.github_token.clear(),
                "claude_oauth_token" => s.claude_oauth_token.clear(),
                _ => s.anthropic_api_key.clear(),
            }
        }
        let s = &s;
        let c = self.conn.lock();
        c.execute(
            "INSERT INTO settings(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json",
            [serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    // ---------- meta
    pub fn meta(&self, k: &str) -> Option<String> {
        let c = self.conn.lock();
        c.query_row("SELECT v FROM meta WHERE k=?1", [k], |r| r.get(0)).optional().ok().flatten()
    }
    pub fn set_meta(&self, k: &str, v: &str) {
        let c = self.conn.lock();
        let _ = c.execute("INSERT INTO meta(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v", params![k, v]);
    }

    /// Persistent random token protecting the MCP endpoint.
    pub fn mcp_token(&self) -> String {
        if let Some(t) = self.meta("mcp_token") {
            return t;
        }
        let mut buf = [0u8; 24];
        let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut buf)).is_ok();
        if !ok {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
            for (i, b) in buf.iter_mut().enumerate() {
                *b = ((n >> ((i % 16) * 8)) as u8) ^ (std::process::id() as u8).wrapping_mul(i as u8 + 7);
            }
        }
        let t: String = buf.iter().map(|b| format!("{b:02x}")).collect();
        self.set_meta("mcp_token", &t);
        t
    }

    // ---------- tickets
    fn attach_deps(c: &Connection, t: &mut Ticket) {
        if let Ok(mut st) = c.prepare_cached("SELECT depends_on FROM deps WHERE ticket_id=?1") {
            t.deps = st
                .query_map([t.id], |r| r.get(0))
                .map(|it| it.filter_map(|x| x.ok()).collect())
                .unwrap_or_default();
        }
    }

    pub fn tickets(&self) -> Vec<Ticket> {
        let c = self.conn.lock();
        let mut st = c.prepare(&format!("SELECT {TICKET_COLS} FROM tickets ORDER BY id")).unwrap();
        let mut v: Vec<Ticket> = st.query_map([], row_ticket).unwrap().filter_map(|x| x.ok()).collect();
        drop(st);
        for t in v.iter_mut() {
            Self::attach_deps(&c, t);
        }
        v
    }

    pub fn ticket(&self, id: i64) -> Option<Ticket> {
        let c = self.conn.lock();
        let mut t = c
            .query_row(&format!("SELECT {TICKET_COLS} FROM tickets WHERE id=?1"), [id], row_ticket)
            .optional()
            .ok()
            .flatten()?;
        Self::attach_deps(&c, &mut t);
        Some(t)
    }

    pub fn ticket_by_number(&self, repo: &str, number: i64) -> Option<Ticket> {
        let id: i64 = {
            let c = self.conn.lock();
            c.query_row(
                "SELECT id FROM tickets WHERE lower(repo)=lower(?1) AND number=?2",
                params![repo, number],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()?
        };
        self.ticket(id)
    }

    /// Upsert from GitHub. Returns (ticket, is_new).
    pub fn upsert_synced(&self, it: &SyncedItem) -> Result<(Ticket, bool)> {
        let id: Option<i64> = {
            let c = self.conn.lock();
            let existing: Option<i64> = c
                .query_row("SELECT id FROM tickets WHERE item_id=?1", [&it.item_id], |r| r.get(0))
                .optional()?;
            let labels = serde_json::to_string(&it.labels)?;
            match existing {
                Some(id) => {
                    c.execute(
                        "UPDATE tickets SET issue_node_id=?2,repo=?3,number=?4,title=?5,body=?6,url=?7,labels=?8,gh_status=?9,gh_state=?10 WHERE id=?1",
                        params![id, it.issue_node_id, it.repo, it.number, it.title, it.body, it.url, labels, it.gh_status, it.gh_state],
                    )?;
                    Some(id)
                }
                None => {
                    c.execute(
                        "INSERT INTO tickets(item_id,issue_node_id,repo,number,title,body,url,labels,gh_status,gh_state,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                        params![it.item_id, it.issue_node_id, it.repo, it.number, it.title, it.body, it.url, labels, it.gh_status, it.gh_state, now()],
                    )?;
                    None
                }
            }
        };
        match id {
            Some(id) => Ok((self.ticket(id).unwrap(), false)),
            None => {
                let nid = self.conn.lock().last_insert_rowid();
                Ok((self.ticket(nid).unwrap(), true))
            }
        }
    }

    pub fn remove_missing(&self, keep_item_ids: &[String]) -> Vec<i64> {
        let all: Vec<(i64, String)> = {
            let c = self.conn.lock();
            let mut st = c.prepare("SELECT id,item_id FROM tickets").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().filter_map(|x| x.ok()).collect()
        };
        let mut removed = vec![];
        for (id, item) in all {
            if !keep_item_ids.contains(&item) {
                let c = self.conn.lock();
                let _ = c.execute("DELETE FROM tickets WHERE id=?1", [id]);
                removed.push(id);
            }
        }
        removed
    }

    /// Generic patch helper: `field` must be a known column.
    pub fn set_field<T: rusqlite::ToSql>(&self, id: i64, field: &str, v: T) -> Result<()> {
        const OK: &[&str] = &[
            "agent", "helpers", "parent", "branch", "pr_number", "pr_url", "attempts", "review_rounds", "feedback", "note", "gh_status",
        ];
        anyhow::ensure!(OK.contains(&field), "bad field");
        let c = self.conn.lock();
        c.execute(&format!("UPDATE tickets SET {field}=?2, updated_at=?3 WHERE id=?1"), params![id, v, now()])?;
        Ok(())
    }

    pub fn add_cost(&self, id: i64, usd: f64) {
        let c = self.conn.lock();
        let _ = c.execute("UPDATE tickets SET cost_usd=cost_usd+?2 WHERE id=?1", params![id, usd]);
    }

    pub fn set_state(&self, id: i64, from: State, to: State, reason: &str) -> Result<()> {
        let c = self.conn.lock();
        let ts = now();
        c.execute("UPDATE tickets SET state=?2, updated_at=?3 WHERE id=?1", params![id, to.as_str(), ts])?;
        c.execute(
            "INSERT INTO transitions(ticket_id,from_state,to_state,reason,ts) VALUES(?1,?2,?3,?4,?5)",
            params![id, from.as_str(), to.as_str(), reason, ts],
        )?;
        Ok(())
    }

    pub fn transitions(&self, ticket_id: i64) -> Vec<serde_json::Value> {
        let c = self.conn.lock();
        let mut st = c
            .prepare("SELECT from_state,to_state,reason,ts FROM transitions WHERE ticket_id=?1 ORDER BY id")
            .unwrap();
        st.query_map([ticket_id], |r| {
            Ok(serde_json::json!({"from": r.get::<_,String>(0)?, "to": r.get::<_,String>(1)?, "reason": r.get::<_,String>(2)?, "ts": r.get::<_,String>(3)?}))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    pub fn set_deps(&self, id: i64, deps: &[i64]) -> Result<()> {
        let c = self.conn.lock();
        c.execute("DELETE FROM deps WHERE ticket_id=?1", [id])?;
        for d in deps {
            if *d != id {
                c.execute("INSERT OR IGNORE INTO deps(ticket_id,depends_on) VALUES(?1,?2)", params![id, d])?;
            }
        }
        Ok(())
    }

    pub fn add_dep(&self, id: i64, dep: i64) -> Result<()> {
        if id == dep {
            return Ok(());
        }
        let c = self.conn.lock();
        c.execute("INSERT OR IGNORE INTO deps(ticket_id,depends_on) VALUES(?1,?2)", params![id, dep])?;
        Ok(())
    }


    // ---------- runs & logs
    pub fn create_run(&self, ticket_id: Option<i64>, kind: &str, agent: &str) -> Result<Run> {
        let c = self.conn.lock();
        c.execute(
            "INSERT INTO runs(ticket_id,kind,agent,status,started_at) VALUES(?1,?2,?3,'running',?4)",
            params![ticket_id, kind, agent, now()],
        )?;
        let id = c.last_insert_rowid();
        Ok(c.query_row("SELECT * FROM runs WHERE id=?1", [id], row_run)?)
    }

    pub fn finish_run(&self, id: i64, status: &str, cost: f64, turns: i64, session: Option<&str>, summary: &str) -> Option<Run> {
        let c = self.conn.lock();
        let _ = c.execute(
            "UPDATE runs SET status=?2, finished_at=?3, cost_usd=?4, turns=?5, session_id=?6, summary=?7 WHERE id=?1",
            params![id, status, now(), cost, turns, session, summary],
        );
        c.query_row("SELECT * FROM runs WHERE id=?1", [id], row_run).ok()
    }

    pub fn runs(&self, ticket_id: Option<i64>, limit: i64) -> Vec<Run> {
        let c = self.conn.lock();
        let (sql, p): (&str, Vec<i64>) = match ticket_id {
            Some(t) => ("SELECT * FROM runs WHERE ticket_id=?1 ORDER BY id DESC LIMIT ?2", vec![t, limit]),
            None => ("SELECT * FROM runs ORDER BY id DESC LIMIT ?1", vec![limit]),
        };
        let mut st = c.prepare(sql).unwrap();
        st.query_map(rusqlite::params_from_iter(p), row_run).unwrap().filter_map(|x| x.ok()).collect()
    }

    pub fn add_log(&self, run_id: i64, kind: &str, text: &str) -> LogLine {
        let ts = now();
        let c = self.conn.lock();
        let _ = c.execute("INSERT INTO logs(run_id,ts,kind,text) VALUES(?1,?2,?3,?4)", params![run_id, ts, kind, text]);
        LogLine { run_id, ts, kind: kind.into(), text: text.into() }
    }

    pub fn logs(&self, run_id: i64) -> Vec<LogLine> {
        let c = self.conn.lock();
        let mut st = c.prepare("SELECT run_id,ts,kind,text FROM logs WHERE run_id=?1 ORDER BY id").unwrap();
        st.query_map([run_id], |r| Ok(LogLine { run_id: r.get(0)?, ts: r.get(1)?, kind: r.get(2)?, text: r.get(3)? }))
            .unwrap()
            .filter_map(|x| x.ok())
            .collect()
    }

    pub fn stats(&self) -> serde_json::Value {
        let c = self.conn.lock();
        let cost: f64 = c.query_row("SELECT COALESCE(SUM(cost_usd),0) FROM runs", [], |r| r.get(0)).unwrap_or(0.0);
        let runs: i64 = c.query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0)).unwrap_or(0);
        let ok: i64 = c.query_row("SELECT COUNT(*) FROM runs WHERE status='success'", [], |r| r.get(0)).unwrap_or(0);
        serde_json::json!({"total_cost_usd": cost, "runs": runs, "successful_runs": ok})
    }
}
