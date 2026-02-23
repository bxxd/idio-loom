# DEVELOPER.md — loom

Binary: `target/release/loom` | Library: `idio_loom` (Rust) + `idio_loom` (Python in `python/`)

## Commands

```bash
make build              # cargo build --release
make install            # build + install to ~/.local/bin/loom
make deploy-dev         # build + install to idio-dev-ibook tenant
make deploy-prod        # build + install to idio-prod-trawler tenant
make fmt                # cargo fmt
make lint               # cargo clippy -- -D warnings
make test               # cargo test
make all                # fmt + lint + test
```

## Architecture

CLI binary + Rust library crate. `Thread` is the single public API. CLI is a thin adapter that parses args and calls `Thread` methods.

```
src/
  lib.rs        Crate root — re-exports public modules
  thread.rs     Thread struct — THE public API (run, show, read, rewind, delete, reset, list)
  main.rs       CLI adapter — arg parsing, display, patterns, agents, init
  config.rs     loom.yaml loading, agent scanning, path resolution
  meta.rs       meta.json types (Meta, AgentState, Turn), read/write
  loom.rs       Turn execution: message building, claude invocation, state updates
  claude.rs     Claude subprocess wrapper (new/resume), system prompt assembly, @file resolution
  snapshot.rs   Snapshot/rewind with session JSONL sync

python/
  idio_loom/__init__.py   Python wrapper (subprocess over CLI)
  pyproject.toml          Package metadata
```

### One True Path

```
Thread::run_opts() → loom::run_turn() → claude::claude_new/resume()
     ↑                                        ↑
  CLI (main.rs)                          timeout enforcement
  Rust crate dep (ibook-requests)        (setsid + process group kill)
  Python subprocess (trawler)
```

### Module responsibilities

- **thread.rs** — `run()`, `run_opts()`, `step()`, `last_output()`, `read_turn()`, `show()`, `rewind()`, `delete()`, `reset()`, `list_threads()`. Returns data, never prints.
- **main.rs** — Arg parsing, display, patterns dispatch, agents listing, init. Calls `Thread` and prints.
- **config.rs** — Loads `loom.yaml`, scans `agents/*.yaml` into `agents_map`. Path resolution methods. Pure config, no side effects.
- **meta.rs** — `Meta::read()` (errors on missing), `Meta::read_or_create()` (manual mode), `Meta::save()`. Owns meta.json schema.
- **loom.rs** — Builds messages from source/nudge, invokes claude, records turn, saves trace files. Returns `TurnResult` (output, session_id, elapsed, cost). One entry point: `run_turn()`. `hears all` filters self out and includes nudges.
- **claude.rs** — `claude_new()` / `claude_resume()` / `build_system_prompt()`. Timeout via `setsid` + process group kill. `resolve_at_refs()` for `@file` expansion.
- **snapshot.rs** — Captures/restores state + claude session JSONLs from `~/.claude/projects/`. `cleanup_sessions()` for delete/reset.

### Path resolution

| What | Method | Resolves to |
|------|--------|-------------|
| Workshop | `workshop_dir()` | `{home}/{workshop}` or absolute |
| Agents | `agents_dir()` | `{workshop}/agents/` |
| Agent prompts | `agent_prompts_dir()` | `{workshop}/agents/prompts/` |
| Patterns | `patterns_dir()` | `{workshop}/patterns/` |
| State | `state_dir()` | `{home}/{state}` or absolute |
| Runs | `runs_dir()` | `{state}/runs/` |

`home` = directory containing loom.yaml. Both `workshop` and `state` support absolute or relative paths.

## CLI

```
loom
├── --version (-V)
├── init [path] [--force]
├── list (ls)
├── agents [show <name>]
├── patterns (p) [<name> show | <name> run [args...]]
└── thread (t) <name>
    ├── do <agent> [hears <source>] ["nudge"]
    ├── show (default)
    ├── read [turn] [--input]
    ├── snapshot
    ├── rewind <N>
    ├── reset (clear)
    └── delete (rm)
```

**Flags**: `--model <model>`, `--use-system <agent>`, `--dir <path>`, `--input`/`-i`, `--force`/`-f`

**Env**: `LOOM_MODEL` — model override (same as `--model`, for use in scripts)

**Aliases**: `t`=thread, `p`=patterns, `ls`=list, `rm`=delete, `clear`=reset

## Key behaviors

**Message routing**: `do <agent> "nudge"` sends nudge as input (first turn creates session, subsequent resume). `hears <other>` routes that agent's last output. `hears all` concatenates full thread excluding self, with nudges formatted as `[user → agent]: text`.

**Sessions**: Each agent gets its own claude session ID in `meta.json`. First turn creates, subsequent resume. Full conversation context maintained.

**Model resolution**: `--model` flag > `LOOM_MODEL` env > agent YAML `model:` > loom.yaml `model:`

**@file resolution**: `@filename` in agent system prompts resolves from `agents/prompts/`. In pattern nudges resolves from `patterns/`. Can be whole value or inline within text.

**--use-system**: `--use-system writer` uses writer's system prompt for one turn while keeping the current agent's session context.

**Scratchpad**: Shared per-thread directory with `RESEARCH.md`, `EVIDENCE.md`, `THESES.md`, `NOTES.md`. Path injected into system prompts automatically.

**Snapshots**: Copy meta.json, scratchpad/, turn files, AND claude session JSONLs from `~/.claude/projects/`. Rewind restores all, rolling back agent memory. Session dir derived from `config.claude_cwd()` (not process cwd) to handle `cwd:` overrides. Missing session files produce warnings.

