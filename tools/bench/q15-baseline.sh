#!/usr/bin/env bash
# Q15.3: the tidiness baseline on the X13 boards that have a Java route reference.
#
#   tools/bench/q15-baseline.sh ARM OUT_DIR
#
# ARM is one of
#   java-ref        score Java's route references (fixtures/reference/route/*.ses); runs nothing
#   legacy          route each board with `freerouting --profile legacy`, then score the SES
#   enhanced-sweep  route with `--profile enhanced --router.autorouter.sweep=free
#                   --router.autorouter.sweep_passes=2`, then score the SES
#
# The two routing arms run the autorouter (minutes per board): run them on a quiet machine, in
# the heavy-job slot, from a release build (`cargo build --release -p freerouting-cli
# -p freerouting-dev`). Each uses the pass bound of Java's reference run
# (tools/java-reference/route-bounds.tsv) and the optimizer off, like `bench` and the reference.
#
# Writes OUT_DIR/<board>.<arm>.ses (routing arms), OUT_DIR/<board>.<arm>.tidy.json and prints a
# Markdown table: connections scored, Java's incomplete count for the same session, and the
# median / p90 of the five measures. FREEROUTING and FREEROUTING_DEV override the binaries.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
FREEROUTING=${FREEROUTING:-$ROOT/target/release/freerouting}
FREEROUTING_DEV=${FREEROUTING_DEV:-$ROOT/target/release/freerouting-dev}
ARM=${1:-}
OUT=${2:-}
[[ -n "$ARM" && -n "$OUT" ]] || { echo "usage: $0 java-ref|legacy|enhanced-sweep OUT_DIR" >&2; exit 2; }
case "$ARM" in
  java-ref) ARGS=() ;;
  legacy) ARGS=(--profile legacy) ;;
  enhanced-sweep) ARGS=(--profile enhanced --router.autorouter.sweep=free --router.autorouter.sweep_passes=2) ;;
  *) echo "unknown arm '$ARM'" >&2; exit 2 ;;
esac
mkdir -p "$OUT"
[[ -x "$FREEROUTING_DEV" ]] || { echo "build freerouting-dev first ($FREEROUTING_DEV)" >&2; exit 2; }
[[ "$ARM" == java-ref || -x "$FREEROUTING" ]] || { echo "build freerouting first ($FREEROUTING)" >&2; exit 2; }

# fixture name and its pass bound in tools/java-reference/route-bounds.tsv
BOARDS=(
  "Issue690-ecc83 100"
  "Issue690-sonde_xilinx 100"
  "Issue093-interf_u 100"
  "Issue690-kit-dev-coldfire-xilinx_5213 3"
)
echo "| board | arm | connections | incomplete | detour | bends/10mm | acute | jogs | vias/conn |"
echo "|---|---|---|---|---|---|---|---|---|"
for entry in "${BOARDS[@]}"; do
  read -r BOARD PASSES <<< "$entry"
  DSN="$ROOT/fixtures/$BOARD.dsn"
  if [[ "$ARM" == java-ref ]]; then
    SES="$ROOT/fixtures/reference/route/$BOARD.ses"
  else
    SES="$OUT/$BOARD.$ARM.ses"
    rm -f "$SES"
    "$FREEROUTING" "${ARGS[@]}" -de "$DSN" -do "$SES" --gui.enabled=false \
      --router.autorouter.max_passes="$PASSES" --router.optimizer.enabled=false \
      > "$OUT/$BOARD.$ARM.log" 2>&1
  fi
  JSON="$OUT/$BOARD.$ARM.tidy.json"
  if [[ ! -s "$SES" ]] || ! "$FREEROUTING_DEV" tidy "$DSN" "$SES" --json "$JSON" > /dev/null 2>&1; then
    echo "| $BOARD | $ARM | not scored (no SES or tidy failed) | | | | | | |"
    continue
  fi
  INCOMPLETE=$("$FREEROUTING_DEV" ses-diff "$DSN" "$SES" "$SES" 2> /dev/null |
    sed -n 's/.*connections.incomplete_count: \([0-9]*\) ->.*/\1/p' | head -n 1)
  python3 - "$JSON" "$BOARD" "$ARM" "${INCOMPLETE:-?}" << 'PY'
import json, sys
path, board, arm, incomplete = sys.argv[1:5]
j = json.load(open(path))
m = j["measures"]
cell = lambda k: "%.2f / %.2f" % (m[k]["median"], m[k]["p90"]) if k in m else "-"
print("| %s | %s | %d | %s | %s |" % (board, arm, j["connections"], incomplete,
      " | ".join(cell(k) for k in ("detour", "bends_per_10mm", "acute", "jogs", "vias"))))
PY
done
