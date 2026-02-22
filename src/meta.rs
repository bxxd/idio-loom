use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{self, Config};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningTurn {
    pub turn: usize,
    pub agent: String,
    pub model: String,
    pub started_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
    pub name: String,
    #[serde(default = "config::default_true")]
    pub snapshots: bool,
    #[serde(default)]
    pub agents: HashMap<String, AgentState>,
    #[serde(default)]
    pub thread: Vec<Turn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<RunningTurn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Turn {
    pub turn: usize,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nudge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub chars: usize,
    pub elapsed_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl Meta {
    pub fn new(name: &str, snapshots: bool) -> Self {
        Self {
            name: name.to_string(),
            snapshots,
            agents: HashMap::new(),
            thread: Vec::new(),
            running: None,
        }
    }

    pub fn run_dir(config: &Config, name: &str) -> PathBuf {
        config.runs_dir().join(name)
    }

    pub fn meta_path(config: &Config, name: &str) -> PathBuf {
        Self::run_dir(config, name).join("meta.json")
    }

    pub fn read(config: &Config, name: &str) -> Result<Self> {
        let path = Self::meta_path(config, name);
        if !path.exists() {
            anyhow::bail!("thread '{}' not found", name);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let meta: Meta =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(meta)
    }

    /// Read meta or create new if thread doesn't exist yet (for `do`)
    pub fn read_or_create(config: &Config, name: &str, snapshots: bool) -> Result<Self> {
        if !Self::meta_path(config, name).exists() {
            return Ok(Meta::new(name, snapshots));
        }
        Self::read(config, name)
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        let dir = Self::run_dir(config, &self.name);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("meta.json");
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    pub fn ensure_agent(&mut self, name: &str) -> &mut AgentState {
        if !self.agents.contains_key(name) {
            self.agents
                .insert(name.to_string(), AgentState { session_id: None });
        }
        self.agents.get_mut(name).unwrap()
    }

    pub fn next_turn_number(&self) -> usize {
        self.thread.len()
    }

    pub fn turn_file(config: &Config, name: &str, turn: usize, agent: &str) -> PathBuf {
        Self::run_dir(config, name).join(format!("turn-{}-{}.md", turn, agent))
    }

    pub fn last_turn_for_agent(&self, agent: &str) -> Option<&Turn> {
        self.thread.iter().rev().find(|t| t.agent == agent)
    }
}
