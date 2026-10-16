#!/usr/bin/env bash
# Regenerates every committed C01 dump (settings, rules, ses, dsn, argv, clirun).
#
#   tools/java-reference/regenerate-dumps.sh JAVA_REPO [NAME_FILTER]
#
# JAVA_REPO  a detached worktree of danbrunski/freerouting at the pin (never ../freerouting itself)
#
# Each step is one `run.sh JAVA_REPO MODE` call, so REFERENCE_WORK_DIR, REFERENCE_FIXTURES_DIR,
# REFERENCE_OUT_DIR, REFERENCE_FULL and JAVA work as described in README.md. `ses` imports
# fixtures/reference/route/*.ses, so rerun this script after every `route` regeneration.
# Full set outside the repo (no `fixtures/` write):
#   REFERENCE_FULL=1 REFERENCE_OUT_DIR=target/java-reference/full tools/java-reference/regenerate-dumps.sh JAVA_REPO
# NAME_FILTER (optional) limits the board modes to boards whose name contains it.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$1
FILTER=${2:-}
for mode in settings rules ses dsn; do
  "$HERE/run.sh" "$REPO" "$mode" ${FILTER:+"$FILTER"}
done
"$HERE/run.sh" "$REPO" argv "$HERE/cases/argv.txt"
"$HERE/run.sh" "$REPO" clirun "$HERE/cases/clirun.tsv"
