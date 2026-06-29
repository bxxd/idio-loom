//! The agent-backend port.
//!
//! Loom orchestrates *turns* — it doesn't care which coding agent actually runs
//! them. A [`Backend`] is the adapter between loom's turn model and a concrete
//! agent CLI (Claude Code, Pi, …). Everything Claude-specific that used to be
//! sprinkled through `loom.rs` and `snapshot.rs` now lives behind this port.
//!
//! Design notes:
//! * Loom assigns the **session key** (the agent's name within a thread). Some
//!   backends let us choose the underlying session id (Pi); others assign their
//!   own and hand it back (Claude). [`TurnOutcome::session_id`] is whatever the
//!   backend wants loom to persist and pass back on the next turn.
//! * Snapshot/rewind is delegated: a backend knows where its own transcript
//!   lives, so it copies it into / out of loom's snapshot dir.

use anyhow::Result;
use std::path::Path;

use crate::config::Config;

mod claude;
mod pi;

pub use claude::ClaudeBackend;
pub use pi::PiBackend;

/// A single turn to execute against a backend.
pub struct TurnRequest<'a> {
    /// Loom's name for the agent in this thread (e.g. `"axe"`). Stable across
    /// turns; backends that let us pick a session id should derive it from this.
    pub agent: &'a str,
    /// The assembled user message (source output + nudge).
    pub message: &'a str,
    /// Model id to use for this turn.
    pub model: &'a str,
    /// Fully-assembled system prompt.
    pub system_prompt: &'a str,
    /// The backend's own session id from a prior turn, if this is a resume.
    /// `None` means "start a fresh session".
    pub session_id: Option<&'a str>,
    /// Soft timeout in seconds (`0` disables).
    pub timeout_secs: u64,
}

/// What a backend returns after running a turn.
pub struct TurnOutcome {
    /// Session id to persist and pass back on the next turn for this agent.
    pub session_id: String,
    /// The agent's final textual output.
    pub output: String,
    /// Wall-clock duration reported by the backend, if available.
    pub duration_ms: u64,
    /// Turn cost in USD, if the backend reports it.
    pub cost_usd: Option<f64>,
}

/// The agent-backend port. One impl per coding agent CLI.
pub trait Backend {
    /// Short identifier, used in logs and config (`"claude"`, `"pi"`).
    fn name(&self) -> &'static str;

    /// Execute one turn.
    fn run_turn(&self, config: &Config, req: &TurnRequest) -> Result<TurnOutcome>;

    /// Absolute path to the transcript file backing `session_id`, if this
    /// backend stores one. Used by the timeout watchdog as an activity probe
    /// and by snapshotting. `None` means "no on-disk transcript to track".
    fn transcript_path(&self, config: &Config, session_id: &str) -> Option<std::path::PathBuf>;

    /// Copy this backend's live transcript for `session_id` **into** the
    /// snapshot directory, keyed by `agent`. Best-effort: warn, don't fail.
    fn snapshot_session(
        &self,
        config: &Config,
        agent: &str,
        session_id: &str,
        sessions_dir: &Path,
    ) -> Result<()>;

    /// Restore a previously-snapshotted transcript for `agent`/`session_id`
    /// **back into** the backend's live location. Best-effort.
    fn restore_session(
        &self,
        config: &Config,
        agent: &str,
        session_id: &str,
        sessions_dir: &Path,
    ) -> Result<()>;

    /// Delete the backend's live transcript for `session_id` (for `reset`/`rm`).
    fn cleanup_session(&self, config: &Config, session_id: &str) -> Result<()>;
}

/// Resolve the configured backend into a concrete impl.
pub fn for_config(config: &Config) -> Result<Box<dyn Backend>> {
    match config.backend.as_str() {
        "claude" => Ok(Box::new(ClaudeBackend)),
        "pi" => Ok(Box::new(PiBackend)),
        other => anyhow::bail!(
            "unknown backend '{}' in loom.yaml — expected 'claude' or 'pi'",
            other
        ),
    }
}
