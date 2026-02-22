use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::loom::{self, TurnResult};
use crate::meta::Meta;
use crate::snapshot;

/// Options for running a turn.
pub struct RunOpts<'a> {
    pub agent: &'a str,
    pub nudge: Option<&'a str>,
    pub source: Option<&'a str>,
    pub system: Option<&'a str>,
    pub model: Option<&'a str>,
    pub stage: Option<&'a str>,
    pub timeout: Option<u64>,
}

/// High-level thread handle. THE public API for loom consumers.
pub struct Thread<'a> {
    config: &'a Config,
    name: String,
}

impl<'a> Thread<'a> {
    pub fn new(config: &'a Config, name: &str) -> Self {
        Self {
            config,
            name: name.to_string(),
        }
    }

    /// Run a turn with minimal args. Returns the turn result.
    pub fn run(&self, agent: &str, nudge: Option<&str>) -> Result<TurnResult> {
        loom::run_turn(
            self.config,
            &self.name,
            agent,
            None,
            nudge,
            None,
            None,
            None,
            None,
        )
    }

    /// Run a turn with full options.
    pub fn run_opts(&self, opts: &RunOpts) -> Result<TurnResult> {
        loom::run_turn(
            self.config,
            &self.name,
            opts.agent,
            opts.source,
            opts.nudge,
            opts.model,
            opts.system,
            opts.stage,
            opts.timeout,
        )
    }

    /// Number of completed turns.
    pub fn step(&self) -> Result<usize> {
        let meta = Meta::read(self.config, &self.name)?;
        Ok(meta.thread.len())
    }

    /// Read .last_output content.
    pub fn last_output(&self) -> Result<String> {
        let path = Meta::run_dir(self.config, &self.name).join(".last_output");
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    }

    /// Read a specific turn's output.
    pub fn read_turn(&self, turn_num: Option<usize>, input: bool) -> Result<String> {
        let meta = Meta::read(self.config, &self.name)?;
        let turn = match turn_num {
            Some(n) => meta
                .thread
                .iter()
                .find(|t| t.turn == n)
                .ok_or_else(|| anyhow::anyhow!("turn {} not found", n))?,
            None => meta
                .thread
                .last()
                .ok_or_else(|| anyhow::anyhow!("no turns yet"))?,
        };
        let path = if input {
            Meta::run_dir(self.config, &self.name)
                .join(format!("turn-{}-{}.input.md", turn.turn, turn.agent))
        } else {
            Meta::turn_file(self.config, &self.name, turn.turn, &turn.agent)
        };
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    }

    /// Rewind to snapshot N.
    pub fn rewind(&self, n: &str) -> Result<()> {
        snapshot::rewind(self.config, &self.name, n)
    }

    /// Delete thread entirely.
    pub fn delete(&self) -> Result<()> {
        let run_dir = Meta::run_dir(self.config, &self.name);
        if !run_dir.exists() {
            bail!("thread '{}' not found", self.name);
        }
        let meta = Meta::read(self.config, &self.name)?;
        snapshot::cleanup_sessions(self.config, &meta)?;
        std::fs::remove_dir_all(&run_dir)?;
        Ok(())
    }

