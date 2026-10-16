#!/usr/bin/env bash
# Writes the autoroute-probe references: fixtures/reference/probe/<kind>/<board>.jsonl.
#
#   tools/java-reference/probes/autoroute/generate.sh PIN_CHECKOUT KIND|all
#
# PIN_CHECKOUT  a detached worktree of danbrunski/freerouting at the port's pin (see probe.sh)
# KIND          a kind row of fixtures.tsv; 'all' runs every kind row in file order
#
# Runs, for every fixture of the kind's row (with '@ladder' expanded to the ladder row),
#   probe.sh PIN AutorouteProbe.java <kind> <dsn|-> <out.jsonl>
# <board> is the DSN path below fixtures/ without ".dsn" (nested fixtures keep their
# directories), or the kind itself for '-'. An output above 1 MiB is replaced by <board>.digest:
# the header line, then one line "<section> <line count> <FNV-1a 64>" per section, where a
# section starts at each line whose first key is "stage" (crates/freerouting/src/autoroute/
# probe.rs, `digest`, computes the same). A non-zero probe status stops the script. Logs of each
# run go to target/probe-logs/autoroute/<kind>/<board>.log. Two runs are byte-identical.
# REFERENCE_WORK_DIR and TMPDIR are passed to probe.sh.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 PIN_CHECKOUT KIND|all" >&2
  exit 2
fi
PIN=$1
KIND=$2
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../../.." && pwd)
PROBE_SH="$ROOT/tools/java-reference/probe.sh"
PROBE="$HERE/AutorouteProbe.java"
TSV="$HERE/fixtures.tsv"
MAX_BYTES=$((1024 * 1024))

row() { # the fixture columns of row $1
  awk -F'\t' -v k="$1" '$1 == k { for (i = 2; i <= NF; i++) print $i }' "$TSV"
}

if [[ "$KIND" == all ]]; then
  mapfile -t KINDS < <(awk -F'\t' '!/^#/ && NF > 1 && $1 != "ladder" { print $1 }' "$TSV")
else
  KINDS=("$KIND")
fi

digest() { # replaces $1 (jsonl) by its digest file $2
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
  mapfile -t FIXTURES < <(row "$kind")
  [[ ${#FIXTURES[@]} -gt 0 ]] || { echo "generate.sh: no row for kind $kind in $TSV" >&2; exit 2; }
  if [[ "${FIXTURES[0]}" == "@ladder" ]]; then
    mapfile -t FIXTURES < <(row ladder)
  fi
  count=0
  for dsn in "${FIXTURES[@]}"; do
    if [[ "$dsn" == - ]]; then
      board=$kind
    else
      board=${dsn#fixtures/}
      board=${board%.dsn}
    fi
    out="fixtures/reference/probe/$kind/$board.jsonl"
    log="target/probe-logs/autoroute/$kind/$board.log"
    mkdir -p "$(dirname "$out")" "$(dirname "$log")"
    rm -f "$out" "${out%.jsonl}.digest"
    if ! TMPDIR=${TMPDIR:-$ROOT/target} "$PROBE_SH" "$PIN" "$PROBE" "$kind" "$dsn" "$out" \
      > "$log" 2>&1; then
      echo "generate.sh: $kind failed on $dsn, see $log" >&2
      exit 1
    fi
    if [[ $(stat -c %s "$out") -gt $MAX_BYTES ]]; then
      digest "$out" "${out%.jsonl}.digest"
    fi
    count=$((count + 1))
  done
  echo "generate.sh: $kind: $count boards"
done
