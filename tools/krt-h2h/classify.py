"""Sorts the connections one X01 arm leaves open and the other doesn't, by cause (X01.7).

    python3 -I classify.py [--out OUT.json] [--examples N] ROW:JAVA_RUN_DIR:KRT_RUN_DIR ...

Each RUN_DIR is a run.sh output directory ($X01_DIR/runs/ROW/ARM) whose
`grade/routed/drc.json` holds KiCad's unconnected items, `grade/work/classwidths.json`
the net-class widths and `grade/unrouted/<board>.kicad_pcb` the input board.
The pads come from `tools/kicad-oracle/kicad_steps.py pads --all` (KiCad's
python, KICAD_PYTHON), cached in the java run dir as `pads.json`.

Matching: an unconnected item is keyed by its net and its two ends ("REF.pad"
for a pad, the item kind for a track, via or zone end). Items with equal keys in
both arms are shared; what is left is paired per net (an open connection of the
same net in both arms, whose ends moved because the routed copper differs, is
shared too); the rest are java-only or krt-only.

Each failure gets one class, by precedence:
1. plane-net: the net has a copper zone on the board;
2. wide-at-fine-pitch: the net's class width exceeds the smaller dimension of an
   end pad that is fine-pitch by KRT's rule (plane_pad_tap.pad_is_fine_pitch at
   0486e799: a pad of the same footprint within 0.65 mm centre to centre, plus
   1 um tolerance, or a smaller dimension under 0.35 mm);
3. dense-escape: an end pad in a footprint with at least 32 pads and a pitch
   (smallest centre-to-centre distance) of at most 0.8 mm;
4. other.
Prints a per-board and per-class table and writes the details (with example
entries per class, for checking by hand) to OUT.json.
"""

import argparse
import collections
import json
import math
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import copper_stats  # noqa: E402

STEPS = os.path.join(HERE, "..", "kicad-oracle", "kicad_steps.py")
CLASSES = ("plane-net", "wide-at-fine-pitch", "dense-escape", "other")
FINE_PITCH_NEIGHBOR_MM = 0.65
FINE_PITCH_TOL_MM = 1e-3
FINE_PITCH_MIN_PAD_MM = 0.35
DENSE_MIN_PADS = 32
DENSE_MAX_PITCH_MM = 0.8
PAD_RE = re.compile(r"[Pp]ad (\S+) \[(.*?)\] of (\S+)")
NET_RE = re.compile(r"\[(.*?)\]")


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def zone_nets(board_text):
    """Net names of the board's copper zones (rule areas left out)."""
    nets = set()
    stack = []
    for tok in copper_stats.tokens(board_text):
        if tok == "(":
            stack.append([])
        elif tok == ")":
            done = stack.pop()
            if not stack:
                continue
            stack[-1].append(done)
            if len(stack) == 1 and done and done[0] == "zone":
                if any(isinstance(c, list) and c and c[0] == "keepout" for c in done):
                    continue
                name = copper_stats.field(done, "net_name")
                net = copper_stats.field(done, "net")
                value = (name or net or [""])[0]
                if value and not re.fullmatch(r"\d+", value):
                    nets.add(value)
        elif stack:
            stack[-1].append(tok[1] if isinstance(tok, tuple) else tok)
    return nets


def end_label(item):
    desc = item.get("description", "")
    m = PAD_RE.search(desc)
    if m:
        return f"{m.group(3)}.{m.group(1)}", m.group(2)
    m = NET_RE.search(desc)
    kind = desc.split(" ", 1)[0].lower() if desc else "?"
    return kind, m.group(1) if m else ""


def failures(drc):
    out = []
    for v in drc.get("unconnected_items", []):
        ends = [end_label(i) for i in v.get("items", [])]
        nets = [n for _, n in ends if n]
        net = nets[0] if nets else ""
        labels = tuple(sorted(lbl for lbl, _ in ends))
        out.append({"net": net, "ends": labels,
                    "pos": [i.get("pos") for i in v.get("items", [])]})
    return out


