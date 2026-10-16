#!/usr/bin/env bash
# K04.8: export every board of the out-of-repo corpus with KiCad (WSL KiCad 10.0.7 by default).
#
#   tools/kicad-reader/export-corpus.sh [--windows] <corpus dir> [--force]
#
# Writes <stem>.kicad10.dsn next to each <stem>.kicad_pcb under <corpus dir> and a
# kicad10-export.log with the KiCad version and the outcome per board. Existing exports are kept
# unless --force. Boards KiCad cannot export are logged and skipped. KiCad processes are checked
# per board by export.sh.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
win=""; [ "${1:-}" = "--windows" ] && { win="--windows"; shift; }
dir="${1:-}"; force="${2:-}"
[ -d "$dir" ] || { echo "usage: $0 [--windows] <corpus dir> [--force]" >&2; exit 2; }
log="$dir/kicad10-export.log"; : > "$log"
find "$dir" -name '*.kicad_pcb' -print0 | sort -z | while IFS= read -r -d '' b; do
  out="${b%.kicad_pcb}.kicad10.dsn"
  if [ -s "$out" ] && [ "$force" != "--force" ]; then echo "kept    $b" >> "$log"; continue; fi
  if v=$("$here/export.sh" $win "$b" "$out" 2>&1); then
    echo "ok      $b  ${v##*$'\n'}" >> "$log"
  else
    rm -f "$out"; echo "FAILED  $b  $(printf '%s' "$v" | tail -1)" >> "$log"
  fi
done
grep -c '^ok\|^kept' "$log" | sed 's/^/exported: /'; grep '^FAILED' "$log" || true
