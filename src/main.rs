mod claude;
mod config;
mod loom;
mod meta;
mod snapshot;

use anyhow::{bail, Context, Result};
use std::path::Path;
use config::Config;
use meta::Meta;

struct Flags {
    model: Option<String>,
    nudge: Option<String>,
    dir: Option<String>,
    force: bool,
    input: bool,
    positional: Vec<String>,
}

fn extract_flags(args: Vec<String>) -> Flags {
    let mut model = None;
    let mut nudge = None;
    let mut dir = None;
    let mut force = false;
    let mut input = false;
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--model" => {
                if i + 1 < args.len() {
                    model = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--nudge" => {
                if i + 1 < args.len() {
                    nudge = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--dir" => {
                if i + 1 < args.len() {
                    dir = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--force" | "-f" => { force = true; i += 1; }
            "--input" | "-i" => { input = true; i += 1; }
            _ => { positional.push(args[i].clone()); i += 1; }
        }
    }
    Flags { model, nudge, dir, force, input, positional }
}

fn main() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    if raw_args.is_empty() {
        print_usage();
        return Ok(());
    }

    let flags = extract_flags(raw_args);
    let args = &flags.positional;

    if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
        print_usage();
        return Ok(());
    }

    if args[0] == "init" {
        return cmd_init(&flags);
    }

    let config = Config::load(flags.dir.as_deref())?;

    match args[0].as_str() {
        "list" | "ls" => list_threads(&config),
        "agents" => {
            let sub = args.get(1).map(|s| s.as_str());
            let sub = match sub {
                Some("show") => args.get(2).map(|s| s.as_str()).or(Some("show")),
                other => other,
            };
            cmd_agents(&config, sub)
        }
        "patterns" | "p" => cmd_patterns(&config, &args[1..]),
        "thread" | "t" => {
            if args.len() < 2 {
                bail!("usage: loom thread <name> [command]");
            }
            if args[1] == "list" || args[1] == "ls" {
                return list_threads(&config);
            }
            let name = &args[1];
            let rest = &args[2..];
            thread_cmd(&config, name, rest, &flags)
        }
        _ => {
            bail!("unknown command '{}' -- try `loom help`", args[0]);
        }
    }
}

fn thread_cmd(config: &Config, name: &str, args: &[String], flags: &Flags) -> Result<()> {
    if args.is_empty() {
        return show(config, name);
    }

    let cmd = &args[0];

    match cmd.as_str() {
        "do" => {
            if args.len() < 2 {
                bail!("usage: loom t <name> do <agent> [hears <source>] [\"nudge\"]");
            }
            let speaker = &args[1];
            let remaining = &args[2..];
            let (source, positional_nudge) = parse_turn_args(remaining)?;
            let nudge = flags.nudge.as_deref().or(positional_nudge.as_deref());

            // LOOM_MODEL env var as fallback for model override (for scripts)
            let env_model = std::env::var("LOOM_MODEL").ok();
            let model_override = flags.model.as_deref()
                .or(env_model.as_deref())
                .filter(|s| !s.is_empty());

            loom::run_turn(config, name, speaker, source.as_deref(), nudge, model_override, None)
        }

        "show" => show(config, name),
        "read" => {
            let turn_num = args.get(1).and_then(|s| s.parse().ok());
            read_turn(config, name, turn_num, flags.input)
        }
        "snapshot" => {
            let meta = Meta::read(config, name)?;
            snapshot::auto_snapshot(config, name, &meta)
        }
        "rewind" => {
            let n = args.get(1).ok_or_else(|| anyhow::anyhow!("usage: loom t <name> rewind <N>"))?;
            snapshot::rewind(config, name, n)
        }
        "delete" | "rm" => delete_thread(config, name),
        "reset" | "clear" => reset_thread(config, name),

        _ => {
            bail!("unknown thread command '{}' -- try `loom help`", cmd);
        }
    }
}

fn parse_turn_args(args: &[String]) -> Result<(Option<String>, Option<String>)> {
    if args.is_empty() {
        return Ok((None, None));
    }

    if args[0] == "hears" {
        if args.len() < 2 {
            bail!("usage: loom t <name> <agent> hears <source> [nudge]");
        }
        let source = args[1].clone();
        let nudge = if args.len() > 2 { Some(args[2..].join(" ")) } else { None };
        Ok((Some(source), nudge))
    } else {
        Ok((None, Some(args.join(" "))))
    }
}

fn list_threads(config: &Config) -> Result<()> {
    let runs_dir = config.runs_dir();
    if !runs_dir.exists() {
        eprintln!("no threads yet");
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&runs_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.is_empty() {
        eprintln!("no threads yet");
        return Ok(());
    }

    entries.sort_by(|a, b| {
        let ta = a.metadata().and_then(|m| m.modified()).ok();
        let tb = b.metadata().and_then(|m| m.modified()).ok();
        tb.cmp(&ta)
    });

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        match Meta::read(config, &name) {
            Ok(meta) => {
                let agents: Vec<_> = meta.agents.keys().map(|s| s.as_str()).collect();
                let cost: f64 = meta.thread.iter().filter_map(|t| t.cost_usd).sum();
                println!("  {} -- {} turns, {} ${:.3}",
                    name, meta.thread.len(),
                    agents.join(", "), cost);
            }
            Err(_) => {
                println!("  {} -- (corrupt meta)", name);
            }
        }
    }
    Ok(())
}

