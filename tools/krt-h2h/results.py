"""Collects X01's records and classes and applies the registered decision table (X01.8).

    python3 -I results.py --runs $X01_DIR/runs --classes CLASSES.json --prereg PREREG.json \
        [--out results/x01-demos.json] [--demos-only]

Reads `runs/ROW/ARM/record.json` for every registered row and the arms java and krt,
prints one markdown table per board (unrouted, drc_real, vias, length, vias in
pads, time) and the decision, and writes the JSON. With --demos-only the
canary-pwr row is left out of the file (its numbers are an owner board's; they
go in the done note only), but it still counts in the decision as the
registration says: until canary-23 grades it, it is not graded for either arm
and is no win for krt.
"""

import argparse
import hashlib
import json
import os
import sys

ARMS = ("java", "krt")
CANARY = "canary-pwr-9511d9a"
COLUMNS = ("kicad_unconnected", "drc_real", "vias", "length_mm", "via_in_pad", "below_minimum",
           "under_class_width_mm", "wall_seconds", "peak_rss_mb", "load_before", "load_after",
           "engine_exit")


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def record(runs, row, arm):
    path = os.path.join(runs, row, arm, "record.json")
    if not os.path.exists(path):
        return {"board": row, "arm": arm, "graded": False, "reason": "not run"}
    return load(path)


def cell(rec, key):
    if not rec.get("graded") and key not in ("wall_seconds", "engine_exit", "peak_rss_mb",
                                               "load_before", "load_after", "vias", "length_mm"):
        return "not graded"
    value = rec.get(key)
    return "-" if value is None else str(value)


def tables(rows, recs):
    out = []
    for row in rows:
        out.append(f"**{row}**\n")
        out.append("| arm | unrouted | drc_real | vias | length mm | vias in pads | below min | "
                   "under class width mm | time s | peak RSS MB | load before/after | exit |")
        out.append("|---|---|---|---|---|---|---|---|---|---|---|---|")
        for arm in ARMS:
            r = recs[row][arm]
            out.append("| " + " | ".join([arm] + [cell(r, k) for k in COLUMNS[:9]] +
                                         [f"{r.get('load_before', '-')}/{r.get('load_after', '-')}",
                                          str(r.get("engine_exit", "-"))]) + " |")
        reasons = [f"{a}: {recs[row][a].get('reason')}" for a in ARMS if not recs[row][a].get("graded")]
        if reasons:
            out.append("\nNot graded: " + "; ".join(reasons))
        out.append("")
    return "\n".join(out)


def decide(rows, recs, classes):
    """The registered decision table, in its order."""
    wins = 0
    more_drc = []
    for row in rows:
        j, k = recs[row]["java"], recs[row]["krt"]
        if not k.get("graded"):
            continue  # a board krt could not have graded is a failure of krt: no win
        if not j.get("graded"):
            wins += 1  # a failure of java's arm
            continue
        if k["kicad_unconnected"] < j["kicad_unconnected"]:
            wins += 1
        if k["drc_real"] > j["drc_real"]:
            more_drc.append(row)
    share = classes.get("java_only_share", {})
    fired = []
    grid = wins >= 4 and not more_drc
    if grid:
        fired.append("grid-first")
    for rule, cls in (("planes-first", "plane-net"), ("neck-first", "wide-at-fine-pitch"),
                      ("escape-first", "dense-escape")):
        if share.get(cls, 0.0) >= 0.30:
            fired.append(rule)
    return {
        "krt_fewer_unrouted_boards": wins,
        "boards_where_krt_has_more_drc_real": more_drc,
        "grid_first": grid,
        "java_only_total": classes.get("java_only_total"),
        "java_only_share": share,
        "fired": fired,
        "outcome": fired if fired else ["default (D9 order stands)"],
    }


def relative(value, root):
    """Paths under X01_DIR written as $X01_DIR/..., so the file names no home directory."""
    if isinstance(value, dict):
        return {k: relative(v, root) for k, v in value.items()}
    if isinstance(value, list):
        return [relative(v, root) for v in value]
    if isinstance(value, str):
        return value.replace(root, "$X01_DIR")
    return value


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", required=True)
    ap.add_argument("--classes", required=True)
    ap.add_argument("--prereg", required=True)
    ap.add_argument("--out")
    ap.add_argument("--demos-only", action="store_true")
    args = ap.parse_args(argv)
    prereg_bytes = open(args.prereg, "rb").read()
    prereg = json.loads(prereg_bytes)
    rows = list(prereg["boards"]["files"])
    recs = {row: {arm: record(args.runs, row, arm) for arm in ARMS} for row in rows}
    classes = load(args.classes)
    decision = decide(rows, recs, classes)
    shown = [r for r in rows if not (args.demos_only and r == CANARY)]
    print(tables(rows, recs))
    print("decision:", json.dumps(decision, indent=1))
    if args.out:
        result = {
            "format": "freerouting-x01-results-1",
            "prereg": os.path.relpath(args.prereg),
            "prereg_sha256": hashlib.sha256(prereg_bytes).hexdigest(),
            "boards": {row: recs[row] for row in shown},
            "classes": {row: v for row, v in classes.get("boards", {}).items() if row in shown},
            "java_only_by_class_total": classes.get("java_only_by_class_total"),
            "decision": decision,
            "omitted": [CANARY] if args.demos_only else [],
        }
        root = os.path.dirname(os.path.abspath(args.runs))
        result = relative(result, root)
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump(result, f, indent=2)
            f.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
