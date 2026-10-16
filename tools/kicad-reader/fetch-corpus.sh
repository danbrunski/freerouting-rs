#!/usr/bin/env bash
# K04.7: fill the out-of-repo .kicad_pcb corpus, or (re)extract the committed set.
#
#   tools/kicad-reader/fetch-corpus.sh <dir>        fill <dir> ($FR_KICAD_CORPUS)
#   tools/kicad-reader/fetch-corpus.sh --committed  re-extract fixtures/kicad/ from the Java repo
#
# Layout of <dir>:
#   java/<Issue dir>/...   the 38 .kicad_pcb boards of the Java repo's fixtures/ and examples/
#                          at $JAVA_COMMIT (with their .kicad_pro/.kicad_dru), by `git show`
#   demos/<demo>/...       KiCad's bundled demos (boards, project files, licence files)
#   canary-pwr/...         a snapshot of canary-pwr (.kicad_pcb, .kicad_pro, .kicad_dru) with
#                          SNAPSHOT.sha256: the board is being edited, so only this file
#                          pins the copy
# Sources that are missing are skipped with a note. Running it twice gives byte-identical
# files, except canary-pwr when the owner has edited the board in between.
set -u
JAVA_REPO="${JAVA_REPO:-$(cd "$(dirname "$0")/../../.." && pwd)/freerouting}"
JAVA_COMMIT="${JAVA_COMMIT:-cda540f}"
DEMOS="${KICAD_DEMOS:-/usr/share/kicad/demos}"
CANARY="${CANARY_PWR_DIR:-$(cd "$(dirname "$0")/../../.." && pwd)/canary/hardware/canary-pwr-rev1.0}"
repo="$(cd "$(dirname "$0")/../.." && pwd)"

# The committed set: path under fixtures/ or examples/ of the Java repo.
COMMITTED=(
  "fixtures/Issue368-CorneyIslandWireless/corney_island_wireless.kicad_pcb"
  "fixtures/Issue269-min_fr_test/min_fr_test.kicad_pcb"
  "fixtures/Issue508-DAC2020/DAC2020_bm01/bm1.unrouted.kicad_pcb"
  "fixtures/Issue558-dev-board-autoroute-demo/dev-board.kicad_pcb"
  "fixtures/Issue742-tastexx-pcb/tastexx-pcb.kicad_pcb"
  "fixtures/Issue184-motorizedopener/motorizedopener.kicad_pcb"
  "fixtures/Issue180-Test/Test.kicad_pcb"
)

have_java() { git -C "$JAVA_REPO" cat-file -e "$JAVA_COMMIT^{commit}" 2>/dev/null; }

# extract <git path> <dest dir>: the file and its sibling .kicad_pro/.kicad_dru of the same stem
extract() {
  local path="$1" dest="$2" stem="${1%.kicad_pcb}" ext
  mkdir -p "$dest"
  git -C "$JAVA_REPO" show "$JAVA_COMMIT:$path" > "$dest/$(basename "$path")" || return 1
  for ext in kicad_pro kicad_dru; do
    if git -C "$JAVA_REPO" cat-file -e "$JAVA_COMMIT:$stem.$ext" 2>/dev/null; then
      git -C "$JAVA_REPO" show "$JAVA_COMMIT:$stem.$ext" > "$dest/$(basename "$stem").$ext"
    fi
  done
  # a project file whose stem differs from the board's (one per directory)
  if [ ! -e "$dest/$(basename "$stem").kicad_pro" ]; then
    local d="${path%/*}" f
    f=$(git -C "$JAVA_REPO" ls-tree --name-only "$JAVA_COMMIT" "$d/" | grep '\.kicad_pro$' | head -1)
    if [ -n "$f" ]; then
      git -C "$JAVA_REPO" show "$JAVA_COMMIT:$f" > "$dest/$(basename "$f")"
    fi
  fi
}

if [ "${1:-}" = "--committed" ]; then
  have_java || { echo "Java repo $JAVA_REPO has no commit $JAVA_COMMIT" >&2; exit 1; }
  for p in "${COMMITTED[@]}"; do
    rel="${p#fixtures/}"
    extract "$p" "$repo/fixtures/kicad/$(dirname "$rel")" || exit 1
    echo "committed: $rel"
  done
  exit 0
fi

dir="${1:-${FR_KICAD_CORPUS:-}}"
if [ -z "$dir" ]; then echo "usage: $0 <dir> | --committed" >&2; exit 2; fi
mkdir -p "$dir"

if have_java; then
  while IFS= read -r -d '' p; do
    case "$p" in fixtures/*) rel="${p#fixtures/}";; examples/*) rel="${p#examples/}";; esac
    extract "$p" "$dir/java/$(dirname "$rel")" || echo "skip: $p"
  done < <(git -C "$JAVA_REPO" ls-tree -r -z --name-only "$JAVA_COMMIT" -- fixtures examples \
            | tr '\0' '\n' | grep '\.kicad_pcb$' | tr '\n' '\0')
else
  echo "note: Java repo $JAVA_REPO has no commit $JAVA_COMMIT; java boards skipped" >&2
fi

if [ -d "$DEMOS" ]; then
  find "$DEMOS" -name '*.kicad_pcb' -print0 | while IFS= read -r -d '' b; do
    d="$(dirname "$b")"; rel="${d#"$DEMOS"/}"
    mkdir -p "$dir/demos/$rel"
    cp -p "$b" "$dir/demos/$rel/"
    for f in "$d"/*.kicad_pro "$d"/*.kicad_dru "$d"/LICENSE* "$d"/COPYING*; do
      [ -e "$f" ] && cp -p "$f" "$dir/demos/$rel/"
    done
    # the licence file of the demo's top folder (nested boards, e.g. nfc_antenna)
    top="$DEMOS/${rel%%/*}"
    for f in "$top"/LICENSE* "$top"/COPYING*; do
      [ -e "$f" ] && cp -p "$f" "$dir/demos/${rel%%/*}/"
    done
  done
else
  echo "note: no KiCad demos at $DEMOS; skipped" >&2
fi

if [ -f "$CANARY/canary-pwr.kicad_pcb" ]; then
  mkdir -p "$dir/canary-pwr"
  for e in kicad_pcb kicad_pro kicad_dru; do
    [ -e "$CANARY/canary-pwr.$e" ] && cp "$CANARY/canary-pwr.$e" "$dir/canary-pwr/"
  done
  (cd "$dir/canary-pwr" && sha256sum canary-pwr.kicad_* > SNAPSHOT.sha256)
else
  echo "note: no canary-pwr at $CANARY; skipped" >&2
fi
echo "corpus in $dir"
