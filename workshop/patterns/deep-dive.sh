#!/bin/bash
# deep-dive.sh — research with iterative critique loop
# Usage: ./patterns/deep-dive.sh <thread-name> <intake> [model]
set -e

NAME="${1:?usage: deep-dive.sh <name> <intake> [model]}"
INTAKE="${2:?usage: deep-dive.sh <name> <intake> [model]}"
MODEL="${3:-}"

do_turn() {
  if [ -n "$MODEL" ]; then
    loom t "$NAME" do "$@" --model "$MODEL"
  else
    loom t "$NAME" do "$@"
  fi
}

LAST_OUTPUT=".loom/runs/$NAME/.last_output"
last_output() { cat "$LAST_OUTPUT"; }

# Research
do_turn axe "$INTAKE

investigate the topic — pull filings, transcripts, data. form a thesis."

# Critique loop
do_turn bobby hears axe "attack this thesis. find gaps, check math, challenge assumptions."

for i in 1 2 3; do
  do_turn axe hears bobby "address the critique"
  do_turn bobby hears axe "re-review. if satisfied, include [NEXT] in your response."
  if grep -q '\[NEXT\]' "$LAST_OUTPUT"; then
    echo "reviewer satisfied after round $i"
    break
  fi
done

# Write and edit
do_turn writer hears all "write the post"
do_turn editor hears writer

echo "done: loom t $NAME show"
