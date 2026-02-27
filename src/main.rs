use idio_loom::{claude, config, meta, snapshot, thread};

use anyhow::{bail, Result};
use config::Config;
use meta::Meta;
use std::path::Path;
use thread::Thread;

struct Flags {
    model: Option<String>,
    use_system: Option<String>,
    nudge: Option<String>,
    dir: Option<String>,
    force: bool,
    input: bool,
    positional: Vec<String>,
}

fn extract_flags(args: Vec<String>) -> Flags {
    let mut model = None;
    let mut use_system = None;
    let mut nudge = None;
    let mut dir = None;
    let mut force = false;
    let mut input = false;
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--use-system" => {
                if i + 1 < args.len() {
                    use_system = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
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
            "--force" | "-f" => {
                force = true;
                i += 1;
            }
            "--input" | "-i" => {
                input = true;
                i += 1;
            }
            _ => {
                positional.push(args[i].clone());
                i += 1;
            }
        }
    }
    Flags {
        model,
        use_system,
        nudge,
        dir,
        force,
        input,
        positional,
    }
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

    if matches!(args[0].as_str(), "version" | "--version" | "-V") {
        println!("loom {}", idio_loom::version());
        return Ok(());
    }

    if args[0] == "init" {
        return cmd_init(&flags);
    }

    let config = Config::load(flags.dir.as_deref())?;

    match args[0].as_str() {
        "list" | "ls" => {
            println!("{}", thread::list_threads(&config)?);
            Ok(())
        }
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
                println!("{}", thread::list_threads(&config)?);
                return Ok(());
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
    let t = Thread::new(config, name);

    if args.is_empty() {
        println!("{}", t.show()?);
        return Ok(());
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
            let model_override = flags
                .model
                .as_deref()
                .or(env_model.as_deref())
                .filter(|s| !s.is_empty());

            let model_display = model_override
                .map(String::from)
                .unwrap_or_else(|| config.agent_model(speaker));
            print_turn_start(
                Meta::read_or_create(config, name, config.snapshots)?.next_turn_number(),
                None,
                speaker,
                source.as_deref(),
                nudge,
                &model_display,
                flags.use_system.as_deref(),
            );

            let opts = thread::RunOpts {
                agent: speaker,
                nudge,
                source: source.as_deref(),
                system: flags.use_system.as_deref(),
                model: model_override,
                stage: None,
                timeout: None,
            };
            let result = t.run_opts(&opts)?;
            print_turn_done(result.elapsed_s, result.output.len(), result.cost_usd);
            println!("{}", result.output);
            Ok(())
        }

        "show" => {
            println!("{}", t.show()?);
            Ok(())
        }
        "read" => {
            let turn_num = args.get(1).and_then(|s| s.parse().ok());
            let content = t.read_turn(turn_num, flags.input)?;
            println!("{}", content);
            Ok(())
        }
        "snapshot" => {
            let meta = Meta::read(config, name)?;
            snapshot::auto_snapshot(config, name, &meta)
        }
        "rewind" => {
            let n = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("usage: loom t <name> rewind <N>"))?;
            t.rewind(n)?;
            eprintln!("rewound {}", name);
            Ok(())
        }
        "delete" | "rm" => {
            t.delete()?;
            eprintln!("deleted {}", name);
            Ok(())
        }
        "reset" | "clear" => {
            t.reset()?;
            eprintln!("reset {}", name);
            Ok(())
        }

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
        let nudge = if args.len() > 2 {
            Some(args[2..].join(" "))
        } else {
            None
        };
        Ok((Some(source), nudge))
    } else {
        Ok((None, Some(args.join(" "))))
    }
}

