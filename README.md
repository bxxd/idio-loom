# loom

Multi-agent research CLI. Named threads of agent conversations via `claude --resume`, routing messages between agents while preserving session context.

## Install

```bash
cargo build --release
cp target/release/loom ~/.local/bin/
```

## Quick Start

```bash
# Initialize a workspace
loom init

# Or run from an existing workspace with loom.yaml
cd /your/workspace

# See what's available
loom agents                              # list agents
loom patterns                            # list patterns

# Pattern mode — structured multi-stage workflow
loom t DOCN-v1 start deep-dive.yaml
loom t DOCN-v1 next
loom t DOCN-v1 next
loom t DOCN-v1 show

# Pattern mode — run all stages at once
loom t DOCN-v2 run deep-dive.yaml

# Manual mode — ad hoc turns
loom t TEST-1 do axe "investigate DOCN"
loom t TEST-1 do bobby hears axe "what's wrong with this thesis?"
loom t TEST-1 do axe hears all

# Inspect
loom list
loom t TEST-1 show
loom t TEST-1 read
```

## Workspace Structure

```
workshop/                    # content (deployable, git-tracked)
├── agents/
│   ├── axe.yaml             # one file per agent
│   ├── bobby.yaml
│   └── prompts/
│       ├── researcher-v8.txt
│       └── writer.txt
└── patterns/
    └── deep-dive.yaml

loom.yaml                    # runtime config (per-tenant)
.loom/                       # state (per-tenant, gitignored)
└── runs/
    └── DOCN-v1/
        ├── meta.json
        ├── scratchpad/
        ├── turn-0-axe.md
        └── snapshots/
```

### Config — loom.yaml

```yaml
model: sonnet                # default model
timeout: 900                 # seconds
workshop: .                  # path to content (absolute or relative)
state: .loom                 # path to state dir (absolute or relative)
pattern: deep-dive.yaml      # default pattern
snapshots: true

system_prompt: |
  Do NOT use run_in_background: true when spawning sub-agents.
```

### Agents — workshop/agents/

Each agent is its own YAML file. All `.yaml` files in `agents/` are loaded automatically.

```yaml
# agents/axe.yaml
model: opus
system_prompt: |
  You are a primary source researcher. Extract facts from filings
  and transcripts. Form a thesis. Be opinionated.
system_file: researcher-v8.txt   # loaded from agents/prompts/
```

### Patterns — workshop/patterns/

Patterns define multi-stage workflows. Each stage references an agent by name.

```yaml
# patterns/deep-dive.yaml
name: deep-dive
stages:
  - name: research
    agent: axe
    nudge: "investigate the topic — pull filings, transcripts, data."

  - name: critique
    agent: bobby
    source: axe
    nudge: "attack this thesis. find gaps, check math."

  - name: rebut
    agent: axe
    source: bobby

  - name: write
    agent: writer
    source: all
    nudge: "write the post"

  - name: edit
    agent: editor
    source: writer
```

### @file references

Nudges and system prompts can reference files with `@path`:

```yaml
nudge: "@critique-checklist.txt"           # whole value from file
nudge: "review this. @checklist.txt"       # inline inclusion
```

Agent `@` refs resolve from `agents/prompts/`. Pattern `@` refs resolve from `patterns/`.

## Commands

```
setup:
  loom init [path]                                         # scaffold workspace

agents:
  loom agents                                              # list agents
  loom agents show <name>                                  # show agent details

patterns:
  loom patterns                                            # list available patterns
  loom patterns show [name]                                # show pattern (default if no name)
  loom patterns set <name>                                 # set default pattern

threads:
  loom list                                                # list all threads

pattern mode:
  loom thread <name> start [pattern.yaml]                  # init thread with pattern
  loom thread <name> run [pattern.yaml]                    # start + run all stages
  loom thread <name> next                                  # run next pattern stage

manual mode:
  loom thread <name> do <agent> ["nudge"]                  # agent speaks
  loom thread <name> do <agent> hears <other> ["nudge"]    # agent hears another
  loom thread <name> do <agent> hears all ["nudge"]        # agent hears full thread

inspection:
  loom thread <name> show                                  # thread summary
  loom thread <name> read [turn]                           # print turn output

snapshots:
  loom thread <name> snapshot                              # manual snapshot
  loom thread <name> rewind <N>                            # restore to after turn N

lifecycle:
  loom thread <name> reset                                 # wipe turns, keep config
  loom thread <name> delete                                # remove thread entirely

shortcuts: t = thread, ls = list, rm = delete, clear = reset

flags:
  --model <model>        override model for this turn
  --dir <path>           directory containing loom.yaml
  --snapshot             enable snapshots
  --no-snapshot          disable snapshots
  --force, -f            overwrite existing thread
```

## How It Works

1. Each agent gets a `claude -p` session with `--append-system-prompt` (global + agent-specific + scratchpad path)
2. Subsequent turns use `claude -p --resume <session_id>` to maintain full context
3. `hears <agent>` routes that agent's last output as input
4. `hears all` concatenates the full thread (excluding self, including nudges)
5. Snapshots capture meta + scratchpad + turn files + claude session JSONLs
6. Rewind restores all of the above, rolling back agent memory

## Deployment

Content (workshop) deploys separately from config+state:

```bash
# Source (git repo, iterate here)
idio-loom/workshop/

# Tenant config + state (per environment)
/var/idio-shared/dev/ibook/
├── loom.yaml                 # workshop: /var/idio-loom/dev
└── .loom/runs/               # state stays local
```
