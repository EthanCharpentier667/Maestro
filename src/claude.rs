//! Runs the local Claude Code CLI headless (`claude -p --output-format stream-json`)
//! and streams its events into run logs.
//!
//! Auth modes:
//!  - account     : uses whatever `claude` is logged into on this machine (Pro/Max subscription)
//!  - oauth_token : long-lived token from `claude setup-token` (CLAUDE_CODE_OAUTH_TOKEN)
//!  - api_key     : ANTHROPIC_API_KEY (pay-as-you-go API)

use crate::agents::Agent;
use crate::app::App;
use crate::model::Settings;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

#[derive(Debug, Default, Clone)]
pub struct Outcome {
    pub ok: bool,
    pub cancelled: bool,
    pub text: String,
    pub structured: Option<Value>,
    pub cost: f64,
    pub turns: i64,
    pub session_id: Option<String>,
}

pub struct Request<'a> {
    pub run_id: i64,
    pub cwd: &'a Path,
    pub agent: &'a Agent,
    pub prompt: String,
    pub json_schema: Option<Value>,
    pub plugin_dir: Option<std::path::PathBuf>,
    /// Override tools (e.g. read-only for orchestrator/reviewer).
    pub tools: Option<Vec<String>>,
    /// Orchestrator / reviewer: may read the repo but never edit it.
    pub read_only: bool,
    /// JSON file defining Claude Code subagents (`--agents`), for delegate teams.
    pub subagents: Option<std::path::PathBuf>,
    /// `--mcp-config` file (Maestro MCP server for tickets).
    pub mcp: Option<std::path::PathBuf>,
}

pub fn base_command(s: &Settings) -> Command {
    let mut cmd = Command::new(if s.claude_bin.is_empty() { "claude" } else { &s.claude_bin });
    match s.claude_auth.as_str() {
        "oauth_token" if !s.claude_oauth_token.is_empty() => {
            cmd.env("CLAUDE_CODE_OAUTH_TOKEN", &s.claude_oauth_token);
            cmd.env_remove("ANTHROPIC_API_KEY");
        }
        "api_key" if !s.anthropic_api_key.is_empty() => {
            cmd.env("ANTHROPIC_API_KEY", &s.anthropic_api_key);
            cmd.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
        }
        _ => {
            // account mode: rely on the local login, never on a stray key/token
            cmd.env_remove("ANTHROPIC_API_KEY");
            cmd.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
        }
    }
    cmd
}

fn short(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

fn describe_tool(name: &str, input: &Value) -> String {
    let pick = ["command", "file_path", "path", "pattern", "url", "description", "prompt", "skill"]
        .iter()
        .find_map(|k| input.get(*k).and_then(|v| v.as_str()));
    match pick {
        Some(p) => format!("{name} · {}", short(p, 220)),
        None => format!("{name} · {}", short(&input.to_string(), 220)),
    }
}

/// Best effort: pull a JSON object out of free text (```json blocks or first {...}).
pub fn extract_json(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        if v.is_object() {
            return Some(v);
        }
    }
    if let Some(i) = text.find("```json") {
        let rest = &text[i + 7..];
        if let Some(j) = rest.find("```") {
            if let Ok(v) = serde_json::from_str(rest[..j].trim()) {
                return Some(v);
            }
        }
    }
    let (a, b) = (text.find('{')?, text.rfind('}')?);
    serde_json::from_str(&text[a..=b]).ok()
}

