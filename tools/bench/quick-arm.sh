#!/usr/bin/env bash
# Routes every row of a board set once with one arm of a registration, one run at a time, under
# the memory cap and the time limits decided on 2026-10-20 (measurement runs stay a few
# minutes): `ulimit -v 8000000`, `timeout 600` for a canary-pwr row, `timeout 120` for any other.
#
#   tools/bench/quick-arm.sh BIN OUT ARM SET [-- ROUTER ARGS...]
#
# BIN   the release CLI (target/release/freerouting-rs of the build the registration names)
# OUT   the output directory; the arm's files go to OUT/ARM/<board>.{ses,log,time,result.json}
# ARM   the arm's name in the registration
# SET   a board set (tools/bench/sets/*.txt): the first word of every line that is not a comment
# ROUTER ARGS  the arm's command after `-de {DSN} -do {SES}`, as the registration gives it
#
# Each route runs as `BIN -de <row> -do OUT/ARM/<board>.ses --result-json
# OUT/ARM/<board>.result.json ROUTER ARGS...`; the result manifest (`board_statistics`) changes no
# route. `/usr/bin/time` writes the wall seconds and the peak resident set (kB) to `<board>.time`.
# A missing row prints `MISSING`, a route that hits its limit prints `TIMEOUT` (the measurement
# is then incomplete: no gate). Run from the repository root. Prints one line per row:
# arm, board, wall seconds, peak kB, exit status.
# With ROW_FLAGS=1 (Q18) the words after a row's path are that row's own flags, appended after
# ROUTER ARGS (the later flag wins); without it only the first word is read.
set -u
if [[ $# -lt 4 ]]; then
  sed -n '2,19p' "$0" >&2
  exit 2
fi
bin=$1; out=$2; arm=$3; set=$4; shift 4
[[ ${1:-} == -- ]] && shift
mkdir -p "$out/$arm"
ulimit -v 8000000
while read -r dsn row_flags; do
  [[ -z ${dsn:-} || $dsn == \#* ]] && continue
  extra=()
  [[ ${ROW_FLAGS:-0} == 1 ]] && read -r -a extra <<< "${row_flags%%#*}"
  name=$(basename "$dsn" .dsn)
  if [[ ! -e $dsn ]]; then echo "MISSING $dsn"; continue; fi
  limit=120
  [[ $dsn == *canary-pwr* ]] && limit=600
  /usr/bin/time -f '%e %M' -o "$out/$arm/$name.time" timeout -k 10 "$limit" "$bin" -de "$dsn" \
    -do "$out/$arm/$name.ses" --result-json "$out/$arm/$name.result.json" "$@" "${extra[@]}" \
    > "$out/$arm/$name.log" 2>&1 < /dev/null
  code=$?
  [[ $code -eq 124 || $code -eq 137 ]] && echo "TIMEOUT $arm $name after ${limit}s"
  printf '%s\t%s\t%s\texit %s\n' "$arm" "$name" "$(tail -1 "$out/$arm/$name.time")" "$code"
done < <(grep -v '^[[:space:]]*#' "$set")
