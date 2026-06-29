//! Claude Code backend.
//!
//! Drives the `claude` CLI in non-interactive JSON mode. Sessions live at
//! `~/.claude/projects/{slug}/{session_id}.jsonl` where `slug` is the claude
//! working directory with `/` → `-`. Claude assigns the session id; we read it
//! back from the JSON result and persist it for the next turn.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Backend, TurnOutcome, TurnRequest};
use crate::config::Config;
use crate::exec::{self, ActivityProbe};

pub struct ClaudeBackend;

#[derive(Deserialize)]
struct ClaudeJson {
    session_id: Option<String>,
    result: Option<String>,
    duration_ms: Option<u64>,
    total_cost_usd: Option<f64>,
    is_error: Option<bool>,
}

impl ClaudeBackend {
    fn find_binary() -> String {
        exec::home_dir()
            .map(|h| h.join(".local/bin/claude"))
            .filter(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "claude".to_string())
    }

    fn base_cmd(config: &Config) -> Command {
        let mut cmd = Command::new(Self::find_binary());
        cmd.arg("-p")
            .arg("--output-format")
            .arg("json")
            .arg("--dangerously-skip-permissions");
        cmd.env_remove("CLAUDECODE");
        cmd.current_dir(config.claude_cwd());
        cmd
    }

    /// `~/.claude/projects/{slug}/` where slug = claude cwd with `/` → `-`.
    fn session_dir(config: &Config) -> PathBuf {
        let cwd = config.claude_cwd();
        let slug = cwd.to_string_lossy().replace('/', "-");
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        PathBuf::from(home)
            .join(".claude")
            .join("projects")
            .join(slug)
    }
}

impl Backend for ClaudeBackend {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn run_turn(&self, config: &Config, req: &TurnRequest) -> Result<TurnOutcome> {
        let mut cmd = Self::base_cmd(config);
        match req.session_id {
            Some(sid) => {
                cmd.arg("--resume").arg(sid);
            }
            None => {
                cmd.arg("--model").arg(req.model);
            }
        }
        if !req.system_prompt.is_empty() {
            cmd.arg("--append-system-prompt").arg(req.system_prompt);
        }

        exec::log_command(&cmd, req.message, req.timeout_secs);

        let probe = ActivityProbe {
            session_jsonl: req
                .session_id
                .map(|sid| Self::session_dir(config).join(format!("{}.jsonl", sid))),
            ..Default::default()
        };

        let out = exec::run_with_timeout(cmd, req.message, req.timeout_secs, probe)?;

        if !out.success && out.stdout.trim().is_empty() {
            bail!("claude exited unsuccessfully: {}", out.stderr.trim());
        }

        let parsed: ClaudeJson = serde_json::from_str(out.stdout.trim()).with_context(|| {
            let mut e = out.stdout.len().min(200);
            while !out.stdout.is_char_boundary(e) {
                e -= 1;
            }
            format!("parsing claude JSON output: {}", &out.stdout[..e])
        })?;

        if parsed.is_error == Some(true) {
            bail!(
                "claude returned error: {}",
                parsed.result.as_deref().unwrap_or("unknown")
            );
        }

        let session_id = parsed.session_id.unwrap_or_default();
        let result_text = parsed.result.unwrap_or_default();

        eprintln!(
            "[loom] claude done: session={} duration={}ms cost=${:.4} result_chars={}",
            &session_id,
            parsed.duration_ms.unwrap_or(0),
            parsed.total_cost_usd.unwrap_or(0.0),
            result_text.len(),
        );
        if !out.stderr.is_empty() {
            let preview: String = out.stderr.lines().take(30).collect::<Vec<_>>().join("\n");
            eprintln!("[loom] claude stderr:\n{}", preview);
        }

        Ok(TurnOutcome {
            session_id,
            output: result_text,
            duration_ms: parsed.duration_ms.unwrap_or(0),
            cost_usd: parsed.total_cost_usd,
        })
    }

    fn transcript_path(&self, config: &Config, session_id: &str) -> Option<PathBuf> {
        Some(Self::session_dir(config).join(format!("{}.jsonl", session_id)))
    }

    fn snapshot_session(
        &self,
        config: &Config,
        agent: &str,
        session_id: &str,
        sessions_dir: &Path,
    ) -> Result<()> {
        let sess_base = Self::session_dir(config);
        let project_jsonl = sess_base.join(format!("{}.jsonl", session_id));
        let snap_jsonl = sessions_dir.join(format!("{}.jsonl", agent));
        if project_jsonl.exists() {
            std::fs::copy(&project_jsonl, &snap_jsonl)?;
            eprintln!("  session saved: {} ({})", agent, session_id);
        } else {
            eprintln!(
                "  WARNING: session file missing for {}: {}",
                agent,
                project_jsonl.display()
            );
        }
        // Sub-agent transcripts live in a sibling directory named after the id.
        let project_sub = sess_base.join(session_id);
        if project_sub.is_dir() {
            let snap_sub = sessions_dir.join(agent);
            copy_dir_recursive(&project_sub, &snap_sub)?;
            eprintln!("  subagents saved: {}", agent);
        }
        Ok(())
    }

    fn restore_session(
        &self,
        config: &Config,
        agent: &str,
        session_id: &str,
        sessions_dir: &Path,
    ) -> Result<()> {
        let sess_base = Self::session_dir(config);
        let project_jsonl = sess_base.join(format!("{}.jsonl", session_id));
        let snap_jsonl = sessions_dir.join(format!("{}.jsonl", agent));
        if snap_jsonl.exists() {
            std::fs::copy(&snap_jsonl, &project_jsonl)?;
            eprintln!("  session restored: {} ({})", agent, session_id);
        } else {
            eprintln!(
                "  WARNING: no session backup for {} — resume will fail",
                agent
            );
        }
        let snap_sub = sessions_dir.join(agent);
        if snap_sub.is_dir() {
            let project_sub = sess_base.join(session_id);
            if project_sub.exists() {
                std::fs::remove_dir_all(&project_sub)?;
            }
            copy_dir_recursive(&snap_sub, &project_sub)?;
            eprintln!("  subagents restored: {}", agent);
        }
        Ok(())
    }

    fn cleanup_session(&self, config: &Config, session_id: &str) -> Result<()> {
        let sess_base = Self::session_dir(config);
        let jsonl = sess_base.join(format!("{}.jsonl", session_id));
        if jsonl.exists() {
            std::fs::remove_file(&jsonl)?;
        }
        let sub = sess_base.join(session_id);
        if sub.is_dir() {
            std::fs::remove_dir_all(&sub)?;
        }
        Ok(())
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
