"""Non-ASCII net names through the plugin's path (K02.6): export, route, import.

    python3 roundtrip_names.py --router PATH_TO_FREEROUTING [--demos DIR] [--out DIR]
                               [--max-passes N] [--case interf_u|edited|all]

Run with KiCad's Python (a python3 that imports `pcbnew`; the system python3 on
WSL). Never imports the freerouting-rs plugin package and never starts the KiCad
GUI. It works on copies under `--out` (default: a new directory under TMPDIR).

For each case (a copy of the `interf_u` demo with nets and references renamed
to carry Omega; and a copy of `ecc83-pp` with some to carry mu and Phi; KiCad
10's `interf_u` has plain ASCII names, unlike the older board behind fixture
`Issue093-interf_u`, whose non-ASCII text is Chinese):

1. strip the tracks and vias, so the router has work (`kicad_steps.py strip`),
2. export the DSN with `pcbnew.ExportSpecctraDSN` (`kicad_steps.py export`),
3. route it with the CLI, `-de x.dsn -do x.ses`, nothing sanitised,
4. import the SES with `pcbnew.ImportSpecctraSES` into the stripped copy
   (`kicad_steps.py import`),
5. compare: every track and via of the routed board has a net name that is a
   net of the original board, the nets of the non-ASCII names were routed, and
   `kicad-cli pcb drc` reports no `shorting_items`.

Each KiCad step is its own process (see kicad_steps.py: stripping and exporting
in one process crashed KiCad 10.0.7). Exit status: 0 every case passes, 1 a name
changed (the failing name is printed, for the C series / the DSN lexer owner), 2 a
step could not run.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
STEPS = HERE / "kicad_steps.py"
DEFAULT_DEMOS = Path("/usr/share/kicad/demos")

def non_ascii(name):
    return any(ord(c) > 127 for c in name)


def run(cmd, log, check=True):
    with open(log, "w", encoding="utf-8") as f:
        done = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, timeout=3600)
    if check and done.returncode != 0:
        raise StepFailed(f"{' '.join(map(str, cmd[:3]))} exited {done.returncode}; see {log}")
    return done.returncode


class StepFailed(Exception):
    pass


# --- the in-process KiCad steps (each runs in its own python) ----------------


def edit_names(src, dst, suffixes, reference_rules):
    """Saves a copy of `src` with every third net that has two or more pads
    (at most 8) renamed to carry the next character of `suffixes`, and the
    references of `reference_rules` (`R=R\u03a6,C=C\u00b5`: a letter prefix and
    what it becomes) renamed. Writes `dst + .renamed.json`."""
    import pcbnew

    board = pcbnew.LoadBoard(src)
    pads = {}
    for p in board.GetPads():
        pads[p.GetNetname()] = pads.get(p.GetNetname(), 0) + 1
    candidates = sorted(n for n, count in pads.items() if n and count >= 2)
    renamed = {"nets": {}, "references": {}}
    for i, name in enumerate(candidates[::3][:8]):
        net = board.FindNet(name)
        if net is not None:
            new = name + suffixes[i % len(suffixes)]
            net.SetNetname(new)
            renamed["nets"][name] = new
    rules = dict(rule.split("=", 1) for rule in reference_rules.split(",") if "=" in rule)
    for fp in board.GetFootprints():
        ref = fp.GetReference()
        letters = ref.rstrip("0123456789")
        if letters in rules and ref[len(letters) :].isdigit():
            fp.SetReference(rules[letters] + ref[len(letters) :])
            renamed["references"][ref] = fp.GetReference()
    pcbnew.SaveBoard(dst, board)
    Path(dst + ".renamed.json").write_text(json.dumps(renamed, ensure_ascii=False), encoding="utf-8")
    return 0


def compare(original, routed, out_json):
    """Net names of the routed board's tracks and vias against the original's."""
    import pcbnew

    orig = pcbnew.LoadBoard(original)
    original_nets = {n.GetNetname() for n in orig.GetNetInfo().NetsByNetcode().values() if n.GetNetname()}
    pad_nets = {p.GetNetname() for p in orig.GetPads() if p.GetNetname()}
    original_refs = sorted(fp.GetReference() for fp in orig.GetFootprints())
    del orig  # a second LoadBoard in one process can return an unwrapped object (kicad_steps.py)
    routed_board = pcbnew.LoadBoard(routed)
    routed_refs = sorted(fp.GetReference() for fp in routed_board.GetFootprints())
    seen = {}
    unknown = {}
    for t in routed_board.Tracks():
        name = t.GetNetname()
        seen[name] = seen.get(name, 0) + 1
        if name not in original_nets:
            unknown[name] = unknown.get(name, 0) + 1
    result = {
        "original_nets": len(original_nets),
        "original_non_ascii_nets": sorted(n for n in original_nets if non_ascii(n)),
        "routed_items_by_net": seen,
        "routed_items": sum(seen.values()),
        "unknown_net_names": unknown,
        "routed_non_ascii_nets": sorted(n for n in seen if non_ascii(n)),
        "nets_with_pads": len(pad_nets),
        "references_changed": sorted(set(original_refs) ^ set(routed_refs)),
        "non_ascii_references": sorted(r for r in original_refs if non_ascii(r)),
    }
    Path(out_json).write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
    return 0


# --- the driver ---------------------------------------------------------------


def case_copy(demos, demo, base, out):
    src_dir = demos / demo
    work = out / "source"
    work.mkdir(parents=True, exist_ok=True)
    for ext in ("kicad_pcb", "kicad_pro", "kicad_dru"):
        f = src_dir / f"{base}.{ext}"
        if f.is_file():
            shutil.copy(f, work / f.name)
            os.chmod(work / f.name, 0o644)
    return work / f"{base}.kicad_pcb"


def run_case(name, demos, demo, base, edit, router, out_root, max_passes, kicad_cli):
    out = out_root / name
    shutil.rmtree(out, ignore_errors=True)
    (out / "work").mkdir(parents=True)
    py = sys.executable
    pcb = case_copy(demos, demo, base, out)
    renamed = {}
    if edit:
        suffixes, reference_rules = edit
        edited_pcb = pcb.with_name(pcb.stem + "_edited.kicad_pcb")
        run([py, __file__, "edit", str(pcb), str(edited_pcb), suffixes, reference_rules], out / "work" / "edit.log")
        renamed = json.loads(Path(str(edited_pcb) + ".renamed.json").read_text(encoding="utf-8"))
        for ext in ("kicad_pro",):
            shutil.copy(pcb.with_suffix("." + ext), edited_pcb.with_suffix("." + ext))
        pcb = edited_pcb
    unrouted = out / "unrouted" / pcb.name
    unrouted.parent.mkdir()
    for ext in ("kicad_pro", "kicad_dru"):
        if pcb.with_suffix("." + ext).is_file():
            shutil.copy(pcb.with_suffix("." + ext), unrouted.with_suffix("." + ext))
    run([py, str(STEPS), "strip", str(pcb), str(unrouted)], out / "work" / "strip.log")
    dsn, ses = out / "work" / (pcb.stem + ".dsn"), out / "work" / (pcb.stem + ".ses")
    run([py, str(STEPS), "export", str(unrouted), str(dsn), str(out / "work" / "areas.json")], out / "work" / "export.log")
    dsn_text = dsn.read_text(encoding="utf-8")
    cmd = [str(router), "-de", str(dsn), "-do", str(ses)] + (["--max-passes", str(max_passes)] if max_passes else [])
    code = run(cmd, out / "work" / "route.log", check=False)
    if code != 0 or not ses.is_file():
        raise StepFailed(f"{name}: the router exited {code} and wrote {'a' if ses.is_file() else 'no'} session; see {out / 'work' / 'route.log'}")
    ses_text = ses.read_text(encoding="utf-8")
    routed = out / "routed" / pcb.name
    routed.parent.mkdir()
    run([py, str(STEPS), "import", str(unrouted), str(ses), str(routed)], out / "work" / "import.log")
    result_json = out / "work" / "compare.json"
    run([py, __file__, "compare", str(pcb), str(routed), str(result_json)], out / "work" / "compare.log")
    result = json.loads(result_json.read_text(encoding="utf-8"))

    # The non-ASCII names the original board carries must be in the DSN and the SES as they are.
    failures = []
    if result["unknown_net_names"]:
        failures += [f"net name changed or invented: {n!r} ({c} items)" for n, c in result["unknown_net_names"].items()]
    expected = sorted(result["original_non_ascii_nets"])
    if not expected:
        failures.append("the case has no non-ASCII net name (nothing was tested)")
    for n in expected:
        if n not in dsn_text:
            failures.append(f"{n!r} is not in the exported DSN as it is (KiCad's export changed it)")
    # A net without wires (a zone net such as GND) has no entry in the session.
    for n in result["routed_non_ascii_nets"]:
        if n not in ses_text:
            failures.append(f"{n!r} was imported but is not in the router's SES as it is (the router changed it)")
    if result["references_changed"]:
        failures.append(f"references changed by the SES import: {result['references_changed']}")
    if not result["non_ascii_references"]:
        failures.append("the case has no non-ASCII reference (nothing was tested)")
    if not result["routed_non_ascii_nets"]:
        failures.append("no track or via of a non-ASCII net was imported")
    result.update(
        case=name,
        renamed=renamed,
        non_ascii_nets_in_dsn=[n for n in expected if n in dsn_text],
        non_ascii_nets_in_ses=[n for n in expected if n in ses_text],
        failures=failures,
    )

    shorts = None
    if kicad_cli:
        report = out / "routed" / "drc.json"
        subprocess.run(
            [kicad_cli, "pcb", "drc", "--format", "json", "--severity-all", "--output", str(report), str(routed)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=3600,
        )
        if report.is_file():
            data = json.loads(report.read_text(encoding="utf-8"))
            shorts = sum(1 for v in data.get("violations", []) if v.get("type") == "shorting_items")
            result["drc_shorting_items"] = shorts
            if shorts:
                failures.append(f"KiCad's DRC reports {shorts} shorting_items on the routed board")
    (out / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")
    return result


def main(argv):
    if argv and argv[0] == "edit":
        return edit_names(argv[1], argv[2], argv[3], argv[4])
    if argv and argv[0] == "compare":
        return compare(argv[1], argv[2], argv[3])
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--router", required=True, help="the freerouting executable")
    ap.add_argument("--demos", default=str(DEFAULT_DEMOS))
    ap.add_argument("--out", default=None)
    ap.add_argument("--max-passes", type=int, default=0, help="0 = the router's default")
    ap.add_argument("--case", default="all", choices=["interf_u", "edited", "all"])
    args = ap.parse_args(argv)
    out = Path(args.out or tempfile.mkdtemp(prefix="roundtrip-names.", dir=os.environ.get("TMPDIR")))
    out.mkdir(parents=True, exist_ok=True)
    cli = shutil.which(os.environ.get("KICAD_CLI", "kicad-cli"))
    cases = [
        ("interf_u", "interf_u", "interf_u", ("\u03a9", "R=R\u03a9,C=C\u03a9")),
        ("edited", "ecc83", "ecc83-pp", ("\u00b5\u03a6", "R=R\u03a6,C=C\u00b5")),
    ]
    status = 0
    for name, demo, base, edit in cases:
        if args.case not in ("all", name):
            continue
        try:
            result = run_case(name, Path(args.demos), demo, base, edit, Path(args.router), out, args.max_passes, cli)
        except (StepFailed, subprocess.TimeoutExpired, OSError) as e:
            print(f"{name}: not run: {e}")
            status = max(status, 2)
            continue
        verdict = "ok" if not result["failures"] else "FAILED"
        print(
            f"{name}: {verdict}: {result['routed_items']} tracks and vias imported, "
            f"{len(result['routed_non_ascii_nets'])} non-ASCII nets routed "
            f"({', '.join(result['routed_non_ascii_nets'][:6])}), shorting_items {result.get('drc_shorting_items')}"
        )
        for f in result["failures"]:
            print(f"  {f}")
        if result["failures"]:
            status = max(status, 1)
    print(f"results in {out}")
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
