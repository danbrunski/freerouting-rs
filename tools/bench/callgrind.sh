#!/usr/bin/env bash
# Callgrind instruction-count baselines (P01.4).
#
#   tools/bench/callgrind.sh                 write tools/bench/callgrind-baseline.json
#   tools/bench/callgrind.sh --compare       count again; report each count in percent
#                                            against the baseline (exit 1 if one grew
#                                            by more than --threshold percent)
#   options: --only <name,name>   run only these entries (kernel/<name>, route/<board>)
#            --threshold <pct>    growth that fails --compare (default 1.0)
#            --baseline <file>    the baseline file (default tools/bench/callgrind-baseline.json)
#            --out <file>         where a counting run writes its counts (default: the baseline
#                                 for a plain run; nothing for --compare unless given)
#            --no-build           use the binaries already in <bin-dir>
#            --bin-dir <dir>      where freerouting and freerouting-dev are (default
#                                 $CARGO_TARGET_DIR/release, else target/release)
#
# Instruction counts are deterministic, so two runs of the same build give the
# same numbers and a change shows even on a busy machine, where wall time does
# not. Wall time stays the number that counts (docs/development/profiling.md).
#
# What is counted:
#   kernel/<name>  the timed loop of one `freerouting-dev microbench` kernel
#                  (overlap, complete_shape, maze, pull_tight, drc) on
#                  Issue433-my-board, with valgrind --toggle-collect on the
#                  kernel's `timed_*` function, so loading and setup are left out;
#   route/<board>  the autorouter (`run_batch_loop`: fanout and 3 passes, optimizer
#                  off, one thread) of three boards from
#                  tools/bench/sets/connection-parity.txt, run through
#                  `examples/route_board` (the same `run_pipeline` the CLI calls).
#
# Every run is pinned to one CPU with taskset: the DRC and the optimizer size
# their thread pools by the CPUs they may use, and callgrind counts only the
# thread a toggle is on. Hash maps in the engine use a fixed hasher
# (`util::HashMap`), so the heap layout, and with it glibc's malloc, repeats.
# Two runs of one build give the same counts; the route entries toggle on
# `run_batch_loop` so the deadline watcher's thread, which wakes on the clock,
# is not counted.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

mode=write
threshold=1.0
baseline=tools/bench/callgrind-baseline.json
out=""
only=""
build=1
bin_dir="${CARGO_TARGET_DIR:-target}/release"

while [ $# -gt 0 ]; do
    case "$1" in
    --compare) mode=compare ;;
    --threshold) threshold="$2"; shift ;;
    --baseline) baseline="$2"; shift ;;
    --out) out="$2"; shift ;;
    --only) only="$2"; shift ;;
    --no-build) build=0 ;;
    --bin-dir) bin_dir="$2"; shift ;;
    -h | --help) sed -n '2,32p' "$0"; exit 0 ;;
    *) echo "callgrind.sh: unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done

command -v valgrind > /dev/null || { echo "callgrind.sh: valgrind is not installed" >&2; exit 2; }

if [ "$build" = 1 ]; then
    cargo build --release -p freerouting-dev
    cargo build --release -p freerouting --example route_board
fi
route="$bin_dir/examples/route_board"
dev="$bin_dir/freerouting-dev"
for bin in "$route" "$dev"; do
    [ -x "$bin" ] || { echo "callgrind.sh: $bin not found (build it or pass --bin-dir)" >&2; exit 2; }
done

kernel_board=fixtures/Issue433-my-board.dsn
kernels="overlap complete_shape maze pull_tight drc"
route_boards="fixtures/Issue143-rpi_splitter.dsn fixtures/Issue508-SMD-routing-issue-demo.dsn fixtures/Issue433-my-board.dsn"

scratch="$(mktemp -d "${TMPDIR:-/tmp}/callgrind.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
counts="$scratch/counts.tsv"
: > "$counts"

