#!/usr/bin/env bash
# Grades an already-routed .kicad_pcb against its unrouted input with the KiCad DRC oracle of
# tools/kicad-oracle (F02.6), for engines that write a board instead of a SES (KRT).
#
#   tools/krt-h2h/grade_routed.sh --out DIR [--meta META.json] INPUT.kicad_pcb ROUTED.kicad_pcb
#
# INPUT    the unrouted input the engine was given; its directory holds the board's ORIGINAL
#          .kicad_pro/.kicad_dru (same base name). Only read.
# ROUTED   the engine's output board. Only read.
# --meta   a JSON object merged into the summary (engine_command, engine_exit, engine_seconds).
#
# The same steps as grade.sh after its SES import, with the same scripts: both boards are
# staged under the input's base name with the ORIGINAL rules copied over them (a router never
# grades itself under rules it wrote) and the copper severities forced to error
# (oracle_lib.py force-errors); the input's DSN is exported only for the net-class widths
# (kicad_steps.py export, oracle_lib.py class-widths); `kicad-cli pcb drc --format json
# --severity-all --refill-zones` runs on both; copper statistics come from the routed board's
# geometry (kicad_steps.py stats); the input's own items are subtracted (oracle_lib.py
# summarize). Writes DIR/summary.json and prints one line. Passing the input as ROUTED grades
# the input itself (its pre-existing DRC).
#
# Exit status as grade.sh: 0 graded clean, 1 router-attributable items, 2 not graded.
# KICAD_CLI and KICAD_PYTHON pick the install; both must report the same version.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ORACLE="$HERE/../kicad-oracle"
LIB="$ORACLE/oracle_lib.py"
STEPS="$ORACLE/kicad_steps.py"
KICAD_CLI=${KICAD_CLI:-kicad-cli}
KICAD_PYTHON=${KICAD_PYTHON:-python3}

usage() {
  echo "usage: $0 --out DIR [--meta META.json] INPUT.kicad_pcb ROUTED.kicad_pcb" >&2
  exit 2
}
OUT="" META_IN=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT=${2:-}; shift 2 || usage ;;
    --meta) META_IN=${2:-}; shift 2 || usage ;;
    -*) usage ;;
    *) break ;;
  esac
