#!/usr/bin/env bash
# Tests tools/determinism-check.sh (F02.4). Not part of cargo test: each check runs one busy
# loop per core for several seconds.
#
#   tools/test-determinism-check.sh PIN_CHECKOUT SCRATCH_DIR
#
# PIN_CHECKOUT  a pin worktree whose Java classes are built (tools/java-reference/probe.sh or
#               run.sh builds them); REFERENCE_WORK_DIR selects the build as for run.sh
# SCRATCH_DIR   an empty directory for the outputs
#
# Checks: a Java load reference dump is identical; a command writing the current time is
# reported as different with exit 1; no burner survives either run, nor a Ctrl-C (SIGINT to
# the script's process group) in the middle of the loaded run.
set -uo pipefail
[[ $# -eq 2 ]] || { echo "usage: $0 PIN_CHECKOUT SCRATCH_DIR" >&2; exit 2; }
PIN=$(cd "$1" && pwd)
mkdir -p "$2"
SCRATCH=$(cd "$2" && pwd)
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CHECK="$ROOT/tools/determinism-check.sh"
W=${REFERENCE_WORK_DIR:-$ROOT/target/java-reference}
JAVA=${JAVA:-$W/jdk4py/jdk4py/java-runtime/bin/java}
FAILED=0

fail() {
  echo "FAIL: $*"
  FAILED=1
}

no_burners() {
  if pgrep -f determinism-check-burne[r] > /dev/null; then
    fail "burners still running after $1: $(pgrep -f determinism-check-burne[r] | tr '\n' ' ')"
    pkill -f determinism-check-burne[r]
  else
    echo "ok: no burner left after $1"
  fi
}

pgrep -f determinism-check-burne[r] > /dev/null && { echo "burners already running; stop them first" >&2; exit 2; }

# 1. A Java load reference dump is identical.
[[ -d "$W/harness/reference" ]] || { echo "no harness classes in $W/harness; run run.sh or probe.sh first" >&2; exit 2; }
CP="$W/harness:$W/classes:$PIN/src/main/resources:$(cat "$PIN/build/reference-classpath.txt")"
OUTPUT=$("$CHECK" --out "$SCRATCH/java-load" -- env -u JAVA_TOOL_OPTIONS "$JAVA" \
  -Djava.awt.headless=true -Dfile.encoding=UTF-8 -cp "$CP" reference.ReferenceDump load \
  "$ROOT/fixtures" {OUT} 100 00:02:00 Issue508-DAC2020_bm01)
STATUS=$?
echo "$OUTPUT"
[[ $STATUS -eq 0 && "$OUTPUT" == *$'\nidentical' ]] || fail "java load dump: exit $STATUS"
[[ -f "$SCRATCH/java-load/idle/load/Issue508-DAC2020_bm01.json" ]] || fail "java load dump wrote no JSON"
no_burners "the java load run"

# 2. A command that writes the current time differs.
OUTPUT=$("$CHECK" --out "$SCRATCH/clock" -- bash -c 'date +%s.%N > "$1"' clock {OUT})
STATUS=$?
echo "$OUTPUT"
[[ $STATUS -eq 1 && "$OUTPUT" == *"different:"* ]] || fail "clock command: exit $STATUS"
no_burners "the clock run"

# 3. Ctrl-C during the loaded run stops the burners. Job control gives the script its own
# process group, as a terminal does, so SIGINT is not ignored in it.
set -m
"$CHECK" --out "$SCRATCH/interrupt" -- bash -c 'case $1 in *loaded*) sleep 60 ;; esac; echo x > "$1"' slow {OUT} \
  > "$SCRATCH/interrupt.log" 2>&1 &
PID=$!
set +m
for _ in $(seq 1 300); do
  grep -q "loaded run: load before" "$SCRATCH/interrupt.log" 2> /dev/null && break
  sleep 0.2
done
pgrep -f determinism-check-burne[r] > /dev/null || fail "no burners were running before Ctrl-C"
kill -INT -- "-$PID"
wait "$PID"
STATUS=$?
cat "$SCRATCH/interrupt.log"
[[ $STATUS -eq 130 ]] || fail "interrupted run: exit $STATUS, expected 130"
no_burners "Ctrl-C"

if [[ $FAILED -ne 0 ]]; then
  echo "determinism-check tests FAILED"
  exit 1
fi
echo "determinism-check tests passed"