fn cmd_patterns(config: &Config, args: &[String]) -> Result<()> {
    let patterns_dir = config.patterns_dir();

    if args.is_empty() {
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

    let name = &args[0];
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");

    match sub {
        "show" => {
            let script = resolve_pattern(config, name)?;
            let content = std::fs::read_to_string(&script)?;
            println!("{}", content);
            Ok(())
        }
        "run" => {
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
        _ => bail!(
            "unknown patterns command '{}' -- try: loom p <name> [show|run]",
            sub
        ),
    }
}

fn resolve_pattern(config: &Config, name: &str) -> Result<String> {
    let dir = config.patterns_dir();
    let exact = dir.join(name);
    if exact.is_file() {
        return Ok(exact.to_string_lossy().to_string());
    }
    let with_sh = dir.join(format!("{}.sh", name));
    if with_sh.is_file() {
        return Ok(with_sh.to_string_lossy().to_string());
    }
    bail!("pattern '{}' not found in {}", name, dir.display())
}

fn cmd_agents(config: &Config, sub: Option<&str>) -> Result<()> {
    if let Some(name) = sub {
        if name == "show" {
            bail!("usage: loom agents show <name>");
        }
        return show_agent(config, name);
    }

    if config.agents_map.is_empty() {
        eprintln!("no agents in {}", config.agents_dir().display());
        return Ok(());
    }
    let mut names: Vec<_> = config.agents_map.keys().collect();
    names.sort();
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let agent = &config.agents_map[*name];
        let model = agent.model.as_deref().unwrap_or(&config.model);
        println!("  {}  ({})", name, model);
        let resolved = claude::resolve_at_refs(&agent.system_prompt, &config.agents_dir());
        let preview: String = resolved
            .trim()
            .lines()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        let preview = if preview.len() > 300 {
            let mut end = 300;
            while !preview.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &preview[..end])
        } else {
            preview
        };
        println!("    {}", preview);
    }
    Ok(())
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

    // Scaffold default agents
    let agents_dir = base.join("agents");
    let prompts_dir = agents_dir.join("prompts");

    let axe_yaml = agents_dir.join("axe.yaml");
    if !axe_yaml.exists() || flags.force {
        std::fs::write(
            &axe_yaml,
            "model: sonnet\nsystem_prompt: \"@prompts/axe.txt\"\n",
        )?;
        std::fs::write(
            prompts_dir.join("axe.txt"),
            "You are a research analyst. Your job is to investigate topics thoroughly using primary sources.\n\n\
             Standards:\n\
             - Every claim needs a source. Primary data over summaries.\n\
             - Math before narrative. Quantify when possible.\n\
             - If there's no signal, say so. \"No finding\" is a valid output.\n\n\
             Use the tools available to you to pull data, read filings, and verify claims. Write findings to the scratchpad as you go.\n",
        )?;
    }

    let bobby_yaml = agents_dir.join("bobby.yaml");
    if !bobby_yaml.exists() || flags.force {
        std::fs::write(
            &bobby_yaml,
            "model: sonnet\nsystem_prompt: |\n  You are an adversarial reviewer. \
             Attack claims, find gaps, check math.\n  Be specific — which source, which line, which metric.\n  \
             If the analysis holds up, say so. If not, explain specifically why.\n",
        )?;
    }

    let writer_yaml = agents_dir.join("writer.yaml");
    if !writer_yaml.exists() || flags.force {
        std::fs::write(
            &writer_yaml,
            "model: sonnet\nsystem_prompt: |\n  You have the data, research, and discussion. \
             Read everything.\n  Write a clear, well-structured memo in markdown. \
             Lead with the conclusion.\n",
        )?;
    }

    eprintln!("initialized {}/", target);
    eprintln!("  agents/          -- axe, bobby, writer");
    eprintln!("  agents/prompts/  -- system prompts");
    eprintln!("  patterns/        -- bash scripts (orchestration)");
    eprintln!("  .loom/runs/      -- thread state");
    Ok(())
}

fn print_turn_start(
    turn_n: usize,
    stage: Option<&str>,
    speaker: &str,
    source: Option<&str>,
    nudge: Option<&str>,
    model: &str,
    system_override: Option<&str>,
) {
    eprint!("[turn {}] ", turn_n);
    if let Some(sn) = stage {
        eprint!("[{}] ", sn);
    }
    eprint!("{} ", speaker);
    if let Some(src) = source {
        eprint!("hears {} ", src);
    }
    if let Some(n) = nudge {
        let preview = if n.len() > 60 {
            let mut e = 60;
            while !n.is_char_boundary(e) {
                e -= 1;
            }
            &n[..e]
        } else {
            n
        };
        eprint!("\"{}\" ", preview);
    }
    if let Some(sys) = system_override {
        eprint!("[system: {}] ", sys);
    }
    eprintln!("({}) ...", model);
}

fn print_turn_done(elapsed: f64, chars: usize, cost: Option<f64>) {
    let now = chrono::Local::now().format("%H:%M:%S");
    eprintln!(
        "  {:.0}s, {} chars, ${:.3} — done at {}",
        elapsed,
        chars,
        cost.unwrap_or(0.0),
        now
    );
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
    eprintln!(
        "  loom thread <name> do <agent> hears all [\"nudge\"]        # agent hears full thread"
    );
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
    eprintln!("  loom patterns <name> show                                # show pattern source");
    eprintln!("  loom patterns <name> run [args...]                       # run pattern script");
    eprintln!();
    eprintln!("agents:");
    eprintln!("  loom agents                                              # list agents");
    eprintln!("  loom agents show <name>                                  # show agent details");
    eprintln!();
    eprintln!("shortcuts: t = thread, p = patterns, ls = list, rm = delete, clear = reset");
    eprintln!();
    eprintln!("flags:");
    eprintln!("  --model <model>        override model for this turn");
    eprintln!("  --use-system <agent>   use another agent's system prompt for this turn");
    eprintln!("  --dir <path>           directory containing loom.yaml");
    eprintln!("  --input, -i            show input instead of output (for read)");
    eprintln!();
    eprintln!("env:");
    eprintln!("  LOOM_MODEL             model override (for scripts calling loom)");
}
