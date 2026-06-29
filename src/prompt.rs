//! Backend-agnostic prompt assembly.
//!
//! System-prompt construction and `@file` expansion are the same regardless of
//! which agent CLI ends up executing the turn, so they live here rather than in
//! any single backend.

use crate::config::{Agent, Config};
use std::path::Path;

/// Assemble the full system prompt for a turn: global prompt + agent prompt +
/// the shared-scratchpad instructions. `@file` references are resolved against
/// the agents directory.
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

/// Resolve `@file` references in `text`, looking in `prompt_dir`.
///
/// Two shapes are supported:
/// * the whole value is a single `@file` token → replaced by file contents
/// * inline `@file` tokens within prose → each replaced in place
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
    let refs: Vec<String> = text
        .split_whitespace()
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
