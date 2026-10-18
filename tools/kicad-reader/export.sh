#!/usr/bin/env bash
# K04.8: export a board's DSN with KiCad's own exporter, on a copy.
#
#   tools/kicad-reader/export.sh [--windows] <board.kicad_pcb> <out.dsn>
#
# Default interpreter: the WSL system python3 (KiCad 10.0.7 from the Ubuntu packages), no path
# translation. --windows: Windows KiCad 10.0.5's python.exe (the version the owner's plugin runs in),
# paths through wslpath -w.
#
# The board and its .kicad_pro/.kicad_dru are copied into a scratch folder and KiCad runs there,
# so the original is never touched (no lock files, no .kicad_prl). The scratch folder is refused
# when it lies inside fixtures/ or the canary repo. KiCad processes are recorded before and
# after (pgrep -x on Linux, tasklist.exe on Windows) and the script fails if they differ; it never
# stops one. The output .dsn is the only file written outside the scratch folder; its first line
# (the scratch path) is replaced by the board's name, unless KEEP_FIRST_LINE=1.
# Prints `KICAD_VERSION <build version>` on stdout.
set -u
mode=wsl
if [ "${1:-}" = "--windows" ]; then mode=windows; shift; fi
board="${1:-}"; out="${2:-}"
if [ -z "$board" ] || [ -z "$out" ]; then echo "usage: $0 [--windows] <board.kicad_pcb> <out.dsn>" >&2; exit 2; fi
[ -f "$board" ] || { echo "no such board: $board" >&2; exit 2; }
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
canary="$(cd "$repo/.." && pwd)/canary"
WIN_PY="${KICAD_WIN_PYTHON:-/mnt/c/Program Files/KiCad/10.0/bin/python.exe}"

scratch="$(mktemp -d "${TMPDIR:-/tmp}/kicad-export.XXXXXX")" || exit 1
scratch="$(cd "$scratch" && pwd -P)"
trap 'rm -rf "$scratch"' EXIT
case "$scratch/" in
  "$repo/fixtures/"*|"$canary/"*) echo "refusing to run KiCad inside $scratch" >&2; exit 2;;
esac

procs() {
  if [ "$mode" = windows ]; then
    tasklist.exe /FO CSV /NH /FI "IMAGENAME eq kicad.exe" 2>/dev/null | tr -d '\r' | awk -F'","' 'NF>1{print "kicad.exe " $2}'
    tasklist.exe /FO CSV /NH /FI "IMAGENAME eq pcbnew.exe" 2>/dev/null | tr -d '\r' | awk -F'","' 'NF>1{print "pcbnew.exe " $2}'
  else
    pgrep -x kicad | sed 's/^/kicad /'
    pgrep -x pcbnew | sed 's/^/pcbnew /'
  fi
}

stem="$(basename "${board%.kicad_pcb}")"
src_dir="$(dirname "$board")"
cp -- "$board" "$scratch/$stem.kicad_pcb"
for e in kicad_pro kicad_dru; do
  [ -f "$src_dir/$stem.$e" ] && cp -- "$src_dir/$stem.$e" "$scratch/$stem.$e"
done
# a board without its own project file may share a folder's single project file (as KiCad finds it)
if [ ! -f "$scratch/$stem.kicad_pro" ]; then
  n=$(ls "$src_dir"/*.kicad_pro 2>/dev/null | wc -l)
  [ "$n" = 1 ] && cp -- "$src_dir"/*.kicad_pro "$scratch/$stem.kicad_pro"
fi
before_sums="$(sha256sum "$board" | cut -d' ' -f1)"

before="$(procs)"
if [ "$mode" = windows ]; then
  [ -x "$WIN_PY" ] || { echo "no Windows KiCad python at $WIN_PY" >&2; exit 2; }
  wb="$(wslpath -w "$scratch/$stem.kicad_pcb")"; wo="$(wslpath -w "$scratch/out.dsn")"
  wscript="$(wslpath -w "$here/export_dsn.py")"
  log="$(cd "$scratch" && "$WIN_PY" -I "$wscript" "$wb" "$wo" 2>&1 < /dev/null)"; rc=$?
else
  log="$(cd "$scratch" && python3 -I "$here/export_dsn.py" "$scratch/$stem.kicad_pcb" "$scratch/out.dsn" 2>&1 < /dev/null)"; rc=$?
fi
after="$(procs)"

version="$(printf '%s\n' "$log" | grep -a '^KICAD_VERSION ' | head -1)"
if [ "$before" != "$after" ]; then
  echo "KiCad processes changed during the export:" >&2
  echo "before: $before" >&2; echo "after: $after" >&2
  exit 3
fi
if [ "$rc" -ne 0 ] || [ ! -s "$scratch/out.dsn" ]; then
  echo "export failed (exit $rc)" >&2; printf '%s\n' "$log" | grep -v PROPERTY_ENUM | tail -5 >&2; exit 1
fi
[ "$(sha256sum "$board" | cut -d' ' -f1)" = "$before_sums" ] || { echo "the original changed!" >&2; exit 3; }
# KiCad's first line holds the (random) scratch path: `(pcb "/tmp/kicad-export.X/out.dsn"`. Replace
# it with the board's name so that exporting twice gives identical files; KEEP_FIRST_LINE=1 keeps it.
# Windows KiCad writes CRLF line ends; they are converted to LF so that both exports compare byte for byte.
if [ "${KEEP_FIRST_LINE:-0}" = 1 ]; then
  tr -d '\r' < "$scratch/out.dsn" > "$out"
else
  { printf '(pcb "%s.dsn"\n' "$stem"; tail -n +2 "$scratch/out.dsn" | tr -d '\r'; } > "$out"
fi
echo "${version:-KICAD_VERSION unknown}"
