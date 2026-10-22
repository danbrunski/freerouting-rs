#!/usr/bin/env python3
"""K15.14's measurement helper (docs/roadmap/prereg/K15.14.json). Plain python3, no pcbnew.

    python3 -I tools/bench/k15_all_pours.py pours BOARD.kicad_pcb
    python3 -I tools/bench/k15_all_pours.py prepare PREREG
    python3 -I tools/bench/k15_all_pours.py run PREREG [--dry-run]
    python3 -I tools/bench/k15_all_pours.py collect PREREG OUT

`pours` lists the board's signal-layer pours, the eligibility rule of the registration's corpus
rows: a board-level `(zone ...)` that is not a rule area (no `(keepout ...)`), has a net, and lies
on a copper layer whose `(layers ...)` type is not `power` or `jumper` (the KiCad reader's
`CopperLayer::layer_type`: `signal`, `mixed` and an unknown word are signal layers, the layers
`router.plane_obstacle_all_pours` lists pours on). Prints one JSON object: the copper layers with
their types, and per pour its net and signal layers.

`prepare` makes each row's board where `boards.rows.<row>.input` says: the source's .kicad_pcb
(`git archive` of `source.commit` for a `source.git` row, else `source.dir`) without its tracks and
vias (`KICAD_PYTHON tools/kicad-oracle/kicad_steps.py strip`, default python3), beside the source's
own .kicad_pro and .kicad_dru, no .kicad_prl; the strip count goes to `<boards>/<row>.strip.json`.
A board that already hashes as `boards.files` says is left alone. KiCad gives an old-format board
new item UUIDs on every load (here the sources at version 20221018 or older), so such a row made
again does not hash as registered; it prints MISMATCH and the command exits 1.

`run` executes the registration's `commands` verbatim, row by row and arm by arm in file order,
each with `bash -c` from the main checkout (the registration's ROOT), waiting while `pgrep -x
kicad-cli` finds one. A row whose board file is missing is skipped with a line on stderr (the set
file's rule). Each call's output goes to `<outdir>.log`, ending in `EXIT <status>`.

`collect` reads `<OUT>/<row>/<arm>/` for every arm the row's `commands` list and writes `<OUT>/records.json` (the gate's records,
`freerouting-gate-records-1`) and `<OUT>/table.md`, and prints the table. Besides the summary.json
metrics of tools/kicad-reader/plugin-input-ab.sh it takes, per arm:
  - zone_zone_unconnected: KiCad's unconnected items on the routed board whose items are all zones
    (routed/drc.json), and zone_zone_zones, the distinct (net, layer) zones they name;
  - router_incomplete and connections: the result manifest's
    `board_statistics.connections.incomplete_count` and `maximum_count`
    (work/<name>.result.json); the row's `connections`, which the gate's default noise band
    reads, is the largest of its arms';
  - kicad_less_router: kicad_unconnected - router_incomplete;
  - final_state: the manifest's `final_state` (COMPLETED, or TIMED_OUT when the router's own
    `--timeout` stopped it);
  - pour_guard_pours: N of the route log's "Pour guard: N pour(s) of every net class" line, or 0.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "kicad-oracle"))
import oracle_lib  # noqa: E402

NOT_SIGNAL = ("power", "jumper")
POUR_GUARD = re.compile(r"Pour guard: (\d+) pour\(s\) of every net class")
ZONE_ITEM = re.compile(r"Zone (?:'[^']*' )?\[([^\]]*)\] on (\S+)")


def load(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def copper_layers(text, root):
    """name -> type of the board's copper layers (canonical names, as zones reference them)."""
    layers = next((s for k, s, _ in oracle_lib.child_scopes(text, root) if k == "layers"), None)
    out = {}
    if layers is None:
        return out
    for entry in oracle_lib._parse_sexpr(text[layers:oracle_lib.scope_end(text, layers)])[1:]:
        if isinstance(entry, list) and len(entry) >= 3 and str(entry[1]).endswith(".Cu"):
            out[entry[1]] = entry[2]
    return out


def zone_net(node):
    """The zone's net name: `(net "name")` (KiCad 10) or `(net_name "name")` (KiCad 6-9)."""
    name = oracle_lib._field(node, "net_name")
    if name is not None and len(name) > 1:
        return name[1]
    net = oracle_lib._field(node, "net")
    if net is not None and len(net) > 1 and not re.fullmatch(r"\d+", net[1]):
        return net[1]
    return ""


def zone_layers(node, copper):
    single = oracle_lib._field(node, "layer")
    many = oracle_lib._field(node, "layers")
    names = [single[1]] if single else list(many[1:]) if many else []
    out = []
    for name in names:
        if name in ("*.Cu", "F&B.Cu"):
            wanted = copper if name == "*.Cu" else [n for n in copper if n in ("F.Cu", "B.Cu")]
            out.extend(n for n in wanted if n not in out)
        elif name in copper and name not in out:
            out.append(name)
    return out


