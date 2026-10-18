#!/usr/bin/env python3
"""Builds tools/corpus/manifest.json from KiCadRoutingTools' corpus manifests.

KRT's manifests are candidate data only: 479 GitHub boards, 31 pinned to a
commit, 372 with licence "see repo". This script pins and verifies each
candidate itself:

  candidates KRT_STRESS_DIR WORK_DIR   pick ~65 candidates (2/4/6 layers, easy..hard)
  resolve    WORK_DIR                  commit sha, SPDX licence and companion files at that commit (gh api)
  download   WORK_DIR                  fetch the .kicad_pcb (and .kicad_pro/.kicad_dru) at the commit
  analyze    WORK_DIR                  layers, zones (planes), footprints, size from the downloaded text
  manifest   WORK_DIR OUT.json         the first 30 usable candidates -> manifest.json

Everything downloaded is untrusted data: it is only hashed and read as text
(by regex), never executed, and lives in WORK_DIR (a new empty directory).
Run it with `python3 -I` so the working directory is not on the import path.
Needs `gh` (authenticated) and `curl`.
"""
import glob
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.parse

ALLOWED = {l.split("#")[0].strip() for l in open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "allowed-licenses.txt"))
           if l.split("#")[0].strip()}
TARGET = 30
# Repositories whose licence GitHub reports as "other": the SPDX id read by hand from the
# licence file at the pinned commit. Boards whose licence file is custom, covers only software
# or only documentation are left out.
LICENSE_OVERRIDES = {
    "TeinkBR/pcie-active-backplane": ("MIT", "LICENSE"),
    "Protocentral/healthypi-move-hw": ("MIT", "LICENSE"),
    "hansj66/overlord": ("CERN-OHL-S-2.0", "LICENSE.md (hardware)"),
    "codesqueak/EPROM_Card": ("CERN-OHL-S-2.0", "LICENSE"),
    "tomojitakasu/PocketSDR": ("BSD-2-Clause", "LICENSE.txt"),
}


def gh(path, *extra):
    r = subprocess.run(["gh", "api", path, *extra], capture_output=True, text=True, timeout=120)
    return json.loads(r.stdout) if r.returncode == 0 and r.stdout.strip() else None


def load(work, name, default=None):
    p = os.path.join(work, name)
    return json.load(open(p)) if os.path.exists(p) else default


