#!/usr/bin/env bash
# K07.4 step 3: import two Specctra sessions into copies of one board with KiCad's own importer and
# compare every track endpoint.
#
#   tools/kicad-reader/frame-check.sh [--windows] <board.kicad_pcb> <a.ses> <b.ses> [outdir]
#
# Each session goes into its own copy of the board (and its .kicad_pro) in a scratch folder, via
# pcbnew.LoadBoard + pcbnew.ImportSpecctraSES (frame_check.py dump); the originals are never
# touched. The two dumps are written to <outdir> (default: the scratch folder, deleted at the end)
# as a.json and b.json and compared with frame_check.py compare (100 nm). Default interpreter: the
# WSL system python3 (KiCad 10.0.7, Ubuntu packages); --windows: Windows KiCad 10.0.5's python.exe,
# paths through wslpath -w (not run by the K07 checklist). KiCad processes are recorded before and
# after and the script fails if they differ; it never stops one.
set -u
mode=wsl
if [ "${1:-}" = "--windows" ]; then mode=windows; shift; fi
board="${1:-}"; a="${2:-}"; b="${3:-}"; outdir="${4:-}"
if [ -z "$board" ] || [ -z "$a" ] || [ -z "$b" ]; then
  echo "usage: $0 [--windows] <board.kicad_pcb> <a.ses> <b.ses> [outdir]" >&2; exit 2
fi
for f in "$board" "$a" "$b"; do [ -f "$f" ] || { echo "no such file: $f" >&2; exit 2; }; done
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
canary="$(cd "$repo/.." && pwd)/canary"
WIN_PY="${KICAD_WIN_PYTHON:-/mnt/c/Program Files/KiCad/10.0/bin/python.exe}"

scratch="$(mktemp -d "${TMPDIR:-/tmp}/kicad-frame.XXXXXX")" || exit 1
scratch="$(cd "$scratch" && pwd -P)"
trap 'rm -rf "$scratch"' EXIT
case "$scratch/" in
  "$repo/fixtures/"*|"$canary/"*) echo "refusing to run KiCad inside $scratch" >&2; exit 2;;
esac
[ -n "$outdir" ] && mkdir -p "$outdir"

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
before_sum="$(sha256sum "$board" | cut -d' ' -f1)"
before="$(procs)"

run_one() { # name ses
  local name="$1" ses="$2" d="$scratch/$1"
  mkdir -p "$d"
  cp -- "$board" "$d/$stem.kicad_pcb"
  [ -f "$src_dir/$stem.kicad_pro" ] && cp -- "$src_dir/$stem.kicad_pro" "$d/$stem.kicad_pro"
  cp -- "$ses" "$d/in.ses"
  if [ "$mode" = windows ]; then
    [ -x "$WIN_PY" ] || { echo "no Windows KiCad python at $WIN_PY" >&2; return 2; }
    (cd "$d" && "$WIN_PY" -I "$(wslpath -w "$here/frame_check.py")" dump "$(wslpath -w "$d/$stem.kicad_pcb")" \
       "$(wslpath -w "$d/in.ses")" "$(wslpath -w "$d/$name.json")" 2>&1 < /dev/null)
  else
    (cd "$d" && python3 -I "$here/frame_check.py" dump "$d/$stem.kicad_pcb" "$d/in.ses" "$d/$name.json" 2>&1 < /dev/null)
  fi
}

la="$(run_one a "$a")"; rca=$?
lb="$(run_one b "$b")"; rcb=$?
after="$(procs)"
printf '%s\n' "$la" | grep -a '^KICAD_VERSION \|^IMPORTED ' | sed 's/^/a: /'
printf '%s\n' "$lb" | grep -a '^KICAD_VERSION \|^IMPORTED ' | sed 's/^/b: /'
if [ "$before" != "$after" ]; then
  echo "KiCad processes changed during the import:" >&2; echo "before: $before" >&2; echo "after: $after" >&2; exit 3
fi
[ "$(sha256sum "$board" | cut -d' ' -f1)" = "$before_sum" ] || { echo "the original changed!" >&2; exit 3; }
if [ "$rca" -ne 0 ] || [ "$rcb" -ne 0 ] || [ ! -s "$scratch/a/a.json" ] || [ ! -s "$scratch/b/b.json" ]; then
  echo "import failed (a: exit $rca, b: exit $rcb)" >&2
  printf '%s\n%s\n' "$la" "$lb" | grep -av PROPERTY_ENUM | tail -8 >&2
  exit 1
fi
if [ -n "$outdir" ]; then cp -- "$scratch/a/a.json" "$scratch/b/b.json" "$outdir/"; fi
python3 -I "$here/frame_check.py" compare "$scratch/a/a.json" "$scratch/b/b.json" "${TOLERANCE_NM:-100}"
