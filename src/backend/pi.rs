//! Pi backend (earendil-works/pi, and downstream forks like vizipi).
//!
//! Pi is friendlier to orchestration than Claude in two ways loom exploits:
//!
//! * **We pick the session id** (`--session-id`). Re-passing the same id on a
//!   later turn reopens the same transcript, so loom doesn't have to scrape an
//!   assigned id back out of the output.
//! * **We pick the session directory** (`--session-dir`). Rather than reverse
//!   engineer pi's per-cwd slug under `~/.pi` (or `~/.vizipi`, or whatever a
//!   fork rebrands to), we point pi at a loom-owned directory. Snapshotting is
//!   then just "copy a file we already know the location of".
//!
//! Pi names transcripts `{timestamp}_{session_id}.jsonl`, so given an id we
//! glob `*_{id}.jsonl` within the session dir.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Backend, TurnOutcome, TurnRequest};
use crate::config::Config;
use crate::exec::{self, ActivityProbe};

pub struct PiBackend;

impl PiBackend {
    fn binary(config: &Config) -> String {
        config
            .backend_bin
            .clone()
            .unwrap_or_else(|| "pi".to_string())
    }

    /// Loom-owned directory holding all pi transcripts. Stable, branding-free.
    fn session_dir(config: &Config) -> PathBuf {
        config.state_dir().join("pi-sessions")
    }

    /// Mint a session id unique within a thread. Pi requires the id to match
    /// `^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$`, so we sanitize the agent
    /// name and append epoch-nanos for uniqueness across threads sharing a dir.
    fn mint_session_id(agent: &str) -> String {
        let sanitized: String = agent
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let stem = sanitized.trim_matches('-');
        let stem = if stem.is_empty() { "agent" } else { stem };
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{}-{}", stem, nanos)
    }

    /// Find the transcript file for `session_id` (pi prefixes a timestamp).
    fn find_transcript(config: &Config, session_id: &str) -> Option<PathBuf> {
        let dir = Self::session_dir(config);
        let suffix = format!("_{}.jsonl", session_id);
        std::fs::read_dir(&dir).ok()?.flatten().find_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            (name.ends_with(&suffix)).then(|| e.path())
        })
    }

    fn base_cmd(config: &Config) -> Command {
        let mut cmd = Command::new(Self::binary(config));
        // Non-interactive single-shot. Piped stdin/stdout already force print
        // mode, but be explicit. Text mode emits only the final assistant text.
        cmd.arg("--print")
            .arg("--mode")
            .arg("text")
            // Trust project-local files (AGENTS.md, extensions) without a prompt.
            .arg("--approve")
            .arg("--session-dir")
            .arg(Self::session_dir(config));
        cmd.current_dir(config.claude_cwd());
        cmd
    }
}

