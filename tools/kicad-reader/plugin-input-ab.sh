#!/usr/bin/env bash
# K10.5, K13.5: one arm of the plugin-input comparison, with WSL KiCad 10.0.7. Runs the plugin's own
# export or board snapshot (kicad-plugin/headless_run.py), routes the file, imports the session into a
# copy of the board and grades it with kicad-cli's DRC under the board's own rules. One job at a time.
#
#   tools/kicad-reader/plugin-input-ab.sh <dsn|kicad_pcb> <board.kicad_pcb> <options.json> <outdir> [-- router args]
#
# <options.json> is a route-options.json (the dialog's file); the arm overrides its `board_input`.
# `headless_run.py snapshot` does not refuse the DSN-rewriting options (only the dialog's form does,
# K10.4): with `board_input = kicad_pcb` they have no DSN text to act on and do nothing. The board's
# folder is copied, so the original and its project files are never touched. Router args after `--`
# come after the plugin's own flags, so they win.
#
# Environment:
#   FREEROUTING_BIN    the freerouting-rs executable (required)
#   ROUTE_TIMEOUT_S    wall-clock limit of each router call (load and route), `timeout -k 10`; 0 = none
#   ROUTE_MEMORY_KB    `ulimit -v` of each router call (default 8000000)
#   SHARE_GRADE_WITH   the other arm's <outdir> on the same board: when its session is byte-identical to
#                      this one's, its grade is copied and this session is not graded again
#   KICAD_CLI, KICAD_PYTHON  as in tools/kicad-oracle/grade.sh (both must report the same KiCad version)
#
# Grading (K13.5) follows tools/kicad-oracle/grade.sh with `--keep-tracks`, on the plugin's own import:
# the input board (the copy as given, with its copper) and the routed board each get the project's
# ORIGINAL .kicad_pro/.kicad_dru with the copper classes forced to error (summary `rules`: `project`,
# `board` when the board has no .kicad_pro and KiCad's migration of its own rules is used, Q19, or
# `defaults`), then `kicad-cli pcb drc --severity-all --refill-zones`. A DRC item is routing-caused when
# it involves a track or via the input board does not have at the same nanometre coordinates, width,
# layer and net (oracle_lib.summarize, Q22.2, `routing_caused_basis: copper`).
#
# Writes <outdir>/summary.json and prints it:
#   arm, rc, aborted, prepare_s, load_s, route_s, drc_s, unconnected, violations (every error of the
#   routed board by type), graded, rules, routing_caused_basis, routing_caused_by_type,
#   manufacturing_breaches (the routing-caused items of the types in MANUFACTURING below),
#   below_minimum (routed tracks and vias under the Board Setup minimums, the sum of
#   below_minimum_by_kind: track width, via diameter, via drill),
#   via_in_pad (vias in pads on the routed board minus the input's, floored at 0), via_in_pad_raw,
#   input_via_in_pad, input_unconnected, session_sha256, design_sha256, args_sha256, grade_shared_with
# `prepare_s` is the export (dsn) or snapshot (kicad_pcb) time, `load_s` the router's time to read
# the file (`-de FILE -drc`: parse and build the board, then check), `route_s` the routing run.
# A router call that hits its timeout or is killed by a signal is `aborted: 1` and is not graded.
set -u
arm="${1:-}"; board="${2:-}"; opts="${3:-}"; out="${4:-}"
[ "$#" -ge 4 ] || { echo "usage: $0 <dsn|kicad_pcb> <board.kicad_pcb> <options.json> <outdir> [-- router args]" >&2; exit 2; }
shift 4
[ "${1:-}" = "--" ] && shift
case "$arm" in dsn|kicad_pcb) ;; *) echo "arm must be dsn or kicad_pcb" >&2; exit 2;; esac
for f in "$board" "$opts"; do [ -f "$f" ] || { echo "no such file: $f" >&2; exit 2; }; done
router="${FREEROUTING_BIN:-}"; [ -x "$router" ] || { echo "set FREEROUTING_BIN to the freerouting-rs executable" >&2; exit 2; }
KICAD_CLI=${KICAD_CLI:-kicad-cli}
KICAD_PYTHON=${KICAD_PYTHON:-python3}
command -v "$KICAD_CLI" > /dev/null || { echo "kicad-cli not found ('$KICAD_CLI')" >&2; exit 2; }
timeout_s="${ROUTE_TIMEOUT_S:-0}"; memory_kb="${ROUTE_MEMORY_KB:-8000000}"
share="${SHARE_GRADE_WITH:-}"
here="$(cd "$(dirname "$0")" && pwd)"; repo="$(cd "$here/../.." && pwd)"
headless="$repo/kicad-plugin/headless_run.py"
oracle="$repo/tools/kicad-oracle"
name="$(basename "$board" .kicad_pcb)"
cli_version=$("$KICAD_CLI" version 2> /dev/null | tail -n 1)
py_version=$("$KICAD_PYTHON" "$oracle/kicad_steps.py" version 2> /dev/null | tail -n 1)
[ -n "$py_version" ] && [ "$cli_version" = "$py_version" ] ||
  { echo "kicad-cli is '$cli_version' but pcbnew is '$py_version'; never mix KiCad versions" >&2; exit 2; }

