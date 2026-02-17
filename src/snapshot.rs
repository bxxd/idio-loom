use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::meta::{AgentState, Meta};

fn session_dir() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let slug = cwd.to_string_lossy().replace('/', "-");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(slug)
}

/// Copy session JSONLs for all agents between a sessions dir and claude's project dir.
/// direction: Save = project → snapshot, Restore = snapshot → project.
fn sync_sessions(
    agents: &HashMap<String, AgentState>,
    sessions_dir: &Path,
    direction: Direction,
) -> Result<()> {
    let sess_base = session_dir();

    for (agent_name, agent) in agents {
        if let Some(ref sid) = agent.session_id {
            let project_jsonl = sess_base.join(format!("{}.jsonl", sid));
            let snap_jsonl = sessions_dir.join(format!("{}.jsonl", agent_name));
            let project_sub = sess_base.join(sid);
            let snap_sub = sessions_dir.join(agent_name);

            match direction {
                Direction::Save => {
                    if project_jsonl.exists() {
                        std::fs::copy(&project_jsonl, &snap_jsonl)?;
                    }
                    if project_sub.exists() && project_sub.is_dir() {
                        copy_dir_recursive(&project_sub, &snap_sub)?;
                    }
                }
                Direction::Restore => {
                    if snap_jsonl.exists() {
                        std::fs::copy(&snap_jsonl, &project_jsonl)?;
                    }
                    if snap_sub.exists() && snap_sub.is_dir() {
                        if project_sub.exists() {
                            std::fs::remove_dir_all(&project_sub)?;
                        }
                        copy_dir_recursive(&snap_sub, &project_sub)?;
                    }
                }
            }
        }
    }
    Ok(())
}

enum Direction { Save, Restore }

pub fn auto_snapshot(config: &Config, name: &str, meta: &Meta) -> Result<()> {
    let n = meta.thread.len();
    let last_agent = meta.thread.last().map(|t| t.agent.as_str()).unwrap_or("init");
    let run_dir = Meta::run_dir(config, name);
    let snap = run_dir.join("snapshots").join(format!("snap-{}-{}", n, last_agent));

    if snap.exists() {
        std::fs::remove_dir_all(&snap)?;
    }
    std::fs::create_dir_all(&snap)?;

    // Copy run state (everything except snapshots/)
    copy_run_state(&run_dir, &snap)?;

    // Copy session JSONLs
    let sessions_dir = snap.join("sessions");
    std::fs::create_dir_all(&sessions_dir)?;
    sync_sessions(&meta.agents, &sessions_dir, Direction::Save)?;

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

    // Restore session JSONLs
    let sessions = snap.join("sessions");
    if sessions.exists() {
        let meta = Meta::read(config, name)?;
        sync_sessions(&meta.agents, &sessions, Direction::Restore)?;
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

/// Remove session JSONLs from claude's project dir for all agents in this run
pub fn cleanup_sessions(meta: &Meta) -> Result<()> {
    let sess_base = session_dir();
    for agent in meta.agents.values() {
        if let Some(ref sid) = agent.session_id {
            let jsonl = sess_base.join(format!("{}.jsonl", sid));
            if jsonl.exists() {
                std::fs::remove_file(&jsonl)?;
            }
            let sub = sess_base.join(sid);
            if sub.exists() && sub.is_dir() {
                std::fs::remove_dir_all(&sub)?;
            }
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
