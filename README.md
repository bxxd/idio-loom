# Loom

Multi-agent thread engine. Named threads of agent conversations via `claude --resume`, routing messages between agents while preserving session context.

## Install

```bash
make build    # cargo build --release
make install  # copies to ~/.local/bin/loom
```

Python: `pip install -e /path/to/loom/python`
Rust: `idio-loom = { path = "../loom" }` in Cargo.toml

## Quick Start

```bash
loom init                                              # scaffold workspace
loom agents                                            # list agents
loom patterns                                          # list patterns

# Pattern mode
loom p idio3 run "DOCN: met coal"

# Manual mode
loom t TEST-1 do axe "investigate DOCN"
loom t TEST-1 do bobby hears axe "attack this thesis"
loom t TEST-1 do axe hears all
loom t TEST-1 show
```

## CLI

```
loom
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

**Env**: `LOOM_MODEL` — model override for scripts

**Aliases**: `t`=thread, `p`=patterns, `ls`=list, `rm`=delete, `clear`=reset

## Workspace

```
workspace/
├── loom.yaml                          # config
├── .mcp.json                          # MCP config for claude (optional)
├── agents/                            # in workshop dir
│   ├── axe.yaml
│   ├── bobby.yaml
│   └── prompts/
│       └── writer.txt
├── patterns/                          # in workshop dir
│   └── idio3.sh
└── .loom/                             # state dir
    └── runs/
        └── <thread>/
            ├── meta.json
            ├── .last_output
            ├── scratchpad/{RESEARCH,EVIDENCE,THESES,NOTES}.md
            ├── {agent}.system.md      # system prompt (first turn)
            ├── turn-{N}-{agent}.md    # output
            ├── turn-{N}-{agent}.input.md
            └── snapshots/{N}/
```

### loom.yaml

```yaml
model: sonnet              # default model
timeout: 900               # turn timeout in seconds
workshop: .                # path to agents/ and patterns/ (absolute or relative to loom.yaml dir)
state: .loom               # path to state dir (absolute or relative to loom.yaml dir)
snapshots: true            # auto-snapshot after each turn
cwd: /some/path            # claude subprocess working directory (optional, picks up CLAUDE.md/.mcp.json)
system_prompt: |           # global system prompt appended to all agents (optional)
  Extra instructions here.
```

### Agents — agents/*.yaml

Each file = one agent. All loaded automatically.

```yaml
model: opus                                          # model override (optional)
system_prompt: "@prompts/writer.txt"                 # from file, inline text, or mixed
```

**Model resolution**: `--model` flag > `LOOM_MODEL` env > agent YAML > loom.yaml default

**@file resolution**: agent prompts resolve from `agents/prompts/`, pattern nudges from `patterns/`

### Patterns — patterns/*.sh

Bash scripts that orchestrate multi-turn threads. `loom p <name> run` invokes via bash, passing args through. No `.sh` extension needed.

```bash
#!/bin/bash
INTAKE="${1:?usage: example.sh <intake> [model]}"
MODEL="${2:-sonnet}"
NAME="example-$(date +%s)"

loom t "$NAME" do axe "$INTAKE" --model "$MODEL"
loom t "$NAME" do bobby hears axe "attack this thesis" --model "$MODEL"
loom t "$NAME" do axe hears bobby --model "$MODEL"
loom t "$NAME" do axe "write it up" --use-system writer --model "$MODEL"
loom t "$NAME" show
```

## Concepts

**Message routing**: `do <agent> "nudge"` sends nudge as input. `hears <other>` routes that agent's last output. `hears all` concatenates full thread excluding self (includes nudges as `[user → agent]: text`).

**Sessions**: Each agent gets its own claude session. First turn creates it, subsequent turns resume. Full conversation history maintained automatically.

**Scratchpad**: Shared `scratchpad/` directory per thread with `RESEARCH.md`, `EVIDENCE.md`, `THESES.md`, `NOTES.md`. Path injected into system prompts.

**--use-system**: `--use-system writer` makes the current agent use writer's system prompt for one turn while keeping its own session context.

**Snapshots**: Capture full state including claude session JSONLs. `rewind <N>` restores to after turn N, rolling back agent memory.

**Turn tracing**: Each turn saves `turn-{N}-{agent}.md` (output) and `turn-{N}-{agent}.input.md` (input). First turn per agent saves `{agent}.system.md`. Read with `loom t <name> read [turn] [--input]`.

## Python API

```python
from idio_loom import Loom, LoomTimeout, LoomError

loom = Loom(workspace="/var/idio-shared/dev/ibook")
t = loom.thread("DOCN-v1", model="opus", timeout=1200)

output = t.do("axe", "investigate DOCN: met coal thesis")
output = t.do("bobby", hears="axe", nudge="attack this thesis")
output = t.do("axe", hears="bobby")
output = t.do("axe", "write a draft", system="writer")

print(t.step)          # completed turns (int)
print(t.last_output)   # last turn content
print(t.show())        # thread summary

t.rewind(2)
t.reset()
t.delete()
```

`t.do()` params: `agent`, `nudge=`, `hears=`, `system=`, `model=`, `timeout=`

Wraps CLI via subprocess. Requires `loom` binary on PATH.

## Rust Library API

```rust
use idio_loom::config::Config;
use idio_loom::thread::{Thread, RunOpts};

let config = Config::load(Some("/path/to/workspace"))?;
let t = Thread::new(&config, "my-thread");

// Simple
let result = t.run("axe", Some("investigate DOCN"))?;

// Full options
let result = t.run_opts(&RunOpts {
    agent: "axe",
    nudge: Some("analyze this"),
    source: Some("bobby"),       // or "all"
    model: Some("opus"),
    system: None,                // --use-system
    stage: Some("research"),
    timeout: Some(1800),
})?;
// result: output, session_id, elapsed_s, cost_usd

t.step()?;                       // turn count
t.last_output()?;                // last turn content
t.show()?;                       // thread summary
t.read_turn(Some(2), false)?;   // turn 2 output
t.read_turn(None, true)?;       // latest input
t.rewind("3")?;
t.reset()?;
t.delete()?;
```

## Examples

```bash
# Research session
loom t DOCN-1 do axe "DOCN: met coal. pull 10-K, latest transcript, price data"
loom t DOCN-1 do axe "what are the key forward factors?"
loom t DOCN-1 do bobby hears axe "attack this thesis"
loom t DOCN-1 do axe hears bobby
loom t DOCN-1 do axe "write it up" --use-system writer
loom t DOCN-1 show

# Patterns
loom p idio3 run "AAPL: earnings analysis" opus
loom p deep-dive run "SLB: offshore drilling"

# Quick test
loom t TEST do axe "what is 2+2" --model haiku
loom t TEST do bobby hears axe --model haiku
loom t TEST rm

# Rewind
loom t DOCN-1 rewind 3
loom t DOCN-1 do axe "try a different angle"

# Debug
loom t DOCN-1 read 0 --input    # what was sent
loom t DOCN-1 read 0            # what came back
loom agents show axe             # system prompt
```
