#!/usr/bin/env bash
# Prepares the fetched boards for routing: strips tracks and vias but keeps zones and layer
# types (KiCadRoutingTools' prep drops both), exports the DSN through the KiCad oracle's steps
# (tools/kicad-oracle, F02.6) and removes the structure keepouts KiCad exports for rule areas
# that allow tracks and vias.
#
#   tools/corpus/prep.sh [--ids a,b]
#
# Reads $FR_CORPUS_DIR/src/<id>/ (tools/corpus/fetch.sh) and writes
#   $FR_CORPUS_DIR/prepped/<id>.dsn   first line rewritten to `(pcb <id>` so the file does not
#                                     depend on the directory (what the KiCad plugin does)
#   $FR_CORPUS_DIR/prepped/<id>.human.dsn   the same board with the human's tracks kept
#   $FR_CORPUS_DIR/prepped/<id>.json  strip and keepout counts, KiCad version
#   $FR_CORPUS_DIR/projects/<id>/     the board with its .kicad_pro/.kicad_dru, all named <id>
#   $FR_CORPUS_DIR/grade-set.txt      those .kicad_pcb paths, for tools/kicad-oracle/grade-set.sh
# A board that KiCad cannot load or export is listed in $FR_CORPUS_DIR/prepped/FAILED.tsv and
# the script exits 1. KICAD_CLI / KICAD_PYTHON as in the oracle (both must be the same version).
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
DIR=${FR_CORPUS_DIR:-$ROOT/target/corpus}
ORACLE=$ROOT/tools/kicad-oracle
KICAD_PYTHON=${KICAD_PYTHON:-python3}
IDS=""
[[ "${1:-}" == "--ids" ]] && IDS=${2:-}
mkdir -p "$DIR/prepped" "$DIR/work"
: > "$DIR/prepped/FAILED.tsv"
: > "$DIR/grade-set.txt"
VERSION=$("$KICAD_PYTHON" "$ORACLE/kicad_steps.py" version 2> /dev/null | tail -n 1)
[[ -n "$VERSION" ]] || { echo "prep: '$KICAD_PYTHON' cannot import pcbnew" >&2; exit 1; }
FAILED=0
for src in "$DIR"/src/*/; do
  id=$(basename "$src")
  [[ -n "$IDS" && ",$IDS," != *",$id,"* ]] && continue
  pcb=$(ls "$src"*.kicad_pcb 2> /dev/null | head -n 1)
  [[ -n "$pcb" ]] || continue
  base=$(basename "$pcb" .kicad_pcb)
  work=$DIR/work/$id
  rm -rf "$work"
  mkdir -p "$work/unrouted" "$work/out"
  cp "$src"/* "$work/unrouted/"
  chmod u+w "$work/unrouted"/*
  fail() {
    printf '%s\t%s\n' "$id" "$1" >> "$DIR/prepped/FAILED.tsv"
    echo "prep: $id: $1" >&2
    FAILED=1
  }
  "$KICAD_PYTHON" "$ORACLE/kicad_steps.py" strip "$pcb" "$work/unrouted/$base.kicad_pcb" \
    > "$work/strip.log" 2>&1 || { fail "strip failed (see $work/strip.log)"; continue; }
  "$KICAD_PYTHON" "$ORACLE/kicad_steps.py" export "$work/unrouted/$base.kicad_pcb" "$work/out/$base.kicad.dsn" \
    "$work/out/areas.json" > "$work/export.log" 2>&1 || { fail "DSN export failed (see $work/export.log)"; continue; }
  removed=$(python3 -I "$ORACLE/oracle_lib.py" fix-keepouts "$work/out/$base.kicad.dsn" "$work/out/areas.json" \
    "$work/out/$base.dsn") || { fail "keepout fix failed"; continue; }
  { echo "(pcb $id"; tail -n +2 "$work/out/$base.dsn"; } > "$DIR/prepped/$id.dsn"
  python3 -I - "$work/unrouted/$base.kicad_pcb.strip.json" "$DIR/prepped/$id.json" "$VERSION" "$removed" << 'PY'
import json, sys
strip, out, version, removed = sys.argv[1:5]
json.dump({"removed_tracks_and_vias": json.load(open(strip))["removed_tracks_and_vias"],
           "keepouts_removed": int(removed), "kicad_version": version}, open(out, "w"), indent=1)
PY
  # The human-routed original as a DSN (tracks kept), for freerouting-dev failed-vs-human.
  mkdir -p "$work/human"
  cp "$src"/* "$work/human/"
  chmod u+w "$work/human"/*
  "$KICAD_PYTHON" "$ORACLE/kicad_steps.py" export "$work/human/$base.kicad_pcb" "$work/out/$base.human.kicad.dsn" \
    "$work/out/human-areas.json" > "$work/export-human.log" 2>&1 || { fail "human DSN export failed (see $work/export-human.log)"; continue; }
  python3 -I "$ORACLE/oracle_lib.py" fix-keepouts "$work/out/$base.human.kicad.dsn" "$work/out/human-areas.json" \
    "$work/out/$base.human.dsn" > /dev/null || { fail "human keepout fix failed"; continue; }
  { echo "(pcb $id"; tail -n +2 "$work/out/$base.human.dsn"; } > "$DIR/prepped/$id.human.dsn"
  # A project directory per board named after the id, so the oracle's output directory
  # (named after the board file) and the bench's row name agree.
  mkdir -p "$DIR/projects/$id"
  for ext in kicad_pcb kicad_pro kicad_dru; do
    [[ -f "$src$base.$ext" ]] && cp "$src$base.$ext" "$DIR/projects/$id/$id.$ext"
  done
  echo "$DIR/projects/$id/$id.kicad_pcb" >> "$DIR/grade-set.txt"
  echo "prep: $id ok"
done
[[ -s "$DIR/prepped/FAILED.tsv" ]] || rm -f "$DIR/prepped/FAILED.tsv"
exit $FAILED
