#!/usr/bin/env bash
# Fills $FR_CORPUS_DIR with the boards of tools/corpus/manifest.json and checks every hash.
#
#   tools/corpus/fetch.sh [--ids a,b] [--manifest FILE]
#
# FR_CORPUS_DIR defaults to <repository>/target/corpus (untracked; tools/bench/sets/
# kicad-corpus.txt names that location). Each board lands in $FR_CORPUS_DIR/src/<id>/ with its
# .kicad_pro/.kicad_dru companions. Files come from raw.githubusercontent.com at the pinned
# 40-hex commit, never a branch, so a second fetch is byte-identical; an existing file with
# the right hash is not downloaded again; a wrong hash is deleted and fails the run (exit 1).
# The boards are untrusted data: this script only downloads and hashes them.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
DIR=${FR_CORPUS_DIR:-$ROOT/target/corpus}
MANIFEST=$HERE/manifest.json
IDS=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --ids) IDS=${2:-}; shift 2 || exit 2 ;;
    --manifest) MANIFEST=${2:-}; shift 2 || exit 2 ;;
    *) echo "usage: $0 [--ids a,b] [--manifest FILE]" >&2; exit 2 ;;
  esac
done
mkdir -p "$DIR/src"
FAIL=0
# One line per file: id, repo, commit, repo path, expected sha256, local name.
python3 -I - "$MANIFEST" "$IDS" << 'PY' | while IFS=$'\t' read -r id repo commit path sha name; do
import json, os, sys
manifest, ids = sys.argv[1], [i for i in sys.argv[2].split(",") if i]
for b in json.load(open(manifest))["boards"]:
    if ids and b["id"] not in ids:
        continue
    base = b["path"][: -len(".kicad_pcb")]
    print("\t".join([b["id"], b["repo"], b["commit"], b["path"], b["sha256"], os.path.basename(b["path"])]))
    for name, sha in sorted(b.get("companions", {}).items()):
        print("\t".join([b["id"], b["repo"], b["commit"], os.path.dirname(b["path"]) + ("/" if "/" in b["path"] else "") + name, sha, name]))
PY
  dest=$DIR/src/$id/$name
  mkdir -p "$DIR/src/$id"
  if [[ -f "$dest" ]] && [[ "$(sha256sum "$dest" | cut -d' ' -f1)" == "$sha" ]]; then
    continue
  fi
  rm -f "$dest"
  url="https://raw.githubusercontent.com/$repo/$commit/$(python3 -I -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$path")"
  if ! curl --fail --silent --location --proto '=https' --max-filesize 30000000 --output "$dest" "$url"; then
    echo "fetch: $id: download failed ($url)" >&2
    rm -f "$dest"
    echo FAIL > "$DIR/.fetch-failed"
    continue
  fi
  got=$(sha256sum "$dest" | cut -d' ' -f1)
  if [[ "$got" != "$sha" ]]; then
    echo "fetch: $id: $name has sha256 $got, the manifest says $sha" >&2
    rm -f "$dest"
    echo FAIL > "$DIR/.fetch-failed"
  fi
done
if [[ -f "$DIR/.fetch-failed" ]]; then
  rm -f "$DIR/.fetch-failed"
  echo "fetch: failed (see above)" >&2
  exit 1
fi
echo "fetch: $DIR is complete and every hash matches"
