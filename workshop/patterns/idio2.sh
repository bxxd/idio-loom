#!/bin/bash
# idio2.sh — guided research → deep dive → write → publish
# The pattern that produced the DOCN memo. Axe-driven, no review loop.
# Usage: loom p idio2 run <intake> [model]
set -e

INTAKE="${1:?usage: idio2.sh <intake> [model]}"
MODEL="${2:-opus}"
PATTERN="idio2"

# Generate thread name with auto-increment
i=1
while loom t "r-${PATTERN}-${i}" show &>/dev/null; do
  i=$((i + 1))
done
NAME="r-${PATTERN}-${i}"

echo "thread: $NAME"

do_turn() {
  if [ -n "$MODEL" ]; then
    loom t "$NAME" do "$@" --model "$MODEL"
  else
    loom t "$NAME" do "$@"
  fi
}

# Initial research
do_turn axe "$INTAKE"

# Factor decomposition
do_turn axe "what are the driving forward factors decompisition"

# Forward alpha
do_turn axe "do a forward alpha calculation"

# Market implied probability
do_turn axe "what is the market implied probability"

# Deep dive — rabbit holes, launch subagents
do_turn axe "are there any threads you want to research - for insight? rabbit holes? do it"

# Write memo
do_turn axe "write a draft to ibook" --use-system writer

# Publish
do_turn publisher hears axe "publish this"

loom t "$NAME" show