impl Backend for PiBackend {
    fn name(&self) -> &'static str {
        "pi"
    }

    fn run_turn(&self, config: &Config, req: &TurnRequest) -> Result<TurnOutcome> {
        // Ensure the loom-owned session dir exists before pi looks for it.
        std::fs::create_dir_all(Self::session_dir(config))?;

        // We own the session id: reuse the prior one, or mint a fresh one.
        let session_id = req
            .session_id
            .map(str::to_string)
            .unwrap_or_else(|| Self::mint_session_id(req.agent));

        let mut cmd = Self::base_cmd(config);
        cmd.arg("--session-id").arg(&session_id);
        cmd.arg("--model").arg(req.model);
        if !req.system_prompt.is_empty() {
            cmd.arg("--append-system-prompt").arg(req.system_prompt);
        }

        exec::log_command(&cmd, req.message, req.timeout_secs);

        let probe = ActivityProbe {
            session_jsonl: Self::find_transcript(config, &session_id),
            ..Default::default()
        };

        let out = exec::run_with_timeout(cmd, req.message, req.timeout_secs, probe)?;

        if !out.success {
            // Pi prints the error to stderr and exits non-zero in text mode.
            let detail = if out.stderr.trim().is_empty() {
                out.stdout.trim()
            } else {
                out.stderr.trim()
            };
            bail!("pi exited unsuccessfully: {}", detail);
        }

        let output = out.stdout.trim_end().to_string();
        eprintln!(
            "[loom] pi done: session={} result_chars={}",
            session_id,
            output.len()
        );
        if !out.stderr.is_empty() {
            let preview: String = out.stderr.lines().take(30).collect::<Vec<_>>().join("\n");
            eprintln!("[loom] pi stderr:\n{}", preview);
        }

        Ok(TurnOutcome {
            session_id,
            output,
            // Pi text mode reports neither duration nor cost; loom measures its
            // own elapsed and treats cost as optional.
            duration_ms: 0,
            cost_usd: None,
        })
    }

    fn snapshot_session(
        &self,
        config: &Config,
        agent: &str,
        session_id: &str,
        sessions_dir: &Path,
    ) -> Result<()> {
        match Self::find_transcript(config, session_id) {
            Some(src) => {
                let dest = sessions_dir.join(format!("{}.jsonl", agent));
                std::fs::copy(&src, &dest)?;
                eprintln!("  session saved: {} ({})", agent, session_id);
            }
            None => eprintln!(
                "  WARNING: pi transcript missing for {} (id {})",
                agent, session_id
            ),
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
        let snap = sessions_dir.join(format!("{}.jsonl", agent));
        if !snap.exists() {
            eprintln!(
                "  WARNING: no pi session backup for {} — resume will fail",
                agent
            );
            return Ok(());
        }
        // Restore to the live transcript path. Reuse the existing filename if pi
        // already made one for this id; otherwise synthesize the standard
        // `{timestamp}_{id}.jsonl` name in the loom-owned dir.
        let dest = Self::find_transcript(config, session_id).unwrap_or_else(|| {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Self::session_dir(config).join(format!("{}_{}.jsonl", ts, session_id))
        });
        std::fs::create_dir_all(Self::session_dir(config))?;
        std::fs::copy(&snap, &dest)?;
        eprintln!("  session restored: {} ({})", agent, session_id);
        Ok(())
    }

    fn cleanup_session(&self, config: &Config, session_id: &str) -> Result<()> {
        if let Some(p) = Self::find_transcript(config, session_id) {
            if p.exists() {
                std::fs::remove_file(&p)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pi's own session-id validation regex (see core/session-manager.ts
    /// `assertValidSessionId`). Minted ids MUST satisfy it or pi rejects them.
    fn is_valid_pi_session_id(id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        let bytes = id.as_bytes();
        let alnum = |b: u8| b.is_ascii_alphanumeric();
        let inner = |b: u8| alnum(b) || b == b'.' || b == b'-' || b == b'_';
        if !alnum(bytes[0]) || !alnum(bytes[bytes.len() - 1]) {
            return false;
        }
        bytes.iter().all(|&b| inner(b))
    }

    #[test]
    fn minted_ids_are_valid_for_pi() {
        for agent in ["axe", "bobby", "agent_007", "the-critic"] {
            let id = PiBackend::mint_session_id(agent);
            assert!(is_valid_pi_session_id(&id), "invalid pi id: {}", id);
        }
    }

    #[test]
    fn minted_ids_sanitize_hostile_names() {
        // Spaces, slashes, leading/trailing junk must not leak into the id.
        let id = PiBackend::mint_session_id(" weird /name! ");
        assert!(is_valid_pi_session_id(&id), "invalid pi id: {}", id);
    }

    #[test]
    fn empty_agent_falls_back_to_stem() {
        let id = PiBackend::mint_session_id("");
        assert!(
            id.starts_with("agent-"),
            "expected agent- fallback, got {}",
            id
        );
        assert!(is_valid_pi_session_id(&id), "invalid pi id: {}", id);
    }

    #[test]
    fn minted_ids_carry_the_agent_stem() {
        let id = PiBackend::mint_session_id("bobby");
        assert!(
            id.starts_with("bobby-"),
            "expected bobby- prefix, got {}",
            id
        );
    }
}
