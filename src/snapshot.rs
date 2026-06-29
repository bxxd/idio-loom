use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::backend::{self, Backend};
use crate::config::Config;
use crate::meta::{AgentState, Meta};

/// Copy session transcripts for all agents between a snapshot dir and the
/// backend's live location. Direction picks save (live → snapshot) vs restore
/// (snapshot → live). The backend owns where its transcripts live.
fn sync_sessions(
    config: &Config,
    backend: &dyn Backend,
    agents: &HashMap<String, AgentState>,
    sessions_dir: &Path,
    direction: Direction,
) -> Result<()> {
    for (agent_name, agent) in agents {
        if let Some(ref sid) = agent.session_id {
            match direction {
                Direction::Save => {
                    backend.snapshot_session(config, agent_name, sid, sessions_dir)?
                }
                Direction::Restore => {
                    backend.restore_session(config, agent_name, sid, sessions_dir)?
                }
            }
        }
    }
    Ok(())
}

enum Direction {
    Save,
    Restore,
}

pub fn auto_snapshot(config: &Config, name: &str, meta: &Meta) -> Result<()> {
    let n = meta.thread.len();
    let last_agent = meta
        .thread
        .last()
        .map(|t| t.agent.as_str())
        .unwrap_or("init");
    let run_dir = Meta::run_dir(config, name);
    let snap = run_dir
        .join("snapshots")
        .join(format!("snap-{}-{}", n, last_agent));

    if snap.exists() {
        std::fs::remove_dir_all(&snap)?;
    }
    std::fs::create_dir_all(&snap)?;

    // Copy run state (everything except snapshots/)
    copy_run_state(&run_dir, &snap)?;

    // Copy session transcripts via the backend
    let sessions_dir = snap.join("sessions");
    std::fs::create_dir_all(&sessions_dir)?;
    let backend = backend::for_config(config)?;
    sync_sessions(
        config,
        backend.as_ref(),
        &meta.agents,
        &sessions_dir,
        Direction::Save,
    )?;

    eprintln!("  snapshot: snap-{}-{}", n, last_agent);
    Ok(())
}

pub fn rewind(config: &Config, name: &str, snap_n: &str) -> Result<()> {
    let run_dir = Meta::run_dir(config, name);
    let snap_dir = run_dir.join("snapshots");
    let snap = find_snapshot(&snap_dir, snap_n)?;

    // Clear current state (except snapshots/)
    for entry in std::fs::read_dir(&run_dir)? {
        let entry = entry?;
        if entry.file_name() != "snapshots" {
            let p = entry.path();
            if p.is_dir() {
                std::fs::remove_dir_all(&p)?;
            } else {
                std::fs::remove_file(&p)?;
            }
        }
    }

    // Restore run state from snapshot (except sessions/)
    for entry in std::fs::read_dir(&snap)? {
        let entry = entry?;
        if entry.file_name() == "sessions" {
            continue;
        }
        let dest = run_dir.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }

    // Restore session transcripts via the backend
    let sessions = snap.join("sessions");
    if sessions.exists() {
        let meta = Meta::read(config, name)?;
        let backend = backend::for_config(config)?;
        sync_sessions(
            config,
            backend.as_ref(),
            &meta.agents,
            &sessions,
            Direction::Restore,
        )?;
    }

    eprintln!("rewound to {}", snap.file_name().unwrap().to_string_lossy());
    Ok(())
}

fn find_snapshot(snap_dir: &Path, target: &str) -> Result<PathBuf> {
    if !snap_dir.exists() {
        bail!("no snapshots directory");
    }

    for entry in std::fs::read_dir(snap_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == format!("snap-{}", target) || name.starts_with(&format!("snap-{}-", target)) {
            return Ok(entry.path());
        }
    }

    bail!("snapshot {} not found", target);
}

/// Remove backend transcripts for all agents in this run (for delete/reset).
pub fn cleanup_sessions(config: &Config, meta: &Meta) -> Result<()> {
    let backend = backend::for_config(config)?;
    for agent in meta.agents.values() {
        if let Some(ref sid) = agent.session_id {
            backend.cleanup_session(config, sid)?;
        }
    }
    Ok(())
}

fn copy_run_state(src: &Path, dest: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_name() == "snapshots" {
            continue;
        }
        let target = dest.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
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
