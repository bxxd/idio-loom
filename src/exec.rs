//! Backend-agnostic subprocess execution.
//!
//! Both the Claude and Pi backends spawn a child CLI, feed it a prompt on
//! stdin, enforce a timeout via a dedicated process group, and capture
//! stdout/stderr. That machinery is identical across backends — only the
//! command and the output parsing differ — so it lives here.

use anyhow::{bail, Context, Result};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// Raw result of running a child process to completion.
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// How the timeout watchdog decides a still-running child is making progress.
///
/// Some agents stream their transcript to a JSONL file as they work. If that
/// file was touched recently we extend the deadline rather than killing a
/// healthy-but-slow turn. Backends that don't write such a file pass `None`.
pub struct ActivityProbe {
    /// Path to a file whose mtime indicates the child is still working.
    pub session_jsonl: Option<PathBuf>,
    /// Seconds of file inactivity tolerated past the soft timeout.
    pub idle_limit: u64,
}

impl Default for ActivityProbe {
    fn default() -> Self {
        Self {
            session_jsonl: None,
            idle_limit: 300,
        }
    }
}

/// Spawn `cmd`, write `message` to its stdin, and wait for completion under a
/// timeout. The child runs in its own process group so the whole tree can be
/// killed on timeout. `timeout_secs == 0` disables the timeout.
pub fn run_with_timeout(
    mut cmd: Command,
    message: &str,
    timeout_secs: u64,
    probe: ActivityProbe,
) -> Result<ExecOutput> {
    // New process group so we can kill the child + its descendants.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn backend process")?;

    // Write the prompt to stdin, then close it.
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(message.as_bytes())?;
    }

    let t0 = Instant::now();
    let max_timeout = timeout_secs.saturating_mul(2);

    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                let elapsed = t0.elapsed().as_secs();
                if timeout_secs > 0 && elapsed > timeout_secs {
                    // Hard cap — kill no matter what.
                    if elapsed > max_timeout {
                        kill_group(&mut child);
                        bail!("backend hit max timeout after {}s", elapsed);
                    }

                    // Soft timeout: keep waiting if the transcript is still
                    // being written.
                    if let Some(ref path) = probe.session_jsonl {
                        if let Ok(md) = std::fs::metadata(path) {
                            if let Ok(modified) = md.modified() {
                                let idle = modified.elapsed().unwrap_or_default().as_secs();
                                if idle < probe.idle_limit {
                                    std::thread::sleep(std::time::Duration::from_secs(1));
                                    continue;
                                }
                                eprintln!(
                                    "[loom] session idle {}s (limit {}s), killing after {}s",
                                    idle, probe.idle_limit, elapsed
                                );
                            }
                        }
                    }

                    kill_group(&mut child);
                    bail!("backend timed out after {}s", elapsed);
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            Err(e) => bail!("error waiting on backend: {}", e),
        }
    }

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout_buf)?;
    }
    if let Some(mut err) = child.stderr.take() {
        err.read_to_end(&mut stderr_buf)?;
    }
    let status = child.wait()?;

    Ok(ExecOutput {
        stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
        stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
        success: status.success(),
    })
}

/// SIGKILL the child's whole process group, then reap it.
fn kill_group(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.wait();
}

/// Emit a `[loom] exec: ...` log line for a command + its stdin preview.
pub fn log_command(cmd: &Command, message: &str, timeout_secs: u64) {
    let prog = cmd.get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| {
            let s = a.to_string_lossy();
            if s.len() > 200 {
                let mut end = 100;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                format!("\"{}...\" ({} chars)", &s[..end], s.len())
            } else if s.contains(' ') || s.contains('\n') {
                format!("\"{}\"", s.replace('\n', "\\n"))
            } else {
                s.to_string()
            }
        })
        .collect();
    let cwd = cmd
        .get_current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    let msg_preview = if message.len() > 200 {
        let mut end = 100;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}... ({} chars)", &message[..end], message.len())
    } else {
        message.to_string()
    };
    eprintln!("[loom] exec: {} {}", prog, args.join(" "));
    eprintln!("[loom]   cwd: {}", cwd);
    eprintln!("[loom]   stdin: {}", msg_preview);
    eprintln!("[loom]   timeout: {}s", timeout_secs);
}

/// Resolve the home directory from `$HOME`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
