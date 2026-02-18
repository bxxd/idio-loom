#!/bin/bash
# deep-dive.sh — research with iterative critique loop
set -e

do_turn() {
  if [ -n "$LOOM_MODEL" ]; then
    loom t "$LOOM_NAME" do "$@" --model "$LOOM_MODEL"
  else
    loom t "$LOOM_NAME" do "$@"
  fi
}
last_output() { cat "$LOOM_RUN_DIR/.last_output"; }

# Research
do_turn axe "$LOOM_INTAKE

investigate the topic — pull filings, transcripts, data. form a thesis."

# Critique loop
do_turn bobby hears axe "attack this thesis. find gaps, check math, challenge assumptions."

for i in 1 2 3; do
  do_turn axe hears bobby "address the critique"
  do_turn bobby hears axe "re-review. if satisfied, include [NEXT] in your response."
  if grep -q '\[NEXT\]' <<< "$(last_output)"; then
    break
  fi
done

# Write and edit
do_turn writer hears all "write the post"
do_turn editor hears writer
