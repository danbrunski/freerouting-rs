#!/usr/bin/env bash
# Writes the A07 probe references of the 90 degree and any-angle boards:
# fixtures/reference/probe/<kind>/autoroute-variants/<board>.jsonl (or .digest).
#
#   tools/java-reference/probes/autoroute/generate-variants.sh PIN_CHECKOUT [KIND...]
#
# The boards are the lines of variants.txt (not a fixtures.tsv row: generate.sh all runs every
# row as a kind); the kinds default to the five that
# tests/autoroute_probe_variants.rs compares (start-rooms drills maze-log locate insert). Each run
# is the one generate.sh makes for a kind row (probe.sh PIN AutorouteProbe.java <kind> <dsn>
# <out>), with the same 1 MiB digest rule (the digest code is a copy of
# generate.sh's). Logs go to target/probe-logs/autoroute/<kind>/<board>.log.
# REFERENCE_WORK_DIR and TMPDIR are passed to probe.sh.
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 PIN_CHECKOUT [KIND...]" >&2
  exit 2
fi
PIN=$1
shift
KINDS=("$@")
[[ ${#KINDS[@]} -gt 0 ]] || KINDS=(start-rooms drills maze-log locate insert)
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../../.." && pwd)
PROBE_SH="$ROOT/tools/java-reference/probe.sh"
PROBE="$HERE/AutorouteProbe.java"
LIST="$HERE/variants.txt"
MAX_BYTES=$((1024 * 1024))

mapfile -t FIXTURES < <(grep -v '^#' "$LIST" | grep .)
[[ ${#FIXTURES[@]} -gt 0 ]] || { echo "$0: no boards in $LIST" >&2; exit 2; }

digest() { # replaces $1 (jsonl) by its digest file $2, as generate.sh does
  python3 - "$1" "$2" <<'PY'
import sys
src, dst = sys.argv[1], sys.argv[2]
lines = open(src, encoding="utf-8").read().splitlines()
def fnv(text):
    h = 0xcbf29ce484222325
    for b in text.encode("utf-8"):
        h = ((h ^ b) * 0x100000001b3) & 0xffffffffffffffff
    return h
out, name, body = [lines[0]], None, []
def flush():
    if name is not None:
        out.append("%s %d %016x" % (name, len(body), fnv("".join(l + "\n" for l in body))))
for line in lines[1:]:
    if line.startswith('{"stage":"'):
        flush()
        name, body = line.split('"')[3], []
    if name is not None:
        body.append(line)
flush()
open(dst, "w", encoding="utf-8").write("".join(l + "\n" for l in out))
PY
  rm "$1"
}

cd "$ROOT"
for kind in "${KINDS[@]}"; do
  for dsn in "${FIXTURES[@]}"; do
    board=${dsn#fixtures/}
    board=${board%.dsn}
    out="fixtures/reference/probe/$kind/$board.jsonl"
    log="target/probe-logs/autoroute/$kind/$board.log"
    mkdir -p "$(dirname "$out")" "$(dirname "$log")"
    rm -f "$out" "${out%.jsonl}.digest"
    if ! TMPDIR=${TMPDIR:-$ROOT/target} "$PROBE_SH" "$PIN" "$PROBE" "$kind" "$dsn" "$out" \
      > "$log" 2>&1; then
      echo "generate-variants.sh: $kind failed on $dsn, see $log" >&2
      exit 1
    fi
    if [[ $(stat -c %s "$out") -gt $MAX_BYTES ]]; then
      digest "$out" "${out%.jsonl}.digest"
    fi
  done
  echo "generate-variants.sh: $kind: ${#FIXTURES[@]} boards"
done
