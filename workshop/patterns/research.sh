#!/bin/bash
# research.sh — axe-driven research with bobby review, axe writes memo
# Usage: loom p research run <intake> [model]
set -e

INTAKE="${1:?usage: research.sh <intake> [model]}"
MODEL="${2:-opus}"
PATTERN="research"

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

# Research
do_turn axe "$INTAKE"

# Factor decomposition
do_turn axe "what are the driving forward factors"

# Forward analysis
do_turn axe "do a forward analysis"

# Deep dive
do_turn axe "are there any threads you want to research — rabbit holes? do it"

# Bobby review — attack the thesis
do_turn bobby hears axe "attack this thesis. find gaps, check math, challenge assumptions."

# Axe addresses critique
do_turn axe hears bobby

# Write memo — axe keeps coherence, writer prompt shapes the output
do_turn axe "write the memo" --use-system writer

loom t "$NAME" show
