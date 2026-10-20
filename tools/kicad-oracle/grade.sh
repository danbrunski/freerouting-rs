#!/usr/bin/env bash
# Grades a router's session with KiCad's own DRC, an oracle independent of Freerouting's model.
#
#   tools/kicad-oracle/grade.sh [--out DIR] [--keep-tracks] PROJECT [--] ENGINE_COMMAND...
#
# PROJECT         a KiCad project directory holding exactly one .kicad_pcb, or the .kicad_pcb
#                 itself; its .kicad_pro and .kicad_dru (same base name) are the board's rules.
#                 It is only read: everything happens on copies.
# ENGINE_COMMAND  routes {DSN} into {SES}, e.g.
#                 java -jar freerouting.jar -de {DSN} -do {SES} --gui.enabled=false
# --out           the work directory (default: a new directory under TMPDIR)
# --keep-tracks   grade the board's existing tracks and vias too (default: strip them first,
#                 so the engine routes the whole board)
#
# Steps: stage copies of the .kicad_pcb/.kicad_pro/.kicad_dru; strip tracks and vias; export the
# DSN with pcbnew.ExportSpecctraDSN; remove the keepouts KiCad exports for rule areas that allow
# tracks and vias; run the engine; strip the SES (placement ...) scope and import it with
# pcbnew.ImportSpecctraSES; copy the ORIGINAL project rules over both staged boards (a router
# never grades itself under rules it wrote; a project without a .kicad_pro gets the one KiCad
# migrated from the original board's own rules, summary `rules`) and force the copper classes
# to error; run
# `kicad-cli pcb drc --severity-all --refill-zones --format json` on the unrouted and the routed
# board; count copper from the geometry; subtract what the unrouted input already reports.
# Writes OUT/summary.json and prints one line. When freerouting-dev is built the summary also has
# `tidiness` (Q15.2, see the README).
#
# Exit status: 0 graded clean, 1 router-attributable items (new DRC items, or copper below the
# board minimums), 2 not graded (no kicad-cli, no pcbnew, mixed KiCad versions, a failed export
# or import, no SES). "Not graded" never counts as a pass.
#
# KICAD_CLI (default kicad-cli) and KICAD_PYTHON (default python3, which must import pcbnew)
# pick the KiCad install; both must report the same version. Never starts the KiCad GUI.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
KICAD_CLI=${KICAD_CLI:-kicad-cli}
KICAD_PYTHON=${KICAD_PYTHON:-python3}
LIB="$HERE/oracle_lib.py"
STEPS="$HERE/kicad_steps.py"

usage() {
  echo "usage: $0 [--out DIR] [--keep-tracks] PROJECT_DIR [--] ENGINE_COMMAND... ({DSN} {SES})" >&2
  exit 2
}

OUT="" KEEP_TRACKS=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT=${2:-}; shift 2 || usage ;;
    --keep-tracks) KEEP_TRACKS=--keep-tracks; shift ;;
    -*) usage ;;
    *) break ;;
  esac
