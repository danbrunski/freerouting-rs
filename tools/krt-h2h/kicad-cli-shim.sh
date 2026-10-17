#!/usr/bin/env bash
# kicad-cli for KiCadRoutingTools' oracle calls in the optional krt-kicad arm of X01.
#
# Runs KICAD_CLI_REAL (default /usr/bin/kicad-cli, the WSL-native KiCad 10.0.7, which takes
# Linux paths as they are) and logs every call, its arguments and exit status to
# KRT_SHIM_LOG (default /dev/null), so a run records whether and how often KRT called KiCad.
#
# When KICAD_CLI_REAL is a Windows kicad-cli.exe, every argument that names an existing file,
# or a file in an existing directory, is translated with `wslpath -w`. That branch exists for
# a machine without a native KiCad; X01 runs on the native one and never starts a Windows
# process.
set -uo pipefail
REAL=${KICAD_CLI_REAL:-/usr/bin/kicad-cli}
LOG=${KRT_SHIM_LOG:-/dev/null}
ARGS=()
if [[ "$REAL" == *.exe ]]; then
  for arg in "$@"; do
    if [[ "$arg" == /* && ( -e "$arg" || -d "$(dirname "$arg")" ) ]]; then
      ARGS+=("$(wslpath -w "$arg")")
    else
      ARGS+=("$arg")
    fi
  done
else
  ARGS=("$@")
fi
"$REAL" "${ARGS[@]}"
STATUS=$?
printf '%s\texit=%d\t%s\n' "$(date -u +%FT%TZ)" "$STATUS" "${ARGS[*]}" >> "$LOG"
exit $STATUS
