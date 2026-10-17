"""K03.4: patching the checksum and sizes of a built package into metadata.json."""

import hashlib
import json
import sys
import zipfile

import pytest
from conftest import PLUGIN_SRC

sys.path.insert(0, str(PLUGIN_SRC.parent))
import package  # noqa: E402
import update_metadata  # noqa: E402

PLUGIN_ROOT = PLUGIN_SRC.parent


@pytest.fixture
def built(tmp_path):
    bins = tmp_path / "bins"
    for platform in package.PLATFORMS:
        for tool in package.TOOLS:
            f = bins / f"bin-{platform}" / package.exe_name(tool, platform)
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_bytes(f"{platform} {tool}\n".encode() * 20)
    zip_path, _ = package.build([str(bins)], tmp_path / "out")
    return zip_path, zip_path.with_name(zip_path.name + ".meta.json")


def entry_of(path):
    (entry,) = json.loads(path.read_text(encoding="utf-8"))["versions"]
    return entry


def test_patched_values_equal_those_computed_from_the_zip(built, tmp_path):
    zip_path, sidecar = built
    meta = tmp_path / "metadata.json"
    meta.write_text((PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8"))
    assert update_metadata.main([str(meta), str(sidecar)]) == 0
    entry = entry_of(meta)
    assert entry["download_sha256"] == hashlib.sha256(zip_path.read_bytes()).hexdigest()
    assert entry["download_size"] == zip_path.stat().st_size
    with zipfile.ZipFile(zip_path) as zf:
        assert entry["install_size"] == sum(i.file_size for i in zf.infolist())
    # nothing else changed
    original = entry_of(PLUGIN_ROOT / "metadata.json")
    assert {k: v for k, v in entry.items() if k not in ("download_sha256", "download_size", "install_size")} == original
    # patching twice gives the same file
    first = meta.read_text()
    assert update_metadata.main([str(meta), str(sidecar)]) == 0
    assert meta.read_text() == first


def test_out_leaves_the_input_alone(built, tmp_path):
    _, sidecar = built
    meta = tmp_path / "metadata.json"
    text = (PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8")
    meta.write_text(text)
    out = tmp_path / "patched.json"
    assert update_metadata.main([str(meta), str(sidecar), "--out", str(out)]) == 0
    assert meta.read_text() == text
    assert "download_sha256" in entry_of(out)


def test_version_mismatch_fails_naming_both(built, tmp_path, capsys):
    _, sidecar = built
    meta = tmp_path / "metadata.json"
    doc = json.loads((PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8"))
    doc["versions"][0]["version"] = "0.2.0"
    meta.write_text(json.dumps(doc))
    before = meta.read_text()
    assert update_metadata.main([str(meta), str(sidecar)]) == 1
    err = capsys.readouterr().err
    assert "0.1.0" in err and "0.2.0" in err
    assert meta.read_text() == before  # a failed patch writes nothing


def test_url_for_another_file_fails(built, tmp_path, capsys):
    _, sidecar = built
    meta = tmp_path / "metadata.json"
    doc = json.loads((PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8"))
    doc["versions"][0]["download_url"] = "https://example.invalid/v0.1.0/other.zip"
    meta.write_text(json.dumps(doc))
    assert update_metadata.main([str(meta), str(sidecar)]) == 1
    assert "other.zip" in capsys.readouterr().err


def test_broken_sidecar_fails(tmp_path, capsys):
    side = tmp_path / "x.zip.meta.json"
    side.write_text('{"version": "0.1.0"}')
    assert update_metadata.main([str(PLUGIN_ROOT / "metadata.json"), str(side)]) == 1
    assert "sidecar" in capsys.readouterr().err
    side.write_text("not json")
    assert update_metadata.main([str(PLUGIN_ROOT / "metadata.json"), str(side)]) == 1
