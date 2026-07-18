use anyhow::{bail, Context, Result};
use std::time::Instant;

use crate::backend::{self, TurnRequest};
use crate::config::Config;
use crate::meta::{Meta, RunningTurn, Turn};
use crate::prompt;
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

/// Result of a completed turn.
pub struct TurnResult {
    pub output: String,
    pub session_id: String,
    pub elapsed_s: f64,
    pub cost_usd: Option<f64>,
}

const SCRATCHPAD_FILES: &[(&str, &str)] = &[
    ("RESEARCH.md", "# Research\n\n"),
    ("EVIDENCE.md", "# Evidence\n\n"),
    ("THESES.md", "# Theses\n\n"),
    ("NOTES.md", "# Notes\n\n"),
];

fn init_scratchpad(config: &Config, name: &str) -> Result<()> {
    let dir = Meta::run_dir(config, name).join("scratchpad");
    std::fs::create_dir_all(&dir)?;
    for (fname, template) in SCRATCHPAD_FILES {
        let p = dir.join(fname);
        if !p.exists() {
            std::fs::write(&p, template)?;
        }
    }
    Ok(())
}

pub fn run_turn(config: &Config, name: &str, opts: &RunOpts) -> Result<TurnResult> {
    let speaker = opts.agent;
    let source = opts.source;
    let nudge = opts.nudge;
    let system_override = opts.system;

    // Require agent exists
    config.require_agent(speaker)?;
    if let Some(sys_agent) = system_override {
        config.require_agent(sys_agent)?;
    }

    let run_dir = Meta::run_dir(config, name);
    std::fs::create_dir_all(&run_dir)?;
    init_scratchpad(config, name)?;

    let mut meta = Meta::read_or_create(config, name, config.snapshots)?;
    meta.name = name.to_string();
    meta.save(config)?;
    let agent = meta.ensure_agent(speaker).clone();

    // Model: override > agent config > config default
    let model = opts
        .model
        .map(String::from)
        .unwrap_or_else(|| config.agent_model(speaker));

    let msg = build_message(config, name, &meta, speaker, source, nudge)?;
    if msg.is_empty() {
        bail!("empty message — provide a nudge or source agent\n\n  loom t {} {} \"message\"\n  loom t {} {} hears <agent>", name, speaker, name, speaker);
    }

    let turn_n = meta.next_turn_number();
    let timeout = opts.timeout.unwrap_or(config.timeout);

    // Mark turn as running
    meta.running = Some(RunningTurn {
        turn: turn_n,
        agent: speaker.to_string(),
        model: model.clone(),
        started_at: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    meta.save(config)?;

    // Build system prompt: --use-system agent > speaker agent
    let scratchpad_dir = run_dir.join("scratchpad");
    let prompt_agent = system_override.unwrap_or(speaker);
    let sys_prompt =
        prompt::build_system_prompt(config, config.agent(prompt_agent), &scratchpad_dir);

    let backend = backend::for_config(config)?;

    let t0 = Instant::now();
    let active_session = agent.session_id.as_deref().filter(|s| !s.is_empty());

    eprintln!(
        "[loom {}] turn {}: backend={} agent={} model={} system={} {} timeout={}s sys_prompt={} chars msg={} chars",
        crate::version(),
        turn_n,
        backend.name(),
        speaker,
        model,
        system_override.unwrap_or("(self)"),
        if let Some(sid) = active_session { format!("RESUME session={}", sid) } else { "NEW".to_string() },
        timeout,
        sys_prompt.len(),
        msg.len(),
    );

    // Save system prompt on first turn for this agent.
    if active_session.is_none() {
        let sys_file = run_dir.join(format!("{}.system.md", speaker));
        std::fs::write(&sys_file, &sys_prompt)?;
    }

    let result = backend.run_turn(
        config,
        &TurnRequest {
            agent: speaker,
            message: &msg,
            model: &model,
            system_prompt: &sys_prompt,
            session_id: active_session,
            timeout_secs: timeout,
        },
    )?;
    let elapsed = t0.elapsed().as_secs_f64();

    // Update state
    meta.ensure_agent(speaker).session_id = Some(result.session_id.clone());
    let input_file = run_dir.join(format!("turn-{}-{}.input.md", turn_n, speaker));
    std::fs::write(&input_file, &msg)?;
    let turn_file = Meta::turn_file(config, name, turn_n, speaker);
    std::fs::write(&turn_file, &result.output)?;

    // Write .last_output for script consumption
    let last_output = run_dir.join(".last_output");
    std::fs::copy(&turn_file, &last_output)?;

    meta.running = None;
    meta.thread.push(Turn {
        turn: turn_n,
        agent: speaker.to_string(),
        source: source.map(String::from),
        nudge: nudge.map(String::from),
        stage: opts.stage.map(String::from),
        model: Some(model.clone()),
        chars: result.output.len(),
        elapsed_s: (elapsed * 10.0).round() / 10.0,
        cost_usd: result.cost_usd,
    });
    meta.save(config)?;

    if meta.snapshots {
        snapshot::auto_snapshot(config, name, &meta)?;
    }

    Ok(TurnResult {
        output: result.output,
        session_id: result.session_id,
        elapsed_s: (elapsed * 10.0).round() / 10.0,
        cost_usd: result.cost_usd,
    })
}

pub fn build_message(
    config: &Config,
    name: &str,
    meta: &Meta,
    speaker: &str,
    source: Option<&str>,
    nudge: Option<&str>,
) -> Result<String> {
    let mut parts = Vec::new();

    match source {
        Some("all") => {
            parts.push(get_full_thread(config, name, meta, Some(speaker))?);
        }
        Some(agent) => {
            parts.push(get_last_output(config, name, meta, agent)?);
        }
        None => {}
    }

    if let Some(text) = nudge {
        if !parts.is_empty() {
            parts.push("---".to_string());
        }
        parts.push(text.to_string());
    }

    Ok(parts.join("\n\n"))
}

fn get_last_output(config: &Config, name: &str, meta: &Meta, agent: &str) -> Result<String> {
    let turn = meta
        .last_turn_for_agent(agent)
        .ok_or_else(|| anyhow::anyhow!("no output from agent '{}'", agent))?;

    let path = Meta::turn_file(config, name, turn.turn, agent);
    std::fs::read_to_string(&path)
        .with_context(|| format!("reading turn output: {}", path.display()))
}

fn get_full_thread(
    config: &Config,
    name: &str,
    meta: &Meta,
    exclude: Option<&str>,
) -> Result<String> {
    let mut parts = Vec::new();
    for turn in &meta.thread {
        if exclude == Some(turn.agent.as_str()) {
            continue;
        }
        // Include nudge as user direction
        if let Some(ref nudge) = turn.nudge {
            parts.push(format!("[user → {}]: {}", turn.agent, nudge));
        }
        let path = Meta::turn_file(config, name, turn.turn, &turn.agent);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            parts.push(format!("[{}]:\n{}", turn.agent, content));
        }
    }
    Ok(parts.join("\n\n---\n\n"))
}
