#!/usr/bin/env bash
# End-to-end tests of tools/kicad-oracle/grade.sh (F02.6). Needs kicad-cli and a python3 with
# pcbnew (KiCad 10), KiCad's bundled demos, and a Freerouting jar. Never starts the KiCad GUI.
#
#   tools/kicad-oracle/tests/run-oracle-tests.sh JAR WORK_DIR [DEMOS_DIR]
#
# DEMOS_DIR defaults to /usr/share/kicad/demos. Checks:
#   1. a via on an SMD pad of another net (pic_programmer), joined to its own net's nearest
#      wire end, is a new copper item that the unedited Java session doesn't have;
#   2. a board whose author set `clearance` to `warning` (ecc83) still reports an injected
#      0.05 mm graze, at severity error;
#   3. an untouched (empty) session on a board with one pre-existing edge item gives
#      preexisting=1, drc_real=0, exit 0;
#   4. a missing kicad-cli gives exit 2 and "not graded";
#   5. `pgrep -x kicad` is the same before and after, and the originals' checksums are unchanged.
set -uo pipefail
[[ $# -ge 2 ]] || { echo "usage: $0 JAR WORK_DIR [DEMOS_DIR]" >&2; exit 2; }
JAR=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
mkdir -p "$2"
W=$(cd "$2" && pwd)
DEMOS=${3:-/usr/share/kicad/demos}
HERE=$(cd "$(dirname "$0")" && pwd)
GRADE="$HERE/../grade.sh"
INJECT="$HERE/inject.py"
STEPS="$HERE/../kicad_steps.py"
export PYTHONDONTWRITEBYTECODE=1
JAVA=(java -jar "$JAR" -de {DSN} -do {SES} --gui.enabled=false
  --router.autorouter.max_passes=5 --router.optimizer.enabled=false)
FAILED=0
fail() { echo "FAIL: $*"; FAILED=1; }
ok() { echo "ok: $*"; }
field() { python3 -c "import json, sys; print(json.load(open(sys.argv[1])).get(sys.argv[2]))" "$1" "$2"; }

KICAD_BEFORE=$(pgrep -x kicad | sort | tr '\n' ' ')
mkdir -p "$W/projects"
cp -r "$DEMOS/pic_programmer" "$W/projects/pic"
mkdir -p "$W/projects/ecc83-warn"
cp "$DEMOS/ecc83/ecc83-pp.kicad_pcb" "$DEMOS/ecc83/ecc83-pp.kicad_pro" "$W/projects/ecc83-warn/"
chmod -R u+w "$W/projects"
python3 - "$W/projects/ecc83-warn/ecc83-pp.kicad_pro" << 'PY'
import json, sys
project = json.load(open(sys.argv[1]))
project["board"]["design_settings"]["rule_severities"]["clearance"] = "warning"
json.dump(project, open(sys.argv[1], "w"), indent=2)
PY
python3 "$HERE/make_edge_board.py" "$W/projects/edge" > /dev/null 2>&1
find "$W/projects" "$DEMOS/ecc83" "$DEMOS/pic_programmer" -type f -print0 | sort -z |
  xargs -0 sha256sum > "$W/originals.sha256"

# 1. A via on an SMD pad of another net.
"$GRADE" --out "$W/pic" "$W/projects/pic" -- "${JAVA[@]}" > "$W/pic.out"
python3 "$STEPS" pads "$W/pic/unrouted/pic_programmer.kicad_pcb" "$W/pic-pads.json" > /dev/null 2>&1
read -r NET PADSTACK X Y EX EY LAYER WIDTH < <(python3 "$INJECT" pick "$W/pic-pads.json" \
  "$W/pic/work/pic_programmer.dsn" "$W/pic/work/pic_programmer.ses" in-pad 2> "$W/pic-pick.txt" | tr -d '"')
cp "$W/pic/work/pic_programmer.ses" "$W/pic-java.ses"
"$GRADE" --out "$W/pic-via" "$W/projects/pic" -- python3 "$INJECT" via "$W/pic-java.ses" {SES} \
  "$NET" "$PADSTACK" "$X" "$Y" "$EX" "$EY" "$LAYER" "$WIDTH" > "$W/pic-via.out"
STATUS=$?
NEW=$(python3 - "$W/pic-via/routed/drc.json" "$W/pic/routed/drc.json" "$HERE/.." << 'PY'
import json, sys
sys.path.insert(0, sys.argv[3])
import oracle_lib as lib
edited, unedited = (json.load(open(p))["violations"] for p in sys.argv[1:3])
new, _ = lib.subtract_preexisting(edited, unedited)
print(" ".join(sorted(v["type"] for v in new if v["type"] in ("clearance", "shorting_items"))))
PY
)
echo "pic: via of net $NET on $(cat "$W/pic-pick.txt"); new copper items: ${NEW:-none}; exit $STATUS"
if [[ -n "$NEW" && $STATUS -eq 1 ]]; then ok "via in an SMD pad is reported"; else fail "via in an SMD pad"; fi

# 2. A 0.05 mm graze on a board whose author set clearance to warning.
"$GRADE" --out "$W/ecc83" "$W/projects/ecc83-warn" -- "${JAVA[@]}" > "$W/ecc83.out"
BASE_REAL=$(field "$W/ecc83/summary.json" drc_real)
python3 "$STEPS" pads "$W/ecc83/unrouted/ecc83-pp.kicad_pcb" "$W/ecc83-pads.json" --all > /dev/null 2>&1
read -r NET PADSTACK X Y < <(python3 "$INJECT" pick "$W/ecc83-pads.json" "$W/ecc83/work/ecc83-pp.dsn" \
  "$W/ecc83/work/ecc83-pp.ses" graze 2> "$W/ecc83-pick.txt" | tr -d '"')
cp "$W/ecc83/work/ecc83-pp.ses" "$W/ecc83-java.ses"
"$GRADE" --out "$W/ecc83-graze" "$W/projects/ecc83-warn" -- python3 "$INJECT" via "$W/ecc83-java.ses" \
  {SES} "$NET" "$PADSTACK" "$X" "$Y" > "$W/ecc83-graze.out"
STATUS=$?
GRAZE=$(python3 - "$W/ecc83-graze/summary.json" "$W/ecc83-graze/routed/drc.json" << 'PY'
import json, sys
summary, drc = (json.load(open(p)) for p in sys.argv[1:3])
severities = sorted({v["severity"] for v in drc["violations"] if v["type"] == "clearance"})
print(summary["drc_real_by_type"].get("clearance", 0), ",".join(severities))
PY
)
echo "ecc83 (clearance=warning): unedited drc_real=$BASE_REAL; graze via of net $NET at ($X, $Y) next to $(cat "$W/ecc83-pick.txt"); new clearance items and severity: $GRAZE; exit $STATUS"
if [[ "$GRAZE" =~ ^[1-9][0-9]*\ error$ && "$BASE_REAL" == 0 ]]; then
  ok "graze reported at error under a warning-level clearance"
else
  fail "graze under a warning-level clearance"
fi

# 3. An untouched session on a board with one pre-existing edge item.
"$GRADE" --out "$W/edge" "$W/projects/edge" -- python3 "$INJECT" empty {DSN} {SES} > "$W/edge.out"
STATUS=$?
PRE=$(field "$W/edge/summary.json" preexisting)
REAL=$(field "$W/edge/summary.json" drc_real)
cat "$W/edge.out"
if [[ $STATUS -eq 0 && "$PRE" == 1 && "$REAL" == 0 ]]; then
  ok "untouched session: preexisting=1, drc_real=0"
else
  fail "untouched session: exit $STATUS preexisting=$PRE drc_real=$REAL"
fi

# 4. No kicad-cli.
OUTPUT=$(KICAD_CLI="$W/no-such-kicad-cli" "$GRADE" --out "$W/nocli" "$W/projects/edge" -- true)
STATUS=$?
echo "$OUTPUT"
if [[ $STATUS -eq 2 && "$OUTPUT" == *"not graded"* && "$(field "$W/nocli/summary.json" graded)" == False ]]; then
  ok "missing kicad-cli: exit 2, not graded"
else
  fail "missing kicad-cli: exit $STATUS"
fi

# 5. KiCad GUI processes and the originals.
KICAD_AFTER=$(pgrep -x kicad | sort | tr '\n' ' ')
if [[ "$KICAD_BEFORE" == "$KICAD_AFTER" ]]; then
  ok "pgrep -x kicad unchanged ('$KICAD_BEFORE')"
else
  fail "KiCad processes changed: '$KICAD_BEFORE' -> '$KICAD_AFTER'"
fi
if sha256sum --quiet -c "$W/originals.sha256"; then
  ok "originals unchanged ($(wc -l < "$W/originals.sha256") files)"
else
  fail "an original changed"
fi

[[ $FAILED -eq 0 ]] && echo "kicad-oracle tests passed" || echo "kicad-oracle tests FAILED"
exit $FAILED
