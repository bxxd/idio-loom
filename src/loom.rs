use anyhow::{bail, Context, Result};
use std::time::Instant;

use crate::claude;
use crate::config::Config;
use crate::meta::{Meta, Turn};
use crate::snapshot;

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

pub fn run_turn(
    config: &Config,
    name: &str,
    speaker: &str,
    source: Option<&str>,
    nudge: Option<&str>,
    model_override: Option<&str>,
    stage_name: Option<&str>,
) -> Result<()> {
    // Require agent exists
    config.require_agent(speaker)?;

    let run_dir = Meta::run_dir(config, name);
    std::fs::create_dir_all(&run_dir)?;
    init_scratchpad(config, name)?;

    let mut meta = Meta::read(config, name)?;
    meta.name = name.to_string();
    let agent = meta.ensure_agent(speaker).clone();

    // Model: override > agent config > config default
    let model = model_override
        .map(String::from)
        .unwrap_or_else(|| config.agent_model(speaker));

    let msg = build_message(config, name, &meta, speaker, source, nudge)?;
    if msg.is_empty() {
        bail!("empty message — provide a nudge or source agent\n\n  loom t {} {} \"message\"\n  loom t {} {} hears <agent>", name, speaker, name, speaker);
    }

    let turn_n = meta.next_turn_number();
    print_turn_start(turn_n, stage_name, speaker, source, nudge, &model);

    let t0 = Instant::now();
    let result = if let Some(ref sid) = agent.session_id {
        claude::claude_resume(sid, &msg, config.timeout)?
    } else {
        let scratchpad_dir = run_dir.join("scratchpad");
        let sys_prompt = claude::build_system_prompt(config, config.agent(speaker), &scratchpad_dir);
        // Save system prompt (only exists on first turn for this agent)
        let sys_file = run_dir.join(format!("{}.system.md", speaker));
        std::fs::write(&sys_file, &sys_prompt)?;
        claude::claude_new(&msg, &model, &sys_prompt, config.timeout)?
    };
    let elapsed = t0.elapsed().as_secs_f64();

    // Update state
    meta.agents.get_mut(speaker).unwrap().session_id = Some(result.session_id.clone());
    let input_file = run_dir.join(format!("turn-{}-{}.input.md", turn_n, speaker));
    std::fs::write(&input_file, &msg)?;
    let turn_file = Meta::turn_file(config, name, turn_n, speaker);
    std::fs::write(&turn_file, &result.result)?;

    meta.thread.push(Turn {
        turn: turn_n,
        agent: speaker.to_string(),
        source: source.map(String::from),
        nudge: nudge.map(String::from),
        stage: stage_name.map(String::from),
        chars: result.result.len(),
        elapsed_s: (elapsed * 10.0).round() / 10.0,
        cost_usd: result.cost_usd,
    });
    meta.save(config)?;

    print_turn_done(elapsed, result.result.len(), result.cost_usd);
    println!("{}", result.result);

    if meta.snapshots {
        snapshot::auto_snapshot(config, name, &meta)?;
    }

    Ok(())
}

fn print_turn_start(turn_n: usize, stage: Option<&str>, speaker: &str, source: Option<&str>, nudge: Option<&str>, model: &str) {
    eprint!("[turn {}] ", turn_n);
    if let Some(sn) = stage {
        eprint!("[{}] ", sn);
    }
    eprint!("{} ", speaker);
    if let Some(src) = source {
        eprint!("hears {} ", src);
    }
    if let Some(n) = nudge {
        let preview = if n.len() > 60 { &n[..60] } else { n };
        eprint!("\"{}\" ", preview);
    }
    eprintln!("({}) ...", model);
}

fn print_turn_done(elapsed: f64, chars: usize, cost: Option<f64>) {
    let now = chrono::Local::now().format("%H:%M:%S");
    eprintln!("  {:.0}s, {} chars, ${:.3} — done at {}", elapsed, chars, cost.unwrap_or(0.0), now);
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

fn get_full_thread(config: &Config, name: &str, meta: &Meta, exclude: Option<&str>) -> Result<String> {
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
