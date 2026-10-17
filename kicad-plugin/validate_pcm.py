#!/usr/bin/env python3
"""Runs KiCad's own package validator on the built zip (K03.5).

    validate_pcm.py --zip ZIP --kicad-metadata DIR [--python PY] [--metadata FILE]

`DIR` is a clone of https://gitlab.com/kicad/addons/metadata (the repository whose CI
validates every submission with `ci/validate.sh`, which runs `ci/validate-package.py`).
`PY` is a Python with that repository's `ci/requirements.txt` installed (jsonschema, requests,
tqdm, munch, pillow); the default is this interpreter.

What it does, without submitting anything: patches the checksum and sizes of `ZIP.meta.json`
(the sidecar next to the zip) into a copy of the repository's `metadata.json`, serves the zip
from 127.0.0.1 on a free port, points the copy's `download_url` there, and runs
`ci/validate-package.py <identifier> <copy>` with `schema-v1.json` and `schema-v2.json` from
the clone. That validator downloads the zip, compares size and sha256, checks every member
against the allowed paths (`metadata.json`, `resources/icon.png`, `plugins/**`), the icon's
size, and the inner `metadata.json` against the outer one. It also checks the unmodified patched
metadata (with its real `download_url`) against both schemas. Exit status is 0 when both pass.
Needs a fixed clone (`git rev-parse HEAD` is printed) so the result is reproducible.
"""

import argparse
import functools
import http.server
import json
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import update_metadata  # noqa: E402

SCHEMA_CHECK = """
import json, sys, jsonschema
meta = json.load(open(sys.argv[1], encoding="utf-8"))
for schema in sys.argv[2:]:
    jsonschema.validate(meta, json.load(open(schema, encoding="utf-8")))
    print("schema ok:", schema)
"""


def serve(directory):
    """Starts an HTTP server for `directory` on 127.0.0.1; returns (server, port)."""

    class Quiet(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *args):
            pass

    handler = functools.partial(Quiet, directory=str(directory))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--zip", required=True)
    ap.add_argument("--kicad-metadata", required=True, help="clone of gitlab.com/kicad/addons/metadata")
    ap.add_argument("--python", default=sys.executable, help="Python with the validator's requirements")
    ap.add_argument("--metadata", default=str(HERE / "metadata.json"))
    args = ap.parse_args(argv)

    zip_path = Path(args.zip).resolve()
    sidecar = zip_path.with_name(zip_path.name + ".meta.json")
    clone = Path(args.kicad_metadata).resolve()
    validator = clone / "ci" / "validate-package.py"
    schemas = [clone / "schema-v1.json", clone / "schema-v2.json"]
    for need in [zip_path, sidecar, validator, *schemas]:
        if not need.exists():
            print(f"validate_pcm.py: {need} does not exist", file=sys.stderr)
            return 2

    head = subprocess.run(["git", "-C", str(clone), "rev-parse", "HEAD"], capture_output=True, text=True)
    print("KiCad metadata repository at", head.stdout.strip() or "(not a git checkout)")

    patched = update_metadata.patch(
        Path(args.metadata).read_text(encoding="utf-8"), update_metadata.load_sidecar(sidecar)
    )
    doc = json.loads(patched)
    identifier = doc["identifier"]

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        shipped = tmp / "metadata.shipped.json"
        shipped.write_text(patched, encoding="utf-8")
        print("== schema check of the metadata as released")
        r = subprocess.run([args.python, "-c", SCHEMA_CHECK, str(shipped), *map(str, schemas)])
        if r.returncode:
            return r.returncode

        web = tmp / "web"
        web.mkdir()
        (web / zip_path.name).symlink_to(zip_path)
        server, port = serve(web)
        try:
            doc["versions"][0]["download_url"] = f"http://127.0.0.1:{port}/{zip_path.name}"
            local = tmp / "metadata.json"
            local.write_text(json.dumps(doc, indent=2), encoding="utf-8")
            work = tmp / "work"
            work.mkdir()
            print(f"== ci/validate-package.py {identifier} (zip served from 127.0.0.1:{port})")
            cmd = [
                args.python, str(validator),
                "--schema-v1", str(schemas[0]), "--schema-v2", str(schemas[1]),
                identifier, str(local),
            ]  # fmt: skip
            r = subprocess.run(cmd, cwd=work)
        finally:
            server.shutdown()
    return r.returncode


if __name__ == "__main__":
    sys.exit(main())