fn show(config: &Config, name: &str) -> Result<()> {
    let meta = Meta::read(config, name)?;
    let agents: Vec<_> = meta.agents.keys().collect();

    println!("{} ({} turns, {} agents: {})",
        name, meta.thread.len(), agents.len(),
        agents.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));

    let mut total_cost = 0.0;
    let mut total_elapsed = 0.0;
    for turn in &meta.thread {
        let stage = turn.stage.as_ref().map(|s| format!(" ({})", s)).unwrap_or_default();
        let source = turn.source.as_ref().map(|s| format!(" < {}", s)).unwrap_or_default();
        let nudge = turn.nudge.as_ref().map(|n| {
            let preview = if n.len() > 40 { &n[..40] } else { n };
            format!(" \"{}\"", preview)
        }).unwrap_or_default();
        let cost = turn.cost_usd.map(|c| format!(" ${:.3}", c)).unwrap_or_default();

        println!("  [{}] {}{}{}{} ({:.0}s, {} chars{})",
            turn.turn, turn.agent, stage, source, nudge, turn.elapsed_s, turn.chars, cost);

        total_cost += turn.cost_usd.unwrap_or(0.0);
        total_elapsed += turn.elapsed_s;
    }

    if !meta.thread.is_empty() {
        println!("  ────────────────────────");
        println!("  total: {:.0}s, ${:.3}", total_elapsed, total_cost);
    }

    Ok(())
}

fn read_turn(config: &Config, name: &str, turn_num: Option<usize>, show_input: bool) -> Result<()> {
    let meta = Meta::read(config, name)?;

    let turn = match turn_num {
        Some(n) => meta.thread.iter().find(|t| t.turn == n)
            .ok_or_else(|| anyhow::anyhow!("turn {} not found", n))?,
        None => meta.thread.last()
            .ok_or_else(|| anyhow::anyhow!("no turns yet"))?,
    };

    let path = if show_input {
        Meta::run_dir(config, name).join(format!("turn-{}-{}.input.md", turn.turn, turn.agent))
    } else {
        Meta::turn_file(config, name, turn.turn, &turn.agent)
    };
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    println!("{}", content);
    Ok(())
}

fn delete_thread(config: &Config, name: &str) -> Result<()> {
    let run_dir = Meta::run_dir(config, name);
    if !run_dir.exists() {
        bail!("thread '{}' not found", name);
    }
    let meta = Meta::read(config, name)?;
    snapshot::cleanup_sessions(&meta)?;
    std::fs::remove_dir_all(&run_dir)?;
    eprintln!("deleted {}", name);
    Ok(())
}

fn reset_thread(config: &Config, name: &str) -> Result<()> {
    let run_dir = Meta::run_dir(config, name);
    if !run_dir.exists() {
        bail!("thread '{}' not found", name);
    }
    let meta = Meta::read(config, name)?;
    snapshot::cleanup_sessions(&meta)?;

    let new_meta = Meta::new(name, meta.snapshots);

    for entry in std::fs::read_dir(&run_dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else {
            std::fs::remove_file(&p)?;
        }
    }

    new_meta.save(config)?;
    eprintln!("reset {}", name);
    Ok(())
}

fn cmd_patterns(config: &Config, args: &[String]) -> Result<()> {
    let patterns_dir = config.patterns_dir();

    if args.is_empty() {
        // loom patterns — list scripts
        if !patterns_dir.exists() {
            eprintln!("no patterns directory: {}", patterns_dir.display());
            return Ok(());
        }
        let mut entries: Vec<_> = std::fs::read_dir(&patterns_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();
        if entries.is_empty() {
            eprintln!("no patterns in {}", patterns_dir.display());
            return Ok(());
        }
        entries.sort_by_key(|e| e.file_name());
        for entry in &entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let stem = name.trim_end_matches(".sh");
            println!("  {}", stem);
        }
        return Ok(());
    }

    match args[0].as_str() {
        "run" => {
            let name = args.get(1)
                .ok_or_else(|| anyhow::anyhow!("usage: loom patterns run <name> [args...]"))?;
            let script = resolve_pattern(config, name)?;
            let pass_through: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();
            let status = std::process::Command::new("bash")
                .arg(&script)
                .args(&pass_through)
                .status()?;
            if !status.success() {
                bail!("script exited with {}", status);
            }
            Ok(())
        }
        "show" => {
            let name = args.get(1)
                .ok_or_else(|| anyhow::anyhow!("usage: loom patterns show <name>"))?;
            let script = resolve_pattern(config, name)?;
            let content = std::fs::read_to_string(&script)?;
            println!("{}", content);
            Ok(())
        }
        _ => bail!("usage: loom patterns [run <name> [args...] | show <name>]"),
    }
}

