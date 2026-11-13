//! Agents = markdown files with YAML front-matter (same spirit as Claude Code
//! subagents). Skills = folders with a SKILL.md. Each agent gets a generated
//! Claude Code plugin dir containing its skills, passed with --plugin-dir.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentMeta {
    pub name: String,
    pub description: String,
    /// orchestrator | worker | reviewer
    pub role: String,
    pub model: String,
    pub color: String,
    /// GitHub labels routed straight to this agent (no triage needed).
    pub labels: Vec<String>,
    /// Allowed Claude Code tools (empty = Claude Code defaults).
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    #[serde(flatten)]
    pub meta: AgentMeta,
    pub prompt: String,
}

/// A cell is a reusable group of agents working on the same ticket.
///  - mode `delegate`: the lead runs with the members declared as Claude Code
///    subagents and delegates to them (one session, one worktree).
///  - mode `pipeline`: lead then members run one after another on the same
///    branch, each one receiving the previous summaries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CellMeta {
    pub name: String,
    pub description: String,
    pub lead: String,
    pub members: Vec<String>,
    /// delegate | pipeline
    pub mode: String,
    pub color: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    #[serde(flatten)]
    pub meta: CellMeta,
    /// Extra instructions given to the lead (or to every pipeline step).
    pub prompt: String,
}

/// Resolved set of agents that will work on one ticket.
#[derive(Debug, Clone)]
pub struct Team {
    pub name: String,
    pub lead: Agent,
    pub members: Vec<Agent>,
    pub pipeline: bool,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
}

// Defaults shipped inside the binary, written to the data dir on first start.
const DEFAULT_AGENTS: &[(&str, &str)] = &[
    ("orchestrator", include_str!("../agents/orchestrator.md")),
    ("engine", include_str!("../agents/engine.md")),
    ("gamedev", include_str!("../agents/gamedev.md")),
    ("gamedesign", include_str!("../agents/gamedesign.md")),
    ("devops", include_str!("../agents/devops.md")),
    ("reviewer", include_str!("../agents/reviewer.md")),
];
const DEFAULT_CELLS: &[(&str, &str)] = &[
    ("gameplay-squad", include_str!("../cells/gameplay-squad.md")),
    ("feature-pipeline", include_str!("../cells/feature-pipeline.md")),
];
const DEFAULT_SKILLS: &[(&str, &str)] = &[
    ("commit-conventions", include_str!("../skills/commit-conventions/SKILL.md")),
    ("game-design-doc", include_str!("../skills/game-design-doc/SKILL.md")),
    ("cpp-engine", include_str!("../skills/cpp-engine/SKILL.md")),
    ("unity-gameplay", include_str!("../skills/unity-gameplay/SKILL.md")),
    ("code-review", include_str!("../skills/code-review/SKILL.md")),
];

pub struct Library {
    pub root: PathBuf,
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn split_frontmatter(src: &str) -> (String, String) {
    let s = src.trim_start_matches('\u{feff}');
    if let Some(rest) = s.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = rest[..end].to_string();
            let body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
            return (fm, body);
        }
    }
    (String::new(), s.to_string())
}

pub fn parse_agent(src: &str) -> Result<Agent> {
    let (fm, body) = split_frontmatter(src);
    let meta: AgentMeta = if fm.trim().is_empty() { AgentMeta::default() } else { serde_yaml::from_str(&fm).context("invalid front-matter")? };
    if !valid_name(&meta.name) {
        bail!("agent `name` must be [a-zA-Z0-9_-]+");
    }
    Ok(Agent { meta, prompt: body })
}

pub fn parse_cell(src: &str) -> Result<Cell> {
    let (fm, body) = split_frontmatter(src);
    let meta: CellMeta = serde_yaml::from_str(&fm).context("invalid front-matter")?;
    if !valid_name(&meta.name) {
        bail!("cell `name` must be [a-zA-Z0-9_-]+");
    }
    if meta.lead.is_empty() && meta.members.is_empty() {
        bail!("a cell needs a `lead` and/or `members`");
    }
    Ok(Cell { meta, prompt: body })
}

pub fn render_cell(c: &Cell) -> String {
    let fm = serde_yaml::to_string(&c.meta).unwrap_or_default();
    format!("---\n{}---\n\n{}\n", fm, c.prompt.trim())
}

pub fn render_agent(a: &Agent) -> String {
    let fm = serde_yaml::to_string(&a.meta).unwrap_or_default();
    format!("---\n{}---\n\n{}\n", fm, a.prompt.trim())
}

