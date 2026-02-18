#!/bin/bash
# test.sh — basic research pattern
# Usage: ./patterns/test.sh <thread-name> <intake> [model]
#
# Example:
#   ./patterns/test.sh AAPL "AAPL: have a theory that services will drive margins" haiku
set -e

NAME="${1:?usage: test.sh <name> <intake> [model]}"
INTAKE="${2:?usage: test.sh <name> <intake> [model]}"
MODEL="${3:-}"

do_turn() {
  if [ -n "$MODEL" ]; then
    loom t "$NAME" do "$@" --model "$MODEL"
  else
    loom t "$NAME" do "$@"
  fi
}

RUN_DIR="$(loom t "$NAME" show 2>/dev/null | head -1 || true)"
LAST_OUTPUT=".loom/runs/$NAME/.last_output"
last_output() { cat "$LAST_OUTPUT"; }

do_turn axe "$INTAKE"
do_turn bobby hears axe "review this"
do_turn axe hears bobby "look for idiosyncratic insights"
do_turn axe "so what is our thesis"
do_turn writer hears all "make a draft"
do_turn publisher hears writer "publish it"

echo "done: loom t $NAME show"
