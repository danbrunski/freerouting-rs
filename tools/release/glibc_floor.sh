#!/usr/bin/env bash
# R01.4: fail when a binary needs a newer glibc than the floor.
#   glibc_floor.sh 2.35 target/release/freerouting target/release/freerouting-gui
# Prints the highest GLIBC_x.y symbol version of each binary (objdump -T).
set -euo pipefail
floor="$1"
shift
status=0
for bin in "$@"; do
  highest="$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/^GLIBC_//' | sort -V | tail -1)"
  highest="${highest:-none}"
  echo "$bin: highest GLIBC_$highest (floor $floor)"
  if [ "$highest" != none ] && [ "$(printf '%s\n%s\n' "$highest" "$floor" | sort -V | tail -1)" != "$floor" ]; then
    echo "error: $bin needs glibc $highest, newer than $floor" >&2
    status=1
  fi
done
exit $status
