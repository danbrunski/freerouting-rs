"""K03.5: KiCad's package validator on a built zip. Needs a clone of
gitlab.com/kicad/addons/metadata and a Python with its ci/requirements.txt, so it runs only
when KICAD_METADATA_REPO (the clone) is set; PCM_VALIDATOR_PYTHON picks the interpreter."""

import os
import sys
import zipfile
from pathlib import Path

import pytest
from conftest import PLUGIN_SRC

sys.path.insert(0, str(PLUGIN_SRC.parent))
import package  # noqa: E402
import validate_pcm  # noqa: E402

CLONE = os.environ.get("KICAD_METADATA_REPO")
needs_clone = pytest.mark.skipif(
    not CLONE or not (Path(CLONE or "x") / "ci" / "validate-package.py").is_file(),
    reason="set KICAD_METADATA_REPO to a clone of gitlab.com/kicad/addons/metadata",
)


def make_bins(root):
    for platform in package.PLATFORMS:
        for tool in package.TOOLS:
            f = root / f"bin-{platform}" / package.exe_name(tool, platform)
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_bytes(b"fake binary\n" * 100)


@needs_clone
def test_kicad_validator_accepts_the_package(tmp_path):
    make_bins(tmp_path / "bins")
    zip_path, _ = package.build([str(tmp_path / "bins")], tmp_path / "out")
    argv = ["--zip", str(zip_path), "--kicad-metadata", CLONE]
    if os.environ.get("PCM_VALIDATOR_PYTHON"):
        argv += ["--python", os.environ["PCM_VALIDATOR_PYTHON"]]
    assert validate_pcm.main(argv) == 0


@needs_clone
def test_kicad_validator_rejects_a_file_outside_the_allowed_paths(tmp_path):
    """The check can fail: a LICENSE at the zip root is 'an extra file'."""
    import hashlib
    import json

    make_bins(tmp_path / "bins")
    zip_path, sidecar = package.build([str(tmp_path / "bins")], tmp_path / "out")
    with zipfile.ZipFile(zip_path, "a") as zf:
        zf.writestr("LICENSE", "x")
    data = zip_path.read_bytes()
    with zipfile.ZipFile(zip_path) as zf:
        sidecar.update(
            sha256=hashlib.sha256(data).hexdigest(),
            download_size=len(data),
            install_size=sum(i.file_size for i in zf.infolist()),
        )
    zip_path.with_name(zip_path.name + ".meta.json").write_text(json.dumps(sidecar))
    argv = ["--zip", str(zip_path), "--kicad-metadata", CLONE]
    if os.environ.get("PCM_VALIDATOR_PYTHON"):
        argv += ["--python", os.environ["PCM_VALIDATOR_PYTHON"]]
    assert validate_pcm.main(argv) != 0