def match(java, krt):
    """(shared, java_only, krt_only): exact keys first, then per net."""
    key = lambda f: (f["net"], f["ends"])
    jk = collections.defaultdict(list)
    kk = collections.defaultdict(list)
    for f in java:
        jk[key(f)].append(f)
    for f in krt:
        kk[key(f)].append(f)
    shared, jrest, krest = [], [], []
    for k in sorted(set(jk) | set(kk)):
        a, b = jk.get(k, []), kk.get(k, [])
        n = min(len(a), len(b))
        shared += [dict(f, match="exact") for f in a[:n]]
        jrest += a[n:]
        krest += b[n:]
    jn = collections.defaultdict(list)
    kn = collections.defaultdict(list)
    for f in jrest:
        jn[f["net"]].append(f)
    for f in krest:
        kn[f["net"]].append(f)
    java_only, krt_only = [], []
    for net in sorted(set(jn) | set(kn)):
        a, b = jn.get(net, []), kn.get(net, [])
        n = min(len(a), len(b))
        shared += [dict(f, match="same net") for f in a[:n]]
        java_only += a[n:]
        krt_only += b[n:]
    return shared, java_only, krt_only


class Board:
    def __init__(self, board_path, pads_json, class_widths):
        with open(board_path, encoding="utf-8") as f:
            self.zone_nets = zone_nets(f.read())
        self.class_widths = class_widths
        self.pads = {}
        self.by_fp = collections.defaultdict(list)
        for p in load(pads_json):
            pad = {
                "ref": p["footprint"], "number": p["number"], "net": p["net"],
                "at": (p["at"][0] / 1000.0, p["at"][1] / 1000.0),  # um -> mm
                "size": (p["size"][0] / 1000.0, p["size"][1] / 1000.0),
            }
            self.pads.setdefault(f"{p['footprint']}.{p['number']}", pad)
            self.by_fp[p["footprint"]].append(pad)
        self._pitch = {}

    def pitch(self, ref):
        if ref not in self._pitch:
            pts = [p["at"] for p in self.by_fp.get(ref, [])]
            best = math.inf
            for i in range(len(pts)):
                for j in range(i + 1, len(pts)):
                    d = math.dist(pts[i], pts[j])
                    if 0.0 < d < best:
                        best = d
            self._pitch[ref] = best
        return self._pitch[ref]

    def fine_pitch(self, pad):
        if min(pad["size"]) < FINE_PITCH_MIN_PAD_MM:
            return True
        for other in self.by_fp.get(pad["ref"], []):
            if other["number"] == pad["number"]:
                continue
            d = math.dist(other["at"], pad["at"])
            if 0.0 < d <= FINE_PITCH_NEIGHBOR_MM + FINE_PITCH_TOL_MM:
                return True
        return False

    def classify(self, failure):
        net = failure["net"]
        if net in self.zone_nets:
            return "plane-net", {"zone_net": net}
        end_pads = [self.pads[e] for e in failure["ends"] if e in self.pads]
        width = self.class_widths.get(net)
        if width:
            for pad in end_pads:
                if width > min(pad["size"]) + 1e-9 and self.fine_pitch(pad):
                    return "wide-at-fine-pitch", {
                        "class_width_mm": width, "pad": f"{pad['ref']}.{pad['number']}",
                        "pad_size_mm": [round(s, 4) for s in pad["size"]],
                        "footprint_pitch_mm": round(self.pitch(pad["ref"]), 4)}
        for pad in end_pads:
            count = len(self.by_fp.get(pad["ref"], []))
            pitch = self.pitch(pad["ref"])
            if count >= DENSE_MIN_PADS and pitch <= DENSE_MAX_PITCH_MM + 1e-9:
                return "dense-escape", {"pad": f"{pad['ref']}.{pad['number']}",
                                        "footprint_pads": count, "pitch_mm": round(pitch, 4)}
        return "other", {"class_width_mm": width,
                         "end_pads": [f"{p['ref']}.{p['number']}" for p in end_pads]}


