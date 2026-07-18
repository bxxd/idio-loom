# DEVELOPER.md — loom

Binary: `target/release/loom` | Library: `idio_loom` (Rust) + `idio_loom` (Python in `python/`)

## Commands

```bash
make build              # cargo build --release
make install            # build + install to ~/.local/bin/loom
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
  loom.rs       Turn execution: message building, backend dispatch, state updates
  prompt.rs     System-prompt assembly + @file resolution (backend-agnostic)
  exec.rs       Subprocess spawn + process-group timeout kill (backend-agnostic)
  backend/
    mod.rs      Backend trait (the port) + TurnRequest/TurnOutcome + factory
    claude.rs   ClaudeBackend — `claude` CLI, ~/.claude/projects sessions
    pi.rs       PiBackend — `pi`/vizipi CLI, loom-owned --session-dir
  snapshot.rs   Snapshot/rewind; delegates session sync to the active Backend

python/
  idio_loom/__init__.py   Python wrapper (subprocess over CLI)
  pyproject.toml          Package metadata
```

### Backends (the agent port)

Loom orchestrates *turns* and doesn't care which coding agent runs them. The
`Backend` trait in `src/backend/mod.rs` is the port; each impl adapts one agent
CLI. Pick one with `backend:` in `loom.yaml` (`claude` default, or `pi`).

| Concern | `claude` | `pi` |
|---------|----------|------|
| Headless flag | `-p --output-format json` | `--print --mode text` |
| Session id | claude assigns; loom reads it back | **loom assigns** via `--session-id` |
| Resume | `--resume <id>` | re-pass `--session-id <id>` |
| System prompt | `--append-system-prompt` | `--append-system-prompt` |
| Transcript home | `~/.claude/projects/{slug}/{id}.jsonl` | loom-owned `{state}/pi-sessions/{ts}_{id}.jsonl` via `--session-dir` |
| Binary override | (find ~/.local/bin/claude) | `backend_bin:` in loom.yaml (e.g. `vizipi`) |

Because pi lets loom choose both the session id and the session directory, the
pi snapshot/rewind path is simpler than claude's: no per-cwd slug to reverse
engineer, no branding guesswork (`~/.pi` vs `~/.vizipi`). Snapshotting is "copy
a file we already named."

Adding a third backend = one file implementing `Backend` + one arm in
`backend::for_config`. `loom.rs` and `snapshot.rs` never change.

### One True Path

```
Thread::run_opts() → loom::run_turn() → backend.run_turn() → exec::run_with_timeout()
     ↑                       │                  ↑                      ↑
  CLI (main.rs)              │           Backend trait          timeout enforcement
  Rust crate dep            │           (claude | pi)          (setsid + pgroup kill)
  Python subprocess         └─ prompt::build_system_prompt()
```

### Module responsibilities

- **thread.rs** — `run()`, `run_opts()`, `step()`, `last_output()`, `read_turn()`, `show()`, `rewind()`, `delete()`, `reset()`, `list_threads()`. Returns data, never prints.
- **main.rs** — Arg parsing, display, patterns dispatch, agents listing, init. Calls `Thread` and prints.
- **config.rs** — Loads `loom.yaml`, scans `agents/*.yaml` into `agents_map`. Owns `backend`/`backend_bin` fields. Path resolution methods. Pure config, no side effects.
- **meta.rs** — `Meta::read()` (errors on missing), `Meta::read_or_create()` (manual mode), `Meta::save()`. Owns meta.json schema.
- **loom.rs** — Builds messages from source/nudge, resolves the `Backend`, records turn, saves trace files. Returns `TurnResult` (output, session_id, elapsed, cost). One entry point: `run_turn()`. `hears all` filters self out and includes nudges.
- **prompt.rs** — `build_system_prompt()` + `resolve_at_refs()` for `@file` expansion. Backend-agnostic.
- **exec.rs** — `run_with_timeout()` (spawn, stdin feed, soft/hard timeout via `setsid` + process-group kill, activity probe), `log_command()`. Backend-agnostic.
- **backend/mod.rs** — `Backend` trait (the port), `TurnRequest`/`TurnOutcome`, `for_config()` factory.
- **backend/claude.rs** — `ClaudeBackend`: `claude` CLI in JSON mode, `~/.claude/projects/{slug}/` sessions + subagent dirs.
- **backend/pi.rs** — `PiBackend`: `pi`/vizipi CLI in text mode, loom-owned `--session-dir`, loom-assigned `--session-id`.
- **snapshot.rs** — Captures/restores state; delegates session-transcript sync to the active `Backend`. `cleanup_sessions()` for delete/reset.

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
- **Workshop/state separation** — Workshop (agents, patterns) is deployable. State (runs, snapshots) is per-workspace.

### Versioning

`build.rs` auto-increments `~/.loom_build_number` on each compile, embeds it + git hash. Format: `0.2.0.5 (747a026)`.

### Debug logging

Turn execution emits `eprintln!` prefixed `[loom]`:
- Turn start: agent, model, system override, new/resume, timeout, prompt sizes
- Claude command: args, cwd, stdin preview, timeout
- Claude result: session ID, duration, cost, output chars, stderr (first 30 lines)

Logs go to stderr.

## State management internals

Loom's key differentiator is full state capture and rewind, including Claude Code's own session memory.

**What gets snapshotted** (after every turn, if `snapshots: true`):
- `meta.json` — session IDs, turn history, agent states
- `scratchpad/` — shared files between agents
- `turn-*.md` / `turn-*.input.md` — all turn traces
- Claude session JSONLs from `~/.claude/projects/{slug}/` — the agent's actual conversation history

**Rewind** restores ALL of the above. When you `rewind 3`, turns 4+ are deleted and Claude's session files are rolled back. The agent literally forgets those turns happened.

**Session JSONL location**: `~/.claude/projects/{slug}/{session_id}.jsonl` where slug = `config.claude_cwd()` path with `/` → `-`. Example: cwd `/home/user/project` → slug `-home-user-project`.

**Why this matters**: Without session rollback, rewinding loom state would leave Claude remembering the "future" turns. The agent would reference deleted context. Snapshot/rewind sync guarantees consistency.

## Testing

```bash
make all                    # fmt + lint + test (29 unit tests)
```

Smoke test from any workspace with `loom.yaml`:

```bash
loom t TEST do axe "what is 2+2? answer briefly" --model haiku
loom t TEST do bobby hears axe "is this correct?" --model haiku
loom t TEST show
loom t TEST read 0
loom t TEST read 1 --input
loom t TEST rm
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| anyhow | Error handling |
| serde, serde_json | Meta.json serialization |
| serde_yaml | Config + agent loading |
| chrono | Timestamp display |
| libc | Process group management (setsid, kill) for timeout |
| dirs | Home directory resolution |
