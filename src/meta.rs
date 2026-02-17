use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;

fn default_true() -> bool { true }

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_stage: Option<usize>,
    #[serde(default = "default_true")]
    pub snapshots: bool,
    #[serde(default)]
    pub agents: HashMap<String, AgentState>,
    #[serde(default)]
    pub thread: Vec<Turn>,
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
    pub chars: usize,
    pub elapsed_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl Meta {
    pub fn new(name: &str, snapshots: bool) -> Self {
        Self {
            name: name.to_string(),
            pattern: None,
            pattern_stage: None,
            snapshots,
            agents: HashMap::new(),
            thread: Vec::new(),
        }
    }

    pub fn init_pattern(name: &str, pattern_file: &str, snapshots: bool) -> Self {
        Self {
            name: name.to_string(),
            pattern: Some(pattern_file.to_string()),
            pattern_stage: Some(0),
            snapshots,
            agents: HashMap::new(),
            thread: Vec::new(),
        }
    }

    pub fn advance_stage(&mut self) {
        if let Some(ref mut stage) = self.pattern_stage {
            *stage += 1;
        }
    }

    pub fn current_stage(&self) -> usize {
        self.pattern_stage.unwrap_or(0)
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
            return Ok(Meta::new(name, true));
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let meta: Meta = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(meta)
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
            self.agents.insert(
                name.to_string(),
                AgentState { session_id: None },
            );
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
