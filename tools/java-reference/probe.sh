#!/usr/bin/env bash
# Compiles and runs one Java probe against the Java Freerouting classes of a pin checkout.
#
#   tools/java-reference/probe.sh PIN_CHECKOUT PROBE.java [ARGS...]
#
# PIN_CHECKOUT  a detached worktree of danbrunski/freerouting at the port's pin (never
#               ../freerouting itself: another session edits it). Create one with
#               git -C ../freerouting worktree add --detach <scratchpad>/java-pin <pin>
# PROBE.java    a source file with a main method; its class name is the file name, prefixed
#               by its package if it declares one
# ARGS          passed to the probe's main
#
# Builds target/java-reference/classes through run.sh when it is missing (no reference file
# is written: the build uses a fixture filter that matches nothing). Refuses to run when the
# cached classes were built from another commit than PIN_CHECKOUT's HEAD. The probe is
# compiled with ECJ into a temporary directory, run on the jdk4py runtime with run.sh's
# classpath, and its stdout and exit status are passed through. A compile error prints ECJ's
# message on stderr and exits 1. REFERENCE_WORK_DIR and JAVA work as in run.sh.
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 PIN_CHECKOUT PROBE.java [ARGS...]" >&2
  exit 2
fi
PIN=$(cd "$1" && pwd)
PROBE_DIR=$(cd "$(dirname "$2")" && pwd)
PROBE="$PROBE_DIR/$(basename "$2")"
shift 2
[[ -f "$PROBE" ]] || { echo "probe.sh: no such probe: $PROBE" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
WORK=${REFERENCE_WORK_DIR:-$ROOT/target/java-reference}
ECJ="$WORK/ecj-3.46.100.jar"
CLASSES="$WORK/classes"
CP_FILE="$PIN/build/reference-classpath.txt"
export LANG=C.UTF-8 LC_ALL=C.UTF-8

if [[ ! -d "$CLASSES/app" || ! -f "$CP_FILE" || ! -f "$ECJ" ]]; then
  echo "probe.sh: building the Java classes of $PIN through run.sh" >&2
  # load mode with a fixture filter that matches no board: builds, dumps nothing.
  "$HERE/run.sh" "$PIN" load 1 00:00:01 "probe.sh: build only, no fixture matches this" >&2
fi

# The class cache must come from this pin (run.sh records the commit in Constants.java).
PIN_SHA=$(git -C "$PIN" rev-parse --short HEAD)
CONSTANTS="$WORK/gen/app/freerouting/constants/Constants.java"
BUILT_SHA=$(sed -n 's/.*FREEROUTING_VERSION = "\([^"]*\)".*/\1/p' "$CONSTANTS" 2>/dev/null || true)
if [[ "$BUILT_SHA" != "$PIN_SHA" ]]; then
  echo "probe.sh: $CLASSES was built from '${BUILT_SHA:-unknown}', but $PIN is at $PIN_SHA." >&2
  echo "probe.sh: delete $CLASSES (and $WORK/gen) to rebuild it from this pin." >&2
  exit 2
fi

JAVA=${JAVA:-$WORK/jdk4py/jdk4py/java-runtime/bin/java}
CP="$CLASSES:$PIN/src/main/resources:$(cat "$CP_FILE")"

OUT=$(mktemp -d "${TMPDIR:-/tmp}/probe.XXXXXX")
trap 'rm -rf "$OUT"' EXIT
if ! env -u JAVA_TOOL_OPTIONS "$JAVA" -jar "$ECJ" --release 25 -nowarn -encoding UTF-8 -cp "$CP" \
  -d "$OUT/classes" "$PROBE" > "$OUT/ecj.log" 2>&1; then
  cat "$OUT/ecj.log" >&2
  echo "probe.sh: $PROBE does not compile" >&2
  exit 1
fi

CLASS=$(basename "$PROBE" .java)
PACKAGE=$(sed -n 's/^[[:space:]]*package[[:space:]]\+\([A-Za-z0-9_.]*\)[[:space:]]*;.*/\1/p' "$PROBE" | head -n 1)
[[ -n "$PACKAGE" ]] && CLASS="$PACKAGE.$CLASS"

set +e
env -u JAVA_TOOL_OPTIONS "$JAVA" -Djava.awt.headless=true -Dfile.encoding=UTF-8 \
  -cp "$OUT/classes:$CP" "$CLASS" "$@"
STATUS=$?
set -e
exit $STATUS
