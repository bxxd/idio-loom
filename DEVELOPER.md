# DEVELOPER.md — idio-loom

## Build

```bash
cargo build --release
```

Binary: `target/release/loom`

## Architecture

```
src/
  main.rs       CLI parsing, subcommand dispatch, display (list/show/read/agents/patterns)
  config.rs     loom.yaml loading, agent scanning from agents/, path resolution
  meta.rs       meta.json data types (Meta, AgentState, Turn), read/write/init
  loom.rs       Turn execution: message building, claude invocation, state updates
  pattern.rs    Pattern YAML loading, stage sequencing (run_all, run_next)
  claude.rs     Claude subprocess wrapper (new/resume), system prompt assembly, @file resolution
  snapshot.rs   Snapshot/rewind with session JSONL sync, session cleanup
```

### Module responsibilities

- **main.rs** — Subcommand dispatch: `list`, `agents`, `patterns`, `thread <name> ...`. Thread subcommands: pattern mode (start/run/next), manual mode (do), inspection (show/read), snapshots, lifecycle (reset/delete). Owns display logic.
- **config.rs** — Loads `loom.yaml`, scans `agents/*.yaml` to populate `agents_map`. Path resolution: `workshop_dir()`, `agents_dir()`, `agent_prompts_dir()`, `patterns_dir()`, `state_dir()`, `runs_dir()`. Pure config, no side effects.
- **meta.rs** — Data model + persistence. `Meta::read()` (errors on missing), `Meta::read_or_create()` (for manual mode), `Meta::save()`, `Meta::init_pattern()`. Owns the meta.json schema.
- **loom.rs** — Core turn orchestrator. Builds messages from source/nudge, invokes claude (new or resume), records turn, saves input/output/system prompt, prints output. One public entry point: `run_turn()`. `hears all` filters self out and includes nudges.
- **pattern.rs** — Pattern YAML schema + stage execution. Calls `loom::run_turn()` per stage, tracks progress via `meta.advance_stage()`. First stage gets intake (CLI nudge) prepended to stage nudge.
- **claude.rs** — Subprocess wrapper. `claude_new()` / `claude_resume()` / `build_system_prompt()`. Also owns `resolve_at_refs()` for `@file` expansion.
- **snapshot.rs** — Captures and restores full state including claude session JSONLs from `~/.claude/projects/`. `cleanup_sessions()` removes session JSONLs for delete/reset.

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

### Turn tracing

Each turn saves three files for debugging:

| File | When | Content |
|------|------|---------|
| `{agent}.system.md` | First turn per agent | Full system prompt sent to Claude |
| `turn-{N}-{agent}.input.md` | Every turn | Assembled user message (hears + nudge) |
| `turn-{N}-{agent}.md` | Every turn | Agent output |

Read with `loom t <name> read [turn]` (output) or `loom t <name> read [turn] --input` (input).

### CLI structure

```
loom
├── init [path]
├── list (ls)
├── agents [show <name>]
├── patterns [show [name] | set <name>]
└── thread (t) <name>
    ├── start [pattern] ["intake"]        # init thread, save intake
    ├── run [pattern] ["intake"]          # start + run all stages (or resume if interrupted)
    ├── next                              # run next pattern stage
    ├── do <agent> [hears <source>] ["nudge"]
    ├── show (default)
    ├── read [turn] [--input]
    ├── snapshot
    ├── rewind <N>
    ├── reset (clear)
    └── delete (rm)
```

Pattern names don't need `.yaml` extension: `loom t X run deep-dive` works.

### Intake

When running a pattern, the CLI argument is the **intake** — the topic/request that gets prepended to stage 0's nudge:

```bash
loom t DOCN run test "DOCN: have a theory that ai will add more demand"
```

Stage 0 (nudge: "research") receives:
```
DOCN: have a theory that ai will add more demand

research
```

Subsequent stages use only their own nudges. Intake is saved in `meta.json` so resumed runs preserve it.

### Run resume

If a `run` is interrupted mid-pattern, re-running picks up where it left off:

```bash
loom t DOCN run              # resumes from current_stage
loom t DOCN run --force      # wipes and starts fresh
```

### Key design decisions

- **No frameworks** — Raw arg parsing, no clap. Keeps binary small and startup instant.
- **Agents as individual files** — Each agent is `agents/{name}.yaml`. All are loaded on startup. No aggregate file. Easy to add/remove agents by dropping files.
- **Workshop/state separation** — Content (agents, patterns) in workshop dir. State (runs, scratchpad, snapshots) in state dir. Workshop is deployable, state is per-tenant.
- **Session management** — Each agent gets a claude session ID stored in `meta.json`. First turn creates session, subsequent turns resume. Session holds full conversation context.
- **hears all** — Concatenates full thread excluding self (agent already has session context). Includes nudges formatted as `[user → agent]: nudge text`.
- **@file resolution** — Context-dependent: agent prompts resolve from `agents/prompts/`, pattern nudges resolve from `patterns/`.
- **Snapshot = state + sessions** — Snapshots copy meta.json, scratchpad/, turn-*.md, *.input.md, *.system.md AND claude session JSONLs from `~/.claude/projects/`. Rewind restores all, rolling back agent memory.

### Session JSONL location

Claude stores sessions at `~/.claude/projects/{slug}/{session_id}.jsonl` where slug is derived from cwd: `/foo/bar` becomes `-foo-bar`.

## Dependencies

| Crate | Purpose |
|-------|---------|
| anyhow | Error handling |
| serde, serde_json | Meta.json serialization |
| serde_yaml | Config + pattern loading |
| chrono | Timestamp display |

## Testing

Smoke test from a workspace with `loom.yaml` and `.mcp.json`:

```bash
cd /var/idio-shared/dev/ibook

# Pattern mode (full run)
loom t TEST-1 run test "AAPL: earnings analysis" --model haiku

# Pattern mode (step by step)
loom t TEST-2 start test "MSFT: cloud growth"
loom t TEST-2 next --model haiku
loom t TEST-2 next --model haiku
loom t TEST-2 show

# Manual mode
loom t TEST-3 do axe "what is 2+2" --model haiku
loom t TEST-3 do bobby hears axe --model haiku
loom t TEST-3 do bobby hears all --model haiku
loom t TEST-3 show
loom t TEST-3 read
loom t TEST-3 read --input
loom t TEST-3 read 0 --input

# Introspection
loom agents
loom agents show axe
loom patterns
loom patterns show

# Cleanup
loom t TEST-1 rm
loom t TEST-2 rm
loom t TEST-3 rm
```

## Workspace setup

Loom runs from a directory containing:
- `loom.yaml` — config (model, workshop path, state path, default pattern)
- `.mcp.json` — MCP server config for claude (optional)

Workshop directory contains:
- `agents/` — agent YAML files + `prompts/` subdirectory
- `patterns/` — pattern YAML files

State goes to `.loom/` (configurable via `state` in loom.yaml).
