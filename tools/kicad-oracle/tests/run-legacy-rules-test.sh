#!/usr/bin/env bash
# End-to-end test of grade.sh on a KiCad 4 board without a .kicad_pro (Q19.2). Needs kicad-cli
# and a python3 with pcbnew (KiCad 10); no Freerouting jar, the "engine" copies a fixed session.
# Never starts the KiCad GUI.
#
#   tools/kicad-oracle/tests/run-legacy-rules-test.sh WORK_DIR
#
# A KiCad 4 board keeps its net classes in the board file; KiCad 6+ moves them to the project
# when it loads the board. The board of make_legacy_board.py says 0.15 mm clearance, KiCad's
# default is 0.2 mm. Checks:
#   1. a 0.17 mm gap between two nets grades with no clearance item, and the summary says the
#      rules came from the board (`rules: board`);
#   2. a 0.12 mm gap is still a router-attributable clearance item (exit 1);
#   3. the originals' checksums are unchanged and no .kicad_pro appeared next to them.
set -uo pipefail
[[ $# -eq 1 ]] || { echo "usage: $0 WORK_DIR" >&2; exit 2; }
mkdir -p "$1"
W=$(cd "$1" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
GRADE="$HERE/../grade.sh"
export PYTHONDONTWRITEBYTECODE=1
FAILED=0
fail() { echo "FAIL: $*"; FAILED=1; }
ok() { echo "ok: $*"; }
field() {
  python3 -c "import json, sys; d = json.load(open(sys.argv[1]))
for k in sys.argv[2:]: d = d.get(k, {}) if isinstance(d, dict) else {}
print(d if d != {} else 0)" "$@"
}

rm -rf "$W/projects" "$W/gap-170" "$W/gap-120"
mkdir -p "$W/projects"
python3 "$HERE/make_legacy_board.py" "$W/projects" > /dev/null
sha256sum "$W/projects/legacy/"* > "$W/originals.sha256"

# 1. Inside the board's rule, outside KiCad's default.
"$GRADE" --out "$W/gap-170" "$W/projects/legacy" -- cp "$W/projects/gap-170.ses" {SES} \
  > "$W/gap-170.out" 2>&1
STATUS=$?
S="$W/gap-170/summary.json"
CLEARANCE=$(field "$S" routed_by_type clearance)
RULES=$(field "$S" rules)
echo "gap 0.17 mm: exit $STATUS, routed clearance items $CLEARANCE, rules $RULES"
if [[ $(field "$S" graded) == True && $CLEARANCE == 0 && $RULES == board ]]; then
  ok "a gap inside the board's own rule is clean"
else
  fail "a gap inside the board's own rule (see $W/gap-170.out)"
fi

# 2. A breach of the board's rule.
"$GRADE" --out "$W/gap-120" "$W/projects/legacy" -- cp "$W/projects/gap-120.ses" {SES} \
  > "$W/gap-120.out" 2>&1
STATUS=$?
REAL=$(field "$W/gap-120/summary.json" drc_real_by_type clearance)
echo "gap 0.12 mm: exit $STATUS, router-attributable clearance items $REAL"
if [[ $STATUS -eq 1 && $REAL -ge 1 ]]; then
  ok "a breach of the board's own rule is reported"
else
  fail "a breach of the board's own rule (see $W/gap-120.out)"
fi

# 3. The originals.
if sha256sum --quiet -c "$W/originals.sha256" > /dev/null 2>&1 &&
  [[ ! -e "$W/projects/legacy/legacy.kicad_pro" ]]; then
  ok "originals unchanged"
else
  fail "an original changed or a .kicad_pro appeared in the project"
fi
exit $FAILED