def pads_json(java_dir, board_path):
    out = os.path.join(java_dir, "pads.json")
    if not os.path.exists(out):
        py = os.environ.get("KICAD_PYTHON", "python3")
        subprocess.run([py, STEPS, "pads", board_path, out, "--all"], check=True,
                       capture_output=True, timeout=600)
    return out


def run_board(row, java_dir, krt_dir, examples):
    jg, kg = os.path.join(java_dir, "grade"), os.path.join(krt_dir, "grade")
    board_path = next(os.path.join(jg, "unrouted", f) for f in sorted(os.listdir(os.path.join(jg, "unrouted")))
                      if f.endswith(".kicad_pcb"))
    board = Board(board_path, pads_json(java_dir, board_path),
                  load(os.path.join(jg, "work", "classwidths.json")))
    java = failures(load(os.path.join(jg, "routed", "drc.json")))
    krt = failures(load(os.path.join(kg, "routed", "drc.json")))
    shared, java_only, krt_only = match(java, krt)
    result = {"java_unconnected": len(java), "krt_unconnected": len(krt),
              "shared": len(shared), "java_only": len(java_only), "krt_only": len(krt_only)}
    for name, group in (("java_only", java_only), ("krt_only", krt_only), ("shared", shared)):
        counts = collections.Counter()
        ex = collections.defaultdict(list)
        for f in group:
            cls, why = board.classify(f)
            counts[cls] += 1
            if len(ex[cls]) < examples:
                ex[cls].append({"net": f["net"], "ends": f["ends"], "pos": f["pos"], "why": why,
                                **({"match": f["match"]} if "match" in f else {})})
        result[f"{name}_by_class"] = {c: counts.get(c, 0) for c in CLASSES}
        result[f"{name}_examples"] = {c: ex[c] for c in CLASSES if ex[c]}
    result["zone_nets"] = sorted(board.zone_nets)
    return result


def table(results):
    lines = ["| Board | java open | krt open | shared | java-only | krt-only | java-only by class "
             "(plane / wide-fine / dense / other) | krt-only by class |",
             "|---|---|---|---|---|---|---|---|"]
    total = collections.Counter()
    ktotal = collections.Counter()
    for row, r in results.items():
        j, k = r["java_only_by_class"], r["krt_only_by_class"]
        total.update(j)
        ktotal.update(k)
        lines.append(f"| {row} | {r['java_unconnected']} | {r['krt_unconnected']} | {r['shared']} | "
                     f"{r['java_only']} | {r['krt_only']} | " + " / ".join(str(j[c]) for c in CLASSES)
                     + " | " + " / ".join(str(k[c]) for c in CLASSES) + " |")
    lines.append("| **sum** | | | | " + str(sum(total.values())) + " | " + str(sum(ktotal.values()))
                 + " | " + " / ".join(str(total[c]) for c in CLASSES) + " | "
                 + " / ".join(str(ktotal[c]) for c in CLASSES) + " |")
    return "\n".join(lines), total


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out")
    ap.add_argument("--examples", type=int, default=3)
    ap.add_argument("boards", nargs="+", help="ROW:JAVA_RUN_DIR:KRT_RUN_DIR")
    args = ap.parse_args(argv)
    results = {}
    for spec in args.boards:
        row, java_dir, krt_dir = spec.split(":", 2)
        results[row] = run_board(row, java_dir, krt_dir, args.examples)
    text, total = table(results)
    n = sum(total.values())
    shares = {c: (round(total[c] / n, 4) if n else 0.0) for c in CLASSES}
    out = {"boards": results, "java_only_total": n, "java_only_by_class_total": dict(total),
           "java_only_share": shares}
    print(text)
    print("java-only share by class:", json.dumps(shares))
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump(out, f, indent=2)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
