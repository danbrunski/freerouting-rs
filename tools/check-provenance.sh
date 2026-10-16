#!/usr/bin/env bash
# Usage: tools/check-provenance.sh [root]   (root defaults to the repo root)
# Fails when a file with a "Portions ported from" line lacks an SPDX header or
# a ledger row, or when a ledger row names a missing file.
set -u
root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
ledger="$root/docs/PROVENANCE.md"
fail=0
[ -f "$ledger" ] || { echo "missing $ledger"; exit 1; }
rows=$(grep -E '^\| *`' "$ledger" | sed -E 's/^\| *`([^`]*)`.*/\1/')
while IFS= read -r f; do
  rel="${f#"$root"/}"
  case "$rel" in docs/PROVENANCE.md|tools/check-provenance.sh|THIRD_PARTY_NOTICES.md|docs/roadmap/*|.claude/*) continue;; esac
  if ! grep -q 'SPDX-License-Identifier: GPL-3.0-only AND MIT' "$f"; then
    echo "$rel: 'Portions ported from' but no SPDX header"; fail=1
  fi
  if ! printf '%s\n' "$rows" | grep -qxF "$rel"; then
    echo "$rel: 'Portions ported from' but no ledger row"; fail=1
  fi
done < <(grep -rIl 'Portions ported from' "$root" \
  --exclude-dir=.git --exclude-dir=target --exclude-dir=node_modules 2>/dev/null)
while IFS= read -r r; do
  [ -z "$r" ] && continue
  [ -f "$root/$r" ] || { echo "ledger row names missing file: $r"; fail=1; }
done <<< "$rows"
[ $fail -eq 0 ] && echo "provenance ok"
exit $fail