pub async fn run(app: &App, req: Request<'_>) -> Result<Outcome> {
    let s = app.db.settings();
    let mut cmd = base_command(&s);
    cmd.current_dir(req.cwd)
        .arg("-p")
        .args(["--output-format", "stream-json", "--verbose"])
        .args(["--permission-mode", if req.read_only { "default" } else { &s.permission_mode }]);
    if req.read_only {
        cmd.args(["--disallowedTools", "Edit,Write,NotebookEdit"]);
    }

    let model = if !req.agent.meta.model.is_empty() { req.agent.meta.model.clone() } else { s.default_model.clone() };
    if !model.is_empty() && model != "inherit" {
        cmd.args(["--model", &model]);
    }
    if !req.agent.meta.effort.is_empty() {
        cmd.args(["--effort", &req.agent.meta.effort]);
    }
    let mut tools = req.tools.clone().unwrap_or_else(|| req.agent.meta.tools.clone());
    if let Some(f) = &req.subagents {
        cmd.arg("--agents").arg(f);
        // the lead must be able to spawn its teammates
        if !tools.is_empty() {
            for t in ["Task", "Agent"] {
                if !tools.iter().any(|x| x == t) {
                    tools.push(t.into());
                }
            }
        }
    }
    if let Some(m) = &req.mcp {
        cmd.arg("--mcp-config").arg(m);
        tools.push("mcp__maestro".into());
    }
    if !tools.is_empty() {
        cmd.arg("--allowedTools").arg(tools.join(","));
    }
    if !req.agent.prompt.trim().is_empty() {
        cmd.args(["--append-system-prompt", req.agent.prompt.trim()]);
    }
    if let Some(p) = &req.plugin_dir {
        cmd.arg("--plugin-dir").arg(p);
    }
    if s.max_budget_usd > 0.0 {
        cmd.args(["--max-budget-usd", &format!("{:.2}", s.max_budget_usd)]);
    }
    if let Some(schema) = &req.json_schema {
        cmd.args(["--json-schema", &schema.to_string()]);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);

    app.log(req.run_id, "system", &format!("▶ claude [{}] model={} cwd={}", req.agent.meta.name, if model.is_empty() { "default" } else { &model }, req.cwd.display()));

    let mut child = cmd.spawn().with_context(|| format!("cannot start `{}` — is Claude Code installed? (npm i -g @anthropic-ai/claude-code)", s.claude_bin))?;
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(req.prompt.as_bytes()).await?;
        stdin.shutdown().await?;
    }
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let (tx, mut cancel_rx) = oneshot::channel::<()>();
    app.cancels.lock().insert(req.run_id, tx);

    let run_id = req.run_id;
    let err_task = {
        // stderr is collected and logged (auth errors show up here)
        let mut lines = BufReader::new(stderr).lines();
        let mut buf = String::new();
        async move {
            while let Ok(Some(l)) = lines.next_line().await {
                if buf.len() < 8000 {
                    buf.push_str(&l);
                    buf.push('\n');
                }
            }
            buf
        }
    };
    let err_handle = tokio::spawn(err_task);

    let mut out = Outcome::default();
    let mut lines = BufReader::new(stdout).lines();
    let mut got_result = false;
    loop {
        tokio::select! {
            _ = &mut cancel_rx => {
                let _ = child.kill().await;
                out.cancelled = true;
                app.log(run_id, "system", "■ cancelled by user");
                break;
            }
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                let Ok(ev) = serde_json::from_str::<Value>(&line) else {
                    if !line.trim().is_empty() { app.log(run_id, "raw", &short(&line, 500)); }
                    continue;
                };
                match ev["type"].as_str().unwrap_or("") {
                    "system" if ev["subtype"] == "init" => {
                        out.session_id = ev["session_id"].as_str().map(String::from);
                        app.log(run_id, "system", &format!("session {} · model {} · {} tools",
                            ev["session_id"].as_str().unwrap_or("?"), ev["model"].as_str().unwrap_or("?"),
                            ev["tools"].as_array().map(|a| a.len()).unwrap_or(0)));
                    }
                    "assistant" => {
                        for c in ev["message"]["content"].as_array().cloned().unwrap_or_default() {
                            match c["type"].as_str() {
                                Some("text") => { let t = c["text"].as_str().unwrap_or(""); if !t.trim().is_empty() { app.log(run_id, "text", t); } }
                                Some("tool_use") => app.log(run_id, "tool", &describe_tool(c["name"].as_str().unwrap_or("?"), &c["input"])),
                                _ => {}
                            }
                        }
                    }
                    "user" => {
                        for c in ev["message"]["content"].as_array().cloned().unwrap_or_default() {
                            if c["type"] == "tool_result" {
                                let txt = match &c["content"] {
                                    Value::String(s) => s.clone(),
                                    Value::Array(a) => a.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n"),
                                    v => v.to_string(),
                                };
                                let kind = if c["is_error"].as_bool().unwrap_or(false) { "tool_error" } else { "tool_result" };
                                app.log(run_id, kind, &short(&txt, 700));
                            }
                        }
                    }
                    "result" => {
                        got_result = true;
                        out.ok = !ev["is_error"].as_bool().unwrap_or(false) && ev["subtype"] == "success";
                        out.text = ev["result"].as_str().unwrap_or("").to_string();
                        out.cost = ev["total_cost_usd"].as_f64().unwrap_or(0.0);
                        out.turns = ev["num_turns"].as_i64().unwrap_or(0);
                        if out.session_id.is_none() { out.session_id = ev["session_id"].as_str().map(String::from); }
                        out.structured = ev.get("structured_output").filter(|v| !v.is_null()).cloned()
                            .or_else(|| extract_json(&out.text));
                        app.log(run_id, "result", &format!("{} · {} turns · ${:.4}\n{}",
                            ev["subtype"].as_str().unwrap_or("?"), out.turns, out.cost, short(&out.text, 4000)));
                    }
                    _ => {}
                }
            }
        }
    }
    app.cancels.lock().remove(&run_id);
    let status = child.wait().await.ok();
    let stderr_txt = err_handle.await.unwrap_or_default();
    if !got_result && !out.cancelled {
        let msg = if stderr_txt.trim().is_empty() {
            format!("claude exited ({:?}) without a result", status.and_then(|s| s.code()))
        } else {
            short(&stderr_txt, 2000)
        };
        app.log(run_id, "error", &msg);
        out.text = msg;
    } else if !stderr_txt.trim().is_empty() {
        app.log(run_id, "stderr", &short(&stderr_txt, 2000));
    }
    Ok(out)
}

/// Quick connectivity/auth check used by Settings → "Test Claude".
pub async fn check(s: &Settings) -> Result<String> {
    let mut cmd = base_command(s);
    cmd.args(["-p", "--output-format", "json", "Reply with exactly: OK"]);
    let out = tokio::time::timeout(std::time::Duration::from_secs(120), cmd.output())
        .await
        .context("timeout")?
        .context("cannot start claude")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Ok(v) = serde_json::from_str::<Value>(&stdout) {
        if v["is_error"].as_bool().unwrap_or(false) {
            anyhow::bail!("{}", v["result"].as_str().unwrap_or("error"));
        }
        return Ok(format!("OK — reply: {} (model run cost ${:.4})", v["result"].as_str().unwrap_or("").trim(), v["total_cost_usd"].as_f64().unwrap_or(0.0)));
    }
    anyhow::bail!("{}{}", stdout.trim(), String::from_utf8_lossy(&out.stderr).trim())
}

pub async fn version(s: &Settings) -> Option<String> {
    let out = base_command(s).arg("--version").output().await.ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
