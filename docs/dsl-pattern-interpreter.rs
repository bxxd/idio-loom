use anyhow::{bail, Context, Result};
use std::collections::HashMap;

use crate::config::Config;
use crate::loom;
use crate::meta::Meta;

// ── AST ──

pub struct Pattern {
    pub steps: Vec<Step>,
}

pub struct Step {
    pub label: Option<String>,
    pub instruction: Instruction,
}

pub enum Instruction {
    Do {
        agent: String,
        source: Option<String>,
        nudge: Option<String>,
    },
    Goto {
        target: String,
        condition: Option<Condition>,
    },
    Exit,
    Shell {
        command: String,
    },
}

pub enum Condition {
    IfGrep(String),
    UnlessGrep(String),
}

// ── Parser ──

pub fn load(path: &str, name: &str, intake: &str) -> Result<Pattern> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading pattern: {}", path))?;
    parse(&text, name, intake)
}

pub fn parse(text: &str, name: &str, intake: &str) -> Result<Pattern> {
    let text = text.replace("$INTAKE", intake).replace("$NAME", name);
    let mut steps = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let step = parse_line(line, i + 1)?;
        steps.push(step);
    }
    if steps.is_empty() {
        bail!("pattern has no steps");
    }
    // Validate: all goto targets exist
    let labels: Vec<_> = steps.iter().filter_map(|s| s.label.as_deref()).collect();
    for step in &steps {
        if let Instruction::Goto { target, .. } = &step.instruction {
            if !labels.contains(&target.as_str()) {
                bail!("goto target '{}' not found", target);
            }
        }
    }
    Ok(Pattern { steps })
}

fn parse_line(line: &str, line_num: usize) -> Result<Step> {
    // Check for label: only if colon is before any space/quote
    let (label, rest) = extract_label(line);

    if rest.is_empty() {
        bail!("line {}: empty instruction after label", line_num);
    }

    let tokens = tokenize(rest);
    if tokens.is_empty() {
        bail!("line {}: empty instruction", line_num);
    }

    let instruction = match tokens[0].as_str() {
        "do" => parse_do(&tokens, line_num)?,
        "goto" => parse_goto(&tokens, line_num)?,
        "exit" => Instruction::Exit,
        "shell" => {
            // Everything after "shell " is the command (raw, not tokenized)
            let cmd = rest.strip_prefix("shell").unwrap().trim();
            Instruction::Shell { command: cmd.to_string() }
        }
        other => bail!("line {}: unknown instruction '{}'", line_num, other),
    };

    Ok(Step { label, instruction })
}

fn extract_label(line: &str) -> (Option<String>, &str) {
    // Label must be a single word before colon, before any space or quote
    if let Some(colon) = line.find(':') {
        let before = &line[..colon];
        if !before.is_empty() && !before.contains(' ') && !before.contains('"') {
            let rest = line[colon + 1..].trim();
            return (Some(before.to_string()), rest);
        }
    }
    (None, line)
}

fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '"' {
            chars.next();
            let mut tok = String::new();
            while let Some(&c) = chars.peek() {
                if c == '"' { chars.next(); break; }
                tok.push(c);
                chars.next();
            }
            tokens.push(tok);
        } else {
            let mut tok = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() { break; }
                tok.push(c);
                chars.next();
            }
            tokens.push(tok);
        }
    }
    tokens
}

fn parse_do(tokens: &[String], line_num: usize) -> Result<Instruction> {
    if tokens.len() < 2 {
        bail!("line {}: do requires agent name", line_num);
    }
    let agent = tokens[1].clone();
    let (source, nudge_start) = if tokens.len() >= 4 && tokens[2] == "hears" {
        (Some(tokens[3].clone()), 4)
    } else {
        (None, 2)
    };
    let nudge = if nudge_start < tokens.len() {
        Some(tokens[nudge_start..].join(" "))
    } else {
        None
    };
    Ok(Instruction::Do { agent, source, nudge })
}

