#!/usr/bin/env bash
# Prepares X01's five input boards (X01.4) under $X01_DIR/boards/<row>/, on copies.
#
#   tools/krt-h2h/prep.sh [ROW...]          (default: all five rows)
#
# Demos: every track, arc and via removed, zones and layer types kept, zones refilled, saved
# (prep_board.py strip, then refill, in separate KiCad processes). canary-pwr: copied as is
# with its .kicad_pro and .kicad_dru and only refilled. Records counts before and after, the
# prepared board's sha256, the originals' sha256 before and after, and the KiCad processes
# (`pgrep -af kicad`) before and after, in $X01_DIR/boards/prep.json. Then grades every
# prepared demo input once with grade_routed.sh (routed = input) to record its pre-existing
# DRC; canary-pwr is graded only by canary-23 (Windows KiCad), never here.
#
# KICAD_PYTHON (default python3, must import pcbnew) is the KiCad install; 10.0.7 here.
set -uo pipefail
source "$(dirname "$0")/common.sh"
KICAD_PYTHON=${KICAD_PYTHON:-python3}

# row|source .kicad_pcb|mode
ROWS=(
  "canary-pwr-9511d9a|$HOME/code/canary/hardware/canary-pwr-rev1.0/canary-pwr.kicad_pcb|refill"
  "pic_programmer|/usr/share/kicad/demos/pic_programmer/pic_programmer.kicad_pcb|strip"
  "interf_u|/usr/share/kicad/demos/interf_u/interf_u.kicad_pcb|strip"
  "kit-dev-coldfire-xilinx_5213|/usr/share/kicad/demos/kit-dev-coldfire-xilinx_5213/kit-dev-coldfire-xilinx_5213.kicad_pcb|strip"
  "video|/usr/share/kicad/demos/video/video.kicad_pcb|strip"
)
WANT=("$@")
BOARDS="$X01_DIR/boards"
mkdir -p "$BOARDS"
pgrep -af kicad | grep -v -e pgrep -e prep.sh -e prep_board > "$BOARDS/kicad-procs-before.txt"
RECORDS=()
for entry in "${ROWS[@]}"; do
  IFS='|' read -r row src mode <<< "$entry"
  if [[ ${#WANT[@]} -gt 0 && ! " ${WANT[*]} " == *" $row "* ]]; then
    continue
  fi
  dir="$BOARDS/$row"
  base=$(basename "$src" .kicad_pcb)
  srcdir=$(dirname "$src")
  rm -rf "$dir"
  mkdir -p "$dir/source" "$dir/prepared" "$dir/log"
  originals=()
  for ext in kicad_pcb kicad_pro kicad_dru; do
    [[ -f "$srcdir/$base.$ext" ]] && originals+=("$srcdir/$base.$ext")
  done
  sha256sum "${originals[@]}" > "$dir/originals.sha256"
  for f in "${originals[@]}"; do
    cp "$f" "$dir/source/"
    cp "$f" "$dir/prepared/"
  done
  chmod u+w "$dir"/source/* "$dir"/prepared/*
  prepared="$dir/prepared/$base.kicad_pcb"
  "$KICAD_PYTHON" "$H2H_DIR/prep_board.py" count "$dir/source/$base.kicad_pcb" "$dir/log/before.json" \
    > "$dir/log/count-before.log" 2>&1 || { echo "$row: count failed" >&2; exit 1; }
  if [[ "$mode" == strip ]]; then
    "$KICAD_PYTHON" "$H2H_DIR/prep_board.py" strip "$dir/source/$base.kicad_pcb" "$prepared" \
      "$dir/log/strip.json" > "$dir/log/strip.log" 2>&1 || { echo "$row: strip failed" >&2; exit 1; }
  fi
  "$KICAD_PYTHON" "$H2H_DIR/prep_board.py" refill "$prepared" "$dir/log/refill.json" \
    > "$dir/log/refill.log" 2>&1 || { echo "$row: refill failed" >&2; exit 1; }
  "$KICAD_PYTHON" "$H2H_DIR/prep_board.py" count "$prepared" "$dir/log/after.json" \
    > "$dir/log/count-after.log" 2>&1 || { echo "$row: count failed" >&2; exit 1; }
  sha256_of "$prepared" > "$dir/prepared.sha256"
  unchanged=true
  sha256sum --quiet -c "$dir/originals.sha256" > /dev/null 2>&1 || unchanged=false
  RECORDS+=("$row|$src|$mode|$dir|$base|$unchanged")
done
pgrep -af kicad | grep -v -e pgrep -e prep.sh -e prep_board > "$BOARDS/kicad-procs-after.txt"

python3 - "$BOARDS/prep.json" "$BOARDS" "$("$KICAD_PYTHON" "$ORACLE_DIR/kicad_steps.py" version 2> /dev/null | tail -n 1)" \
  "${RECORDS[@]}" << 'PY'
import json, os, sys
out, boards, version = sys.argv[1:4]
load = lambda p: json.load(open(p))
try:
    data = load(out)
except (OSError, ValueError):
    data = {"rows": {}}
data["kicad_version"] = version
read = lambda p: open(p).read().splitlines()
procs_before = read(os.path.join(boards, "kicad-procs-before.txt"))
procs_after = read(os.path.join(boards, "kicad-procs-after.txt"))
data["kicad_processes_before"] = procs_before
data["kicad_processes_after"] = procs_after
data["kicad_processes_unchanged"] = procs_before == procs_after
for rec in sys.argv[4:]:
    row, src, mode, d, base, unchanged = rec.split("|")
    data["rows"][row] = {
        "source": src,
        "mode": mode,
        "prepared": os.path.join(d, "prepared", base + ".kicad_pcb"),
        "prepared_sha256": open(os.path.join(d, "prepared.sha256")).read().strip(),
        "originals_sha256": dict(reversed(l.split("  ", 1)) for l in read(os.path.join(d, "originals.sha256"))),
        "originals_unchanged": unchanged == "true",
        "before": load(os.path.join(d, "log", "before.json")),
        "after": load(os.path.join(d, "log", "after.json")),
        "refilled": load(os.path.join(d, "log", "refill.json"))["refilled"],
    }
json.dump(data, open(out, "w"), indent=2, sort_keys=True)
for row, r in sorted(data["rows"].items()):
    b, a = r["before"], r["after"]
    print(f"{row}: tracks {b['tracks']}->{a['tracks']} arcs {b['arcs']}->{a['arcs']} vias {b['vias']}->{a['vias']} "
          f"zones {b['zones']}->{a['zones']} refilled={r['refilled']} originals_unchanged={r['originals_unchanged']} "
          f"sha256={r['prepared_sha256']}")
print("kicad processes unchanged:", data["kicad_processes_unchanged"])
PY