pin=()
if command -v taskset > /dev/null; then pin=(taskset -c 0); fi

wanted() { [ -z "$only" ] || [[ ",$only," == *",$1,"* ]]; }

# Runs valgrind with the given extra options and the command; prints "Collected".
collect() {
    local name="$1"
    shift
    local log="$scratch/log.txt"
    "${pin[@]}" valgrind --tool=callgrind --callgrind-out-file="$scratch/cg.out" "$@" \
        < /dev/null > /dev/null 2> "$log" || {
        echo "callgrind.sh: $name failed:" >&2
        tail -5 "$log" >&2
        exit 1
    }
    local n
    n="$(sed -n 's/^==[0-9]*== Collected : \([0-9]*\).*/\1/p' "$log")"
    [ -n "$n" ] || { echo "callgrind.sh: no count for $name" >&2; exit 1; }
    printf '%s\t%s\n' "$name" "$n" >> "$counts"
    echo "  $name: $n" >&2
}

for k in $kernels; do
    wanted "kernel/$k" || continue
    collect "kernel/$k" --toggle-collect="*microbench::timed_$k*" \
        "$dev" microbench "$kernel_board" --kernel "$k" --runs 1
done
for board in $route_boards; do
    stem="$(basename "$board" .dsn)"
    wanted "route/$stem" || continue
    collect "route/$stem" --toggle-collect='*run_batch_loop*' \
        "$route" "$board" "$scratch/$stem.ses" --max-threads 1 --no-bounds -- \
        --router.autorouter.max_passes=3 --router.optimizer.enabled=false
done

sha="$(git rev-parse --short HEAD 2> /dev/null || echo unknown)"
dirty="$(git status --porcelain --untracked-files=no 2> /dev/null | wc -l | tr -d ' ')"
vg="$(valgrind --version)"

COUNTS="$counts" MODE="$mode" BASELINE="$baseline" OUT="$out" THRESHOLD="$threshold" \
    SHA="$sha" DIRTY="$dirty" VG="$vg" ONLY="$only" python3 -I - << 'EOF'
import json, os, sys

counts = {}
for line in open(os.environ["COUNTS"]):
    name, n = line.rstrip("\n").split("\t")
    counts[name] = int(n)

mode = os.environ["MODE"]
baseline = os.environ["BASELINE"]
out = os.environ["OUT"] or (baseline if mode == "write" else "")

if mode == "write" and os.environ["ONLY"] and os.path.exists(out):
    # A partial run updates only its entries.
    merged = json.load(open(out))["counts"]
    merged.update(counts)
    counts = merged

record = {
    "schema": 1,
    "what": "callgrind instruction counts (P01.4); see tools/bench/callgrind.sh",
    "git": os.environ["SHA"],
    "tracked_files_modified": int(os.environ["DIRTY"]),
    "valgrind": os.environ["VG"],
    "counts": dict(sorted(counts.items())),
}
if out:
    with open(out, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    print(f"wrote {out}", file=sys.stderr)

if mode == "compare":
    base = json.load(open(baseline))["counts"]
    threshold = float(os.environ["THRESHOLD"])
    worst = 0.0
    print(f"{'entry':<40} {'baseline':>14} {'now':>14} {'change':>9}")
    for name in sorted(counts):
        now = counts[name]
        if name not in base:
            print(f"{name:<40} {'-':>14} {now:>14}       new")
            continue
        was = base[name]
        pct = (now - was) * 100.0 / was if was else float("inf")
        worst = max(worst, pct)
        flag = "  REGRESSED" if pct > threshold else ""
        print(f"{name:<40} {was:>14} {now:>14} {pct:>+8.2f}%{flag}")
    for name in sorted(set(base) - set(counts)):
        if not os.environ["ONLY"]:
            print(f"{name:<40} {base[name]:>14} {'-':>14}   missing")
    if worst > threshold:
        sys.exit(1)
EOF
