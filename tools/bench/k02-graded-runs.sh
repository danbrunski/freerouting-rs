#!/usr/bin/env bash
# K02.7: the plugin's path and Java at the pin, graded by KiCad's own DRC, board by board.
#
#   tools/bench/k02-graded-runs.sh SET OUT_DIR RUST_FREEROUTING JAVA_JAR [BOARD_NAME...]
#
# SET is a board list (tools/bench/sets/kicad-demos.txt). For each board, both engines get
# the same export (grade.sh: tracks and vias stripped, DSN exported by KiCad, footprint-only
# rule areas removed, which is what the plugin's K01.2 rewrite achieves) and the same
# --router.min_trace_width_um, the board's Board Setup minimum track width (D8; the DSN does
# not carry it, so the plugin passes it).
#   rust  freerouting -de {DSN} -do {SES} --router.min_trace_width_um=W
#         (the plugin's default options, nothing else; --result-json for the time and counts)
#   java  java -Xmx3g -jar JAR -de {DSN} -do {SES} --gui.enabled=false
#         --router.min_trace_width_um=W --router.optimizer.enabled=false
#         (Java's optimizer still depends on thread scheduling, J01; X01 turned it off too)
# Each run is capped by ENGINE_TIMEOUT seconds (default 3600). One kicad-cli at a time.
# ENGINES (default "rust java") picks the engines, e.g. ENGINES=rust to redo one row.
# OUT_DIR/<board>/{rust,java}/ holds grade.sh's output, OUT_DIR/<board>/<engine>/via_in_pad.json
# the via-in-pad count; OUT_DIR/table.md is the table (tools/kicad-oracle/graded_table.py).
set -uo pipefail
[[ $# -ge 4 ]] || { echo "usage: $0 SET OUT_DIR RUST_FREEROUTING JAVA_JAR [BOARD_NAME...]" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
ORACLE="$HERE/../kicad-oracle"
SET=$1 OUT=$2 RUST=$(readlink -f "$3") JAR=$(readlink -f "$4")
shift 4
ONLY=("$@")
KICAD_PYTHON=${KICAD_PYTHON:-python3}
ENGINE_TIMEOUT=${ENGINE_TIMEOUT:-3600}
read -r -a ENGINE_LIST <<< "${ENGINES:-rust java}"
mkdir -p "$OUT" && OUT=$(cd "$OUT" && pwd)
BEFORE=$(pgrep -x kicad; pgrep -x pcbnew)
while IFS= read -r line <&3; do
  line=${line%%#*}
  line=$(echo "$line" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
  [[ -z "$line" ]] && continue
  path=${line/#\~\//$HOME/}
  [[ -e "$path" ]] || { echo "skipping $line (missing)"; continue; }
  name=$(basename "$path" .kicad_pcb | tr -c 'A-Za-z0-9_.\n-' '_')
  if [[ ${#ONLY[@]} -gt 0 ]] && [[ ! " ${ONLY[*]} " == *" $name "* ]]; then continue; fi
  width=$("$KICAD_PYTHON" -c "
import sys, pcbnew
b = pcbnew.LoadBoard(sys.argv[1])
print(b.GetDesignSettings().m_TrackMinWidth / 1000.0)" "$path" 2> /dev/null | tail -n 1)
  [[ -n "$width" ]] || { echo "$name: cannot read the board minimum track width"; continue; }
  for engine in "${ENGINE_LIST[@]}"; do
    dir="$OUT/$name/$engine"
    if [[ $engine == rust ]]; then
      cmd=(timeout "$ENGINE_TIMEOUT" "$RUST" -de '{DSN}' -do '{SES}' --result-json '{SES}.result.json'
           "--router.min_trace_width_um=$width")
    else
      cmd=(timeout "$ENGINE_TIMEOUT" java -Xmx3g -jar "$JAR" -de '{DSN}' -do '{SES}' --gui.enabled=false
           "--router.min_trace_width_um=$width" --router.optimizer.enabled=false)
    fi
    rm -rf "$dir"
    result=$("$ORACLE/grade.sh" --out "$dir" "$path" -- "${cmd[@]}" | sed -n '1p')
    echo "$name $engine (min width $width um): $result"
    routed=$(ls "$dir"/routed/*.kicad_pcb 2> /dev/null | head -n 1)
    if [[ -n "$routed" ]]; then
      "$KICAD_PYTHON" "$ORACLE/via_in_pad.py" "$routed" "$dir/via_in_pad.json" > /dev/null 2>&1 ||
        echo "$name $engine: via-in-pad check failed"
    fi
  done
done 3< "$SET"
python3 "$ORACLE/graded_table.py" "$OUT" > "$OUT/table.md"
cat "$OUT/table.md"
AFTER=$(pgrep -x kicad; pgrep -x pcbnew)
[[ "$BEFORE" == "$AFTER" ]] && echo "KiCad processes unchanged (before: '${BEFORE//$'\n'/ }')" ||
  echo "KiCad processes CHANGED: before '$BEFORE' after '$AFTER'"
