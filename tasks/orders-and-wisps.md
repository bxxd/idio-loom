# Orders and Wisps

## Problem

Loom patterns today are **manually instantiated** — something has to call `loom p run <pattern>` for a thread to exist. Two consequences:

1. **No event-driven dispatch.** Trawler polls EDGAR on a schedule and creates work items. There's no path where alpha-server (or any other gateway) publishes a typed event and loom auto-instantiates the right pattern. Filings, transcripts, factor drift, thesis-LR drops — all events that should auto-trigger work, but today they each need a bespoke poller.

2. **No deferred step materialization.** A pattern's step structure is fixed at definition time. For deterministic pipelines (factor analysis on a known step list) this is fine. For exploratory work (a deep-dive that decides mid-flight to spawn `corroborate-with-AMD` and `check-13F-holdings`) the pattern has to be flat or hand-decomposed upfront.

Gas City's MEOW stack names these two missing primitives explicitly:

- **Orders** = formula + gate condition on event bus → auto-instantiate when matching events arrive
- **Wisps** = molecules with deferred step materialization (root-only or poured)

Both are additive to loom's existing pattern/thread/agent model. Adding them unlocks event-driven research workflows and recursive exploratory patterns without rewriting loom.

## Solution

Two new primitives, scoped narrow:

### 1. Orders — event-gated patterns

```toml
# orders/process-filing.toml
pattern = "factor-analysis"

[gate]
event = "new_filing"
when = { form = ["10-K", "10-Q", "8-K"], ticker_in_universe = true }

[mapping]
ticker = "{{ event.ticker }}"
filing_id = "{{ event.id }}"
```

Order = pattern + event matcher + argument mapping. When a matching event lands on the bus, loom instantiates the pattern with mapped args.

### 2. Wisps — deferred-materialization patterns

Two flavors:

- **Root-only wisp** — pattern declares only the root step; child steps materialize at runtime as the agent decides what's needed. Useful for research deep-dives.
- **Poured wisp** — pattern step can spawn a child pattern (recursive). Each spawn is a sub-thread with checkpoint recovery. Useful for compositional workflows.

```toml
# patterns/deep-dive.toml
[[step]]
name = "initial-survey"
agent = "researcher"
prompt = "survey.md"
mode = "wisp-spawner"        # this step can spawn child patterns
```

```toml
# patterns/factor-analysis.toml
[[step]]
name = "fetch-prices"
agent = "data-fetcher"

[[step]]
name = "spawn-thesis-evidence"
spawn = "thesis-evidence"    # recursive — spawns thesis-evidence as sub-thread
inputs = { ticker = "{{ root.ticker }}" }
wait = true                  # parent waits on child completion
```

## Architecture

### Event bus (new)

Minimum viable: filesystem-backed append-only JSONL log at a known path.

```
.loom/events/
├── 2026-05-02.jsonl    # one event per line
└── current             # symlink to today's file
```

Each event:
```json
{"id":"01HXXX...","type":"new_filing","ticker":"AVGO","form":"10-Q","at":"2026-05-02T14:00:00Z","payload":{...}}
```

Producers: alpha-server, trawler scanner, manual `loom event publish ...`.
Consumers: order daemon (below).

Future: replace filesystem JSONL with NATS/Redis Streams/Kafka if scale demands. Filesystem version is enough for one-host idio.

### Order daemon (new)

Long-running process that:
1. Loads orders from `orders/*.toml`
2. Tails `.loom/events/current`
3. For each event, evaluates `[gate]` matchers against all loaded orders
4. On match: substitutes `{{ event.* }}` templates into `[mapping]`, runs `loom p <pattern>` with mapped args

```rust
// src/orders.rs (new)
pub struct OrderDaemon {
    orders: Vec<Order>,
    event_log: EventLog,
    loom: LoomLib,
}

impl OrderDaemon {
    pub async fn run(&mut self) -> Result<()> {
        for event in self.event_log.tail().await? {
            for order in &self.orders {
                if order.gate.matches(&event)? {
                    let args = order.mapping.resolve(&event)?;
                    self.loom.spawn_pattern(&order.pattern, args).await?;
                }
            }
        }
        Ok(())
    }
}
```

CLI:
```
loom order list
loom order daemon          # long-running, systemd unit
loom event publish <type> <payload-json>
loom event tail            # debug
```

### Wisps — pattern engine extension

Two new step modes in pattern TOML:

```toml
[[step]]
mode = "wisp-spawner"      # agent emits child step descriptors at runtime
```

```toml
[[step]]
spawn = "<pattern-name>"   # recursive: spawn child pattern as sub-thread
inputs = { ... }
wait = true | false        # block parent on child completion
```

For `wisp-spawner`: agent's output is parsed for child step blocks (existing structured-output convention), each block becomes a new step bead in the same thread. Materialization is per-turn.

For `spawn`: a sub-thread is created with name `{parent}/{step-name}`. Parent thread tracks sub-thread state via existing thread metadata. On parent restart, sub-thread is either resumed (if still alive) or restarted (configurable).

Thread tree:
```
trawl-sec_AVGO_10-Q_2026-05-02         (parent thread)
  ├── spawn-factor-analysis             (sub-thread, runs factor-analysis pattern)
  │   ├── fetch-prices
  │   ├── run-regression
  │   └── write-evidence
  └── spawn-thesis-evidence             (sub-thread, runs thesis-evidence pattern)
      └── ...
```

Existing `loom t <thread> show` walks the tree.

### Layering

```
Orders + Wisps              <- new primitives (this task)
Patterns + Threads          <- existing loom primitives
Agents (yaml)               <- existing
Claude subprocess           <- existing
```

No upward dependencies. Patterns don't know about orders. Threads don't know about wisps unless their pattern uses the spawn mode. Existing patterns work unchanged.

## Use cases

**Orders:**
- alpha-server publishes `new_filing` from EDGAR webhook → triggers `factor-analysis` for ticker
- Trawler emits `thesis_lr_drop` from worldview → triggers `review-thesis` pattern
- User pushes "rebalance" via dashboard → publishes `rebalance_requested` → triggers full universe pipeline

**Wisps:**
- `deep-dive` pattern spawns `corroborate-with-{related-ticker}` discovered mid-research
- `portfolio-construct` spawns `factor-analysis` per ticker in universe (poured wisp, parallel)
- `idio3` request pipeline currently flat — could decompose into spawned sub-patterns for `factor-decomposition` + `forward-alpha` + `insider-analysis` with independent checkpoint recovery

## Scope

**In scope:**
- Filesystem event bus (`.loom/events/*.jsonl`)
- Order TOML format + daemon
- Wisp step modes (`wisp-spawner`, `spawn`)
- Sub-thread tracking
- CLI commands (`loom order *`, `loom event *`)

**Out of scope:**
- Distributed event bus (NATS/Kafka) — filesystem is enough for one host
- Cross-host coordination (Wasteland-style) — speculative
- Beads-style work graph (separate concern; could compose with orders later)
- Full Gas City pack format — this task adopts only the two primitives that fill loom's gaps

## References

- Gas City: `tmp/gascity/AGENTS.md` — five primitives + four mechanisms
- Yegge, "Welcome to Gas City": https://steve-yegge.medium.com/welcome-to-gas-city-57f564bb3607
- MEOW (Molecular Expression of Work) — Beads-backed work tracking
- Existing loom primitives: see `tasks/python-lib.md` and `DEVELOPER.md`
- Use case driver: `meta/AGENT_OS_LANDSCAPE.md` "portfolio universe creation, optimization, tuning"
