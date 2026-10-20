#!/usr/bin/env bash
# K10.5: one arm of the plugin-input comparison, with WSL KiCad 10.0.7. Runs the plugin's own export
# or board snapshot (kicad-plugin/headless_run.py), routes the file, imports the session into a copy
# of the board and grades it with kicad-cli's DRC. One job at a time: it routes for as long as the
# options say (the graded runs take minutes).
#
#   tools/kicad-reader/plugin-input-ab.sh <dsn|kicad_pcb> <board.kicad_pcb> <options.json> <outdir> [-- router args]
#
# <options.json> is a route-options.json (the dialog's file); the arm overrides its `board_input`.
# For the kicad_pcb arm the DSN-rewriting options must be off (the plugin refuses them): use
# `edge_and_pad_keepouts: false` and no class widths or pour lists. The board's folder is copied,
# so the original and its project files are never touched. Needs FREEROUTING_BIN (the router) and
# `kicad-cli` on PATH. Writes <outdir>/summary.json:
#   {arm, rc, prepare_s, load_s, route_s, unconnected, violations: {type: n}, drc_s}
# `prepare_s` is the export (dsn) or snapshot (kicad_pcb) time, `load_s` the router's time to read
# the file (`-de FILE -drc`: parse and build the board, then check), `route_s` the routing run.
set -u
arm="${1:-}"; board="${2:-}"; opts="${3:-}"; out="${4:-}"
[ "$#" -ge 4 ] || { echo "usage: $0 <dsn|kicad_pcb> <board.kicad_pcb> <options.json> <outdir> [-- router args]" >&2; exit 2; }
shift 4
[ "${1:-}" = "--" ] && shift
case "$arm" in dsn|kicad_pcb) ;; *) echo "arm must be dsn or kicad_pcb" >&2; exit 2;; esac
for f in "$board" "$opts"; do [ -f "$f" ] || { echo "no such file: $f" >&2; exit 2; }; done
router="${FREEROUTING_BIN:-}"; [ -x "$router" ] || { echo "set FREEROUTING_BIN to the freerouting-rs executable" >&2; exit 2; }
command -v kicad-cli > /dev/null || { echo "kicad-cli not found" >&2; exit 2; }
here="$(cd "$(dirname "$0")" && pwd)"; repo="$(cd "$here/../.." && pwd)"
headless="$repo/kicad-plugin/headless_run.py"
name="$(basename "$board" .kicad_pcb)"

rm -rf "$out"; mkdir -p "$out"; out="$(cd "$out" && pwd -P)"
cp -r "$(dirname "$board")" "$out/proj"
pcb="$out/proj/$name.kicad_pcb"
python3 - "$opts" "$out/opts.json" "$arm" <<'PY'
import json, sys
d = json.load(open(sys.argv[1])); d["board_input"] = sys.argv[3]
json.dump(d, open(sys.argv[2], "w"))
PY
q() { grep -v -E "memory leak|property.h|^$|Font '"; }
now() { date +%s.%N; }

t0=$(now)
if [ "$arm" = dsn ]; then mode=export; design="$out/work/$name.dsn"; else mode=snapshot; design="$out/work/$name.kicad_pcb"; fi
python3 -I "$headless" $mode "$pcb" "$out/work" "$out/opts.json" 2>&1 | q > "$out/prepare.log"
t1=$(now)
mapfile -t args < <(python3 -c "import json; print('\n'.join(json.load(open('$out/work/$name.args.json'))['args']))")
"$router" -de "$design" -drc "$out/load.json" > "$out/load.log" 2>&1
t2=$(now)
"$router" -de "$design" -do "$out/work/$name.ses" --progress json --result-json "$out/work/$name.result.json" "${args[@]}" "$@" > "$out/route.log" 2>&1
rc=$?
t3=$(now)
python3 -I "$headless" import "$pcb" "$out/work/$name.ses" "$pcb" "$out/opts.json" 2>&1 | q > "$out/import.log"
kicad-cli pcb drc --refill-zones --save-board --severity-all --format json -o "$out/drc.json" "$pcb" > /dev/null 2>&1
t4=$(now)
python3 - "$out" "$arm" "$rc" "$t0" "$t1" "$t2" "$t3" "$t4" <<'PY'
import collections, json, sys
out, arm, rc, *t = sys.argv[1:]
t = [float(x) for x in t]
r = json.load(open(f"{out}/drc.json"))
c = collections.Counter(v["type"] for v in r["violations"] if v["severity"] == "error")
s = {"arm": arm, "rc": int(rc), "prepare_s": round(t[1] - t[0], 2), "load_s": round(t[2] - t[1], 2),
     "route_s": round(t[3] - t[2], 1), "drc_s": round(t[4] - t[3], 1),
     "unconnected": len(r.get("unconnected_items", [])), "violations": dict(sorted(c.items()))}
json.dump(s, open(f"{out}/summary.json", "w"), indent=1)
print(json.dumps(s))
PY
