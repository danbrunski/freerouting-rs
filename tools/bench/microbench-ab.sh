#!/usr/bin/env bash
# Wall-time A/B of two `freerouting-dev` builds on the microbenchmark kernels (P05).
#
#   tools/bench/microbench-ab.sh <dev-A> <dev-B> [options] <board.dsn>...
#   options: --kernel <a,b>   kernels to time (default overlap,drc,maze,pull_tight,complete_shape)
#            --runs <n>       runs per microbench call (default 9)
#            --rounds <n>     alternating rounds per board (default 3)
#            --out <dir>      where the per-call JSON files go (default: a temporary dir)
#
# The two builds alternate, A B A B ..., so a change in the machine's load hits
# both. Per board and kernel it prints the minimum time of each build over all
# rounds, the ratio A/B (above 1 means B is faster) and the highest 1-minute load
# average seen, and it fails if the two builds' work counts differ: a change that
# claims to keep results identical must not move them. Wall time only counts on a
# quiet machine (load average at most 1.0, docs/development/profiling.md); the
# script prints the load so a number taken under load can be labelled as such.
set -euo pipefail

[ $# -ge 3 ] || { sed -n '2,16p' "$0"; exit 2; }
dev_a="$1"
dev_b="$2"
shift 2
kernels="overlap,drc,maze,pull_tight,complete_shape"
runs=9
rounds=3
out=""
boards=()
while [ $# -gt 0 ]; do
    case "$1" in
    --kernel) kernels="$2"; shift ;;
    --runs) runs="$2"; shift ;;
    --rounds) rounds="$2"; shift ;;
    --out) out="$2"; shift ;;
    -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
    *) boards+=("$1") ;;
    esac
    shift
done
for bin in "$dev_a" "$dev_b"; do
    [ -x "$bin" ] || { echo "microbench-ab.sh: $bin is not executable" >&2; exit 2; }
done
if [ -z "$out" ]; then
    out="$(mktemp -d "${TMPDIR:-/tmp}/microbench-ab.XXXXXX")"
    trap 'rm -rf "$out"' EXIT
fi
mkdir -p "$out"

for board in "${boards[@]}"; do
    stem="$(basename "$board" .dsn)"
    for round in $(seq 1 "$rounds"); do
        for arm in a b; do
            bin="$dev_a"
            [ "$arm" = b ] && bin="$dev_b"
            "$bin" microbench "$board" --kernel "$kernels" --runs "$runs" \
                --json "$out/$stem.$arm.$round.json" > /dev/null
        done
    done
done

OUT="$out" python3 -I - "${boards[@]}" << 'EOF'
import glob, json, os, sys

out = os.environ["OUT"]
failed = False
print(f"{'board':<36} {'kernel':<15} {'A min ms':>10} {'B min ms':>10} {'A/B':>7} {'max load':>9}")
for board in sys.argv[1:]:
    stem = os.path.basename(board)[:-4] if board.endswith(".dsn") else os.path.basename(board)
    best = {}
    work = {}
    load = 0.0
    for arm in ("a", "b"):
        for path in sorted(glob.glob(f"{out}/{stem}.{arm}.*.json")):
            record = json.load(open(path))
            load = max(load, record["load_average"])
            for k in record["kernels"]:
                name = k["kernel"]
                key = (arm, name)
                best[key] = min(best.get(key, float("inf")), k["min_ms"])
                w = json.dumps(k["work"], sort_keys=True)
                if work.setdefault(name, w) != w:
                    print(f"{stem}: work of {name} differs between the builds", file=sys.stderr)
                    failed = True
    for name in sorted({n for (_, n) in best}):
        a, b = best[("a", name)], best[("b", name)]
        print(f"{stem:<36} {name:<15} {a:>10.3f} {b:>10.3f} {a / b:>7.3f} {load:>9.2f}")
sys.exit(1 if failed else 0)
EOF
