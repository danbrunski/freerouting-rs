#!/usr/bin/env bash
# Usage: tools/krt/vendor-diff.sh [--check]
# Regenerates crates/krt-grid-router/VENDORED.diff: `git diff --no-index` of
# every vendored file against KiCadRoutingTools' version at the pin, in
# file-name order. The KRT file is read with `git show <pin>:<path>`; the KRT
# repository is never written to. With --check, fails when the committed
# VENDORED.diff differs from the regenerated one instead of rewriting it.
# KRT_REPO overrides the repository location (default ../KiCadRoutingTools,
# then ~/code/KiCadRoutingTools).
set -eu
pin=0486e799
root="$(cd "$(dirname "$0")/../.." && pwd)"
crate="$root/crates/krt-grid-router"
krt="${KRT_REPO:-}"
if [ -z "$krt" ]; then
  for c in "$root/../KiCadRoutingTools" "$HOME/code/KiCadRoutingTools"; do
    [ -d "$c/.git" ] && { krt="$c"; break; }
  done
fi
[ -n "$krt" ] || { echo "KiCadRoutingTools repository not found (set KRT_REPO)" >&2; exit 2; }
git -C "$krt" cat-file -e "$pin^{commit}" || { echo "commit $pin not in $krt" >&2; exit 2; }
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
# (KRT path relative to rust_router/, vendored path relative to the crate), in file-name order.
pairs="Cargo.toml:Cargo.toml
src/dubins.rs:src/dubins.rs
src/lib.rs:src/lib.rs
src/obstacle_map.rs:src/obstacle_map.rs
src/pose_router.rs:src/pose_router.rs
src/router.rs:src/router.rs
src/types.rs:src/types.rs"
out="$tmp/out.diff"
: > "$out"
while IFS=: read -r k v; do
  mkdir -p "$tmp/a/rust_router/$(dirname "$k")" "$tmp/b/rust_router/$(dirname "$k")"
  git -C "$krt" show "$pin:rust_router/$k" > "$tmp/a/rust_router/$k"
  cp "$crate/$v" "$tmp/b/rust_router/$k"
  (cd "$tmp" && git diff --no-index --no-color --no-prefix -- "a/rust_router/$k" "b/rust_router/$k" || true) >> "$out"
done <<< "$pairs"
if [ "${1:-}" = "--check" ]; then
  cmp -s "$out" "$crate/VENDORED.diff" || { echo "VENDORED.diff is stale; run tools/krt/vendor-diff.sh" >&2; exit 1; }
  echo "VENDORED.diff ok"
else
  cp "$out" "$crate/VENDORED.diff"
  echo "wrote $crate/VENDORED.diff ($(grep -c '^[+-][^+-]' "$out") changed lines)"
fi
