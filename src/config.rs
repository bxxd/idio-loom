use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn default_model() -> String {
    "sonnet".into()
}
fn default_timeout() -> u64 {
    900
}
fn default_workshop() -> String {
    ".".into()
}
fn default_state() -> String {
    ".loom".into()
}
pub(crate) fn default_true() -> bool {
    true
}

/// Root config from loom.yaml
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default = "default_workshop")]
    pub workshop: String,
    #[serde(default = "default_state")]
    pub state: String,
    #[serde(default = "default_true")]
    pub snapshots: bool,
    #[serde(default)]
    pub system_prompt: String,
    /// Working directory for claude subprocess (absolute or relative to loom.yaml dir).
    /// When set, claude runs from this directory (picks up CLAUDE.md, .mcp.json, etc).
    /// Loom's own state/agents/patterns remain relative to loom.yaml location.
    pub cwd: Option<String>,

    /// Directory containing loom.yaml (set after load, not serialized)
    #[serde(skip)]
    pub home: PathBuf,
    /// Populated after load by scanning agents/
    #[serde(skip)]
    pub agents_map: HashMap<String, Agent>,
}

/// An agent definition — name, model, system prompt.
#[derive(Debug, Clone, Deserialize)]
pub struct Agent {
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: String,
}

impl Config {
    /// Load config from loom.yaml. Looks in dir_override, else cwd.
    pub fn load(dir_override: Option<&str>) -> Result<Self> {
        let mut config = Self::load_root(dir_override)?;
        config.load_agents()?;
        Ok(config)
    }

    fn load_root(dir_override: Option<&str>) -> Result<Self> {
        let search_dir = match dir_override {
            Some(d) => PathBuf::from(d),
            None => std::env::current_dir()?,
        };

        let p = search_dir.join("loom.yaml");
        if !p.exists() {
            bail!("no loom.yaml found in {}", search_dir.display());
        }
        let text =
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        let mut config: Config =
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
        config.home = search_dir.canonicalize().unwrap_or(search_dir);
        Ok(config)
    }

    /// Resolve a path: absolute stays as-is, relative joins to home.
    fn resolve_path(&self, p: &str) -> PathBuf {
        let path = Path::new(p);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.home.join(path)
        }
    }

    /// Agents directory
    pub fn agents_dir(&self) -> PathBuf {
        self.workshop_dir().join("agents")
    }

    /// Load all agents from individual YAML files in agents/
    pub fn load_agents(&mut self) -> Result<()> {
        let dir = self.agents_dir();
        if !dir.exists() {
            return Ok(());
        }
        self.agents_map.clear();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".yaml") && !name.ends_with(".yml") {
                continue;
            }
            let agent_name = name
                .trim_end_matches(".yaml")
                .trim_end_matches(".yml")
                .to_string();
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading agent: {}", path.display()))?;
            let agent: Agent = serde_yaml::from_str(&text)
                .with_context(|| format!("parsing agent: {}", path.display()))?;
            self.agents_map.insert(agent_name, agent);
        }
        Ok(())
    }

    /// Workshop directory — where content lives (agents, patterns)
    pub fn workshop_dir(&self) -> PathBuf {
        self.resolve_path(&self.workshop)
    }

    /// Patterns directory
    pub fn patterns_dir(&self) -> PathBuf {
        self.workshop_dir().join("patterns")
    }

    /// State directory — resolved from `state` field (absolute or relative to home)
    pub fn state_dir(&self) -> PathBuf {
        self.resolve_path(&self.state)
    }

    /// Runs directory — {state}/runs/
    pub fn runs_dir(&self) -> PathBuf {
        self.state_dir().join("runs")
    }

    /// Resolved cwd for claude subprocess. Canonicalized to absolute path.
    /// Defaults to loom.yaml directory when not set — one path, always explicit.
    pub fn claude_cwd(&self) -> PathBuf {
        let resolved = match self.cwd.as_ref() {
            Some(d) => self.resolve_path(d),
            None => self.home.clone(),
        };
        // Canonicalize to get the true absolute path (resolves .., symlinks)
        // so claude's project hash matches what we expect
        resolved.canonicalize().unwrap_or(resolved)
    }

    pub fn agent_model(&self, name: &str) -> String {
        self.agents_map
            .get(name)
            .and_then(|a| a.model.clone())
            .unwrap_or_else(|| self.model.clone())
    }

    pub fn agent(&self, name: &str) -> Option<&Agent> {
        self.agents_map.get(name)
    }

    pub fn require_agent(&self, name: &str) -> Result<&Agent> {
        self.agents_map
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("agent '{}' not found — check agents/ directory", name))
    }
}
