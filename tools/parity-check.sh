#!/usr/bin/env bash
# N07.5: the parity check in one command, timed. `legacy` (Java at the pin, D11) must stay
# byte-identical to Java on the connection-parity set and pass the probe tests. Run it after a
# change to code that `legacy` also runs (the search, the board, the writers); a change that only
# touches `enhanced` does not need it (AGENTS.md, "Matching Java").
#
#   tools/parity-check.sh [--full] [--no-probes] [--no-parity]
#
# Two steps, each timed, then a summary and the total:
#   1. probes: the Java-probe tests, every crates/freerouting/tests/*probe*.rs (release build);
#   2. connection parity: tests/autoroute_board_parity.rs on
#      tools/bench/sets/connection-parity.txt (PARITY_SET), every board's SES compared with
#      Java's; the verdict is the test's pass/fail ("N exact" in its summary).
# --full        step 2 runs every board (the full A14 gate, about 1.5 h, on request only:
#               AGENTS.md, "The machine"), not the small set
# --no-probes   skip step 1
# --no-parity   skip step 2
# Environment: PARITY_OUT (default target/parity) takes the records; CARGO_TARGET_DIR and
# CARGO_BUILD_JOBS apply to cargo as usual. Run from anywhere; it changes to the repository root.
# Exit status: 0 both steps passed, 1 a step failed, 2 usage error.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

full=0 probes=1 parity=1
for arg in "$@"; do
  case $arg in
    --full) full=1 ;;
    --no-probes) probes=0 ;;
    --no-parity) parity=0 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "parity-check.sh: unknown argument $arg" >&2; sed -n '7,7p' "$0" >&2; exit 2 ;;
  esac
done

status=0
summary=()
total_start=$SECONDS

# step NAME COMMAND...: run, time and record one step.
step() {
  local name=$1; shift
  local start=$SECONDS
  echo "== $name: $*"
  "$@"
  local code=$?
  local took=$((SECONDS - start))
  summary+=("$(printf '%-22s %-5s %ss' "$name" "$([[ $code -eq 0 ]] && echo PASS || echo FAIL)" "$took")")
  [[ $code -ne 0 ]] && status=1
  return 0
}

if [[ $probes -eq 1 ]]; then
  test_args=() files=0
  for f in crates/freerouting/tests/*probe*.rs; do
    test_args+=(--test "$(basename "$f" .rs)")
    files=$((files + 1))
  done
  step "probe tests ($files files)" \
    cargo test --release -p freerouting "${test_args[@]}"
fi

if [[ $parity -eq 1 ]]; then
  if [[ $full -eq 1 ]]; then
    name="board parity (full A14)"
    unset PARITY_SET
  else
    name="connection parity"
    export PARITY_SET=tools/bench/sets/connection-parity.txt
  fi
  step "$name" \
    cargo test --release -p freerouting --test autoroute_board_parity -- --ignored \
      autoroute_board_parity --test-threads=1 --nocapture
fi

echo
echo "== parity-check summary"
printf '%s\n' "${summary[@]}"
echo "total $((SECONDS - total_start))s"
[[ $status -eq 0 ]] && echo "PARITY OK" || echo "PARITY FAILED"
exit $status
