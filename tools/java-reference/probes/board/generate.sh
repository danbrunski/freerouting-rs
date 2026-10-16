#!/usr/bin/env bash
# Writes the board-probe references: fixtures/reference/probe/<kind>/<board>.jsonl.
#
#   tools/java-reference/probes/board/generate.sh PIN_CHECKOUT KIND|all
#
# PIN_CHECKOUT  a detached worktree of danbrunski/freerouting at the port's pin (see probe.sh)
# KIND          a probe kind: the kebab-case name of a <Kind>Probe.java in this directory
#               (ids -> IdsProbe.java, state-hash -> StateHashProbe.java); 'all' runs every one
#
# Runs probe.sh once per fixtures.tsv row and kind, as
#   probe.sh PIN <Kind>Probe.java <dsn> <ses|-> <out.jsonl>
# A probe that exits with status 3 has nothing to say about that board (probes that only run on
# a few fixtures); no file is written for it. Any other non-zero status stops the script. An
# output above 1 MiB is replaced by <board>.digest: the header line, then one line
# "<section> <line count> <FNV-1a 64>" per section (a section starts at each line whose first key
# is "stage"). <board> is the DSN path below fixtures/ without ".dsn", so nested fixtures keep
# their directories. Logs of each run go to target/probe-logs/<kind>/<board>.log.
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
MAX_BYTES=$((1024 * 1024))

class_of() { # state-hash -> StateHashProbe.java
  local out="" part
  IFS='-' read -ra parts <<< "$1"
  for part in "${parts[@]}"; do out+="${part^}"; done
  echo "${out}Probe.java"
}
kind_of() { # StateHashProbe.java -> state-hash
  local name=${1%Probe.java}
  echo "$name" | sed -E 's/([a-z0-9])([A-Z])/\1-\2/g' | tr '[:upper:]' '[:lower:]'
}

if [[ "$KIND" == all ]]; then
  KINDS=()
  for file in "$HERE"/*Probe.java; do KINDS+=("$(kind_of "$(basename "$file")")"); done
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
    body.append(line)
flush()
open(dst, "w", encoding="utf-8").write("".join(l + "\n" for l in out))
PY
  rm "$1"
}

cd "$ROOT"
for kind in "${KINDS[@]}"; do
  probe="$HERE/$(class_of "$kind")"
  [[ -f "$probe" ]] || { echo "generate.sh: no probe $probe for kind $kind" >&2; exit 2; }
  count=0
  while IFS=$'\t' read -r dsn ses; do
    [[ -z "$dsn" || "$dsn" == \#* ]] && continue
    board=${dsn#fixtures/}
    board=${board%.dsn}
    out="fixtures/reference/probe/$kind/$board.jsonl"
    log="target/probe-logs/$kind/$board.log"
    mkdir -p "$(dirname "$out")" "$(dirname "$log")"
    rm -f "$out" "${out%.jsonl}.digest"
    set +e
    TMPDIR=${TMPDIR:-$ROOT/target} "$PROBE_SH" "$PIN" "$probe" "$dsn" "$ses" "$out" > "$log" 2>&1
    status=$?
    set -e
    if [[ $status -eq 3 ]]; then
      rm -f "$out"
      continue
    elif [[ $status -ne 0 ]]; then
      echo "generate.sh: $kind failed on $dsn (status $status), see $log" >&2
      exit 1
    fi
    if [[ $(stat -c %s "$out") -gt $MAX_BYTES ]]; then
      digest "$out" "${out%.jsonl}.digest"
    fi
    count=$((count + 1))
  done < "$HERE/fixtures.tsv"
  echo "generate.sh: $kind: $count boards"
done
