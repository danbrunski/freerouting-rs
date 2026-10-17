#!/usr/bin/env python3
"""The top functions of a callgrind profile, by exclusive instruction count, summed per function.

    callgrind_top.py <callgrind.out> [--top N] [--inclusive]

`callgrind_annotate` prints one row per (source file, function): code that the compiler inlined
into a function is charged to the file it came from, so one Rust function shows up as several
rows (the function itself, `btree/node.rs`, `iter/adapters/map.rs`, ...). This sums the rows of
each function, which is the number to compare. With --inclusive each function's count includes its
callees (the largest of its rows, since every row of a function carries the whole inclusive cost).
See docs/development/profiling.md.
"""
import collections
import re
import subprocess
import sys


def main(argv):
    args = [a for a in argv if not a.startswith("--") and not a.isdigit()]
    top = 5
    if "--top" in argv:
        top = int(argv[argv.index("--top") + 1])
    if not args:
        print(__doc__)
        return 2
    inclusive = "--inclusive" in argv
    cmd = ["callgrind_annotate", "--auto=no", "--threshold=100"]
    if inclusive:
        cmd.append("--inclusive=yes")
    out = subprocess.run(cmd + [args[0]], capture_output=True, text=True, check=True).stdout
    totals = collections.Counter()
    program = 0
    for line in out.splitlines():
        m = re.match(r"\s*([\d,]+) \(\s*[\d.]+%\)\s+(\S.*)$", line)
        if not m:
            continue
        n = int(m.group(1).replace(",", ""))
        rest = re.sub(r"\s+\[[^\]]*\]$", "", m.group(2))  # drop the object file
        if rest == "PROGRAM TOTALS":
            program = n
            continue
        function = rest.split(":", 1)[1] if ":" in rest else rest
        if inclusive:
            totals[function] = max(totals[function], n)
        else:
            totals[function] += n
    print(f"total {program:,} instructions")
    for function, n in totals.most_common(top):
        print(f"{n:>14,} {100 * n / program:5.1f}%  {function}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
