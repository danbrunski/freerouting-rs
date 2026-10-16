#!/usr/bin/env bash
# Runs a command twice, once idle and once beside one busy-loop process per core, and checks
# that both runs wrote the same output.
#
#   tools/determinism-check.sh --out DIR [--ignore REGEX] [--warmup SECONDS] -- COMMAND...
#
# COMMAND   the command and its arguments; every {OUT} in them is replaced by the run's own
#           output path, DIR/idle for the first run and DIR/loaded for the second. The command
#           writes a file or a directory there.
# --out     where both outputs and the burners' log go (created; must not hold old outputs)
# --ignore  drop lines matching this extended regex from both outputs before comparing, for
#           fields that are allowed to differ such as wall-clock timings ("routeMillis").
#           Without it the comparison is byte for byte.
# --warmup  seconds the burners run before the second run starts (default 5)
#
# Prints the 1-minute load average before and after each run, the exit status of each run, and
# then "identical" or the first differing line (with its file for directory outputs).
# Exit status: 0 identical, 1 the outputs or exit statuses differ, 2 usage error or both runs
# failed. The burners are killed when the second run ends, and on Ctrl-C or any other exit.
#
# Example, a Java load reference dumped into {OUT} instead of fixtures/reference (the harness
# classes are those run.sh builds into target/java-reference/):
#   W=target/java-reference; PIN=<pin worktree>
#   CP="$W/harness:$W/classes:$PIN/src/main/resources:$(cat $PIN/build/reference-classpath.txt)"
#   tools/determinism-check.sh --out <scratch>/det -- env JAVA_TOOL_OPTIONS= \
#     $W/jdk4py/jdk4py/java-runtime/bin/java -Djava.awt.headless=true -cp "$CP" \
#     reference.ReferenceDump load fixtures {OUT} 100 00:02:00 Issue508-DAC2020_bm01
set -uo pipefail

usage() {
  echo "usage: $0 --out DIR [--ignore REGEX] [--warmup SECONDS] -- COMMAND... ({OUT} = output path)" >&2
  exit 2
}

OUT_DIR="" IGNORE="" WARMUP=5
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT_DIR=${2:-}; shift 2 || usage ;;
    --ignore) IGNORE=${2:-}; shift 2 || usage ;;
    --warmup) WARMUP=${2:-}; shift 2 || usage ;;
    --) shift; break ;;
    *) usage ;;
  esac
