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
do_turn axe hears bobby
do_turn axe "write it up" --use-system writer

loom t "$NAME" show