    /// Reset thread (clear turns, keep name).
    pub fn reset(&self) -> Result<()> {
        let run_dir = Meta::run_dir(self.config, &self.name);
        if !run_dir.exists() {
            bail!("thread '{}' not found", self.name);
        }
        let meta = Meta::read(self.config, &self.name)?;
        snapshot::cleanup_sessions(self.config, &meta)?;
        let new_meta = Meta::new(&self.name, meta.snapshots);
        for entry in std::fs::read_dir(&run_dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                std::fs::remove_dir_all(&p)?;
            } else {
                std::fs::remove_file(&p)?;
            }
        }
        new_meta.save(self.config)?;
        Ok(())
    }

    /// Show thread summary. Returns formatted string.
    pub fn show(&self) -> Result<String> {
        let meta = Meta::read(self.config, &self.name)?;
        let mut lines = Vec::new();

        if meta.thread.is_empty() && meta.running.is_none() {
            lines.push(format!("{} (no turns yet)", self.name));
            return Ok(lines.join("\n"));
        }

        let agents: Vec<_> = meta.agents.keys().collect();
        lines.push(format!(
            "{} ({} turns, {} agents: {})",
            self.name,
            meta.thread.len(),
            agents.len(),
            agents
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        let mut total_cost = 0.0;
        let mut total_elapsed = 0.0;
        for turn in &meta.thread {
            let stage = turn
                .stage
                .as_ref()
                .map(|s| format!(" ({})", s))
                .unwrap_or_default();
            let source = turn
                .source
                .as_ref()
                .map(|s| format!(" < {}", s))
                .unwrap_or_default();
            let nudge = turn
                .nudge
                .as_ref()
                .map(|n| {
                    let preview = if n.len() > 40 { &n[..40] } else { n };
                    format!(" \"{}\"", preview)
                })
                .unwrap_or_default();
            let cost = turn
                .cost_usd
                .map(|c| format!(" ${:.3}", c))
                .unwrap_or_default();
            let model = turn
                .model
                .as_ref()
                .map(|m| format!(" {}", m))
                .unwrap_or_default();
            lines.push(format!(
                "  [{}] {}{}{}{}{} ({}, {} chars{})",
                turn.turn,
                turn.agent,
                model,
                stage,
                source,
                nudge,
                fmt_duration(turn.elapsed_s),
                turn.chars,
                cost
            ));
            total_cost += turn.cost_usd.unwrap_or(0.0);
            total_elapsed += turn.elapsed_s;
        }

        if let Some(ref r) = meta.running {
            let elapsed = elapsed_since(&r.started_at);
            lines.push(format!(
                "  [{}] {} {} ▶ running ({})",
                r.turn,
                r.agent,
                r.model,
                fmt_duration(elapsed)
            ));
        }

        if !meta.thread.is_empty() {
            lines.push("  ────────────────────────".to_string());
            lines.push(format!(
                "  total: {}, ${:.3}",
                fmt_duration(total_elapsed),
                total_cost
            ));
        }

        Ok(lines.join("\n"))
    }
}

/// List all threads. Returns formatted string.
pub fn list_threads(config: &Config) -> Result<String> {
    let runs_dir = config.runs_dir();
    if !runs_dir.exists() {
        return Ok("no threads yet".to_string());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&runs_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.is_empty() {
        return Ok("no threads yet".to_string());
    }

    entries.sort_by(|a, b| {
        let ta = a.metadata().and_then(|m| m.modified()).ok();
        let tb = b.metadata().and_then(|m| m.modified()).ok();
        tb.cmp(&ta)
    });

    let mut lines = Vec::new();
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        match Meta::read(config, &name) {
            Ok(meta) => {
                let agents: Vec<_> = meta.agents.keys().map(|s| s.as_str()).collect();
                let cost: f64 = meta.thread.iter().filter_map(|t| t.cost_usd).sum();
                let elapsed: f64 = meta.thread.iter().map(|t| t.elapsed_s).sum();
                let running = if let Some(ref r) = meta.running {
                    let r_elapsed = elapsed_since(&r.started_at);
                    format!(" ▶ {} {} ({})", r.agent, r.model, fmt_duration(r_elapsed))
                } else {
                    String::new()
                };
                if meta.thread.is_empty() && meta.running.is_some() {
                    lines.push(format!("  {} --{}", name, running));
                } else if meta.thread.is_empty() {
                    lines.push(format!("  {} -- (no turns)", name));
                } else {
                    lines.push(format!(
                        "  {} -- {} turns, {} {}, ${:.3}{}",
                        name,
                        meta.thread.len(),
                        agents.join(", "),
                        fmt_duration(elapsed),
                        cost,
                        running
                    ));
                }
            }
            Err(_) => {
                lines.push(format!("  {} -- (corrupt meta)", name));
            }
        }
    }
    Ok(lines.join("\n"))
}

fn elapsed_since(started_at: &str) -> f64 {
    use chrono::Local;
    chrono::NaiveDateTime::parse_from_str(started_at, "%Y-%m-%dT%H:%M:%S")
        .map(|start| {
            let now = Local::now().naive_local();
            (now - start).num_seconds() as f64
        })
        .unwrap_or(0.0)
}

fn fmt_duration(secs: f64) -> String {
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}h{}m{}s", h, m, s)
    } else if m > 0 {
        format!("{}m{}s", m, s)
    } else {
        format!("{}s", s)
    }
}
