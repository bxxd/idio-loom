use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::claude;
use crate::config::Config;
use crate::loom;
use crate::meta::Meta;

#[derive(Debug, Deserialize)]
pub struct Pattern {
    pub name: Option<String>,
    /// Override system prompt for this pattern
    pub system_prompt: Option<String>,
    pub stages: Vec<Stage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stage {
    pub name: String,
    pub agent: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub nudge: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

pub fn load_pattern(path: &str) -> Result<Pattern> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading pattern: {}", path))?;
    let pattern: Pattern = serde_yaml::from_str(&text)
        .with_context(|| format!("parsing pattern: {}", path))?;
    if pattern.stages.is_empty() {
        bail!("pattern has no stages");
    }
    Ok(pattern)
}

/// Apply pattern overrides to config
pub fn apply_overrides(config: &mut Config, pattern: &Pattern) -> Result<()> {
    if let Some(ref sp) = pattern.system_prompt {
        config.system_prompt = sp.clone();
    }
    Ok(())
}

/// Run the full pattern from current stage to end
pub fn run_all(
    config: &Config,
    meta: &mut Meta,
    pattern: &Pattern,
    model_override: Option<&str>,
) -> Result<()> {
    let start = meta.current_stage();

    for i in start..pattern.stages.len() {
        let stage = &pattern.stages[i];
        eprintln!("\n── stage {}/{}: {} ──", i + 1, pattern.stages.len(), stage.name);
        run_stage(config, &meta.name.clone(), stage, model_override)?;
        reload_and_advance(config, meta)?;
    }

    eprintln!("\npattern complete.");
    Ok(())
}

/// Run the next unexecuted stage
pub fn run_next(
    config: &Config,
    meta: &mut Meta,
    pattern: &Pattern,
    model_override: Option<&str>,
) -> Result<()> {
    let stage_idx = meta.current_stage();

    if stage_idx >= pattern.stages.len() {
        eprintln!("all {} stages complete", pattern.stages.len());
        return Ok(());
    }

    let stage = &pattern.stages[stage_idx];
    eprintln!("── stage {}/{}: {} ──", stage_idx + 1, pattern.stages.len(), stage.name);
    run_stage(config, &meta.name.clone(), stage, model_override)?;
    reload_and_advance(config, meta)?;

    Ok(())
}

fn run_stage(
    config: &Config,
    thread_name: &str,
    stage: &Stage,
    model_override: Option<&str>,
) -> Result<()> {
    let model = model_override.or(stage.model.as_deref());
    let nudge = resolve_nudge(stage.nudge.as_deref(), config);
    loom::run_turn(
        config,
        thread_name,
        &stage.agent,
        stage.source.as_deref(),
        nudge.as_deref(),
        model,
        Some(&stage.name),
    )
}

fn resolve_nudge(nudge: Option<&str>, config: &Config) -> Option<String> {
    nudge.map(|text| claude::resolve_at_refs(text, &config.patterns_dir()))
}

/// Reload meta after run_turn saved it, advance stage counter
fn reload_and_advance(config: &Config, meta: &mut Meta) -> Result<()> {
    *meta = Meta::read(config, &meta.name)?;
    meta.advance_stage();
    meta.save(config)
}
