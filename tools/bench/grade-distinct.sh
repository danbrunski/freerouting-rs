#!/usr/bin/env bash
# Grades one board's sessions of several arms with KiCad, once per distinct session (decided
# 2026-10-20: one KiCad grade per distinct session). The sessions are tools/bench/quick-arm.sh's,
# OUT/ARM/BOARD.ses; an arm whose session is byte-identical (`cmp`) to an arm graded before it
# gets a symbolic link to that grade instead of a second KiCad run, and an arm graded by an
# earlier call whose session is unchanged (`OUT/grade/ARM/BOARD.sha256`) keeps its grade, so
# listing a graded arm again (Q11.6's `mp1`) costs no KiCad run.
#
#   tools/bench/grade-distinct.sh OUT BOARD PROJECT ARM...
#
# PROJECT  a KiCad project directory, graded by tools/kicad-oracle/grade.sh with the engine
#          `cp OUT/ARM/BOARD.ses {SES}` into OUT/grade/ARM/BOARD (summary.json); or canary-pwr's
#          project directory (a path ending in canary-pwr-rev1.0), graded by Q10's canary grade
#          (target/orchestrator/q10/s104/canary-grade.sh with its KiCad shim, CANARY_REV default
#          9511d9a3) into OUT/grade/ARM/BOARD, its output in grade.log there.
# Run from the repository root, one board at a time (heavy: KiCad).
set -u
if [[ $# -lt 4 ]]; then
  sed -n '2,15p' "$0" >&2
  exit 2
fi
out=$1; board=$2; proj=$3; shift 3
q10=/home/thebo/code/freerouting-rs/target/orchestrator/q10/s104
graded=()
for arm in "$@"; do
  ses=$out/$arm/$board.ses
  dir=$out/grade/$arm/$board
  if [[ ! -e $ses ]]; then echo "NO SES $arm $board"; continue; fi
  sum=$(sha256sum "$ses" | cut -d' ' -f1)
  if [[ -e $dir && $(cat "$dir.sha256" 2>/dev/null) == "$sum" ]]; then
    echo "$arm $board: graded before, session unchanged"
    graded+=("$arm")
    continue
  fi
  same=
  for prev in "${graded[@]}"; do
    if cmp -s "$ses" "$out/$prev/$board.ses"; then same=$prev; break; fi
  done
  mkdir -p "$out/grade/$arm"
  rm -rf "$dir" "$dir.sha256"
  if [[ -n $same ]]; then
    ln -s "$(realpath "$out/grade/$same/$board")" "$dir"
    echo "$arm $board: same session as $same, grade shared"
    echo "$sum" > "$dir.sha256"
    continue
  fi
  if [[ $proj == *canary-pwr-rev1.0 ]]; then
    mkdir -p "$dir"
    KICAD_BIN=$q10/kshim bash "$q10/canary-grade.sh" "$proj" "$ses" "$dir/work" > "$dir/grade.log" 2>&1
    grep -E '^(DRC:|via-in-pad|inner-layer)' "$dir/grade.log" | sed "s/^/$arm $board: /"
  else
    tools/kicad-oracle/grade.sh --out "$dir" "$proj" -- cp "$(realpath "$ses")" '{SES}' \
      > "$out/grade/$arm/$board.grade.out" 2>&1
    echo "$arm $board: grade.sh exit $?"
  fi
  echo "$sum" > "$dir.sha256"
  graded+=("$arm")
done