fn resolve_pattern(config: &Config, name: &str) -> Result<String> {
    let dir = config.patterns_dir();
    // Exact match
    let exact = dir.join(name);
    if exact.is_file() {
        return Ok(exact.to_string_lossy().to_string());
    }
    // Try .sh
    let with_sh = dir.join(format!("{}.sh", name));
    if with_sh.is_file() {
        return Ok(with_sh.to_string_lossy().to_string());
    }
    bail!("pattern '{}' not found in {}", name, dir.display())
}

fn cmd_agents(config: &Config, sub: Option<&str>) -> Result<()> {
    if let Some(name) = sub {
        let name = if name == "show" { return bail_no_agent_name(); } else { name };
        return show_agent(config, name);
    }

    if config.agents_map.is_empty() {
        eprintln!("no agents in {}", config.agents_dir().display());
        return Ok(());
    }
    let mut names: Vec<_> = config.agents_map.keys().collect();
    names.sort();
    for (i, name) in names.iter().enumerate() {
        if i > 0 { println!(); }
        let agent = &config.agents_map[*name];
        let model = agent.model.as_deref().unwrap_or(&config.model);
        println!("  {}  ({})", name, model);
        let resolved = claude::resolve_at_refs(&agent.system_prompt, &config.agents_dir());
        let preview: String = resolved.trim().lines()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        let preview = if preview.len() > 300 { format!("{}...", &preview[..300]) } else { preview };
        println!("    {}", preview);
    }
    Ok(())
}

fn bail_no_agent_name() -> Result<()> {
    anyhow::bail!("usage: loom agents show <name>")
}

fn show_agent(config: &Config, name: &str) -> Result<()> {
    let agent = config.require_agent(name)?;
    let model = agent.model.as_deref().unwrap_or(&config.model);

    println!("{}", name);
    println!("  model: {}", model);
    println!("  source: {}", agent.system_prompt.trim());

    let resolved = claude::resolve_at_refs(&agent.system_prompt, &config.agents_dir());
    println!();
    for line in resolved.trim().lines() {
        println!("  {}", line);
    }

    Ok(())
}

fn cmd_init(flags: &Flags) -> Result<()> {
    let target = flags.positional.get(1).map(|s| s.as_str()).unwrap_or(".");
    let base = Path::new(target);

    for sub in &["agents/prompts", "patterns"] {
        std::fs::create_dir_all(base.join(sub))?;
    }

    std::fs::create_dir_all(base.join(".loom").join("runs"))?;

    let yaml_path = base.join("loom.yaml");
    if yaml_path.exists() && !flags.force {
        eprintln!("loom.yaml already exists -- use --force to overwrite");
    } else {
        let content = "model: sonnet\ntimeout: 900\nworkshop: .\nsnapshots: true\n";
        std::fs::write(&yaml_path, content)?;
        eprintln!("wrote {}", yaml_path.display());
    }

    eprintln!("initialized {}/", target);
    eprintln!("  agents/          -- agent definitions and prompt files");
    eprintln!("  patterns/        -- bash scripts (orchestration)");
    eprintln!("  .loom/runs/      -- thread state");
    Ok(())
}

fn print_usage() {
    eprintln!("loom -- multi-agent turn engine");
    eprintln!();
    eprintln!("setup:");
    eprintln!("  loom init [path]                                         # scaffold workspace");
    eprintln!();
    eprintln!("threads:");
    eprintln!("  loom list                                                # list all threads");
    eprintln!("  loom thread <name> do <agent> [\"nudge\"]                  # agent speaks");
    eprintln!("  loom thread <name> do <agent> hears <other> [\"nudge\"]    # agent hears another");
    eprintln!("  loom thread <name> do <agent> hears all [\"nudge\"]        # agent hears full thread");
    eprintln!();
    eprintln!("inspection:");
    eprintln!("  loom thread <name> show                                  # thread summary");
    eprintln!("  loom thread <name> read [turn]                           # print turn output");
    eprintln!("  loom thread <name> read [turn] --input                   # print turn input");
    eprintln!();
    eprintln!("state:");
    eprintln!("  loom thread <name> snapshot                              # manual snapshot");
    eprintln!("  loom thread <name> rewind <N>                            # restore snapshot");
    eprintln!("  loom thread <name> reset                                 # wipe turns");
    eprintln!("  loom thread <name> delete                                # remove thread");
    eprintln!();
    eprintln!("patterns:");
    eprintln!("  loom patterns                                            # list patterns");
    eprintln!("  loom patterns run <name> [args...]                       # run pattern script");
    eprintln!("  loom patterns show <name>                                # show pattern source");
    eprintln!();
    eprintln!("agents:");
    eprintln!("  loom agents                                              # list agents");
    eprintln!("  loom agents show <name>                                  # show agent details");
    eprintln!();
    eprintln!("shortcuts: t = thread, p = patterns, ls = list, rm = delete, clear = reset");
    eprintln!();
    eprintln!("flags:");
    eprintln!("  --model <model>        override model for this turn");
    eprintln!("  --dir <path>           directory containing loom.yaml");
    eprintln!("  --input, -i            show input instead of output (for read)");
    eprintln!();
    eprintln!("env:");
    eprintln!("  LOOM_MODEL             model override (for scripts calling loom)");
}