fn parse_goto(tokens: &[String], line_num: usize) -> Result<Instruction> {
    if tokens.len() < 2 {
        bail!("line {}: goto requires label", line_num);
    }
    let target = tokens[1].clone();
    let condition = if tokens.len() >= 5 && tokens[3] == "grep" {
        let pattern = tokens[4..].join(" ");
        match tokens[2].as_str() {
            "if" => Some(Condition::IfGrep(pattern)),
            "unless" => Some(Condition::UnlessGrep(pattern)),
            _ => bail!("line {}: expected 'if' or 'unless' after goto label", line_num),
        }
    } else if tokens.len() > 2 {
        bail!("line {}: expected 'if grep ...' or 'unless grep ...' after goto label", line_num);
    } else {
        None
    };
    Ok(Instruction::Goto { target, condition })
}

// ── Display ──

impl Pattern {
    pub fn display(&self) {
        for (i, step) in self.steps.iter().enumerate() {
            let label = step.label.as_deref().unwrap_or("");
            let desc = match &step.instruction {
                Instruction::Do { agent, source, nudge } => {
                    let src = source.as_ref().map(|s| format!(" hears {}", s)).unwrap_or_default();
                    let n = nudge.as_ref().map(|n| {
                        let preview = if n.len() > 50 { &n[..50] } else { n.as_str() };
                        format!(" \"{}\"", preview)
                    }).unwrap_or_default();
                    format!("do {}{}{}", agent, src, n)
                }
                Instruction::Goto { target, condition } => {
                    let cond = match condition {
                        Some(Condition::IfGrep(p)) => format!(" if grep {}", p),
                        Some(Condition::UnlessGrep(p)) => format!(" unless grep {}", p),
                        None => String::new(),
                    };
                    format!("goto {}{}", target, cond)
                }
                Instruction::Exit => "exit".to_string(),
                Instruction::Shell { command } => format!("shell {}", command),
            };
            if label.is_empty() {
                println!("  {:>2}. {}", i + 1, desc);
            } else {
                println!("  {:>2}. {}:  {}", i + 1, label, desc);
            }
        }
    }

    pub fn display_with_cursor(&self, pc: usize) {
        for (i, step) in self.steps.iter().enumerate() {
            let marker = if i == pc { ">" } else { " " };
            let label = step.label.as_deref().unwrap_or("");
            let desc = match &step.instruction {
                Instruction::Do { agent, source, nudge } => {
                    let src = source.as_ref().map(|s| format!(" hears {}", s)).unwrap_or_default();
                    let n = nudge.as_ref().map(|n| {
                        let preview = if n.len() > 50 { &n[..50] } else { n.as_str() };
                        format!(" \"{}\"", preview)
                    }).unwrap_or_default();
                    format!("do {}{}{}", agent, src, n)
                }
                Instruction::Goto { target, condition } => {
                    let cond = match condition {
                        Some(Condition::IfGrep(p)) => format!(" if grep {}", p),
                        Some(Condition::UnlessGrep(p)) => format!(" unless grep {}", p),
                        None => String::new(),
                    };
                    format!("goto {}{}", target, cond)
                }
                Instruction::Exit => "exit".to_string(),
                Instruction::Shell { command } => format!("shell {}", command),
            };
            if label.is_empty() {
                println!(" {} {:>2}. {}", marker, i + 1, desc);
            } else {
                println!(" {} {:>2}. {}:  {}", marker, i + 1, label, desc);
            }
        }
    }
}

// ── Interpreter ──

fn find_label(steps: &[Step], label: &str) -> Result<usize> {
    steps.iter().position(|s| s.label.as_deref() == Some(label))
        .ok_or_else(|| anyhow::anyhow!("label '{}' not found", label))
}

fn read_last_output(config: &Config, name: &str) -> Result<String> {
    let path = Meta::run_dir(config, name).join(".last_output");
    std::fs::read_to_string(&path)
        .with_context(|| "no previous output — .last_output not found".to_string())
}

