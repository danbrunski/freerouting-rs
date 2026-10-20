#!/bin/bash
# KICAD_BIN for canary's hardware/tools/grade.sh on WSL-native KiCad 10.0.7 (X13): that script
# calls python.exe and kicad-cli.exe with Windows paths (wslpath -w); this converts each such
# argument back to a WSL path and runs the real tool.
#   conv.sh TOOL ARGS...
tool=$1; shift
args=()
for a in "$@"; do
  if [[ $a == \\\\wsl* || $a =~ ^[A-Za-z]:\\ ]]; then args+=("$(wslpath -u "$a")"); else args+=("$a"); fi
done
exec "$tool" "${args[@]}"
