#!/usr/bin/env bash
# Routes one X01 board with one arm, grades it and writes one record (X01.5).
#
#   tools/krt-h2h/run.sh --arm java|krt|krt-kicad --board ROW [--rep N] [--allow-load]
#                        [--max-load L] [--load-wait SECONDS]
#
# ROW is a row of docs/roadmap/prereg/X01.json, prepared by prep.sh under $X01_DIR/boards/ROW.
# Before starting, waits up to --load-wait (default 600) s for the 1-minute load average to be
# at most --max-load (default 1.0, F02.3's rule) and refuses (exit 3, a "not run" record)
# if it stays above, unless --allow-load: then it starts anyway and the record says so
# (wall time is then an upper bound). The load before and after is recorded.
#
# java       tools/kicad-oracle/grade.sh --keep-tracks on the prepared board, with the registered
#            jar command (export DSN, route, import SES, DRC). canary-pwr: the jar routes
#            fixtures/canary-pwr/canary-pwr-9511d9a.dsn into a SES; no local grading (canary-23
#            grades it through Windows KiCad).
# krt        route.py from the KRT export on a copy of the prepared board, with no kicad-cli
#            and no pcbnew reachable, then grade_routed.sh on the result. canary-pwr: no
#            grading here, as above.
# krt-kicad  the same with kicad-cli-shim.sh as kicad-cli (optional, reported only).
# Then copper_stats.py (vias and length added) and via_in_pad_kicad.py on the routed board.
#
# Writes $X01_DIR/runs/ROW/ARM[-repN]/record.json (metrics, commands, environment set, exit
# codes) and prints it. Timeout 7200 s per run (X01_TIMEOUT); it only stops the run.
# Exit 0 when a record was written (graded or "not graded" with its reason), 3 when refused.
set -uo pipefail
source "$(dirname "$0")/common.sh"
KICAD_PYTHON=${KICAD_PYTHON:-python3}
TIMEOUT=${X01_TIMEOUT:-7200}
JAVA_HEAP=${JAVA_HEAP:--Xmx3g}
JAR="$X01_DIR/java/freerouting-$JAVA_PIN.jar"
KRT_FLAGS=(--nets '*' --fab-tier standard --escalation off --same-net-pad-clearance 0.1
  --no-fix-drc-settings --keep-input-copper)
CANARY_ROW=canary-pwr-9511d9a
CANARY_DSN="$REPO_ROOT/fixtures/canary-pwr/canary-pwr-9511d9a.dsn"

usage() {
  echo "usage: $0 --arm java|krt|krt-kicad --board ROW [--rep N] [--allow-load] [--max-load L] [--load-wait S]" >&2
  exit 2
}
ARM="" ROW="" REP="" ALLOW_LOAD="" MAX_LOAD=1.0 LOAD_WAIT=600
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arm) ARM=${2:-}; shift 2 || usage ;;
    --board) ROW=${2:-}; shift 2 || usage ;;
    --rep) REP=${2:-}; shift 2 || usage ;;
    --allow-load) ALLOW_LOAD=1; shift ;;
    --max-load) MAX_LOAD=${2:-}; shift 2 || usage ;;
    --load-wait) LOAD_WAIT=${2:-}; shift 2 || usage ;;
    *) usage ;;
  esac
