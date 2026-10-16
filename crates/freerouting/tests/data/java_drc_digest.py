#!/usr/bin/env python3
"""Writes java_drc_digest.tsv from Java DRC reports.

The reports come from Java Freerouting (cda540f) run on every fixture:

    FreeroutingCli -de fixtures/<board>.dsn -drc <reports>/load/<board>.json
    FreeroutingCli -de fixtures/<board>.dsn fixtures/reference/route/<board>.ses \\
        -drc <reports>/route/<board>.json

(the classpath is the one tools/java-reference/run.sh builds). Usage:

    java_drc_digest.py <reports> > java_drc_digest.tsv

One line per mode, fixture and violation type, tab separated:

    <mode> <fixture> <type> <count> <value>

For clearance, holeClearance and via_dangling the value is the FNV-1a
64-bit hash of the violations in report order, each written as its
description followed by `<uuid>@<x>,<y>` per item, tab separated, and a
newline. For unconnectedItems it is the hash of the sorted item id sets,
one per line (Java picks the representatives in hash order). For
track_dangling it is the sorted list of trace ids. The `scores` line holds
the quality and optimizer score. The zone island findings are left out:
the port decomposes the fill differently (docs/DIVERGENCES.md).
"""

import json
import os
import sys

HASHED = ["clearance", "holeClearance", "via_dangling"]


def fnv(text):
    h = 0xCBF29CE484222325
    for b in text.encode("utf-8"):
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h


def number(v):
    return "null" if v is None else repr(float(v))


def line(v):
    parts = [v["description"]]
    for item in v["items"]:
        parts.append("%s@%s,%s" % (item["uuid"], number(item["pos"]["x"]), number(item["pos"]["y"])))
    return "\t".join(parts) + "\n"


def main():
    root = sys.argv[1]
    out = []
    for mode in ["load", "route"]:
        base = os.path.join(root, mode)
        reports = []
        for directory, _, files in os.walk(base):
            for f in files:
                if f.endswith(".json"):
                    reports.append(os.path.relpath(os.path.join(directory, f), base))
        for rel in sorted(reports):
            with open(os.path.join(base, rel), encoding="utf-8") as fh:
                report = json.load(fh)
            fixture = rel[: -len(".json")] + ".dsn"
            for t in HASHED:
                vs = [v for v in report["violations"] if v["type"] == t]
                out.append((mode, fixture, t, len(vs), fnv("".join(line(v) for v in vs))))
            groups = sorted(
                ",".join(sorted((i["uuid"] for i in v["items"]), key=int))
                for v in report["unconnectedItems"]
                if v["type"] == "unconnectedItems"
            )
            out.append((mode, fixture, "unconnectedItems", len(groups), fnv("\n".join(groups))))
            dangling = sorted(
                (v["items"][0]["uuid"] for v in report["violations"] if v["type"] == "track_dangling"),
                key=int,
            )
            out.append((mode, fixture, "track_dangling", len(dangling), ",".join(dangling)))
            out.append(
                (
                    mode,
                    fixture,
                    "scores",
                    number(report.get("qualityScore")),
                    number(report.get("optimizerScore")),
                )
            )
    for row in out:
        print("\t".join(str(x) for x in row))


if __name__ == "__main__":
    main()