def signal_pours(path):
    text = Path(path).read_text(encoding="utf-8")
    root = text.index("(")
    copper = copper_layers(text, root)
    pours = []
    for keyword, start, end in oracle_lib.child_scopes(text, root):
        if keyword != "zone":
            continue
        node = oracle_lib._parse_sexpr(text[start:end])
        net = zone_net(node)
        if oracle_lib._field(node, "keepout") is not None or not net:
            continue
        layers = [n for n in zone_layers(node, list(copper)) if copper[n] not in NOT_SIGNAL]
        if layers:
            pours.append({"net": net, "layers": layers})
    return {"copper_layers": copper, "signal_pours": pours}


def sha256(path):
    try:
        return hashlib.sha256(Path(path).read_bytes()).hexdigest()
    except OSError:
        return None


def prepare(prereg_path):
    """Makes each row's board as `boards.prepare` says; a row whose board already hashes as
    registered is left alone. Exit 1 when a made board does not hash as registered."""
    prereg = load(prereg_path)
    root = Path(prereg["run"]["root"])
    kicad_python = os.environ.get("KICAD_PYTHON", "python3")
    strip = root / "tools" / "kicad-oracle" / "kicad_steps.py"
    status = 0
    for row, spec in prereg["boards"]["rows"].items():
        board = Path(spec["input"])
        want = prereg["boards"]["files"][row]
        if sha256(board) == want:
            print(f"{row}: present, {want}")
            continue
        source = spec["source"]
        src = Path(source["dir"]) if Path(source["dir"]).is_absolute() else root / source["dir"]
        if "git" in source:
            unpacked = board.parent.parent / "src" / row
            shutil.rmtree(unpacked, ignore_errors=True)
            unpacked.mkdir(parents=True)
            archive = subprocess.run(["git", "-C", source["git"], "archive", source["commit"], source["dir"]],
                                     capture_output=True, check=False)
            if archive.returncode != 0:
                print(f"{row}: skipped, git archive failed: {archive.stderr.decode().strip()}", file=sys.stderr)
                continue
            subprocess.run(["tar", "-x", "-C", str(unpacked)], input=archive.stdout, check=True)
            src = unpacked / source["dir"]
        name = board.stem
        if not (src / f"{name}.kicad_pcb").is_file():
            print(f"{row}: skipped, no {name}.kicad_pcb in {src}", file=sys.stderr)
            continue
        shutil.rmtree(board.parent, ignore_errors=True)
        board.parent.mkdir(parents=True)
        subprocess.run([kicad_python, "-I", str(strip), "strip", str(src / f"{name}.kicad_pcb"), str(board)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        for ext in ("kicad_pro", "kicad_dru"):
            target = board.parent / f"{name}.{ext}"
            target.unlink(missing_ok=True)
            if (src / f"{name}.{ext}").is_file():
                shutil.copyfile(src / f"{name}.{ext}", target)
        (board.parent / f"{name}.kicad_prl").unlink(missing_ok=True)
        counts = board.parent / f"{name}.kicad_pcb.strip.json"
        if counts.is_file():
            shutil.move(counts, board.parent.parent / f"{row}.strip.json")
        got = sha256(board)
        print(f"{row}: made, {got}" + ("" if got == want else f" (registered {want}: MISMATCH)"))
        status |= got != want
    return int(status)


def run(prereg_path, dry_run):
    prereg = load(prereg_path)
    root = Path(prereg["run"]["root"])
    rows = prereg["boards"]["rows"]
    for row, arms in prereg["commands"].items():
        board = Path(rows[row]["input"])
        if not board.is_file():
            print(f"{row}: skipped, no board at {board}", file=sys.stderr)
            continue
        for arm, command in arms.items():
            log = root / prereg["run"]["out"] / row / f"{arm}.log"
            print(f"{row} {arm}: {command}", flush=True)
            if dry_run:
                continue
            while subprocess.run(["pgrep", "-x", "kicad-cli"], stdout=subprocess.DEVNULL).returncode == 0:
                subprocess.run(["timeout", "5", "tail", "-f", "/dev/null"])
            log.parent.mkdir(parents=True, exist_ok=True)
            with open(log, "w", encoding="utf-8") as f:
                code = subprocess.run(["bash", "-c", command], cwd=root, stdin=subprocess.DEVNULL,
                                      stdout=f, stderr=subprocess.STDOUT).returncode
            with open(log, "a", encoding="utf-8") as f:
                f.write(f"EXIT {code}\n")
            print(f"{row} {arm}: EXIT {code}", flush=True)
    return 0


def zone_zone(drc):
    items, zones = 0, set()
    for item in (drc or {}).get("unconnected_items", []):
        descriptions = [i.get("description", "") for i in item.get("items", [])]
        if descriptions and all(d.startswith("Zone") for d in descriptions):
            items += 1
            for d in descriptions:
                m = ZONE_ITEM.match(d)
                if m:
                    zones.add(m.groups())
    return items, len(zones)


def arm_values(directory):
    """The record of one arm, or None when it has no summary."""
    s = load(directory / "summary.json")
    if s is None:
        return None
    work = directory / "work"
    manifest = next(iter(sorted(work.glob("*.result.json"))), None) if work.is_dir() else None
    stats = ((load(manifest) or {}) if manifest else {})
    connections = (stats.get("board_statistics") or {}).get("connections") or {}
    log = directory / "route.log"
    guard = POUR_GUARD.search(log.read_text(encoding="utf-8", errors="replace")) if log.is_file() else None
    routed = load(directory / "routed" / "drc.json")
    if routed is None and s.get("grade_shared_with"):
        routed = load(Path(s["grade_shared_with"]) / "routed" / "drc.json")
    zz_items, zz_zones = zone_zone(routed)
    caused = s.get("routing_caused_by_type") or {}
    v = {
        "via_in_pad": s.get("via_in_pad"),
        "below_minimum": s.get("below_minimum"),
        "manufacturing_breaches": s.get("manufacturing_breaches"),
        "kicad_unconnected": s.get("unconnected"),
        "drc_real": sum(caused.values()) if s.get("graded") else None,
        "route_seconds": s.get("route_s"),
        "zone_zone_unconnected": zz_items if routed is not None else None,
        "zone_zone_zones": zz_zones if routed is not None else None,
        "router_incomplete": connections.get("incomplete_count"),
        "connections": connections.get("maximum_count"),
        "routing_caused_by_type": caused,
        "input_unconnected": s.get("input_unconnected"),
        "pour_guard_pours": int(guard.group(1)) if guard else 0,
        "aborted": s.get("aborted"),
        "final_state": stats.get("final_state"),
        "graded": bool(s.get("graded")),
        "session_sha256": s.get("session_sha256"),
    }
    if v["kicad_unconnected"] is not None and v["router_incomplete"] is not None:
        v["kicad_less_router"] = v["kicad_unconnected"] - v["router_incomplete"]
    else:
        v["kicad_less_router"] = None
    return v


def collect(prereg_path, out):
    prereg = load(prereg_path)
    out = Path(out)
    metrics = [m["name"] for m in prereg["metrics"]]
    rows, lines = {}, []
    head = ["row", "arm", "KiCad unc.", "Zone-Zone (zones)", "router inc.", "KiCad - router", "below min",
            "manuf.", "drc_real", "route s", "state", "pours guarded"]
    lines.append("| " + " | ".join(head) + " |")
    lines.append("|" + "---|" * len(head))
    for row in prereg["boards"]["rows"]:
        arms = {}
        for arm in prereg["commands"][row]:  # the arms the row runs (the DSN ones on dsn_rows only)
            v = arm_values(out / row / arm)
            if v is None:
                lines.append(f"| {row} | {arm} | not run |" + " |" * (len(head) - 3))
                continue
            arms[arm] = {k: v[k] for k in metrics if k in v}
            lines.append(
                f"| {row} | {arm} | {v['kicad_unconnected']} | {v['zone_zone_unconnected']} ({v['zone_zone_zones']}) "
                f"| {v['router_incomplete']} | {v['kicad_less_router']} | {v['below_minimum']} "
                f"| {v['manufacturing_breaches']} | {v['drc_real']} | {v['route_seconds']} | {v['final_state']} "
                f"| {v['pour_guard_pours']} |")
        if arms:
            rows[row] = {"board": row, "input_sha256": prereg["boards"]["files"][row], "arms": arms}
            # The gate's default band reads `connections` on the row (gate_totals.rs).
            counts = [a["connections"] for a in arms.values() if isinstance(a.get("connections"), int)]
            if counts:
                rows[row]["connections"] = max(counts)
    records = {
        "format": "freerouting-gate-records-1",
        "partial": len(rows) < len(prereg["boards"]["rows"]),
        "declared": list(prereg["boards"]["rows"]),
        "metrics": metrics,
        "arms": prereg["arms"],
        "baseline_arm": prereg["baseline_arm"],
        "candidate_arm": prereg["candidate_arm"],
        "set": prereg["boards"]["set"],
        "rows": rows,
    }
    (out / "records.json").write_text(json.dumps(records, indent=1) + "\n", encoding="utf-8")
    table = "\n".join(lines) + "\n"
    (out / "table.md").write_text(table, encoding="utf-8")
    print(table, end="")
    return 0


def main(argv):
    if len(argv) == 2 and argv[0] == "pours":
        print(json.dumps(signal_pours(argv[1]), indent=1))
        return 0
    if len(argv) == 2 and argv[0] == "prepare":
        return prepare(argv[1])
    if len(argv) in (2, 3) and argv[0] == "run" and argv[2:] in ([], ["--dry-run"]):
        return run(argv[1], argv[2:] == ["--dry-run"])
    if len(argv) == 3 and argv[0] == "collect":
        return collect(argv[1], argv[2])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