done
[[ $# -eq 2 && -n "$OUT" ]] || usage
INPUT=$(realpath "$1")
ROUTED=$(realpath "$2")
mkdir -p "$OUT/work"
OUT=$(cd "$OUT" && pwd)
SUMMARY="$OUT/summary.json"
META="$OUT/meta.json"
BASE=$(basename "$INPUT" .kicad_pcb)
PROJECT=$(dirname "$INPUT")

python3 - "$META" "$INPUT" "$ROUTED" "$BASE" "$OUT" "$META_IN" << 'PY'
import json, sys
meta, inp, routed, base, out, extra = sys.argv[1:7]
data = {"project": inp, "routed_board": routed, "board": base, "out_dir": out,
        "grade_entry": "tools/krt-h2h/grade_routed.sh"}
if extra:
    data.update(json.load(open(extra)))
json.dump(data, open(meta, "w"), indent=2)
PY

not_graded() {
  python3 "$LIB" not-graded "$1" "$SUMMARY" "$META"
  echo "not graded: $1 (summary: $SUMMARY)"
  exit 2
}
[[ -f "$INPUT" ]] || not_graded "no input board $INPUT"
[[ -s "$ROUTED" ]] || not_graded "no routed board $ROUTED"
command -v "$KICAD_CLI" > /dev/null 2>&1 || not_graded "kicad-cli not found ('$KICAD_CLI')"
CLI_VERSION=$("$KICAD_CLI" version 2> /dev/null | tail -n 1)
PY_VERSION=$("$KICAD_PYTHON" "$STEPS" version 2> /dev/null | tail -n 1)
[[ -n "$PY_VERSION" ]] || not_graded "'$KICAD_PYTHON' cannot import pcbnew"
[[ "$CLI_VERSION" == "$PY_VERSION" ]] ||
  not_graded "kicad-cli is $CLI_VERSION but pcbnew is $PY_VERSION; never mix KiCad versions"
python3 - "$META" "$CLI_VERSION" << 'PY'
import json, sys
meta = json.load(open(sys.argv[1]))
meta["kicad_version"] = sys.argv[2]
json.dump(meta, open(sys.argv[1], "w"), indent=2)
PY

ORIGINALS=("$INPUT")
for ext in kicad_pro kicad_dru; do
  [[ -f "$PROJECT/$BASE.$ext" ]] && ORIGINALS+=("$PROJECT/$BASE.$ext")
done
sha256sum "${ORIGINALS[@]}" "$ROUTED" > "$OUT/originals.sha256"

stage() { # dir board
  mkdir -p "$1"
  cp "$2" "$1/$BASE.kicad_pcb"
  local ext
  for ext in kicad_pro kicad_dru; do
    rm -f "$1/$BASE.$ext"
    [[ -f "$PROJECT/$BASE.$ext" ]] && cp "$PROJECT/$BASE.$ext" "$1/"
  done
  chmod u+w "$1"/*
  [[ -f "$1/$BASE.kicad_pro" ]] && python3 "$LIB" force-errors "$1/$BASE.kicad_pro"
  return 0
}
stage "$OUT/unrouted" "$INPUT"
stage "$OUT/routed" "$ROUTED"

"$KICAD_PYTHON" "$STEPS" export "$OUT/unrouted/$BASE.kicad_pcb" "$OUT/work/$BASE.dsn" \
  "$OUT/work/areas.json" > "$OUT/work/export.log" 2>&1 ||
  not_graded "DSN export for the class widths failed (see $OUT/work/export.log)"
# The export saved nothing into the staged board, but restage it to be sure the DRC reads the
# input byte for byte.
stage "$OUT/unrouted" "$INPUT"
python3 "$LIB" class-widths "$OUT/work/$BASE.dsn" "$OUT/work/classwidths.json"

for variant in unrouted routed; do
  dir="$OUT/$variant"
  "$KICAD_CLI" pcb drc --format json --severity-all --refill-zones --output "$dir/drc.json" \
    "$dir/$BASE.kicad_pcb" > "$dir/drc.log" 2>&1
  [[ -s "$dir/drc.json" ]] || not_graded "kicad-cli DRC of the $variant board failed (see $dir/drc.log)"
done
"$KICAD_PYTHON" "$STEPS" stats "$OUT/routed/$BASE.kicad_pcb" "$OUT/work/classwidths.json" \
  "$OUT/routed/copper.json" > "$OUT/work/stats.log" 2>&1 ||
  not_graded "copper statistics failed (see $OUT/work/stats.log)"

sha256sum --quiet -c "$OUT/originals.sha256" > /dev/null 2>&1 ||
  not_graded "the input, its rules or the routed board changed during grading"
python3 - "$META" << 'PY'
import json, sys
meta = json.load(open(sys.argv[1]))
meta["originals_unchanged"] = True
json.dump(meta, open(sys.argv[1], "w"), indent=2)
PY
# Q15.2: the tidiness of the routed board's copper (the DSN KiCad exported from the input holds
# the pins; the input should be unrouted, or its tracks count twice). Reported, not judged;
# skipped without a built freerouting-dev (FREEROUTING_DEV, default target/release/freerouting-dev).
FREEROUTING_DEV=${FREEROUTING_DEV:-$HERE/../../target/release/freerouting-dev}
rm -f "$OUT/tidy.json"
if [[ -x "$FREEROUTING_DEV" ]]; then
  "$FREEROUTING_DEV" tidy "$OUT/work/$BASE.dsn" "$ROUTED" --json "$OUT/tidy.json" \
    > "$OUT/work/tidy.log" 2>&1 || rm -f "$OUT/tidy.json"
fi
python3 "$LIB" summarize "$OUT/routed/drc.json" "$OUT/unrouted/drc.json" "$OUT/routed/copper.json" \
  "$META" "$SUMMARY"
STATUS=$?
python3 "$LIB" add-tidiness "$SUMMARY" "$OUT/tidy.json"
echo "summary: $SUMMARY (KiCad $CLI_VERSION)"
exit $STATUS
