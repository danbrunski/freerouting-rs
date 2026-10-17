#!/usr/bin/env python3
"""Builds tools/corpus/pcbworld-d3c.json: PCBWorld-Bench's D3-C boards, pinned at their sources.

PCBWorld-Bench (arXiv 2607.05915, LGAI-Research/PCBWorld, BSD-3-Clause) takes its real
boards from PCBench (PCBench/PCBench at dec3be75, whose MIT licence covers PCBench's own
files, not the boards it collected). D3-C is the `hard` difficulty of PCBWorld's
`configs/datasets/d3.json` (more than 100 pins, 292 boards). The corpus takes a few of them:
PCBWorld's own D3-C test split (`hard.test`, 10 two-layer boards) and the boards in EXTRA.
Neither project redistributes a board under the board's own licence, so this script pins each
board at its original GitHub repository:

  boards   D3_JSON MASTER_METADATA WORK  the chosen boards, PCBench's source URL and retrieval date
  pcbench  WORK                          the git blob sha of PCBench's raw.kicad_pcb per board
  resolve  WORK                          a commit of the source whose tree holds that exact blob
                                         (the commit at PCBench's retrieval date, else the
                                         default branch's head, else `git log --find-object`
                                         in a blobless clone), its path, the SPDX licence at
                                         that commit and the .kicad_pro/.kicad_dru beside it
  download WORK                          fetch the pinned files
  analyze  WORK                          layers, zones, footprints, BGA arrays from the text
  manifest WORK OUT.json SET.txt         the manifest and the bench set

A board is kept only if its source holds PCBench's exact file (same git blob), so the pinned
board is the one PCBWorld measured before its own conversion. PCBWorld's KiCad 9 conversion
and its rule patches (drc_fix_v9.py) are not applied: the corpus grades every board at its
own rules (D8). Licences GitHub reports as "other" are read by hand and listed in
LICENSE_OVERRIDES; one the script cannot map is left out.

Everything downloaded is untrusted data: it is only hashed and read as text (by regex), never
executed, and lives in WORK (a new empty directory). Run it with `python3 -I`. Needs `gh`
(authenticated), `curl` and `git`.
"""
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
ALLOWED = {l.split("#")[0].strip() for l in open(os.path.join(HERE, "allowed-licenses.txt"))
           if l.split("#")[0].strip()}
PCBENCH = ("PCBench/PCBench", "dec3be75cbdef74787625f9043c7391cd473bb64")
PCBWORLD = ("LGAI-Research/PCBWorld", "b3d62f5c37e7528670d112e03d9a90029f23f4f3")
# Boards taken beyond PCBWorld's hard.test split, and why.
EXTRA = {
    "0677_kitspace_glasgow": "4 layers with inner planes and an 11 x 11, 0.8 mm BGA (U30): a BGA board for X12.6",
    "0581_a123-battery-integration_BCM": "4 layers with inner planes, a battery power board like the owner's",
}
# Repositories whose licence GitHub reports as "other" (NOASSERTION): the SPDX id read by hand
# from the licence file at the pinned commit, and the file. Filled after `resolve` lists them.
LICENSE_OVERRIDES = {}


def gh(path, *extra):
    r = subprocess.run(["gh", "api", path, *extra], capture_output=True, text=True, timeout=180)
    return json.loads(r.stdout) if r.returncode == 0 and r.stdout.strip() else None


def load(work, name, default=None):
    p = os.path.join(work, name)
    return json.load(open(p)) if os.path.exists(p) else default


