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

pub fn render_agent(a: &Agent) -> String {
    let fm = serde_yaml::to_string(&a.meta).unwrap_or_default();
    format!("---\n{}---\n\n{}\n", fm, a.prompt.trim())
}

impl Library {
    pub fn new(data: &Path) -> Result<Library> {
        let lib = Library { root: data.to_path_buf() };
        std::fs::create_dir_all(lib.agents_dir())?;
        std::fs::create_dir_all(lib.skills_dir())?;
        let seeded = data.join(".seeded");
        if !seeded.exists() {
            for (n, c) in DEFAULT_AGENTS {
                std::fs::write(lib.agents_dir().join(format!("{n}.md")), c)?;
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
        if a.meta.skills.is_empty() {
            return Ok(None);
        }
        let dir = self.root.join("plugins").join(&a.meta.name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude-plugin"))?;
        std::fs::write(
            dir.join(".claude-plugin/plugin.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "name": format!("maestro-{}", a.meta.name),
                "version": "1.0.0",
                "description": format!("Skills for the Maestro agent '{}'", a.meta.name)
            }))?,
        )?;
        for s in &a.meta.skills {
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
