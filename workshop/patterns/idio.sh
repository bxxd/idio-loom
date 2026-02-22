#!/bin/bash
# idio.sh — full idiobook pipeline: research, review, thesis, write, publish
# Usage: loom p idio run <intake> [model]
set -e

INTAKE="${1:?usage: idio.sh <intake> [model]}"
MODEL="${2:-opus}"
PATTERN="idio"

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

# Deep research
do_turn axe "do real research on each. launch agents"

# Review
do_turn bobby hears all "review this"

# Address critique
do_turn axe hears bobby

# Thesis
do_turn axe "explain thesis and evaluate entry exit"

# Write
do_turn writer hears all "make a draft using ibook, with precision and depth"

# Review draft
do_turn axe hears writer "review this draft"

# Rewrite
do_turn writer hears axe "rewrite the draft if needed"

# Publish
do_turn publisher hears writer "publish draft"

loom t "$NAME" show
