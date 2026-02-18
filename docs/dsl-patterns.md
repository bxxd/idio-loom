# DSL Pattern Interpreter — Design Notes

Shelved in favor of bash scripts + loom as pure turn engine. Saved here in case we want to revisit.

## The idea

Instead of bash scripts calling loom externally, loom owns a tiny DSL interpreter. Patterns are flat instruction lists with labels and a program counter. Loom tracks which step you're on, so you get stepping (`next`), resume after crash, and rewind to any step.

## Language

5 constructs, no nesting, no blocks:

```
# Labels are optional. Only needed for goto targets.
research:  do axe "$INTAKE"
critique:  do bobby hears axe "review this"
rebut:     do axe hears bobby "address critique"
recheck:   do bobby hears axe "re-review. if satisfied, include [NEXT]."
           goto rebut unless grep [NEXT]
thesis:    do axe "so what is our thesis"
write:     do writer hears all "make a draft"
publish:   do publisher hears writer "publish it"
           shell ibook draft publish "$NAME"
```

### Instructions

| Instruction | Description |
|---|---|
| `do agent ["nudge"]` | Run a turn — agent speaks |
| `do agent hears source ["nudge"]` | Run a turn — agent hears another agent (or `all`) |
| `goto label` | Unconditional jump |
| `goto label if grep PATTERN` | Jump if last output contains PATTERN |
| `goto label unless grep PATTERN` | Jump if last output does NOT contain PATTERN |
| `exit` | Stop execution |
| `shell command` | Run arbitrary shell command |

### Variables

`$INTAKE` and `$NAME` are substituted at parse time from the thread name and intake text.

## CLI

```
loom t <name> run <pattern> ["intake"]     # run all steps from current position
loom t <name> run <pattern> --force        # wipe and start fresh
loom t <name> next                         # run next action step (skips goto/exit)
loom t <name> show                         # show steps with cursor
```

## Implementation

~350 lines of Rust in `pattern.rs`. Three parts:

**Parser** — tokenizer respects quoted strings, extracts optional `label:` prefix, parses instruction. Variable substitution on full text before parsing. Validates all goto targets exist.

**AST** — `Vec<Step>` where each Step has optional label + Instruction enum. Flat list, no tree.

**Interpreter** — program counter stored in `meta.pattern_step`. `run_all` loops until end. `run_next` executes one action step (skips control flow). `execute_step` dispatches by instruction type, updates PC, saves meta after each step.

## Why we didn't build it

Breaks separation of concerns. Loom becomes both the turn engine AND the sequencer. Bash already has `for`/`if`/`while`/`grep`/pipes. Users know bash. Every new feature request (nested loops, functions, variables) means more interpreter code. The 350 lines becomes 1200.

The cleaner architecture: loom = turn engine (threads, sessions, snapshots, rewind). Bash = orchestrator (scripts that call `loom t X do ...`).

## When it might make sense again

- If we need resume-from-crash semantics (script dies mid-run, restart picks up where it left off)
- If stepping through turns one-at-a-time becomes a common workflow
- If we want `loom t X show` to display pattern progress, not just turn history

The full interpreter implementation is saved in `docs/dsl-pattern-interpreter.rs`.
