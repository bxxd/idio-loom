#!/bin/bash
# test.sh — basic research pattern
set -e

do_turn() {
  if [ -n "$LOOM_MODEL" ]; then
    loom t "$LOOM_NAME" do "$@" --model "$LOOM_MODEL"
  else
    loom t "$LOOM_NAME" do "$@"
  fi
}
last_output() { cat "$LOOM_RUN_DIR/.last_output"; }

do_turn axe "$LOOM_INTAKE"
do_turn bobby hears axe "review this"
do_turn axe hears bobby "look for idiosyncratic insights"
do_turn axe "so what is our thesis"
do_turn writer hears all "make a draft"
do_turn publisher hears writer "publish it"
