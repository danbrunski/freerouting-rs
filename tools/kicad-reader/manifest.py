"""K04.7/K04.8: write fixtures/kicad/MANIFEST.tsv and check it.

    python3 -I tools/kicad-reader/manifest.py write   # regenerate the manifest
    python3 -I tools/kicad-reader/manifest.py check   # verify files against it and `git show`

One row per committed board: path (under fixtures/kicad/), source
(freerouting@<commit>:<path in the Java repo>), sha256 and bytes of the board, the
(version N) of the file, whether a .kicad_pro sits next to it, the licence, then the
KiCad 10 reference export: its path, sha256 and the (host_version ...) it recorded.
`check` also compares the board with `git show <commit>:<path>` when the Java repo is there.
"""
import hashlib
import os
import re
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
FIX = os.path.join(ROOT, "fixtures", "kicad")
JAVA = os.path.join(ROOT, "..", "freerouting")
COMMIT = "cda540f"
LICENCE = "LicenseRef-freerouting-fixture"
HEADER = ["path", "source", "sha256", "bytes", "file_version", "kicad_pro", "licence",
          "kicad10_dsn", "kicad10_dsn_sha256", "kicad10_host_version"]


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def boards():
    out = []
    for d, _, files in os.walk(FIX):
        for f in files:
            if f.endswith(".kicad_pcb"):
                out.append(os.path.relpath(os.path.join(d, f), FIX))
    return sorted(out)


def rows():
    res = []
    for rel in boards():
        p = os.path.join(FIX, rel)
        data = open(p, "rb").read()
        version = re.search(rb"\(version (\d+)\)", data[:400]).group(1).decode()
        stem = rel[: -len(".kicad_pcb")]
        pro = any(f.endswith(".kicad_pro") for f in os.listdir(os.path.dirname(p)))
        dsn = stem + ".kicad10.dsn"
        dp = os.path.join(FIX, dsn)
        host = ""
        if os.path.exists(dp):
            m = re.search(r'\(host_version "([^"]*)"\)', open(dp, encoding="utf-8").read())
            host = m.group(1) if m else ""
        res.append([rel, "freerouting@%s:fixtures/%s" % (COMMIT, rel), sha(p), str(len(data)), version,
                    "yes" if pro else "no", LICENCE, dsn if os.path.exists(dp) else "",
                    sha(dp) if os.path.exists(dp) else "", host])
    return res


def read_manifest():
    lines = open(os.path.join(FIX, "MANIFEST.tsv"), encoding="utf-8").read().splitlines()
    assert lines[0].split("\t") == HEADER, "unexpected header"
    return [l.split("\t") for l in lines[1:]]


def main(mode):
    if mode == "write":
        with open(os.path.join(FIX, "MANIFEST.tsv"), "w", encoding="utf-8", newline="\n") as f:
            f.write("\t".join(HEADER) + "\n")
            for r in rows():
                f.write("\t".join(r) + "\n")
        return 0
    if mode == "check":
        bad = 0
        have = read_manifest()
        if [r[0] for r in have] != boards():
            print("manifest and files disagree on the board list")
            bad += 1
        for r in have:
            p = os.path.join(FIX, r[0])
            if sha(p) != r[2] or str(os.path.getsize(p)) != r[3]:
                print("changed:", r[0])
                bad += 1
            if r[7] and sha(os.path.join(FIX, r[7])) != r[8]:
                print("export changed:", r[7])
                bad += 1
            src = r[1].split(":", 1)[1]
            if os.path.isdir(JAVA):
                g = subprocess.run(["git", "-C", JAVA, "show", "%s:%s" % (COMMIT, src)], capture_output=True)
                if g.returncode == 0 and hashlib.sha256(g.stdout).hexdigest() != r[2]:
                    print("differs from git show:", r[0])
                    bad += 1
        print("manifest ok" if not bad else "manifest FAILED")
        return 1 if bad else 0
    print(__doc__)
    return 2


sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else ""))
