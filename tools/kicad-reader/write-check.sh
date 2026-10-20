#!/usr/bin/env bash
# K08.5: KiCad grades a session brought back through ImportSpecctraSES and through the direct
# .kicad_pcb writer (K08) the same.
#
#   tools/kicad-reader/write-check.sh <board-dir | board.kicad_pcb> <session.ses> [outdir]
#
# On copies in a scratch folder (the originals are never touched; the script checks):
#   path A  pcbnew.LoadBoard + pcbnew.ImportSpecctraSES + pcbnew.SaveBoard (what the plugin does),
#   path B  freerouting-rs -de <board>.kicad_pcb+<session.ses> -do <board>.kicad_pcb with the
#           autorouter, fanout and optimizer off, so the session is all that changes (K08.4),
# then `kicad-cli pcb drc --format json --severity-all --refill-zones` on both, one at a time,
# and a comparison of the violations by type and of the unconnected count. The .kicad_pro and
# .kicad_dru next to the board go along. Exit 0 when both paths grade the same, 1 when they
# differ, 2 for a usage or setup error, 3 when a KiCad process appeared or vanished or an
# original changed.
#
# KiCad: the WSL system python3 (pcbnew of KiCad 10.0.7, Ubuntu packages) and /usr/bin/kicad-cli
# (KICAD_PYTHON and KICAD_CLI override; never mix versions). FREEROUTING defaults to the release
# CLI of this tree. KiCad processes are recorded before and after (pgrep -x kicad / pcbnew); the
# script never starts the KiCad GUI, never stops a KiCad process and never touches the Java
# Freerouting plugin. With [outdir] the two boards and DRC reports are kept there.
set -u
src="${1:-}"; ses="${2:-}"; outdir="${3:-}"
if [ -z "$src" ] || [ -z "$ses" ]; then
  echo "usage: $0 <board-dir | board.kicad_pcb> <session.ses> [outdir]" >&2; exit 2
fi
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
canary="$(cd "$repo/.." && pwd)/canary"
KICAD_PYTHON="${KICAD_PYTHON:-python3}"
KICAD_CLI="${KICAD_CLI:-kicad-cli}"
FREEROUTING="${FREEROUTING:-$repo/target/release/freerouting-rs}"

if [ -d "$src" ]; then
  mapfile -t found < <(find "$src" -maxdepth 1 -name '*.kicad_pcb' | sort)
  [ "${#found[@]}" -eq 1 ] || { echo "$src must hold exactly one .kicad_pcb (found ${#found[@]})" >&2; exit 2; }
  board="${found[0]}"
else
  board="$src"
fi
[ -f "$board" ] || { echo "no such board: $board" >&2; exit 2; }
[ -f "$ses" ] || { echo "no such session: $ses" >&2; exit 2; }
[ -x "$FREEROUTING" ] || { echo "no freerouting-rs CLI at $FREEROUTING (cargo build --release)" >&2; exit 2; }
command -v "$KICAD_CLI" > /dev/null 2>&1 || { echo "kicad-cli not found ('$KICAD_CLI')" >&2; exit 2; }
cli_version="$("$KICAD_CLI" version 2>/dev/null | tr -d '\r' | head -1)"
py_version="$("$KICAD_PYTHON" -I -c 'import pcbnew; print(pcbnew.GetBuildVersion())' 2>/dev/null | tail -1)"
case "$py_version" in
  "$cli_version"*) ;;
  *) echo "kicad-cli is '$cli_version' but pcbnew is '$py_version'; never mix KiCad versions" >&2; exit 2;;
esac

scratch="$(mktemp -d "${TMPDIR:-/tmp}/kicad-write.XXXXXX")" || exit 1
scratch="$(cd "$scratch" && pwd -P)"
trap 'rm -rf "$scratch"' EXIT
case "$scratch/" in
  "$repo/fixtures/"*|"$canary/"*) echo "refusing to run KiCad inside $scratch" >&2; exit 2;;
esac
[ -n "$outdir" ] && mkdir -p "$outdir"

procs() { pgrep -x kicad | sed 's/^/kicad /'; pgrep -x pcbnew | sed 's/^/pcbnew /'; }

stem="$(basename "${board%.kicad_pcb}")"
src_dir="$(dirname "$board")"
sums() { sha256sum "$board" "$ses" | cut -d' ' -f1; }
before_sum="$(sums)"
before="$(procs)"