def save(work, name, data):
    with open(os.path.join(work, name), "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)


def candidates(stress_dir, work):
    seen_repo, pool = set(), []
    for f in sorted(glob.glob(os.path.join(stress_dir, "manifest_set*.json"))):
        for m in json.load(open(f)):
            if not all(k in m for k in ("repo", "path", "layers_est", "size_kb", "footprints")):
                continue
            if m["layers_est"] not in (2, 4, 6) or m["size_kb"] > 2500 or not 6 <= m["footprints"] <= 250:
                continue
            if m["repo"] in seen_repo or not m["path"].endswith(".kicad_pcb"):
                continue
            seen_repo.add(m["repo"])
            pool.append(m)
    picked = []
    for layers, want in ((2, 26), (4, 26), (6, 14)):
        group = sorted((m for m in pool if m["layers_est"] == layers), key=lambda m: (m["footprints"], m["repo"]))
        step = max(1, len(group) / want)
        picked += [group[int(i * step)] for i in range(min(want, len(group)))]
    os.makedirs(work, exist_ok=True)
    save(work, "candidates.json", [{k: m[k] for k in ("repo", "path", "layers_est", "footprints", "size_kb", "note")
                                    if k in m} for m in picked])
    print(len(picked), "candidates")


def resolve(work):
    out = []
    for c in load(work, "candidates.json"):
        repo = c["repo"]
        info = gh(f"repos/{repo}")
        if not info:
            print("skip (repo unavailable):", repo)
            continue
        branch = info["default_branch"]
        commit = gh(f"repos/{repo}/commits/{branch}", "--jq", "{sha: .sha, date: .commit.committer.date}")
        if not commit:
            continue
        sha = commit["sha"]
        lic = gh(f"repos/{repo}/license?ref={sha}")
        spdx = (lic or {}).get("license", {}).get("spdx_id") if lic else None
        if not spdx or spdx == "NOASSERTION":
            spdx = "LicenseRef-none" if not lic else "LicenseRef-other"
        tree = gh(f"repos/{repo}/git/trees/{sha}?recursive=1")
        files = {t["path"] for t in (tree or {}).get("tree", []) if t["type"] == "blob"}
        if c["path"] not in files:
            print("skip (path not at commit):", repo, c["path"])
            continue
        base = c["path"][: -len(".kicad_pcb")]
        companions = [base + e for e in (".kicad_pro", ".kicad_dru") if base + e in files]
        out.append({**c, "commit": sha, "commit_date": commit["date"], "license": spdx,
                    "html_url": info["html_url"], "owner": info["owner"]["login"],
                    "companions": companions})
        print("resolved", repo, sha[:10], spdx)
    save(work, "resolved.json", out)


def raw(repo, sha, path, dest):
    url = f"https://raw.githubusercontent.com/{repo}/{sha}/" + urllib.parse.quote(path)
    r = subprocess.run(["curl", "--fail", "--silent", "--location", "--proto", "=https", "--max-filesize",
                        "30000000", "--output", dest, url], capture_output=True, timeout=300)
    return r.returncode == 0


def download(work):
    for c in load(work, "resolved.json"):
        d = os.path.join(work, "src", c["repo"].replace("/", "__"))
        os.makedirs(d, exist_ok=True)
        for path in [c["path"], *c["companions"]]:
            dest = os.path.join(d, os.path.basename(path))
            if not os.path.exists(dest) and not raw(c["repo"], c["commit"], path, dest):
                print("download failed:", c["repo"], path)


def analyze(work):
    out = []
    for c in load(work, "resolved.json"):
        d = os.path.join(work, "src", c["repo"].replace("/", "__"))
        pcb = os.path.join(d, os.path.basename(c["path"]))
        if not os.path.exists(pcb):
            continue
        data = open(pcb, "rb").read()
        text = data.decode("utf-8", "replace")
        if not text.startswith("(kicad_pcb"):
            print("not a kicad_pcb (LFS pointer?):", c["repo"])
            continue
        copper = re.findall(r'\(\d+\s+"([^"]+\.Cu)"\s+(signal|power|mixed|jumper)', text)
        zones = re.findall(r'\(zone\b.*?\(layers?\s+([^)]*)\)', text, re.S)
        inner_zone = any('In' in z for z in zones)
        net_zone = len(zones)
        out.append({**c, "sha256": hashlib.sha256(data).hexdigest(), "size_bytes": len(data),
                    "layer_count": len(copper), "has_zones": net_zone > 0, "has_planes": inner_zone,
                    "zone_count": net_zone,
                    "tracks_and_vias": len(re.findall(r'^\s*\((?:segment|via|arc)\s', text, re.M)), "footprint_count": len(re.findall(r'\(footprint\s', text)),
                    "kicad_version": (re.search(r'\(version (\d+)\)', text) or [None, None])[1],
                    "companion_hashes": {os.path.basename(p): hashlib.sha256(open(os.path.join(d, os.path.basename(p)), "rb").read()).hexdigest()
                                         for p in c["companions"] if os.path.exists(os.path.join(d, os.path.basename(p)))}})
    save(work, "analyzed.json", out)
    print(len(out), "analyzed")


def tier(fp):
    return "easy" if fp <= 30 else "medium" if fp <= 80 else "hard"


# The owner's own boards, already committed under fixtures/ (the owner approved: fixtures/canary-pwr/README.md).
# They are graded by the CANary layout session (canary-09) through Windows KiCad, not by this
# repository's oracle; the procedure is in docs/roadmap/routing-quality.md and the handover note.
OWNER_BOARDS = [
    ("canary-pwr-9511d9a", "the placement at canary 9511d9a, the board on hold for freerouting-rs"),
    ("canary-pwr-9511d9a-addonly", "9511d9a with the best routing so far (36 unconnected) as fixed wires; the result to beat is 36"),
    ("canary-pwr-9511d9a-in2-open", "9511d9a exported with In2 open for signals (Q09.5 baseline)"),
]


def owner_entries():
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    out = []
    for name, note in OWNER_BOARDS:
        path = f"fixtures/canary-pwr/{name}.dsn"
        data = open(os.path.join(root, path), "rb").read()
        out.append({
            "id": name, "committed_path": path, "sha256": hashlib.sha256(data).hexdigest(),
            "license": "LicenseRef-owner", "layers": 4, "has_planes": True, "tier": "hard",
            "note": note + ". Graded by canary-09 through Windows KiCad (grading procedure: "
                    "ses_import.py -> kicad-cli pcb drc --schematic-parity --refill-zones --severity-all "
                    "-> via_in_pad.py = 0; zero clearance/width/annular/via-in-pad errors; no tracks on "
                    "In1/In2). Via in a pad is a hard failure. Not graded by tools/kicad-oracle.",
        })
    return out


def manifest(work, out_path):
    usable = []
    for c in load(work, "analyzed.json"):
        if c["layer_count"] not in (2, 4, 6):
            continue
        source = "GitHub license API at the pinned commit"
        if c["license"] == "LicenseRef-other":
            if c["repo"] not in LICENSE_OVERRIDES:
                continue
            c["license"], file = LICENSE_OVERRIDES[c["repo"]]
            source = f"read by hand from {file} at the pinned commit"
        elif c["license"] == "LicenseRef-none":
            source = "GitHub license API reports no licence at the pinned commit"
        c["license_source"] = source
        usable.append(c)
    chosen = []
    for layers, quota in ((2, 12), (4, 12), (6, 6)):
        group = [c for c in usable if c["layer_count"] == layers]
        licensed = sorted((c for c in group if c["license"] != "LicenseRef-none"), key=lambda c: c["footprint_count"])
        unlicensed = sorted((c for c in group if c["license"] == "LicenseRef-none"), key=lambda c: c["footprint_count"])
        take = licensed[:quota]
        rest = quota - len(take)
        if rest > 0 and unlicensed:
            step = len(unlicensed) / rest
            take += [unlicensed[min(int(i * step), len(unlicensed) - 1)] for i in range(rest)]
        chosen += take
    entries = []
    for c in sorted(chosen, key=lambda c: (c["layer_count"], c["footprint_count"], c["repo"])):
        eid = re.sub(r"[^a-z0-9]+", "-", f"{c['repo'].split('/')[1]}-{os.path.basename(c['path'])[:-10]}".lower()).strip("-")
        entries.append({
            "id": eid, "repo": c["repo"], "commit": c["commit"], "path": c["path"],
            "sha256": c["sha256"], "size_bytes": c["size_bytes"], "license": c["license"],
            "license_source": c["license_source"],
            "attribution": f"{c['owner']}, {c['html_url']} (commit {c['commit'][:10]}, {c['commit_date'][:10]})",
            "layers": c["layer_count"], "tier": tier(c["footprint_count"]),
            "has_planes": c["has_planes"], "has_zones": c["has_zones"],
            "footprints": c["footprint_count"], "human_routed_items": c["tracks_and_vias"],
            "kicad_pcb_version": c["kicad_version"],
            "companions": c["companion_hashes"],
            "committable": c["license"] in ALLOWED,
        })
    doc = {"format": "freerouting-corpus-manifest-1",
           "seeded_from": "KiCadRoutingTools tests/stress/manifest_set*.json at 0486e799 (candidate data only; "
                          "commits, hashes and licences below were read from GitHub by tools/corpus/seed.py)",
           "boards": entries, "owner_boards": owner_entries()}
    json.dump(doc, open(out_path, "w"), indent=1)
    print(len(entries), "entries,", sum(e["has_planes"] for e in entries), "with planes,",
          sum(e["committable"] for e in entries), "committable")


if __name__ == "__main__":
    cmd, args = (sys.argv[1], sys.argv[2:]) if len(sys.argv) > 1 else ("", [])
    {"candidates": candidates, "resolve": resolve, "download": download, "analyze": analyze,
     "manifest": manifest}.get(cmd, lambda *a: sys.exit(__doc__))(*args)
