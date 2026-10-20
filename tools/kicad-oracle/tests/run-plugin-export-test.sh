#!/usr/bin/env bash
# End-to-end test of `grade.sh --export plugin` (Q22.6). Needs kicad-cli and a python3 with pcbnew
# (KiCad 10); no Freerouting jar, the "engine" records its arguments and copies a fixed session.
# Never starts the KiCad GUI.
#
#   tools/kicad-oracle/tests/run-plugin-export-test.sh WORK_DIR
#
# The board and session are make_legacy_board.py's. Checks:
#   1. the DSN the grader routes is byte-identical to the one `headless_run.py export` writes for the
#      same board and options (the plugin's export and fix-ups, not the bare ExportSpecctraDSN);
#      and carries its fix-ups (the 8 edge-strip keepouts of the 0.5 mm edge clearance);
#   2. the engine gets the plugin's router flags (the options' --max-passes and --seed, the
#      board minimums) after {SES} and before the command's own flag, so the command's flag wins;
#   3. `{PLUGIN_ARGS}` puts the same flags where the command says (one string, for `bash -c`);
#   4. a command with neither is "not graded" (exit 2), never routed with the wrong flags;
#   5. without the flag nothing changes: the bare export, no plugin flags, `export: bare`;
#   6. the summary is graded from the plugin's import (`export: plugin`, copper basis).
set -uo pipefail
[[ $# -eq 1 ]] || { echo "usage: $0 WORK_DIR" >&2; exit 2; }
mkdir -p "$1"
W=$(cd "$1" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
GRADE="$HERE/../grade.sh"
HEADLESS="$HERE/../../../kicad-plugin/headless_run.py"
KICAD_PYTHON=${KICAD_PYTHON:-python3}
export PYTHONDONTWRITEBYTECODE=1
FAILED=0
fail() { echo "FAIL: $*"; FAILED=1; }
ok() { echo "ok: $*"; }
field() {
  python3 -c "import json, sys; d = json.load(open(sys.argv[1]))
for k in sys.argv[2:]: d = d.get(k, {}) if isinstance(d, dict) else {}
print(d if d != {} else 0)" "$@"
}

rm -rf "$W/projects" "$W/plugin" "$W/placeholder" "$W/missing" "$W/bare"
mkdir -p "$W/projects"
python3 "$HERE/make_legacy_board.py" "$W/projects" > /dev/null
cat > "$W/options.json" << 'JSON'
{"schema": 4, "max_passes": 3, "seed": 7}
JSON
cat > "$W/engine.sh" << 'SH'
#!/usr/bin/env bash
# engine.sh DSN SES [FLAGS...]: records all its arguments, then writes the fixed session.
echo "$@" > "$(dirname "$2")/engine-args.txt"
cp "$ENGINE_SES" "$2"
SH
chmod +x "$W/engine.sh"
export ENGINE_SES="$W/projects/gap-170.ses"

# 1 + 2 + 6. Plugin export, flags after {SES}.
"$GRADE" --out "$W/plugin" --export plugin "$W/options.json" "$W/projects/legacy" -- \
  "$W/engine.sh" '{DSN}' '{SES}' --engine-flag > "$W/plugin.out" 2>&1
STATUS=$?
S="$W/plugin/summary.json"
echo "plugin export: exit $STATUS"
if [[ $(field "$S" graded) == True && $(field "$S" export) == plugin &&
  $(field "$S" routing_caused_basis) == copper ]]; then
  ok "graded through the plugin's export and import, routing-caused counted from the copper"
else
  fail "plugin export not graded (see $W/plugin.out)"
fi
# The DSN's first line names its own path, so export again to the same path, after keeping a copy.
cp "$W/plugin/work/legacy.dsn" "$W/graded.dsn"
"$KICAD_PYTHON" -I "$HEADLESS" export "$W/plugin/unrouted/legacy.kicad_pcb" "$W/plugin/work" \
  "$W/options.json" > "$W/headless.out" 2>&1
if [[ -s "$W/graded.dsn" ]] && cmp -s "$W/graded.dsn" "$W/plugin/work/legacy.dsn"; then
  ok "the graded DSN is byte-identical to headless_run.py export's"
else
  fail "the graded DSN differs from headless_run.py export's (see $W/headless.out)"
fi
KEEPOUTS=$(grep -c '(keepout' "$W/plugin/work/legacy.dsn")
if [[ $KEEPOUTS -eq 8 ]]; then
  ok "the plugin's fix-ups are in the DSN (8 edge-strip keepouts; the bare export has none)"
else
  fail "expected the plugin's 8 edge-strip keepouts in the DSN, found $KEEPOUTS"
fi
ARGS=$(cat "$W/plugin/work/engine-args.txt")
echo "engine arguments: $ARGS"
if [[ $ARGS == "$W/plugin/work/legacy.dsn $W/plugin/work/legacy.ses --max-passes 3 --seed 7 "*"--engine-flag" &&
  $ARGS == *"--router.board_minimums.track_width_um="* ]]; then
  ok "the plugin's flags come after {SES} and before the command's own flag"
else
  fail "engine arguments: $ARGS"
fi

# 3. {PLUGIN_ARGS}.
"$GRADE" --out "$W/placeholder" --export plugin "$W/options.json" "$W/projects/legacy" -- \
  bash -c '"$0" "$@"' "$W/engine.sh" '{DSN}' '{SES}' '{PLUGIN_ARGS}' --engine-flag > "$W/placeholder.out" 2>&1
echo "placeholder: exit $?"
PLACED=$(cat "$W/placeholder/work/engine-args.txt" 2> /dev/null)
if [[ $PLACED == *" --max-passes 3 --seed 7 "*"--engine-flag" ]]; then
  ok "{PLUGIN_ARGS} is replaced by the shell-quoted flags"
else
  fail "{PLUGIN_ARGS}: $PLACED (see $W/placeholder.out)"
fi

# 4. No place for the flags.
"$GRADE" --out "$W/missing" --export plugin "$W/options.json" "$W/projects/legacy" -- \
  bash -c 'exit 0' '{DSN}' > "$W/missing.out" 2>&1
STATUS=$?
if [[ $STATUS -eq 2 && $(field "$W/missing/summary.json" graded) == False ]]; then
  ok "no {SES} and no {PLUGIN_ARGS}: not graded"
else
  fail "a command with no place for the plugin flags (exit $STATUS, see $W/missing.out)"
fi

# 5. Without the flag.
"$GRADE" --out "$W/bare" "$W/projects/legacy" -- "$W/engine.sh" '{DSN}' '{SES}' --engine-flag \
  > "$W/bare.out" 2>&1
BARE=$(cat "$W/bare/work/engine-args.txt" 2> /dev/null)
if [[ $BARE == "$W/bare/work/legacy.dsn $W/bare/work/legacy.ses --engine-flag" &&
  $(field "$W/bare/summary.json" export) == bare && -f "$W/bare/work/legacy.kicad.dsn" ]]; then
  ok "without --export the bare export is routed, with no plugin flags"
else
  fail "bare run: $BARE (see $W/bare.out)"
fi

exit $FAILED