done
[[ "$ARM" =~ ^(java|krt|krt-kicad)$ && -n "$ROW" ]] || usage
PREP_DIR="$X01_DIR/boards/$ROW/prepared"
INPUT=$(ls "$PREP_DIR"/*.kicad_pcb 2> /dev/null | head -n 1)
[[ -n "$INPUT" ]] || { echo "no prepared board for $ROW (run prep.sh)" >&2; exit 2; }
BASE=$(basename "$INPUT" .kicad_pcb)
OUT="$X01_DIR/runs/$ROW/$ARM${REP:+-rep$REP}"
rm -rf "$OUT"
mkdir -p "$OUT"
RECORD="$OUT/record.json"

above() { python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) > float(sys.argv[2]) else 1)" "$1" "$2"; }
LOAD_BEFORE=$(load1)
waited=0
while above "$LOAD_BEFORE" "$MAX_LOAD" && [[ $waited -lt $LOAD_WAIT ]]; do
  timeout 15 tail -f /dev/null
  waited=$((waited + 15))
  LOAD_BEFORE=$(load1)
done
LOAD_RULE=quiet
if above "$LOAD_BEFORE" "$MAX_LOAD"; then
  if [[ -z "$ALLOW_LOAD" ]]; then
    printf '{"board": "%s", "arm": "%s", "run": false, "reason": "the 1-minute load average is %s, above %s after %s s"}\n' \
      "$ROW" "$ARM" "$LOAD_BEFORE" "$MAX_LOAD" "$waited" | tee "$RECORD"
    exit 3
  fi
  LOAD_RULE="overridden (--allow-load): load $LOAD_BEFORE above $MAX_LOAD after waiting $waited s"
fi

GRADE_EXIT="" GRADE_DIR="$OUT/grade" ROUTED="" OUTPUT="" ENV_SET="" ENGINE_CMD=""
TIME_FILE="$OUT/time.txt"
TIMER=(/usr/bin/time -o "$TIME_FILE" -f '%e %M')
[[ -x /usr/bin/time ]] || TIMER=()
START=$(date +%s.%N)
case "$ARM" in
  java)
    [[ -f "$JAR" ]] || { echo "no jar $JAR (run build-java.sh)" >&2; exit 2; }
    ENGINE=(java "$JAVA_HEAP" -jar "$JAR" -de {DSN} -do {SES} --gui.enabled=false
      --router.scoring.start_ripup_costs=1500)
    ENGINE_CMD="${ENGINE[*]}"
    if [[ "$ROW" == "$CANARY_ROW" ]]; then
      mkdir -p "$OUT/work"
      cp "$CANARY_DSN" "$OUT/work/$BASE.dsn"
      DSN="$OUT/work/$BASE.dsn" SES="$OUT/work/$BASE.ses"
      ARGS=()
      for a in "${ENGINE[@]}"; do a=${a//\{DSN\}/$DSN}; ARGS+=("${a//\{SES\}/$SES}"); done
      (cd "$OUT/work" && "${TIMER[@]}" timeout "$TIMEOUT" "${ARGS[@]}") > "$OUT/work/engine.log" 2>&1
      ENGINE_EXIT=$?
      OUTPUT="$SES"
    else
      "$ORACLE_DIR/grade.sh" --out "$GRADE_DIR" --keep-tracks "$INPUT" -- \
        "${TIMER[@]}" timeout "$TIMEOUT" "${ENGINE[@]}" > "$OUT/grade.log" 2>&1
      GRADE_EXIT=$?
      ENGINE_EXIT=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('engine_exit', ''))" \
        "$GRADE_DIR/summary.json" 2> /dev/null)
      OUTPUT="$GRADE_DIR/work/$BASE.ses"
      ROUTED="$GRADE_DIR/routed/$BASE.kicad_pcb"
    fi
    ;;
  krt | krt-kicad)
    [[ -x "$KRT_VENV/bin/python" ]] || { echo "no KRT venv (run setup-krt.sh)" >&2; exit 2; }
    if [[ "$ARM" == krt ]]; then RUN_PATH=$(no_kicad_path) || exit 2; else RUN_PATH=$(shim_path) || exit 2; fi
    mkdir -p "$OUT/input" "$OUT/routed"
    cp "$PREP_DIR"/* "$OUT/input/"
    chmod u+w "$OUT"/input/*
    OUTPUT="$OUT/routed/$BASE.kicad_pcb"
    for ext in kicad_pro kicad_dru; do
      [[ -f "$PREP_DIR/$BASE.$ext" ]] && cp "$PREP_DIR/$BASE.$ext" "$OUT/routed/" && chmod u+w "$OUT/routed/$BASE.$ext"
    done
    ENGINE_CMD="$KRT_VENV/bin/python -E -s py_router/route.py {IN} {OUT} ${KRT_FLAGS[*]}"
    ENV_SET="PATH=$RUN_PATH; KICAD_CLI unset; cwd $KRT_EXPORT"
    [[ "$ARM" == krt-kicad ]] && ENV_SET="$ENV_SET; KRT_SHIM_LOG=$OUT/shim.log"
    (cd "$KRT_EXPORT" && env -u KICAD_CLI PATH="$RUN_PATH" KRT_SHIM_LOG="$OUT/shim.log" \
      "${TIMER[@]}" timeout "$TIMEOUT" "$KRT_VENV/bin/python" -E -s py_router/route.py \
      "$OUT/input/$BASE.kicad_pcb" "$OUTPUT" "${KRT_FLAGS[@]}") > "$OUT/engine.log" 2>&1
    ENGINE_EXIT=$?
    ROUTED="$OUTPUT"
    ;;
esac
END=$(date +%s.%N)
LOAD_AFTER=$(load1)
WALL=$(python3 -c "import sys; print(round(float(sys.argv[2]) - float(sys.argv[1]), 2))" "$START" "$END")

if [[ "$ARM" != java && "$ROW" != "$CANARY_ROW" ]]; then
  python3 - "$OUT/meta-in.json" "$ENGINE_CMD" "$ENGINE_EXIT" "$WALL" << 'PY'
import json, sys
json.dump({"engine_command": sys.argv[2], "engine_exit": int(sys.argv[3]),
           "engine_seconds": float(sys.argv[4])}, open(sys.argv[1], "w"), indent=2)
PY
  if [[ -s "$ROUTED" ]]; then
    "$H2H_DIR/grade_routed.sh" --out "$GRADE_DIR" --meta "$OUT/meta-in.json" "$INPUT" "$ROUTED" \
      > "$OUT/grade.log" 2>&1
    GRADE_EXIT=$?
  else
    mkdir -p "$GRADE_DIR"
    python3 "$ORACLE_DIR/oracle_lib.py" not-graded \
      "the engine wrote no board (exit $ENGINE_EXIT, see $OUT/engine.log)" "$GRADE_DIR/summary.json" "$OUT/meta-in.json"
    GRADE_EXIT=2
  fi
fi

if [[ -n "$ROUTED" && -s "$ROUTED" ]]; then
  python3 -I "$H2H_DIR/copper_stats.py" "$ROUTED" "$INPUT" "$OUT/copper_added.json" > /dev/null
  if [[ "$ROW" != "$CANARY_ROW" ]]; then
    "$KICAD_PYTHON" "$H2H_DIR/via_in_pad_kicad.py" "$ROUTED" "$OUT/via_in_pad.json" > "$OUT/via_in_pad.log" 2>&1
  fi
fi

python3 - "$RECORD" "$ROW" "$ARM" "${REP:-1}" "$OUT" "$INPUT" "$ENGINE_CMD" "$ENGINE_EXIT" \
  "${GRADE_EXIT:-}" "$WALL" "$LOAD_BEFORE" "$LOAD_AFTER" "$LOAD_RULE" "$ENV_SET" "$OUTPUT" \
  "$TIME_FILE" "$ROUTED" << 'PY'
import hashlib, json, os, re, sys
(record, row, arm, rep, out, inp, cmd, engine_exit, grade_exit, wall, load_before, load_after,
 load_rule, env_set, output, time_file, routed) = sys.argv[1:18]
def load(p):
    try:
        return json.load(open(p))
    except (OSError, ValueError):
        return None
sha = lambda p: hashlib.sha256(open(p, "rb").read()).hexdigest() if p and os.path.exists(p) else None
summary = load(os.path.join(out, "grade", "summary.json"))
copper = load(os.path.join(out, "copper_added.json"))
vip = load(os.path.join(out, "via_in_pad.json"))
peak_rss_mb = None
if os.path.exists(time_file):
    parts = open(time_file).read().split()
    if len(parts) >= 2 and parts[-1].isdigit():
        peak_rss_mb = round(int(parts[-1]) / 1024, 1)
rec = {
    "board": row, "arm": arm, "rep": int(rep), "input": inp, "input_sha256": sha(inp),
    "engine_command": cmd, "environment": env_set or None,
    "engine_exit": int(engine_exit) if engine_exit not in ("", "None") else None,
    "grade_exit": int(grade_exit) if grade_exit else None,
    "wall_seconds": float(wall),
    "engine_seconds": summary.get("engine_seconds") if summary else None,
    "peak_rss_mb": peak_rss_mb,
    "load_before": float(load_before), "load_after": float(load_after), "load_rule": load_rule,
    "output": output, "output_sha256": sha(output),
    "routed_board": routed or None,
}
if row == "canary-pwr-9511d9a":
    rec["graded"] = False
    rec["reason"] = "canary-pwr is graded only by canary-23 (Windows KiCad); output handed over"
elif summary and summary.get("graded"):
    cu = summary.get("copper") or {}
    rec.update({
        "graded": True,
        "kicad_unconnected": summary["unconnected_items"],
        "input_unconnected": summary["input_unconnected_items"],
        "drc_real": summary["drc_real"],
        "drc_real_by_type": summary["drc_real_by_type"],
        "below_minimum": sum((cu.get("below_minimum") or {}).values()),
        "below_minimum_by_type": cu.get("below_minimum"),
        "under_class_width_mm": cu.get("under_class_width_mm"),
        "under_class_width_tracks": cu.get("under_class_width_tracks"),
        "capped_types": summary.get("capped_types"),
        "kicad_version": summary.get("kicad_version"),
    })
else:
    rec["graded"] = False
    rec["reason"] = (summary or {}).get("reason", "no grading summary")
if copper:
    rec["vias"] = copper["added"]["vias"]
    rec["length_mm"] = round(copper["added"]["length_mm"], 3)
    rec["length_per_layer_mm"] = {k: round(v, 3) for k, v in copper["added"]["per_layer_mm"].items()}
if vip:
    rec["via_in_pad"] = vip["via_in_pad"]
log = os.path.join(out, "engine.log") if os.path.exists(os.path.join(out, "engine.log")) else \
      os.path.join(out, "grade", "work", "engine.log")
if not os.path.exists(log):
    log = os.path.join(out, "work", "engine.log")
if os.path.exists(log):
    text = open(log, encoding="utf-8", errors="replace").read()
    rec["engine_log"] = log
    if arm.startswith("krt"):
        rec["krt_fallbacks"] = sorted({l.strip()[:200] for l in text.splitlines()
            if re.search(r"kicad-cli|pcbnew|raster|exact.fill|oracle_check|fallback", l, re.I)
            and not l.startswith("JSON_")})[:40]
        m = re.findall(r'"oracle_check": "([^"]*)"', text)
        rec["krt_oracle_check"] = m
    else:
        rec["java_log_tail"] = [l[:240] for l in text.splitlines() if "Auto-routing" in l or "optimiz" in l.lower()][-6:]
if arm == "krt-kicad":
    shim = os.path.join(out, "shim.log")
    rec["shim_calls"] = open(shim).read().splitlines() if os.path.exists(shim) else []
json.dump(rec, open(record, "w"), indent=2)
keys = ("board", "arm", "rep", "graded", "reason", "engine_exit", "kicad_unconnected", "drc_real",
        "below_minimum", "vias", "length_mm", "via_in_pad", "under_class_width_mm", "wall_seconds",
        "peak_rss_mb", "load_before", "load_after")
print(json.dumps({k: rec.get(k) for k in keys if k in rec}))
PY
exit 0
