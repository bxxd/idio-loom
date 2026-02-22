# Loom as a library

## Problem

Pattern scripts (bash) are fire-and-forget. Loom has snapshots and rewind but bash doesn't know about them. When a pattern hangs (4h+ opus deep-dive on ibook request edf932da) or crashes mid-run, there's no resume, no timeout, no error handling. The shelved Rust DSL reinvents a language. Python already is the language. Rust already is the language.

## Solution

Loom becomes a library with two interfaces:

1. **Rust crate** — ibook-requests (Rust) imports `idio-loom` as a git dependency
2. **Python lib** — trawler (Python) imports `loom` package
3. **CLI** — unchanged, becomes a thin consumer of the Rust lib

Consumers get native timeout, error handling, resume, and rewind without needing loom CLI deployed.

## Architecture

```
idio-loom (git@github.com:bxxd/idio-loom.git)
├── src/
│   ├── lib.rs          # Public API: re-exports modules
│   ├── main.rs         # CLI (thin consumer of lib)
│   ├── config.rs       # Config, Agent (already exists)
│   ├── meta.rs         # Meta, Turn, AgentState (already exists)
│   ├── thread.rs       # NEW: Thread struct — high-level API
│   ├── loom.rs         # Turn engine (already exists, called by thread.rs)
│   ├── claude.rs       # Claude subprocess (already exists, +timeout fix)
│   └── snapshot.rs     # Snapshots (already exists)
├── python/
│   └── loom/
│       ├── __init__.py # Loom, Thread classes (subprocess wrapper)
│       └── py.typed
└── Cargo.toml          # [lib] + [[bin]]
```

### Rust crate usage (ibook-requests)

```toml
# book/crates/requests/Cargo.toml
[dependencies]
idio-loom = { git = "ssh://git@github.com/bxxd/idio-loom.git", tag = "v0.2.0" }
```

```rust
use idio_loom::{Config, Thread};

let config = Config::load(Some(workspace_dir))?;
let mut t = Thread::new(&config, "req-abc123", "opus", 600);

t.run("axe", &intake)?;
t.run("axe", "forward factors")?;
t.run("bobby", RunOpts { hears: Some("axe"), nudge: Some("attack this"), ..default() })?;

if t.last_output().contains("[VERDICT:RETRY]") {
    t.rewind(t.step() - 1)?;
    t.run("axe", RunOpts { hears: Some("bobby"), ..default() })?;
}

// Error handling — caller owns it
match t.run("axe", "deep dive") {
    Ok(_) => {},
    Err(e) if e.is_timeout() => {
        t.rewind(t.step() - 1)?;
        t.run("axe", "deep dive — be concise")?;
    }
    Err(e) => {
        shell("ibook request update {req_id} --status error");
        return Err(e);
    }
}
```

### Python lib usage (trawler)

```python
from loom import Loom

l = Loom()  # finds loom.yaml in cwd
t = l.thread("trawl-123", model="sonnet", timeout=300)

t.do("trawler", intake)
t.do("trawler", "review your work", system="trawler-reviewer")

if "[VERDICT:RETRY]" in t.last_output:
    t.do("trawler", "address the critique")

if "[VERDICT:ESCALATE]" in t.last_output:
    t.do("trawler", "forward factors")
    t.do("trawler", "write a draft", system="writer")
    t.do("publisher", hears="trawler", nudge="publish this")
```

## Scope

### 1. Rust: split into lib + bin

**Cargo.toml changes:**
```toml
[lib]
name = "idio_loom"
path = "src/lib.rs"

[[bin]]
name = "loom"
path = "src/main.rs"
```

**`src/lib.rs`** — re-exports public API:
- `Config`, `Agent`
- `Meta`, `Turn`, `AgentState`
- `Thread` (new high-level struct)
- `run_turn` (existing, low-level)

**`src/thread.rs`** — NEW high-level Thread API:
- `Thread::new(config, name, model, timeout)` — create/resume thread
- `Thread::run(agent, nudge)` — run a turn (wraps `loom::run_turn`)
- `Thread::run_with(RunOpts { hears, system, timeout, stage })` — full options
- `Thread::rewind(n)` — rewind to snapshot N
- `Thread::delete()` / `Thread::reset()`
- `Thread::step()` — completed turn count
- `Thread::last_output()` — read .last_output
- `Thread::meta()` — current meta state
- Timeout enforcement: wraps `run_turn` with wall-clock check

**`src/claude.rs`** — fix timeout:
- Replace dead `_timeout` with actual enforcement
- `try_wait()` loop instead of blocking `wait_with_output()`
- Process group kill on timeout (catches claude child)

### 2. Python package (`python/loom/`)

Thin subprocess wrapper over loom CLI. Stdlib only, no deps.

**`__init__.py`:**
- `Loom(workspace=None)` — path to loom.yaml dir
- `Thread` — wraps `loom t <name>` subcommands
  - `do(agent, nudge, hears, system, timeout)` — subprocess with process group timeout
  - `show()`, `read()`, `rewind()`, `delete()`, `reset()`
  - `step`, `last_output`, `meta` properties (read files directly)
- `LoomError`, `TimeoutError` exceptions

Install: `pip install git+ssh://git@github.com/bxxd/idio-loom.git#subdirectory=python`
Or for trawler: `poetry add git+ssh://git@github.com/bxxd/idio-loom.git#subdirectory=python`

### 3. Pattern migration

**`book/requests/`** — ibook-requests (Rust) orchestrates directly via Rust crate:
- Pattern logic moves into the Rust binary, no external script
- idio3 sequence = a function that calls `thread.run()` in a loop
- Stage updates, cancellation checks, error handling = native Rust

**`trawler/`** — trawler (Python) imports loom package:
- `trawl2.py` pattern uses `from loom import Loom`
- Or pattern logic moves into trawler's own Python code

Bash patterns stay for manual/ad-hoc use. No migration required.

## Implementation order

1. **Cargo.toml** — add `[lib]` section, dual lib+bin
2. **`src/lib.rs`** — re-export public modules
3. **`src/claude.rs`** — fix timeout (defense-in-depth)
4. **`src/thread.rs`** — high-level Thread API wrapping run_turn
5. **`src/main.rs`** — CLI uses lib imports instead of `crate::`
6. **Tag v0.2.0** — push, verify git dep works
7. **`python/loom/__init__.py`** — subprocess wrapper
8. **ibook-requests** — switch from bash subprocess to native loom crate
9. **trawler** — switch from loom CLI calls to Python lib

## What NOT to do

- No Rust DSL interpreter (shelved design stays shelved)
- No relative path deps (`path = "../../loom"`) — git deps only
- No breaking changes to loom CLI (stays a thin consumer)
- No rewrite of ibook-requests polling loop (just the pattern execution)

## Refs

- Loom repo: `git@github.com:bxxd/idio-loom.git`
- Book repo: `git@github.com:bxxd/idio-book.git`
- Shelved DSL: `loom/docs/dsl-patterns.md`
- Current bash patterns: `book/requests/patterns/idio3.sh`, `trawler/loom/patterns/trawl2.sh`
- ibook-requests source: `book/crates/requests/`
- Trawler loom runner: `trawler/trawler/loom_runner.py`
- The hang: request edf932da, turn 4, 4h+ opus deep-dive, SIGTERM'd manually