rm -rf "$out"; mkdir -p "$out"; out="$(cd "$out" && pwd -P)"
cp -r "$(dirname "$board")" "$out/proj"
pcb="$out/proj/$name.kicad_pcb"
python3 -I - "$opts" "$out/opts.json" "$arm" <<'PY'
import json, sys
d = json.load(open(sys.argv[1])); d["board_input"] = sys.argv[3]
json.dump(d, open(sys.argv[2], "w"))
PY
q() { grep -v -E "memory leak|property.h|^$|Font '"; }
now() { date +%s.%N; }
# One router call, under the memory cap and the timeout.
run_router() {
  ( ulimit -v "$memory_kb"
    if [ "$timeout_s" != 0 ]; then exec timeout -k 10 "$timeout_s" "$router" "$@"; else exec "$router" "$@"; fi )
}

t0=$(now)
if [ "$arm" = dsn ]; then mode=export; design="$out/work/$name.dsn"; else mode=snapshot; design="$out/work/$name.kicad_pcb"; fi
"$KICAD_PYTHON" -I "$headless" $mode "$pcb" "$out/work" "$out/opts.json" 2>&1 | q > "$out/prepare.log"
t1=$(now)
mapfile -t args < <(python3 -I -c "import json, sys; print('\n'.join(json.load(open(sys.argv[1]))['args']))" "$out/work/$name.args.json")
run_router -de "$design" -drc "$out/load.json" > "$out/load.log" 2>&1
load_rc=$?
t2=$(now)
ses="$out/work/$name.ses"
run_router -de "$design" -do "$ses" --progress json --result-json "$out/work/$name.result.json" "${args[@]}" "$@" > "$out/route.log" 2>&1
rc=$?
t3=$(now)
aborted=0
for code in "$load_rc" "$rc"; do [ "$code" -ge 124 ] && aborted=1; done
grade=fresh
if [ "$aborted" = 1 ] || [ ! -s "$ses" ]; then
  grade=none
elif [ -n "$share" ] && [ -s "$share/work/$name.ses" ] && cmp -s "$ses" "$share/work/$name.ses" &&
  [ -f "$share/summary.json" ]; then
  grade=shared
fi
if [ "$grade" = fresh ]; then
  # The input board as given and the routed board, each beside the original rules.
  mkdir -p "$out/input" "$out/routed"
  cp "$pcb" "$out/input/"
  "$KICAD_PYTHON" -I "$headless" import "$pcb" "$ses" "$out/routed/$name.kicad_pcb" "$out/opts.json" 2>&1 | q > "$out/import.log"
  rules=defaults
  [ -f "$out/proj/$name.kicad_pro" ] && rules=project
  if [ "$rules" = defaults ] && [ -f "$out/routed/$name.kicad_pro" ]; then
    # Q19: a board without a .kicad_pro grades under the one KiCad migrated from its own rules
    # (written by the import's SaveBoard from the input board).
    mkdir -p "$out/rules"; cp "$out/routed/$name.kicad_pro" "$out/rules/"; rules=board
  fi
  for dir in "$out/input" "$out/routed"; do
    for ext in kicad_pro kicad_dru; do
      rm -f "$dir/$name.$ext"
      if [ -f "$out/proj/$name.$ext" ]; then cp "$out/proj/$name.$ext" "$dir/"
      elif [ "$ext" = kicad_pro ] && [ "$rules" = board ]; then cp "$out/rules/$name.kicad_pro" "$dir/"; fi
    done
    [ -f "$dir/$name.kicad_pro" ] && python3 -I "$oracle/oracle_lib.py" force-errors "$dir/$name.kicad_pro"
    "$KICAD_CLI" pcb drc --format json --severity-all --refill-zones --output "$dir/drc.json" "$dir/$name.kicad_pcb" > "$dir/drc.log" 2>&1
    "$KICAD_PYTHON" -I "$oracle/via_in_pad.py" "$dir/$name.kicad_pcb" "$dir/via_in_pad.json" > "$dir/via_in_pad.log" 2>&1
  done
  echo '{}' > "$out/routed/classwidths.json"
  "$KICAD_PYTHON" -I "$oracle/kicad_steps.py" stats "$out/routed/$name.kicad_pcb" "$out/routed/classwidths.json" \
    "$out/routed/copper.json" > "$out/routed/stats.log" 2>&1
  echo "{\"rules\": \"$rules\"}" > "$out/grade-meta.json"