stage() { # dir: the board and the files next to it
  mkdir -p "$1"
  cp -- "$board" "$1/$stem.kicad_pcb"
  for e in kicad_pro kicad_dru; do
    [ -f "$src_dir/$stem.$e" ] && cp -- "$src_dir/$stem.$e" "$1/$stem.$e"
  done
}
stage "$scratch/a"
stage "$scratch/b/in"
stage "$scratch/b"
rm -f "$scratch/b/$stem.kicad_pcb"
cp -- "$ses" "$scratch/session.ses"

# path A: KiCad's own import
(cd "$scratch/a" && "$KICAD_PYTHON" -I -c '
import sys, pcbnew
board = pcbnew.LoadBoard(sys.argv[1])
ok = pcbnew.ImportSpecctraSES(board, sys.argv[2])
if not ok:
    sys.exit("ImportSpecctraSES failed")
pcbnew.SaveBoard(sys.argv[1], board)
print("A: KiCad %s imported the session, %d tracks, arcs and vias" % (pcbnew.GetBuildVersion(), len(board.GetTracks())))
' "$scratch/a/$stem.kicad_pcb" "$scratch/session.ses" 2>&1 < /dev/null) | grep -av PROPERTY_ENUM
rca=${PIPESTATUS[0]}

# path B: the direct writer
(cd "$scratch/b" && "$FREEROUTING" -de "in/$stem.kicad_pcb+$scratch/session.ses" -do "$stem.kicad_pcb" \
  --router.autorouter.enabled=false --router.fanout.enabled=false --router.optimizer.enabled=false \
  > "$scratch/b.log" 2>&1 < /dev/null)
rcb=$?
grep -a "SES session loaded\|Successfully saved\|error" "$scratch/b.log" | sed 's/^/B: /'

if [ "$rca" -ne 0 ] || [ "$rcb" -ne 0 ] || [ ! -s "$scratch/b/$stem.kicad_pcb" ]; then
  echo "a path failed (A: exit $rca, B: exit $rcb)" >&2; tail -5 "$scratch/b.log" >&2; exit 1
fi

drc() { # dir
  "$KICAD_CLI" pcb drc --format json --severity-all --refill-zones --output "$1/drc.json" \
    "$1/$stem.kicad_pcb" > "$1/drc.log" 2>&1 < /dev/null
  [ -s "$1/drc.json" ] || { echo "kicad-cli DRC failed in $1:" >&2; tail -5 "$1/drc.log" >&2; return 1; }
}
drc "$scratch/a" || exit 1
drc "$scratch/b" || exit 1

after="$(procs)"
if [ "$before" != "$after" ]; then
  echo "KiCad processes changed:" >&2; echo "before: $before" >&2; echo "after: $after" >&2; exit 3
fi
[ "$(sums)" = "$before_sum" ] || { echo "an original changed!" >&2; exit 3; }
if [ -n "$outdir" ]; then
  cp -- "$scratch/a/$stem.kicad_pcb" "$outdir/A.$stem.kicad_pcb"; cp -- "$scratch/a/drc.json" "$outdir/A.drc.json"
  cp -- "$scratch/b/$stem.kicad_pcb" "$outdir/B.$stem.kicad_pcb"; cp -- "$scratch/b/drc.json" "$outdir/B.drc.json"
fi
echo "KiCad: $cli_version (kicad-cli and pcbnew)"
python3 -I -c '
import json, sys
from collections import Counter
def grade(path):
    d = json.load(open(path, encoding="utf-8"))
    kinds = Counter(v.get("type", "?") for v in d.get("violations", []))
    return kinds, len(d.get("unconnected_items", []))
(ka, ua), (kb, ub) = grade(sys.argv[1]), grade(sys.argv[2])
print("%-32s %8s %8s" % ("violation", "A", "B"))
for k in sorted(set(ka) | set(kb)):
    print("%-32s %8d %8d%s" % (k, ka[k], kb[k], "" if ka[k] == kb[k] else "   <-- differs"))
print("%-32s %8d %8d%s" % ("unconnected", ua, ub, "" if ua == ub else "   <-- differs"))
same = ka == kb and ua == ub
print("SAME" if same else "DIFFERENT")
sys.exit(0 if same else 1)
' "$scratch/a/drc.json" "$scratch/b/drc.json"
