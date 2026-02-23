#!/bin/bash
# test.sh — basic research pattern
# Usage: loom p test run <intake> [model]
set -e

INTAKE="${1:?usage: test.sh <intake> [model]}"
MODEL="${2:-}"
PATTERN="test"

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

do_turn axe "$INTAKE"
do_turn bobby hears axe "review this"
do_turn axe hears bobby "look for idiosyncratic insights"
do_turn axe "so what is our thesis"
do_turn writer hears all "make a draft"
do_turn publisher hears writer "publish it"

loom t "$NAME" show