fi
t4=$(now)
python3 -I - "$out" "$name" "$arm" "$rc" "$aborted" "$grade" "$share" "$oracle" "$design" "$t0" "$t1" "$t2" "$t3" "$t4" <<'PY'
import collections, hashlib, json, os, sys
out, name, arm, rc, aborted, grade, share, oracle, design, *t = sys.argv[1:]
t = [float(x) for x in t]
sys.path.insert(0, oracle)
import oracle_lib  # noqa: E402

# The manufacturing rule types of K13's guard (routing-quality.md, order of importance item 1).
MANUFACTURING = ("clearance", "hole_clearance", "hole_to_hole", "copper_edge_clearance", "track_width",
                 "via_diameter", "annular_width", "drill_out_of_range", "solder_mask_bridge", "shorting_items")


def sha(path):
    try:
        return hashlib.sha256(open(path, "rb").read()).hexdigest()
    except OSError:
        return None


def load(path):
    try:
        return json.load(open(path))
    except (OSError, ValueError):
        return None


def below_minimum(routed_copper, input_copper, minimums):
    """Routed tracks and vias (not in the input at the same nanometre key) under the Board Setup
    minimums, as kicad_steps.py stats counts them (strictly less than the minimum)."""
    nm = {k: round(v * 1e6) for k, v in minimums.items()}
    keys = oracle_lib.input_copper_keys(input_copper)
    out = {"track_width": 0, "via_diameter": 0, "via_drill": 0}
    for record in routed_copper.values():
        if oracle_lib.copper_key(record) in keys:
            continue
        if record["kind"] == "via":
            out["via_diameter"] += record["size"] < nm["via_diameter"]
            out["via_drill"] += record["drill"] < nm["via_drill"]
        else:
            out["track_width"] += record["width"] < nm["track_width"]
    return out


s = {"arm": arm, "rc": int(rc), "aborted": int(aborted), "prepare_s": round(t[1] - t[0], 2),
     "load_s": round(t[2] - t[1], 2), "route_s": round(t[3] - t[2], 1), "drc_s": round(t[4] - t[3], 1),
     "session_sha256": sha(f"{out}/work/{name}.ses"), "design_sha256": sha(design),
     "args_sha256": sha(f"{out}/work/{name}.args.json"), "grade_shared_with": None}
if grade == "shared":
    other = load(f"{share}/summary.json")
    for key in ("graded", "rules", "routing_caused_basis", "unconnected", "violations", "routing_caused_by_type",
                "manufacturing_breaches", "below_minimum", "below_minimum_by_kind", "via_in_pad", "via_in_pad_raw", "input_via_in_pad",
                "input_unconnected"):
        s[key] = other.get(key)
    s["grade_shared_with"] = share
elif grade == "fresh":
    routed, unrouted = load(f"{out}/routed/drc.json"), load(f"{out}/input/drc.json")
    copper = load(f"{out}/routed/copper.json")
    vip, vip_in = load(f"{out}/routed/via_in_pad.json"), load(f"{out}/input/via_in_pad.json")
    meta = load(f"{out}/grade-meta.json") or {}
    if None in (routed, unrouted, copper, vip, vip_in):
        s.update({"graded": False, "reason": "a DRC report, the copper statistics or a via-in-pad count is missing"})
    else:
        routed_copper = oracle_lib.read_copper(open(f"{out}/routed/{name}.kicad_pcb", encoding="utf-8").read())
        input_copper = oracle_lib.read_copper(open(f"{out}/input/{name}.kicad_pcb", encoding="utf-8").read())
        summary, _ = oracle_lib.summarize(routed, unrouted, copper, meta, routed_copper, input_copper)
        json.dump(summary, open(f"{out}/grade.json", "w"), indent=1)
        caused = summary["drc_real_by_type"]
        errors = collections.Counter(v["type"] for v in routed["violations"] if v["severity"] == "error")
        below = below_minimum(routed_copper, input_copper, copper["board_minimums_mm"])
        s.update({"graded": True, "rules": meta.get("rules"), "routing_caused_basis": summary["routing_caused_basis"],
                  "unconnected": len(routed.get("unconnected_items", [])), "violations": dict(sorted(errors.items())),
                  "routing_caused_by_type": caused,
                  "manufacturing_breaches": sum(caused.get(k, 0) for k in MANUFACTURING),
                  "below_minimum": sum(below.values()), "below_minimum_by_kind": below,
                  "via_in_pad": max(0, vip["vias_in_pads"] - vip_in["vias_in_pads"]),
                  "via_in_pad_raw": vip["vias_in_pads"], "input_via_in_pad": vip_in["vias_in_pads"],
                  "input_unconnected": len(unrouted.get("unconnected_items", []))})
else:
    s.update({"graded": False, "reason": "aborted" if int(aborted) else "no session"})
json.dump(s, open(f"{out}/summary.json", "w"), indent=1)
print(json.dumps(s))
PY