impl Library {
    pub fn new(data: &Path) -> Result<Library> {
        let lib = Library { root: data.to_path_buf() };
        std::fs::create_dir_all(lib.agents_dir())?;
        std::fs::create_dir_all(lib.skills_dir())?;
        // cells were added after the first release: seed them once on upgrade
        if !lib.cells_dir().exists() {
            std::fs::create_dir_all(lib.cells_dir())?;
            if data.join(".seeded").exists() {
                for (n, c) in DEFAULT_CELLS {
                    std::fs::write(lib.cells_dir().join(format!("{n}.md")), c)?;
                }
            }
        }
        let seeded = data.join(".seeded");
        if !seeded.exists() {
            for (n, c) in DEFAULT_AGENTS {
                std::fs::write(lib.agents_dir().join(format!("{n}.md")), c)?;
            }
            std::fs::create_dir_all(lib.cells_dir())?;
            for (n, c) in DEFAULT_CELLS {
                std::fs::write(lib.cells_dir().join(format!("{n}.md")), c)?;
            }
            for (n, c) in DEFAULT_SKILLS {
                let d = lib.skills_dir().join(n);
                std::fs::create_dir_all(&d)?;
                std::fs::write(d.join("SKILL.md"), c)?;
            }
            std::fs::write(seeded, "1")?;
        }
        Ok(lib)
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.root.join("agents")
    }
    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }
    pub fn cells_dir(&self) -> PathBuf {
        self.root.join("cells")
    }

    pub fn cells(&self) -> Vec<Cell> {
        let mut v: Vec<Cell> = std::fs::read_dir(self.cells_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
                    .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                    .filter_map(|s| parse_cell(&s).ok())
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
        v
    }

    pub fn cell(&self, name: &str) -> Option<Cell> {
        self.cells().into_iter().find(|c| c.meta.name == name)
    }

    pub fn save_cell(&self, c: &Cell, old_name: Option<&str>) -> Result<()> {
        if self.agent(&c.meta.name).is_some() {
            bail!("an agent is already named '{}'", c.meta.name);
        }
        for m in std::iter::once(&c.meta.lead).chain(c.meta.members.iter()).filter(|m| !m.is_empty()) {
            if self.agent(m).is_none() {
                bail!("unknown agent '{m}' in cell");
            }
        }
        if let Some(o) = old_name {
            if o != c.meta.name && valid_name(o) {
                let _ = std::fs::remove_file(self.cells_dir().join(format!("{o}.md")));
            }
        }
        std::fs::create_dir_all(self.cells_dir())?;
        std::fs::write(self.cells_dir().join(format!("{}.md", c.meta.name)), render_cell(c))?;
        Ok(())
    }

    pub fn delete_cell(&self, name: &str) -> Result<()> {
        if !valid_name(name) {
            bail!("invalid name");
        }
        std::fs::remove_file(self.cells_dir().join(format!("{name}.md")))?;
        Ok(())
    }

    /// Is `name` something a ticket can be assigned to (worker agent or cell)?
    pub fn is_assignable(&self, name: &str) -> bool {
        self.workers().iter().any(|a| a.meta.name == name) || self.cell(name).is_some()
    }

    /// Resolve an assignee (agent or cell) plus optional helper agents into a team.
    pub fn team(&self, assignee: &str, helpers: &[String]) -> Result<Team> {
        let get = |n: &str| self.agent(n).ok_or_else(|| anyhow::anyhow!("agent '{n}' not found"));
        let mut extra: Vec<Agent> = vec![];
        let (name, lead, mut members, pipeline, instructions) = if let Some(c) = self.cell(assignee) {
            let mut order: Vec<String> = vec![];
            if !c.meta.lead.is_empty() {
                order.push(c.meta.lead.clone());
            }
            order.extend(c.meta.members.iter().cloned());
            let lead = get(&order[0])?;
            let members = order[1..].iter().map(|m| get(m)).collect::<Result<Vec<_>>>()?;
            (c.meta.name.clone(), lead, members, c.meta.mode == "pipeline", c.prompt.clone())
        } else {
            (assignee.to_string(), get(assignee)?, vec![], false, String::new())
        };
        for h in helpers {
            if h != &lead.meta.name && !members.iter().any(|m| &m.meta.name == h) {
                extra.push(get(h)?);
            }
        }
        members.extend(extra);
        Ok(Team { name, lead, members, pipeline, instructions })
    }

    pub fn agents(&self) -> Vec<Agent> {
        let mut v: Vec<Agent> = std::fs::read_dir(self.agents_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
                    .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                    .filter_map(|s| parse_agent(&s).ok())
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|a| (role_rank(&a.meta.role), a.meta.name.clone()));
        v
    }

    pub fn agent(&self, name: &str) -> Option<Agent> {
        self.agents().into_iter().find(|a| a.meta.name == name)
    }

    pub fn workers(&self) -> Vec<Agent> {
        self.agents().into_iter().filter(|a| a.meta.role.is_empty() || a.meta.role == "worker").collect()
    }

    pub fn save_agent(&self, a: &Agent, old_name: Option<&str>) -> Result<()> {
        if !valid_name(&a.meta.name) {
            bail!("invalid agent name");
        }
        if self.cell(&a.meta.name).is_some() {
            bail!("a cell is already named '{}'", a.meta.name);
        }
        if let Some(o) = old_name {
            if o != a.meta.name && valid_name(o) {
                let _ = std::fs::remove_file(self.agents_dir().join(format!("{o}.md")));
            }
        }
        std::fs::write(self.agents_dir().join(format!("{}.md", a.meta.name)), render_agent(a))?;
        Ok(())
    }

    pub fn delete_agent(&self, name: &str) -> Result<()> {
        if !valid_name(name) {
            bail!("invalid name");
        }
        std::fs::remove_file(self.agents_dir().join(format!("{name}.md")))?;
        Ok(())
    }

    pub fn skills(&self) -> Vec<Skill> {
        let mut v: Vec<Skill> = std::fs::read_dir(self.skills_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        let content = std::fs::read_to_string(e.path().join("SKILL.md")).ok()?;
                        let (fm, _) = split_frontmatter(&content);
                        let desc = serde_yaml::from_str::<serde_yaml::Value>(&fm)
                            .ok()
                            .and_then(|v| v.get("description").and_then(|d| d.as_str()).map(String::from))
                            .unwrap_or_default();
                        Some(Skill { name, description: desc, content })
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn save_skill(&self, name: &str, content: &str) -> Result<()> {
        if !valid_name(name) {
            bail!("invalid skill name");
        }
        let d = self.skills_dir().join(name);
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join("SKILL.md"), content)?;
        Ok(())
    }

    pub fn delete_skill(&self, name: &str) -> Result<()> {
        if !valid_name(name) {
            bail!("invalid name");
        }
        std::fs::remove_dir_all(self.skills_dir().join(name))?;
        Ok(())
    }

    /// Build `<data>/plugins/<agent>` = a Claude Code plugin exposing the
    /// agent's skills. Returns None if the agent has no skills.
    pub fn build_plugin(&self, a: &Agent) -> Result<Option<PathBuf>> {
        self.build_plugin_for(&a.meta.name, &a.meta.skills)
    }

    /// Plugin for a delegate team: lead + members skills (subagents share the session).
    pub fn build_team_plugin(&self, t: &Team) -> Result<Option<PathBuf>> {
        let mut skills: Vec<String> = t.lead.meta.skills.clone();
        for m in &t.members {
            for s in &m.meta.skills {
                if !skills.contains(s) {
                    skills.push(s.clone());
                }
            }
        }
        self.build_plugin_for(&format!("team-{}", t.name), &skills)
    }

    fn build_plugin_for(&self, name: &str, skills: &[String]) -> Result<Option<PathBuf>> {
        if skills.is_empty() {
            return Ok(None);
        }
        let dir = self.root.join("plugins").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude-plugin"))?;
        std::fs::write(
            dir.join(".claude-plugin/plugin.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "name": format!("maestro-{name}"),
                "version": "1.0.0",
                "description": format!("Skills for Maestro '{name}'")
            }))?,
        )?;
        for s in skills {
            let src = self.skills_dir().join(s);
            if src.is_dir() {
                copy_dir(&src, &dir.join("skills").join(s))?;
            }
        }
        Ok(Some(dir))
    }
}

fn role_rank(r: &str) -> u8 {
    match r {
        "orchestrator" => 0,
        "reviewer" => 2,
        _ => 1,
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &dst.join(e.file_name()))?;
        } else {
            std::fs::copy(&p, dst.join(e.file_name()))?;
        }
    }
    Ok(())
}
