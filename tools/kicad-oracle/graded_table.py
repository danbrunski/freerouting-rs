"""Turns the output of tools/bench/k02-graded-runs.sh into a markdown table.

    python3 graded_table.py OUT_DIR

One row per board and engine (rust, java): the KiCad DRC violations by type that
the router caused (`drc_real_by_type`), unconnected items, vias, tracks, track
length, vias inside pads, copper below the board minimums, and the run time.
"""

import json
import sys
from pathlib import Path


def load(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def row(name, engine, directory):
    s = load(directory / "summary.json")
    if s is None:
        return f"| {name} | {engine} | no summary | | | | | | | |"
    if not s.get("graded"):
        return f"| {name} | {engine} | not graded: {s.get('reason', '?')} | | | | | | | {s.get('engine_seconds', '')} |"
    by_type = s.get("drc_real_by_type") or {}
    violations = ", ".join(f"{k} {v}" for k, v in sorted(by_type.items())) or "none"
    copper = s.get("copper") or {}
    below = copper.get("below_minimum") or {}
    below_total = sum(v for v in below.values() if isinstance(v, int))
    vip = load(directory / "via_in_pad.json") or {}
    return (
        f"| {name} | {engine} | {violations} | {s.get('unconnected_items')} / {s.get('input_unconnected_items')} "
        f"| {copper.get('vias')} | {copper.get('tracks')} | {copper.get('track_length_mm')} "
        f"| {vip.get('vias_in_pads', '?')} | {below_total} | {s.get('engine_seconds')} |"
    )


def main(out):
    out = Path(out)
    lines = [
        "| board | engine | KiCad DRC violations caused by the router | unconnected / input | vias | tracks | length mm | vias in pads | below board minimum | engine s |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for board in sorted(p for p in out.iterdir() if p.is_dir()):
        for engine in ("rust", "java"):
            if (board / engine).is_dir():
                lines.append(row(board.name, engine, board / engine))
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
