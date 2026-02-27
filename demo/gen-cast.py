#!/usr/bin/env python3
"""Generate a fake asciinema .cast file for the loom demo GIF."""
import json
import sys

events = []
t = 0.0

PROMPT = "\x1b[32m$\x1b[0m "
BOLD = "\x1b[1m"
DIM = "\x1b[2m"
RESET = "\x1b[0m"
CYAN = "\x1b[36m"
YELLOW = "\x1b[33m"
GREEN = "\x1b[32m"

def out(text, dt=0.0):
    global t
    t += dt
    events.append([round(t, 3), "o", text])

def type_cmd(cmd, pause_after=0.3):
    """Simulate typing a command."""
    out(PROMPT)
    for ch in cmd:
        out(ch, 0.04)
    out("\r\n", pause_after)

def wait(s):
    global t
    t += s

# Header
header = {
    "version": 2,
    "width": 96,
    "height": 32,
    "timestamp": 1740700000,
    "env": {"SHELL": "/bin/bash", "TERM": "xterm-256color"},
    "title": "loom demo"
}

# -- Scene 1: Research with two agents --

wait(0.5)
type_cmd('loom t DEMO do axe "what is 2+2? answer in one sentence." --model haiku')
wait(0.3)
out(f"{DIM}[loom] turn 0: axe (new session, model=haiku){RESET}\r\n", 0.1)
wait(1.5)
out(f"\r\n2 + 2 = 4.\r\n\r\n", 0.1)
out(f"{DIM}[loom] 0:03 | $0.002 | 8 chars{RESET}\r\n", 0.2)

wait(0.8)
type_cmd('loom t DEMO do bobby hears axe "is this correct? check carefully." --model haiku')
wait(0.3)
out(f"{DIM}[loom] turn 1: bobby (new session, model=haiku) hears axe{RESET}\r\n", 0.1)
wait(2.0)
out(f"\r\nYes, 2 + 2 = 4 is correct. This is basic arithmetic — the sum is\r\n", 0.05)
out(f"exact and universally agreed upon. No edge cases or caveats.\r\n\r\n", 0.1)
out(f"{DIM}[loom] 0:04 | $0.003 | 142 chars{RESET}\r\n", 0.2)

# -- Scene 2: Show thread --

wait(0.8)
type_cmd("loom t DEMO show")
wait(0.2)
out(f"\r\n{BOLD}DEMO{RESET} (2 turns)\r\n")
out(f"  #0 axe          0:03  $0.002  {DIM}\"2 + 2 = 4.\"{RESET}\r\n", 0.05)
out(f"  #1 bobby        0:04  $0.003  {DIM}\"Yes, 2 + 2 = 4 is correct. This is basic ar...\"{RESET}\r\n\r\n", 0.05)

# -- Scene 3: Rewind --

wait(1.2)
type_cmd("loom t DEMO rewind 0")
wait(0.2)
out(f"rewound to snapshot 0 (after turn #0)\r\n", 0.1)

wait(0.8)
type_cmd('loom t DEMO do bobby hears axe "wrong. prove it from first principles." --model haiku')
wait(0.3)
out(f"{DIM}[loom] turn 1: bobby (resumed, model=haiku) hears axe{RESET}\r\n", 0.1)
wait(2.0)
out(f"\r\nStarting from the Peano axioms: 2 is defined as S(S(0)), so 2 + 2 =\r\n", 0.05)
out(f"S(S(0)) + S(S(0)) = S(S(S(S(0)))) = 4. The claim holds by the\r\n", 0.05)
out(f"recursive definition of addition.\r\n\r\n", 0.1)
out(f"{DIM}[loom] 0:05 | $0.004 | 198 chars{RESET}\r\n", 0.2)

# -- Scene 4: Show again -- different result after rewind

wait(0.8)
type_cmd("loom t DEMO show")
wait(0.2)
out(f"\r\n{BOLD}DEMO{RESET} (2 turns)\r\n")
out(f"  #0 axe          0:03  $0.002  {DIM}\"2 + 2 = 4.\"{RESET}\r\n", 0.05)
out(f"  #1 bobby        0:05  $0.004  {DIM}\"Starting from the Peano axioms: 2 is defin...\"{RESET}\r\n\r\n", 0.05)

# -- Cleanup --
wait(1.0)
type_cmd("loom t DEMO rm")
wait(0.2)
out(f"deleted thread DEMO\r\n", 0.1)
wait(0.5)

# Write
print(json.dumps(header))
for ev in events:
    print(json.dumps(ev))