**Session JSONL location**: `~/.claude/projects/{slug}/{session_id}.jsonl` where slug comes from claude working directory (`cwd:` in loom.yaml): `/foo/bar` → `-foo-bar`. Resolved via `config.claude_cwd()`.

### Turn tracing

Each turn saves files in the run directory:

| File | When | Content |
|------|------|---------|
| `{agent}.system.md` | First turn per agent | Full system prompt sent to Claude |
| `turn-{N}-{agent}.input.md` | Every turn | Assembled user message (hears + nudge) |
| `turn-{N}-{agent}.md` | Every turn | Agent output |

Read with `loom t <name> read [turn]` (output) or `loom t <name> read [turn] --input` (input).

### Patterns

Bash scripts in `workshop/patterns/`. `loom p <name> run` invokes via `bash`, passing args through. No `.sh` extension needed. Each script handles its own thread naming, model selection, and turn sequencing. No Rust-side interpreter.

### Design decisions

- **No frameworks** — Raw arg parsing, no clap. Small binary, instant startup.
- **Agents as files** — `agents/{name}.yaml`, all loaded on startup. Drop a file to add an agent.
- **Workshop/state separation** — Workshop (agents, patterns) is deployable. State (runs, snapshots) is per-tenant.

### Versioning

`build.rs` auto-increments `~/.loom_build_number` on each compile, embeds it + git hash. Format: `0.2.0.5 (747a026)`.

### Debug logging

Turn execution emits `eprintln!` prefixed `[loom]`:
- Turn start: agent, model, system override, new/resume, timeout, prompt sizes
- Claude command: args, cwd, stdin preview, timeout
- Claude result: session ID, duration, cost, output chars, stderr (first 30 lines)

Logs go to stderr → journalctl via systemd.

## Using as a Rust library

```toml
idio-loom = { git = "ssh://git@github.com/bxxd/idio-loom.git" }
# or: idio-loom = { path = "../loom" }
```

```rust
use idio_loom::config::Config;
use idio_loom::thread::{Thread, RunOpts};

let config = Config::load(Some("/path/to/workspace"))?;
let t = Thread::new(&config, "my-thread");

let result = t.run("axe", Some("what is 2+2"))?;
println!("{}", result.output);

let result = t.run_opts(&RunOpts {
    agent: "axe",
    nudge: Some("analyze this"),
    source: Some("bobby"),    // or "all"
    model: Some("opus"),
    system: None,             // --use-system override
    stage: Some("research"),
    timeout: Some(1800),
})?;
// result: output, session_id, elapsed_s, cost_usd

t.step()?;                       // turn count
t.last_output()?;                // last turn content
t.show()?;                       // thread summary
t.read_turn(Some(2), false)?;   // turn 2 output
t.rewind("3")?;
t.reset()?;
t.delete()?;
```

## Using as a Python package

```bash
pip install git+ssh://git@github.com/bxxd/idio-loom.git#subdirectory=python
# or: pip install -e /path/to/loom/python
```

```python
from idio_loom import Loom, LoomTimeout, LoomError

loom = Loom(workspace="/path/to/workspace")
t = loom.thread("my-thread", model="sonnet", timeout=900)

output = t.do("axe", "what is 2+2")
output = t.do("bobby", hears="axe", nudge="attack this")
output = t.do("axe", "write it up", system="writer")

print(t.step)          # turn count
print(t.last_output)   # last turn content
t.rewind(3)
t.delete()
```

Wraps CLI via subprocess. Requires `loom` binary on PATH.

## Claude Code Version

**Service users pinned to Claude Code 2.0.76.**

2.1.x bug: subagents share parent's MCP SSE connection → deadlock when parent blocks on Task tool. Caused 4+ hour hang on Feb 19, 2026.

```bash
# Install/downgrade (same for idio-dev-trawler)
sudo rm -rf /home/idio-dev-ibook/.local/bin/claude /home/idio-dev-ibook/.claude/
sudo -u idio-dev-ibook bash -c 'curl -fsSL https://claude.ai/install.sh | bash -s -- 2.0.76'
sudo -u idio-dev-ibook mkdir -p /home/idio-dev-ibook/.claude
sudo tee /home/idio-dev-ibook/.claude/settings.json <<< '{"skipDangerousModePermissionPrompt":true,"env":{"DISABLE_AUTOUPDATER":"1"}}'
sudo chown idio-dev-ibook:ubuntu /home/idio-dev-ibook/.claude/settings.json
```

**Do not upgrade past 2.0.76 without testing subagent MCP behavior.**

## Dependencies

| Crate | Purpose |
|-------|---------|
| anyhow | Error handling |
| serde, serde_json | Meta.json serialization |
| serde_yaml | Config + agent loading |
| chrono | Timestamp display |
| libc | Process group management (setsid, kill) for timeout |

## Testing

Smoke test from a workspace with `loom.yaml` and `.mcp.json`:

```bash
cd /var/idio-shared/dev/ibook

loom p test run TEST-1 "AAPL: earnings analysis"

loom t TEST-2 do axe "what is 2+2" --model haiku
loom t TEST-2 do bobby hears axe --model haiku
loom t TEST-2 do bobby hears all --model haiku
loom t TEST-2 show
loom t TEST-2 read
loom t TEST-2 read --input
loom t TEST-2 read 0 --input

loom agents
loom agents show axe
loom patterns
loom p idio3 show

loom t TEST-1 rm
loom t TEST-2 rm
```