/// Run all remaining steps
pub fn run_all(
    config: &Config,
    name: &str,
    pattern: &Pattern,
    model: Option<&str>,
) -> Result<()> {
    let mut meta = Meta::read(config, name)?;
    let mut pc = meta.pattern_step.unwrap_or(0);

    while pc < pattern.steps.len() {
        let is_action = matches!(
            pattern.steps[pc].instruction,
            Instruction::Do { .. } | Instruction::Shell { .. }
        );
        if is_action {
            let label = step_label(&pattern.steps[pc], pc);
            eprintln!("\n── {} ({}/{}) ──", label, pc + 1, pattern.steps.len());
        }
        pc = execute_step(config, name, pattern, pc, model, &mut meta)?;
    }

    eprintln!("\npattern complete.");
    Ok(())
}

/// Run the next action step (skip control flow)
pub fn run_next(
    config: &Config,
    name: &str,
    pattern: &Pattern,
    model: Option<&str>,
) -> Result<()> {
    let mut meta = Meta::read(config, name)?;
    let mut pc = meta.pattern_step.unwrap_or(0);

    loop {
        if pc >= pattern.steps.len() {
            eprintln!("all steps complete");
            return Ok(());
        }
        let is_action = matches!(
            pattern.steps[pc].instruction,
            Instruction::Do { .. } | Instruction::Shell { .. }
        );
        if is_action {
            let label = step_label(&pattern.steps[pc], pc);
            eprintln!("── {} ({}/{}) ──", label, pc + 1, pattern.steps.len());
        }
        let new_pc = execute_step(config, name, pattern, pc, model, &mut meta)?;
        if is_action {
            return Ok(());
        }
        pc = new_pc;
    }
}

fn step_label(step: &Step, index: usize) -> String {
    step.label.clone().unwrap_or_else(|| format!("step {}", index + 1))
}

fn execute_step(
    config: &Config,
    name: &str,
    pattern: &Pattern,
    pc: usize,
    model: Option<&str>,
    meta: &mut Meta,
) -> Result<usize> {
    let step = &pattern.steps[pc];

    match &step.instruction {
        Instruction::Do { agent, source, nudge } => {
            loom::run_turn(
                config, name, agent,
                source.as_deref(), nudge.as_deref(),
                model, step.label.as_deref(),
            )?;
            // Reload meta (run_turn saved it)
            *meta = Meta::read(config, name)?;
            let next = pc + 1;
            meta.pattern_step = Some(next);
            meta.save(config)?;
            Ok(next)
        }
        Instruction::Goto { target, condition } => {
            let jump = match condition {
                None => true,
                Some(Condition::IfGrep(pat)) => {
                    let output = read_last_output(config, name)?;
                    output.contains(pat.as_str())
                }
                Some(Condition::UnlessGrep(pat)) => {
                    let output = read_last_output(config, name)?;
                    !output.contains(pat.as_str())
                }
            };
            let next = if jump {
                let idx = find_label(&pattern.steps, target)?;
                if condition.is_some() {
                    let dir = if idx <= pc { "back" } else { "forward" };
                    eprintln!("  goto {} ({})", target, dir);
                }
                idx
            } else {
                pc + 1
            };
            meta.pattern_step = Some(next);
            meta.save(config)?;
            Ok(next)
        }
        Instruction::Exit => {
            eprintln!("  exit");
            let end = pattern.steps.len();
            meta.pattern_step = Some(end);
            meta.save(config)?;
            Ok(end)
        }
        Instruction::Shell { command } => {
            let label = step_label(step, pc);
            eprintln!("  [{}] $ {}", label, command);
            let run_dir = Meta::run_dir(config, name);
            let status = std::process::Command::new("bash")
                .arg("-c")
                .arg(command)
                .env("LOOM_NAME", name)
                .env("LOOM_DIR", config.state_dir().to_string_lossy().as_ref())
                .env("LOOM_RUN_DIR", run_dir.to_string_lossy().as_ref())
                .status()?;
            if !status.success() {
                bail!("shell command exited with {}", status);
            }
            let next = pc + 1;
            meta.pattern_step = Some(next);
            meta.save(config)?;
            Ok(next)
        }
    }
}
