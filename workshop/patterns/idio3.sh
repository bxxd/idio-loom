#!/bin/bash
# idio3.sh — axe-driven research with bobby review, axe writes memo
# Combines idio1 (bobby critique) with idio2 (axe coherence, axe writes)
# Usage: loom p idio3 run <intake> [model]
set -e

INTAKE="${1:?usage: idio3.sh <intake> [model]}"
MODEL="${2:-opus}"
PATTERN="idio3"

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
do_turn axe "what are the driving forward factors decompisition"

# Forward alpha
do_turn axe "do a forward alpha calculation"

# Market implied probability
do_turn axe "what is the market implied probability"

# Deep dive
do_turn axe "are there any threads you want to research - for insight? rabbit holes? do it"

# Insider analysis
do_turn axe "insider analysis. who is buying and selling? what are the patterns? what does it tell us about the thesis?"

# Bobby review — attack the thesis
do_turn bobby hears axe "attack this thesis. find gaps, check math, challenge assumptions."

# Axe addresses critique
do_turn axe hears bobby

# Write memo — axe keeps coherence, writer prompt shapes the output
do_turn axe "write a draft to ibook" --use-system writer

# Publish
do_turn publisher hears axe "publish this"

loom t "$NAME" show
