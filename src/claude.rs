use crate::config::{Agent, Config};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

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

fn base_cmd(config: &Config) -> Command {
    let mut cmd = Command::new(find_claude());
    cmd.arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--dangerously-skip-permissions");
    cmd.env_remove("CLAUDECODE");
    cmd.current_dir(config.claude_cwd());
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

pub fn claude_new(
    config: &Config,
    message: &str,
    model: &str,
    system_prompt: &str,
    timeout_secs: u64,
) -> Result<ClaudeResult> {
    let mut cmd = base_cmd(config);
    cmd.arg("--model").arg(model);

    if !system_prompt.is_empty() {
        cmd.arg("--append-system-prompt").arg(system_prompt);
    }

    run_claude_cmd(cmd, message, timeout_secs)
}

pub fn claude_resume(
    config: &Config,
    session_id: &str,
    message: &str,
    system_prompt: &str,
    timeout_secs: u64,
) -> Result<ClaudeResult> {
    let mut cmd = base_cmd(config);
    cmd.arg("--resume").arg(session_id);
    if !system_prompt.is_empty() {
        cmd.arg("--append-system-prompt").arg(system_prompt);
    }
    run_claude_cmd(cmd, message, timeout_secs)
}

fn run_claude_cmd(mut cmd: Command, message: &str, timeout_secs: u64) -> Result<ClaudeResult> {
    use std::io::Read;
    use std::time::Instant;

    // Log the exact command being executed
    log_command(&cmd, message, timeout_secs);

    // Create new process group so we can kill claude + its children
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
        .context("failed to spawn claude")?;

    // Write message to stdin, then close
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(message.as_bytes())?;
    }

    let t0 = Instant::now();

    // Poll for completion with timeout
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if timeout_secs > 0 && t0.elapsed().as_secs() > timeout_secs {
                    // Kill the process group
                    let pid = child.id() as i32;
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                    let _ = child.wait();
                    bail!("claude timed out after {}s", timeout_secs);
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            Err(e) => bail!("error waiting on claude: {}", e),
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

    let stdout = String::from_utf8_lossy(&stdout_buf);
    let stderr = String::from_utf8_lossy(&stderr_buf);

    if !status.success() && stdout.trim().is_empty() {
        bail!("claude exited with {}: {}", status, stderr.trim());
    }

    let parsed: ClaudeJson = serde_json::from_str(stdout.trim()).with_context(|| {
        format!("parsing claude JSON output: {}", {
            let mut e = stdout.len().min(200);
            while !stdout.is_char_boundary(e) {
                e -= 1;
            }
            &stdout[..e]
        })
    })?;

    if parsed.is_error == Some(true) {
        eprintln!(
            "[loom] claude error: {}",
            parsed.result.as_deref().unwrap_or("unknown")
        );
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

    if !stderr.is_empty() {
        // Log stderr (contains tool use activity from claude)
        let stderr_preview: String = stderr.lines().take(30).collect::<Vec<_>>().join("\n");
        eprintln!("[loom] claude stderr:\n{}", stderr_preview);
    }

    Ok(ClaudeResult {
        session_id,
        result: result_text,
        duration_ms: parsed.duration_ms.unwrap_or(0),
        cost_usd: parsed.total_cost_usd,
    })
}

fn log_command(cmd: &Command, message: &str, timeout_secs: u64) {
    let prog = cmd.get_program().to_string_lossy();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| {
            let s = a.to_string_lossy();
            // Truncate long args (system prompts) to keep logs readable
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

mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}
