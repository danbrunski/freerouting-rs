#!/usr/bin/env bash
# Grades every project of a set (tools/bench/sets/kicad.txt) with grade.sh.
#
#   tools/kicad-oracle/grade-set.sh SET OUT_DIR [--keep-tracks] [--] ENGINE_COMMAND...
#
# One line per project (a directory or a .kicad_pcb), `#` comments, `~/` for the home
# directory; missing projects are skipped with a note. Each project is graded into
# OUT_DIR/<name>/ and its summary line goes to OUT_DIR/grades.tsv (name, exit, line).
# Exit status: the highest grade.sh exit status seen (0 all clean, 1 router items, 2 any
# project not graded).
set -uo pipefail
[[ $# -ge 3 ]] || { echo "usage: $0 SET OUT_DIR [--keep-tracks] [--] ENGINE_COMMAND..." >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
SET=$1 OUT=$2
shift 2
FLAGS=()
[[ "${1:-}" == "--keep-tracks" ]] && { FLAGS+=(--keep-tracks); shift; }
[[ "${1:-}" == "--" ]] && shift
mkdir -p "$OUT"
: > "$OUT/grades.tsv"
WORST=0
while IFS= read -r line; do
  line=${line%%#*}
  line=$(echo "$line" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
  [[ -z "$line" ]] && continue
  path=${line/#\~\//$HOME/}
  if [[ ! -e "$path" ]]; then
    echo "skipping $line (missing)"
    printf '%s\tskipped\tmissing\n' "$line" >> "$OUT/grades.tsv"
    continue
  fi
  name=$(basename "$path" .kicad_pcb | tr -c 'A-Za-z0-9_.-\n' '_')
  result=$("$HERE/grade.sh" --out "$OUT/$name" "${FLAGS[@]}" "$path" -- "$@" | head -n 1)
  status=${PIPESTATUS[0]}
  echo "$name: exit $status: $result"
  printf '%s\t%s\t%s\n' "$line" "$status" "$result" >> "$OUT/grades.tsv"
  (( status > WORST )) && WORST=$status
done < "$SET"
exit $WORST
