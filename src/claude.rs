use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use crate::config::{Config, Agent};

#[derive(Debug)]
#[allow(dead_code)]
pub struct ClaudeResult {
    pub session_id: String,
    pub result: String,
    pub duration_ms: u64,
    pub cost_usd: Option<f64>,
}

#[derive(Deserialize)]
struct ClaudeJson {
    session_id: Option<String>,
    result: Option<String>,
    duration_ms: Option<u64>,
    total_cost_usd: Option<f64>,
    is_error: Option<bool>,
}

fn find_claude() -> String {
    let local = dirs::home_dir()
        .map(|h| h.join(".local/bin/claude"))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string());
    local.unwrap_or_else(|| "claude".to_string())
}

fn base_cmd() -> Command {
    let mut cmd = Command::new(find_claude());
    cmd.arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--dangerously-skip-permissions");
    cmd.env_remove("CLAUDECODE");
    cmd
}

pub fn build_system_prompt(
    config: &Config,
    agent: Option<&Agent>,
    scratchpad_dir: &Path,
) -> String {
    let mut parts = Vec::new();

    // Global system prompt
    if !config.system_prompt.is_empty() {
        parts.push(resolve_at_refs(&config.system_prompt, &config.agents_dir()));
    }

    // Agent-specific system prompt (supports @file refs)
    if let Some(ag) = agent {
        if !ag.system_prompt.is_empty() {
            parts.push(resolve_at_refs(&ag.system_prompt, &config.agents_dir()));
        }
    }

    // Scratchpad instructions
    parts.push(format!(
        "## Shared Scratchpad\n\n\
         Your research group shares a scratchpad folder at `{}/`.\n\n\
         Read what's there. Add what you find. Edit to correct errors.",
        scratchpad_dir.display()
    ));

    parts.join("\n\n")
}

/// Resolve @file references in text. Looks in prompt_dir.
pub fn resolve_at_refs(text: &str, prompt_dir: &Path) -> String {
    let trimmed = text.trim();

    // Whole text is a single @file
    if trimmed.starts_with('@') && !trimmed.contains(' ') && !trimmed.contains('\n') {
        let path = prompt_dir.join(&trimmed[1..]);
        if let Ok(content) = std::fs::read_to_string(&path) {
            return content.trim().to_string();
        }
        eprintln!("warning: @file {} not found", path.display());
        return trimmed.to_string();
    }

    // Inline @file references
    if !text.contains('@') {
        return trimmed.to_string();
    }

    let mut result = text.to_string();
    let refs: Vec<String> = text.split_whitespace()
        .filter(|w| w.starts_with('@') && w.len() > 1)
        .map(|w| w.to_string())
        .collect();
    for r in &refs {
        let path = prompt_dir.join(&r[1..]);
        if let Ok(content) = std::fs::read_to_string(&path) {
            result = result.replace(r.as_str(), content.trim());
        }
    }
    result.trim().to_string()
}

pub fn claude_new(
    message: &str,
    model: &str,
    system_prompt: &str,
    _timeout: u64,
) -> Result<ClaudeResult> {
    let mut cmd = base_cmd();
    cmd.arg("--model").arg(model);

    if !system_prompt.is_empty() {
        cmd.arg("--append-system-prompt").arg(system_prompt);
    }

    run_claude_cmd(cmd, message)
}

pub fn claude_resume(
    session_id: &str,
    message: &str,
    _timeout: u64,
) -> Result<ClaudeResult> {
    let mut cmd = base_cmd();
    cmd.arg("--resume").arg(session_id);
    run_claude_cmd(cmd, message)
}

fn run_claude_cmd(mut cmd: Command, message: &str) -> Result<ClaudeResult> {
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn claude")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(message.as_bytes())?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait on claude")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() && stdout.trim().is_empty() {
        bail!(
            "claude exited with {}: {}",
            output.status,
            stderr.trim()
        );
    }

    let parsed: ClaudeJson = serde_json::from_str(stdout.trim())
        .with_context(|| format!("parsing claude JSON output: {}", &stdout[..stdout.len().min(200)]))?;

    if parsed.is_error == Some(true) {
        bail!(
            "claude returned error: {}",
            parsed.result.as_deref().unwrap_or("unknown")
        );
    }

    Ok(ClaudeResult {
        session_id: parsed.session_id.unwrap_or_default(),
        result: parsed.result.unwrap_or_default(),
        duration_ms: parsed.duration_ms.unwrap_or(0),
        cost_usd: parsed.total_cost_usd,
    })
}

mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}