def save(work, name, data):
    with open(os.path.join(work, name), "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)


def boards(d3_json, master_metadata, work):
    d3, meta = json.load(open(d3_json)), json.load(open(master_metadata))
    test = set(d3["hard"]["test"])
    out = []
    for pid in d3["hard"]["train"]:  # the full hard list; `test` is a subset of it
        if pid not in test and pid not in EXTRA:
            continue
        name = re.sub(r"^\d{4}_", "", pid)
        m = meta[name]
        out.append({"pcbworld_id": pid, "pcbench_name": name, "source": m.get("source") or "",
                    "retrieved": (m.get("retrieved at") or "")[:10], "pcbench_layers": m.get("layers"),
                    "pcbworld_split": "hard.test" if pid in test else "hard.train",
                    "why": "PCBWorld's D3-C test split" if pid in test else EXTRA[pid]})
    os.makedirs(work, exist_ok=True)
    save(work, "boards.json", out)
    print(len(out), "boards,", sum(b["pcbworld_split"] == "hard.test" for b in out), "from hard.test")


def pcbench(work):
    repo, commit = PCBENCH
    root = gh(f"repos/{repo}/git/trees/{commit}")
    pcbs = next(t["sha"] for t in root["tree"] if t["path"] == "PCBs")
    tree = gh(f"repos/{repo}/git/trees/{pcbs}?recursive=1")
    assert not tree["truncated"], "PCBench's PCBs tree is truncated"
    blobs = {t["path"].split("/")[0]: t["sha"] for t in tree["tree"]
             if t["type"] == "blob" and t["path"].endswith("/raw.kicad_pcb")}
    sizes = {t["path"].split("/")[0]: t["size"] for t in tree["tree"]
             if t["type"] == "blob" and t["path"].endswith("/raw.kicad_pcb")}
    out = []
    for b in load(work, "boards.json"):
        out.append({**b, "pcbench_blob": blobs.get(b["pcbench_name"]), "pcbench_size": sizes.get(b["pcbench_name"])})
    save(work, "pcbench.json", out)
    print(sum(1 for b in out if b["pcbench_blob"]), "of", len(out), "have a raw.kicad_pcb")


def github_repo(url):
    m = re.match(r"https?://github\.com/([^/\s]+)/([^/\s#?]+)", url or "")
    return f"{m.group(1)}/{m.group(2).removesuffix('.git')}" if m else None


def find_blob(repo, sha, blob, hint):
    tree = gh(f"repos/{repo}/git/trees/{sha}?recursive=1")
    if not tree:
        return None, None
    files = {t["path"]: t["sha"] for t in tree["tree"] if t["type"] == "blob"}
    hits = sorted(p for p, s in files.items() if s == blob and p.endswith(".kicad_pcb"))
    if not hits:
        return None, files
    named = [p for p in hits if os.path.basename(p)[:-len(".kicad_pcb")].lower() in hint.lower()]
    return (named or hits)[0], files


def find_by_clone(repo, blob, work):
    """`git log --find-object` in a blobless clone: the newest commit holding the blob."""
    d = os.path.join(work, "clones", repo.replace("/", "__"))
    if not os.path.isdir(d):
        r = subprocess.run(["git", "clone", "--quiet", "--filter=blob:none", "--no-checkout", "--bare",
                            f"https://github.com/{repo}.git", d], capture_output=True, text=True, timeout=900)
        if r.returncode != 0:
            return None
    r = subprocess.run(["git", "-C", d, "log", "--all", "--format=%H", "--find-object=" + blob],
                       capture_output=True, text=True, timeout=900)
    shas = r.stdout.split()
    for sha in shas:  # newest first; the blob must be in the commit's tree, not only removed there
        t = subprocess.run(["git", "-C", d, "ls-tree", "-r", sha], capture_output=True, text=True, timeout=300)
        if blob in t.stdout:
            return sha
    return None


def resolve(work):
    done = {b["pcbworld_id"]: b for b in load(work, "resolved.json", [])}
    out = []
    for b in load(work, "pcbench.json"):
        if b["pcbworld_id"] in done:  # a re-run keeps what it resolved and takes the new board fields
            out.append({**done[b["pcbworld_id"]], **b})
            continue
        r = {**b, "status": None}
        out.append(r)
        repo = github_repo(b["source"])
        if not repo or not b["pcbench_blob"]:
            r["status"] = "no GitHub source" if not repo else "no raw.kicad_pcb in PCBench"
            continue
        info = gh(f"repos/{repo}")
        if not info:
            r["status"] = "source repository unavailable"
            continue
        repo, branch = info["full_name"], info["default_branch"]
        candidates = []
        dated = gh(f"repos/{repo}/commits?sha={branch}&until={b['retrieved']}T23:59:59Z&per_page=1")
        if dated:
            candidates.append(dated[0]["sha"])
        head = (gh(f"repos/{repo}/commits/{branch}", "--jq", "{sha: .sha}") or {}).get("sha")
        if head and head not in candidates:
            candidates.append(head)
        path = files = commit = None
        for sha in candidates:
            path, files = find_blob(repo, sha, b["pcbench_blob"], b["pcbench_name"])
            if path:
                commit = sha
                break
        if not path:
            sha = find_by_clone(repo, b["pcbench_blob"], work)
            if sha:
                path, files = find_blob(repo, sha, b["pcbench_blob"], b["pcbench_name"])
                commit = sha if path else None
        if not path:
            r["status"] = "PCBench's file is in no commit of the source"
            continue
        date = (gh(f"repos/{repo}/commits/{commit}", "--jq", "{date: .commit.committer.date}") or {}).get("date")
        lic = gh(f"repos/{repo}/license?ref={commit}")
        spdx = ((lic or {}).get("license") or {}).get("spdx_id")
        if not spdx or spdx == "NOASSERTION":
            spdx = "LicenseRef-none" if not lic else "LicenseRef-other"
        base = path[: -len(".kicad_pcb")]
        r.update({"status": "resolved", "repo": repo, "commit": commit, "commit_date": date or "",
                  "path": path, "license": spdx, "license_file": (lic or {}).get("path"),
                  "html_url": info["html_url"], "owner": info["owner"]["login"],
                  "companions": [base + e for e in (".kicad_pro", ".kicad_dru") if base + e in files]})
        print("resolved", b["pcbworld_id"], repo, commit[:10], spdx, flush=True)
        save(work, "resolved.json", out)
    save(work, "resolved.json", out)
    status = {}
    for r in out:
        status[r["status"]] = status.get(r["status"], 0) + 1
    print(status)


def raw(repo, sha, path, dest):
    url = f"https://raw.githubusercontent.com/{repo}/{sha}/" + urllib.parse.quote(path)
    r = subprocess.run(["curl", "--fail", "--silent", "--location", "--proto", "=https", "--max-filesize",
                        "30000000", "--output", dest, url], capture_output=True, timeout=300)
    return r.returncode == 0


def download(work):
    for c in load(work, "resolved.json"):
        if c["status"] != "resolved":
            continue
        d = os.path.join(work, "src", c["pcbworld_id"])
        os.makedirs(d, exist_ok=True)
        for path in [c["path"], *c["companions"]]:
            dest = os.path.join(d, os.path.basename(path))
            if not os.path.exists(dest) and not raw(c["repo"], c["commit"], path, dest):
                print("download failed:", c["repo"], path)


def git_blob_sha(data):
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def pads_by_footprint(text):
    """Per footprint: its reference and the local centres of its SMD pads, in mm."""
    out = []
    starts = [m.start() for m in re.finditer(r"\((?:footprint|module)\s", text)]
    for i, s in enumerate(starts):
        block = text[s: starts[i + 1] if i + 1 < len(starts) else len(text)]
        ref = re.search(r'\(fp_text\s+reference\s+"?([^"\s)]+)|\(property\s+"Reference"\s+"([^"]*)"', block)
        ref = (ref.group(1) or ref.group(2)) if ref else "?"
        pads = [(float(x), float(y)) for x, y in
                re.findall(r'\(pad\s+"?[^"\s)]*"?\s+smd\s+\w+\s+\(at\s+(-?[\d.]+)\s+(-?[\d.]+)', block)]
        out.append((ref, pads))
    return out


def ball_array(pads):
    """X12's array test (crates/freerouting-krt/src/fanout/bga/array.rs) on footprint-local
    centres: a lattice of one pitch on both axes, every pad on it, at least 6 x 6 positions,
    not staggered, and both outer rings at least half populated. Returns (rows, cols, pitch)."""
    if len(pads) < 36:
        return None

    def axis(vals):
        cl = []
        for v in sorted(vals):
            if cl and v - cl[-1][-1] < 0.001:
                cl[-1].append(v)
            else:
                cl.append([v])
        pos = [sum(c) / len(c) for c in cl]
        gaps = [round(b - a, 2) for a, b in zip(pos, pos[1:])]
        if not gaps:
            return None, None
        best = min(set(gaps), key=lambda g: (-gaps.count(g), g))
        if gaps.count(best) * 2 < len(gaps):
            return None, None
        return pos, best

    xs, px = axis([p[0] for p in pads])
    ys, py = axis([p[1] for p in pads])
    if not xs or not ys or abs(px - py) > 0.002 or px <= 0:
        return None
    pitch = px
    cols = round((xs[-1] - xs[0]) / pitch) + 1
    rows = round((ys[-1] - ys[0]) / pitch) + 1
    if rows < 6 or cols < 6:
        return None
    cells = set()
    for x, y in pads:
        c, r = (x - xs[0]) / pitch, (y - ys[0]) / pitch
        if abs(c - round(c)) * pitch > 0.001 or abs(r - round(r)) * pitch > 0.001:
            return None
        cells.add((round(r), round(c)))
    if sum(1 for r, c in cells if {(r + 1, c), (r - 1, c), (r, c + 1), (r, c - 1)} & cells) * 2 < len(cells):
        return None  # staggered

    def ring(k):
        want = {(r, c) for r in range(k, rows - k) for c in range(k, cols - k)
                if r in (k, rows - 1 - k) or c in (k, cols - 1 - k)}
        return len(want & cells) * 2 >= len(want)

    return (rows, cols, pitch) if ring(0) and ring(1) else None


def analyze(work):
    out = []
    for c in load(work, "resolved.json"):
        if c["status"] != "resolved":
            continue
        d = os.path.join(work, "src", c["pcbworld_id"])
        pcb = os.path.join(d, os.path.basename(c["path"]))
        if not os.path.exists(pcb):
            continue
        data = open(pcb, "rb").read()
        if git_blob_sha(data) != c["pcbench_blob"]:
            print("blob mismatch after download:", c["pcbworld_id"])
            continue
        text = data.decode("utf-8", "replace")
        if not text.startswith("(kicad_pcb"):
            print("not a kicad_pcb:", c["pcbworld_id"])
            continue
        layers_block = re.search(r"\(layers\s*((?:\(\d+[^()]*\)\s*)+)\)", text)
        copper = re.findall(r'\(\d+\s+"?([^"\s]+\.Cu)"?\s+(?:signal|power|mixed|jumper)',
                            layers_block.group(1) if layers_block else "")
        zones = re.findall(r"\(zone\b.*?\(layers?\s+([^)]*)\)", text, re.S)
        arrays = [{"ref": ref, "rows": a[0], "cols": a[1], "pitch_mm": a[2]}
                  for ref, pads in pads_by_footprint(text) for a in [ball_array(pads)] if a]
        out.append({**c, "sha256": hashlib.sha256(data).hexdigest(), "size_bytes": len(data),
                    "layer_count": len(copper), "has_zones": bool(zones),
                    "has_planes": any(re.search(r"\bIn\d+\.Cu", z) for z in zones),
                    "tracks_and_vias": len(re.findall(r"^\s*\((?:segment|via|arc)\s", text, re.M)),
                    "footprint_count": len(re.findall(r"\((?:footprint|module)\s", text)),
                    "kicad_version": (re.search(r"\(version (\d+)\)", text) or [None, None])[1],
                    "ball_arrays": arrays,
                    "companion_hashes": {os.path.basename(p): hashlib.sha256(
                        open(os.path.join(d, os.path.basename(p)), "rb").read()).hexdigest()
                        for p in c["companions"] if os.path.exists(os.path.join(d, os.path.basename(p)))}})
    save(work, "analyzed.json", out)
    print(len(out), "analyzed;", sum(1 for c in out if c["ball_arrays"]), "with a ball array")


def tier(fp):
    return "easy" if fp <= 30 else "medium" if fp <= 80 else "hard"


def manifest(work, out_path, set_path):
    entries = []
    left_out = [(r["pcbworld_id"], r["status"]) for r in load(work, "resolved.json") if r["status"] != "resolved"]
    for c in load(work, "analyzed.json"):
        if c["layer_count"] not in (2, 4, 6):
            left_out.append((c["pcbworld_id"], f"{c['layer_count']} copper layers (the corpus takes 2, 4 or 6)"))
            continue
        source = "GitHub license API at the pinned commit"
        if c["license"] == "LicenseRef-other":
            if c["repo"] not in LICENSE_OVERRIDES:
                left_out.append((c["pcbworld_id"], "licence GitHub calls other, not mapped by hand"))
                continue
            c["license"], file = LICENSE_OVERRIDES[c["repo"]]
            source = f"read by hand from {file} at the pinned commit"
        elif c["license"] == "LicenseRef-none":
            source = "GitHub license API reports no licence at the pinned commit"
        num, rest = c["pcbworld_id"].split("_", 1)
        eid = "d3c-" + num + "-" + re.sub(r"[^a-z0-9]+", "-", rest.lower()).strip("-")
        entries.append({
            "id": eid, "repo": c["repo"], "commit": c["commit"], "path": c["path"],
            "sha256": c["sha256"], "size_bytes": c["size_bytes"], "license": c["license"],
            "license_source": source,
            "attribution": f"{c['owner']}, {c['html_url']} (commit {c['commit'][:10]}, {c['commit_date'][:10]})",
            "layers": c["layer_count"], "tier": tier(c["footprint_count"]),
            "has_planes": c["has_planes"], "has_zones": c["has_zones"],
            "footprints": c["footprint_count"], "human_routed_items": c["tracks_and_vias"],
            "kicad_pcb_version": c["kicad_version"],
            "companions": c["companion_hashes"],
            "committable": c["license"] in ALLOWED,
            "pcbworld_id": c["pcbworld_id"], "pcbworld_split": c["pcbworld_split"], "why": c["why"],
            "pcbench_blob": c["pcbench_blob"],
            "ball_arrays": c["ball_arrays"],
        })
    entries.sort(key=lambda e: e["pcbworld_id"])
    doc = {"format": "freerouting-corpus-manifest-1",
           "seeded_from": (f"PCBWorld-Bench D3-C: the `hard` boards of {PCBWORLD[0]} configs/datasets/d3.json at "
                           f"{PCBWORLD[1]}, whose boards are PCBench's ({PCBENCH[0]} at {PCBENCH[1]}); each "
                           "board pinned at its original repository at a commit holding PCBench's exact "
                           "raw.kicad_pcb blob, licence read there (tools/corpus/seed_pcbworld.py)"),
           "left_out": [{"pcbworld_id": i, "reason": why} for i, why in sorted(left_out)],
           "boards": entries}
    with open(out_path, "w") as f:
        json.dump(doc, f, indent=1)
        f.write("\n")
    with open(set_path, "w") as f:
        f.write("# PCBWorld-Bench D3-C (tools/corpus/pcbworld-d3c.json): fetch with\n"
                "#   FR_CORPUS_DIR=target/corpus-d3c tools/corpus/fetch.sh --manifest tools/corpus/pcbworld-d3c.json\n"
                "#   FR_CORPUS_DIR=target/corpus-d3c tools/corpus/prep.sh\n"
                "# Rows marked hard.test are PCBWorld's own D3-C test split. Boards are skipped with a note\n"
                "# while missing. Not committable unless the manifest says so (tools/corpus/README.md).\n")
        for e in entries:
            note = f"{e['layers']} layers, {e['footprints']} footprints, {e['license']}, {e['pcbworld_split']}"
            if e["ball_arrays"]:
                note += ", BGA " + " ".join(f"{a['ref']} {a['rows']}x{a['cols']}@{a['pitch_mm']}" for a in e["ball_arrays"])
            f.write(f"target/corpus-d3c/prepped/{e['id']}.dsn   # {note}\n")
    lic = {}
    for e in entries:
        lic[e["license"]] = lic.get(e["license"], 0) + 1
    print(len(entries), "entries,", sum(e["committable"] for e in entries), "committable,",
          len(left_out), "left out;", dict(sorted(lic.items())))


if __name__ == "__main__":
    cmd, args = (sys.argv[1], sys.argv[2:]) if len(sys.argv) > 1 else ("", [])
    {"boards": boards, "pcbench": pcbench, "resolve": resolve, "download": download, "analyze": analyze,
     "manifest": manifest}.get(cmd, lambda *a: sys.exit(__doc__))(*args)