done
[[ -n "$OUT_DIR" && $# -gt 0 ]] || usage
[[ "$WARMUP" =~ ^[0-9]+$ ]] || usage
if [[ " $* " != *"{OUT}"* ]]; then
  echo "determinism-check: the command has no {OUT} placeholder" >&2
  exit 2
fi
mkdir -p "$OUT_DIR"
OUT_DIR=$(cd "$OUT_DIR" && pwd)
for run in idle loaded; do
  if [[ -e "$OUT_DIR/$run" ]]; then
    echo "determinism-check: $OUT_DIR/$run exists; use an empty --out directory" >&2
    exit 2
  fi
done

load1() {
  if [[ -r /proc/loadavg ]]; then
    cut -d ' ' -f 1 /proc/loadavg
  else
    sysctl -n vm.loadavg 2> /dev/null | tr -d '{}' | awk '{print $1}'
  fi
}

cores() {
  nproc 2> /dev/null || sysctl -n hw.ncpu 2> /dev/null || echo 1
}

BURNERS=()
CMD_PID=""
stop_burners() {
  if [[ ${#BURNERS[@]} -gt 0 ]]; then
    kill "${BURNERS[@]}" 2> /dev/null
    wait "${BURNERS[@]}" 2> /dev/null
    BURNERS=()
  fi
}
stop_command() {
  if [[ -n "$CMD_PID" ]]; then
    # The command runs in its own process group (set -m below): stop all of it.
    kill -TERM -- "-$CMD_PID" 2> /dev/null || kill -TERM "$CMD_PID" 2> /dev/null
    CMD_PID=""
  fi
}
on_signal() {
  stop_command
  stop_burners
  echo "determinism-check: interrupted; command and burners stopped" >&3
  exit 130
}
# The traps may run while a run's output is redirected to its log; fd 3 is the real stderr.
exec 3>&2
trap 'stop_command; stop_burners' EXIT
trap on_signal INT TERM HUP

# Runs the command with {OUT} replaced by $1, in the background so that a signal reaches the
# trap at once instead of after the command ends, and in its own process group so that the
# trap can stop the command's children too.
run_command() {
  local out=$1
  shift
  local args=()
  local arg
  for arg in "$@"; do
    args+=("${arg//\{OUT\}/$out}")
  done
  set -m
  "${args[@]}" &
  CMD_PID=$!
  set +m
  wait "$CMD_PID"
  local status=$?
  CMD_PID=""
  return "$status"
}

echo "idle run: load before $(load1)"
run_command "$OUT_DIR/idle" "$@" > "$OUT_DIR/idle.log" 2>&1
IDLE_STATUS=$?
echo "idle run: load after $(load1), exit $IDLE_STATUS"

N=$(cores)
for ((i = 0; i < N; i++)); do
  bash -c 'while :; do :; done' determinism-check-burner &
  BURNERS+=($!)
done
echo "burners: $N started, warming up ${WARMUP} s"
sleep "$WARMUP"
echo "loaded run: load before $(load1)"
run_command "$OUT_DIR/loaded" "$@" > "$OUT_DIR/loaded.log" 2>&1
LOADED_STATUS=$?
echo "loaded run: load after $(load1), exit $LOADED_STATUS"
stop_burners

if [[ $IDLE_STATUS -ne 0 && $LOADED_STATUS -ne 0 ]]; then
  echo "determinism-check: both runs failed (logs in $OUT_DIR/idle.log, loaded.log)" >&2
  exit 2
fi
if [[ $IDLE_STATUS -ne $LOADED_STATUS ]]; then
  echo "different: exit status $IDLE_STATUS idle, $LOADED_STATUS loaded"
  exit 1
fi

# Prints the first differing line of two files, after dropping --ignore lines.
first_difference() {
  local a=$1 b=$2
  if [[ -n "$IGNORE" ]]; then
    diff <(grep -Ev -- "$IGNORE" "$a") <(grep -Ev -- "$IGNORE" "$b") | grep -m 3 -E '^[0-9<>]'
  else
    cmp -s "$a" "$b" && return 0
    diff "$a" "$b" | grep -m 3 -E '^[0-9<>]' || cmp "$a" "$b"
  fi
}

IDLE="$OUT_DIR/idle" LOADED="$OUT_DIR/loaded"
if [[ ! -e "$IDLE" || ! -e "$LOADED" ]]; then
  echo "different: idle output $([[ -e $IDLE ]] && echo exists || echo missing), loaded output $([[ -e $LOADED ]] && echo exists || echo missing)"
  exit 1
fi
if [[ -f "$IDLE" && -f "$LOADED" ]]; then
  DIFF=$(first_difference "$IDLE" "$LOADED")
  if [[ -n "$DIFF" ]]; then
    echo "different:"
    echo "$DIFF"
    exit 1
  fi
elif [[ -d "$IDLE" && -d "$LOADED" ]]; then
  LIST_A=$(cd "$IDLE" && find . -type f | LC_ALL=C sort)
  LIST_B=$(cd "$LOADED" && find . -type f | LC_ALL=C sort)
  if [[ "$LIST_A" != "$LIST_B" ]]; then
    echo "different: file lists differ"
    diff <(echo "$LIST_A") <(echo "$LIST_B") | grep -m 3 -E '^[<>]'
    exit 1
  fi
  while IFS= read -r file; do
    DIFF=$(first_difference "$IDLE/$file" "$LOADED/$file")
    if [[ -n "$DIFF" ]]; then
      echo "different: ${file#./}"
      echo "$DIFF"
      exit 1
    fi
  done <<< "$LIST_A"
else
  echo "different: one output is a file, the other a directory"
  exit 1
fi
echo "identical"
exit 0