done
[[ $# -ge 1 ]] || usage
BOARD_FILE=""
if [[ -f "$1" && "$1" == *.kicad_pcb ]]; then
  BOARD_FILE=$(basename "$1")
  PROJECT=$(cd "$(dirname "$1")" && pwd)
else
  PROJECT=$(cd "$1" 2> /dev/null && pwd) || { echo "no such project directory: $1" >&2; exit 2; }
fi
shift
[[ "${1:-}" == "--" ]] && shift
[[ $# -ge 1 ]] || usage
ENGINE=("$@")
if [[ -z "$OUT" ]]; then
  OUT=$(mktemp -d "${TMPDIR:-/tmp}/kicad-oracle.XXXXXX")
fi
mkdir -p "$OUT/work"
OUT=$(cd "$OUT" && pwd)
SUMMARY="$OUT/summary.json"
META="$OUT/meta.json"

not_graded() {
  python3 "$LIB" not-graded "$1" "$SUMMARY" ${META_READY:+"$META"}
  echo "not graded: $1 (summary: $SUMMARY)"
  exit 2
}

command -v "$KICAD_CLI" > /dev/null 2>&1 || not_graded "kicad-cli not found ('$KICAD_CLI')"
CLI_VERSION=$("$KICAD_CLI" version 2> /dev/null | tail -n 1)
PY_VERSION=$("$KICAD_PYTHON" "$STEPS" version 2> /dev/null | tail -n 1)
[[ -n "$PY_VERSION" ]] || not_graded "'$KICAD_PYTHON' cannot import pcbnew"
[[ "$CLI_VERSION" == "$PY_VERSION" ]] ||
  not_graded "kicad-cli is $CLI_VERSION but pcbnew is $PY_VERSION; never mix KiCad versions"

shopt -s nullglob
if [[ -n "$BOARD_FILE" ]]; then
  PCBS=("$PROJECT/$BOARD_FILE")
else
  PCBS=("$PROJECT"/*.kicad_pcb)
fi
shopt -u nullglob
[[ ${#PCBS[@]} -eq 1 ]] ||
  not_graded "expected one .kicad_pcb in $PROJECT, found ${#PCBS[@]}; pass the .kicad_pcb itself"
BASE=$(basename "${PCBS[0]}" .kicad_pcb)
ORIGINALS=()
for ext in kicad_pcb kicad_pro kicad_dru; do
  [[ -f "$PROJECT/$BASE.$ext" ]] && ORIGINALS+=("$PROJECT/$BASE.$ext")
done
sha256sum "${ORIGINALS[@]}" > "$OUT/originals.sha256"

# Each staged board gets its own directory under the original base name: kicad-cli takes the
# rules from the .kicad_pro/.kicad_dru with the board's own name, or silently uses defaults.
stage() {
  mkdir -p "$1"
  local f
  for f in "${ORIGINALS[@]}"; do
    cp "$f" "$1/"
  done
  chmod u+w "$1"/*
}

# The original rules, with the copper classes forced to error. A project without a .kicad_pro
# (a KiCad 4 or 5 board keeps its net classes and setup in the board file, KiCad 6+ moves them to
# the project when it loads it) gets the .kicad_pro KiCad wrote when it first saved the original
# board, before any routing (Q19): without it kicad-cli grades under KiCad's defaults, 0.2 mm
# clearance where such a board may say 0.15.
original_rules() {
  local ext
  for ext in kicad_pro kicad_dru; do
    rm -f "$1/$BASE.$ext"
    if [[ -f "$PROJECT/$BASE.$ext" ]]; then
      cp "$PROJECT/$BASE.$ext" "$1/"
      chmod u+w "$1/$BASE.$ext"
    elif [[ $ext == kicad_pro && $RULES == board ]]; then
      cp "$OUT/work/rules/$BASE.kicad_pro" "$1/"
    fi
  done
  [[ -f "$1/$BASE.kicad_pro" ]] && python3 "$LIB" force-errors "$1/$BASE.kicad_pro"
  return 0
}

stage "$OUT/source"
stage "$OUT/unrouted"
"$KICAD_PYTHON" "$STEPS" strip "$OUT/source/$BASE.kicad_pcb" "$OUT/unrouted/$BASE.kicad_pcb" \
  $KEEP_TRACKS > "$OUT/work/strip.log" 2>&1 || not_graded "stripping tracks failed (see $OUT/work/strip.log)"
REMOVED_TRACKS=$(python3 -c "import json, sys; print(json.load(open(sys.argv[1]))['removed_tracks_and_vias'])" \
  "$OUT/unrouted/$BASE.kicad_pcb.strip.json")
mv "$OUT/unrouted/$BASE.kicad_pcb.strip.json" "$OUT/work/strip.json"
# Where the grading rules come from: the project's .kicad_pro, KiCad's migration of the board's
# own rules (the .kicad_pro the strip step's SaveBoard wrote from the original board), or
# KiCad's defaults.
RULES=defaults
if [[ -f "$PROJECT/$BASE.kicad_pro" ]]; then
  RULES=project
elif [[ -f "$OUT/unrouted/$BASE.kicad_pro" ]]; then
  mkdir -p "$OUT/work/rules"
  cp "$OUT/unrouted/$BASE.kicad_pro" "$OUT/work/rules/$BASE.kicad_pro"
  RULES=board
fi
"$KICAD_PYTHON" "$STEPS" export "$OUT/unrouted/$BASE.kicad_pcb" "$OUT/work/$BASE.kicad.dsn" \
  "$OUT/work/areas.json" > "$OUT/work/export.log" 2>&1 ||
  not_graded "DSN export failed (see $OUT/work/export.log)"
KEEPOUTS_REMOVED=$(python3 "$LIB" fix-keepouts "$OUT/work/$BASE.kicad.dsn" "$OUT/work/areas.json" \
  "$OUT/work/$BASE.dsn") || not_graded "keepout fix failed"
python3 "$LIB" class-widths "$OUT/work/$BASE.dsn" "$OUT/work/classwidths.json"

DSN="$OUT/work/$BASE.dsn"
SES="$OUT/work/$BASE.ses"
ARGS=()
for arg in "${ENGINE[@]}"; do
  arg=${arg//\{DSN\}/$DSN}
  ARGS+=("${arg//\{SES\}/$SES}")
done
START=$(date +%s.%N)
(cd "$OUT/work" && "${ARGS[@]}") > "$OUT/work/engine.log" 2>&1
ENGINE_EXIT=$?
END=$(date +%s.%N)
ENGINE_SECONDS=$(python3 -c "import sys; print(round(float(sys.argv[2]) - float(sys.argv[1]), 2))" "$START" "$END")

python3 - "$META" "$PROJECT" "$BASE" "$CLI_VERSION" "$ENGINE_EXIT" "$ENGINE_SECONDS" \
  "$KEEPOUTS_REMOVED" "$OUT" "${REMOVED_TRACKS:-0}" "$RULES" "${ENGINE[@]}" << 'PY'
import json, sys
meta, project, base, version, rc, secs, removed, out, removed_tracks, rules = sys.argv[1:11]
removed_tracks = int(removed_tracks)
json.dump({"project": project, "board": base, "kicad_version": version,
           "engine_command": " ".join(sys.argv[11:]), "engine_exit": int(rc),
           "engine_seconds": float(secs), "keepouts_removed": int(removed),
           "input_tracks_and_vias_removed": removed_tracks, "rules": rules, "out_dir": out},
          open(meta, "w"), indent=2)
PY
META_READY=1
[[ -s "$SES" ]] || not_graded "the engine wrote no SES (exit $ENGINE_EXIT, see $OUT/work/engine.log)"

python3 "$LIB" strip-placement "$SES" "$OUT/work/$BASE.noplace.ses"
stage "$OUT/routed"
"$KICAD_PYTHON" "$STEPS" import "$OUT/unrouted/$BASE.kicad_pcb" "$OUT/work/$BASE.noplace.ses" \
  "$OUT/routed/$BASE.kicad_pcb" > "$OUT/work/import.log" 2>&1 ||
  not_graded "SES import failed (see $OUT/work/import.log)"

for variant in unrouted routed; do
  dir="$OUT/$variant"
  original_rules "$dir"
  "$KICAD_CLI" pcb drc --format json --severity-all --refill-zones --output "$dir/drc.json" \
    "$dir/$BASE.kicad_pcb" > "$dir/drc.log" 2>&1
  [[ -s "$dir/drc.json" ]] || not_graded "kicad-cli DRC of the $variant board failed (see $dir/drc.log)"
done
"$KICAD_PYTHON" "$STEPS" stats "$OUT/routed/$BASE.kicad_pcb" "$OUT/work/classwidths.json" \
  "$OUT/routed/copper.json" > "$OUT/work/stats.log" 2>&1 ||
  not_graded "copper statistics failed (see $OUT/work/stats.log)"

if ! sha256sum --quiet -c "$OUT/originals.sha256" > /dev/null 2>&1; then
  not_graded "an original project file changed during grading"
fi
python3 - "$META" << 'PY'
import json, sys
meta = json.load(open(sys.argv[1]))
meta["originals_unchanged"] = True
json.dump(meta, open(sys.argv[1], "w"), indent=2)
PY

# Q15.2: the tidiness of the route (detour, bends, acute corners, jogs, vias per connection:
# median and p90), from the SES the engine wrote. Reported next to the DRC, never part of the
# verdict. Needs a built freerouting-dev (FREEROUTING_DEV, default target/release/freerouting-dev);
# without it, or when it fails, the summary simply has no `tidiness`.
FREEROUTING_DEV=${FREEROUTING_DEV:-$HERE/../../target/release/freerouting-dev}
rm -f "$OUT/tidy.json"
if [[ -x "$FREEROUTING_DEV" ]]; then
  "$FREEROUTING_DEV" tidy "$DSN" "$SES" --json "$OUT/tidy.json" > "$OUT/work/tidy.log" 2>&1 ||
    rm -f "$OUT/tidy.json"
fi
python3 "$LIB" summarize "$OUT/routed/drc.json" "$OUT/unrouted/drc.json" "$OUT/routed/copper.json" \
  "$META" "$SUMMARY"
STATUS=$?
python3 "$LIB" add-tidiness "$SUMMARY" "$OUT/tidy.json"
echo "summary: $SUMMARY (KiCad $CLI_VERSION)"
exit $STATUS
