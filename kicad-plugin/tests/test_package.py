"""K03.1 and K03.2: metadata.json and the PCM zip."""

import hashlib
import json
import os
import re
import shutil
import stat
import struct
import sys
import zipfile

import pytest
from conftest import PLUGIN_SRC, REPO

sys.path.insert(0, str(PLUGIN_SRC.parent))
import install  # noqa: E402
import package  # noqa: E402
import version  # noqa: E402

JAVA_IDENTIFIER = "app.freerouting.kicad-plugin"
PLUGIN_ROOT = PLUGIN_SRC.parent


@pytest.fixture
def fake_bins(tmp_path):
    """bin-<platform>/ folders as the release workflow's artifacts lay them out."""
    root = tmp_path / "bins"
    for platform in package.PLATFORMS:
        for tool in package.TOOLS:
            f = root / f"bin-{platform}" / package.exe_name(tool, platform)
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_bytes(f"fake {platform} {tool}\n".encode() * 50)
    return root


def build(fake_bins, out, **kw):
    zip_path, sidecar = package.build([str(fake_bins)], out, **kw)
    return zip_path, sidecar


def workspace_version():
    text = (REPO / "Cargo.toml").read_text(encoding="utf-8")
    table = text.split("[workspace.package]", 1)[1].split("\n[", 1)[0]
    return re.search(r'^version\s*=\s*"([^"]+)"', table, re.M).group(1)


# --- K03.1 -------------------------------------------------------------------


def test_metadata_is_consistent():
    meta = json.loads((PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8"))
    assert len(meta["versions"]) == 1
    entry = meta["versions"][0]
    assert entry["version"] == version.__version__ == workspace_version()
    assert re.fullmatch(r"\d+\.\d+\.\d+", entry["version"])  # three parts: the PCM rejects four
    assert meta["identifier"] == "com.github.danbrunski.freerouting-rs"
    assert meta["identifier"] != JAVA_IDENTIFIER
    assert meta["identifier"] in install.OUR_IDENTIFIERS
    assert JAVA_IDENTIFIER not in install.OUR_IDENTIFIERS
    assert meta["type"] == "plugin" and meta["license"] == "GPL-3.0"
    assert entry["runtime"] == "swig" and entry["status"] == "testing"
    assert entry["kicad_version"] == "10.0"
    # the download URL names the asset package.py writes, under this version's tag
    assert entry["download_url"].endswith(f"/download/v{entry['version']}/freerouting-rs-{entry['version']}-pcm.zip")
    # the source file carries no checksum or sizes: update_metadata.py adds them for a release
    assert not {"download_sha256", "download_size", "install_size"} & entry.keys()


def test_icon_is_64_px_png_within_the_validators_limits():
    data = (PLUGIN_ROOT / "resources" / "icon.png").read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n" and data[12:16] == b"IHDR"
    width, height = struct.unpack(">II", data[16:24])
    assert (width, height) == (64, 64)
    assert len(data) <= 20480  # KiCad's ci/validate/image.py: --max-icon-size


# --- K03.2 -------------------------------------------------------------------


def test_zip_is_deterministic(fake_bins, tmp_path):
    a, side_a = build(fake_bins, tmp_path / "a")
    # a later mtime on every input must not matter
    for f in fake_bins.rglob("*"):
        os.utime(f, (2_000_000_000, 2_000_000_000))
    b, side_b = build(fake_bins, tmp_path / "b")
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    assert sha(a) == sha(b) == side_a["sha256"] == side_b["sha256"]
    assert a.read_bytes() == b.read_bytes()


def expected_entries(platforms=package.PLATFORMS):
    names = {"metadata.json", "resources/icon.png"}
    for p in PLUGIN_SRC.iterdir():
        if p.is_file() and p.name != "settings.json":
            names.add(f"plugins/{p.name}")
    names |= {"plugins/LICENSE", "plugins/THIRD_PARTY_NOTICES.md"}
    for p in (REPO / "LICENSES").iterdir():
        names.add(f"plugins/LICENSES/{p.name}")
    for platform in platforms:
        for tool in package.TOOLS:
            names.add(f"plugins/bin/{platform}/{package.exe_name(tool, platform)}")
    return names


def test_zip_layout(fake_bins, tmp_path):
    zip_path, _ = build(fake_bins, tmp_path / "out")
    with zipfile.ZipFile(zip_path) as zf:
        assert zf.testzip() is None
        files = [i for i in zf.infolist() if not i.is_dir()]
        names = [i.filename for i in files]
        assert names == sorted(names)
        assert set(names) == expected_entries()
        for i in files:
            assert not any(part in ("__pycache__", "tests") for part in i.filename.split("/"))
            assert not i.filename.endswith((".pyc", "settings.json"))
            mode = (i.external_attr >> 16) & 0o777
            is_binary = "/bin/" in i.filename
            assert mode == (0o755 if is_binary else 0o644), i.filename
            assert i.date_time == (1980, 1, 1, 0, 0, 0)
            assert i.create_system == 3
        for i in zf.infolist():
            if i.is_dir():
                assert (i.external_attr >> 16) & 0o777 == 0o755
        # every directory entry has its parent
        dirs = {i.filename for i in zf.infolist() if i.is_dir()}
        for name in names:
            parent = name.rsplit("/", 1)[0] + "/" if "/" in name else None
            assert parent is None or parent in dirs


def test_denylist_not_allowlist(fake_bins, tmp_path):
    """A new module is packaged without touching package.py; caches and local files are not."""
    src = tmp_path / "src"
    shutil.copytree(PLUGIN_SRC, src, ignore=shutil.ignore_patterns("__pycache__"))
    (src / "brand_new_module.py").write_text("x = 1\n")
    (src / "sub").mkdir()
    (src / "sub" / "helper.py").write_text("y = 2\n")
    (src / "settings.json").write_text('{"bin_dir": "/home/dan/secret"}')
    (src / "__pycache__").mkdir()
    (src / "__pycache__" / "a.cpython-312.pyc").write_bytes(b"\0")
    (src / "tests").mkdir()
    (src / "tests" / "test_x.py").write_text("")
    (src / "bin" / "linux-x86_64").mkdir(parents=True)
    (src / "bin" / "linux-x86_64" / "freerouting").write_text("stale local build")
    (src / "mod.pyc").write_bytes(b"\0")
    zip_path, _ = build(fake_bins, tmp_path / "out", plugin_src=src)
    with zipfile.ZipFile(zip_path) as zf:
        names = set(zf.namelist())
        assert zf.read("plugins/bin/linux-x86_64/freerouting") == zf_bytes(fake_bins, "linux-x86_64", "freerouting")
    assert "plugins/brand_new_module.py" in names and "plugins/sub/helper.py" in names
    for bad in ("plugins/settings.json", "plugins/__pycache__/a.cpython-312.pyc", "plugins/tests/test_x.py", "plugins/mod.pyc"):
        assert bad not in names
    assert "plugins/bin/linux-x86_64/freerouting" in names  # the bundled one, not the stale copy


def zf_bytes(fake_bins, platform, tool):
    return (fake_bins / f"bin-{platform}" / package.exe_name(tool, platform)).read_bytes()


def test_inner_metadata(fake_bins, tmp_path):
    zip_path, _ = build(fake_bins, tmp_path / "out")
    outer = json.loads((PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8"))
    with zipfile.ZipFile(zip_path) as zf:
        inner = json.loads(zf.read("metadata.json"))
    assert len(inner["versions"]) == 1
    entry = inner["versions"][0]
    for field in ("download_url", "download_sha256", "download_size", "install_size"):
        assert field not in entry
    assert entry["version"] == outer["versions"][0]["version"]
    # everything else, including the platforms and kicad_version the validator compares, is unchanged
    wanted = {k: v for k, v in outer["versions"][0].items() if not k.startswith("download_")}
    assert entry == wanted
    assert {k: v for k, v in inner.items() if k != "versions"} == {k: v for k, v in outer.items() if k != "versions"}


def test_sidecar_matches_the_zip(fake_bins, tmp_path):
    zip_path, sidecar = build(fake_bins, tmp_path / "out")
    on_disk = json.loads(zip_path.with_name(zip_path.name + ".meta.json").read_text(encoding="utf-8"))
    assert on_disk == sidecar
    assert sidecar["file"] == zip_path.name == f"freerouting-rs-{version.__version__}-pcm.zip"
    assert sidecar["version"] == version.__version__
    assert sidecar["sha256"] == hashlib.sha256(zip_path.read_bytes()).hexdigest()
    assert sidecar["download_size"] == zip_path.stat().st_size
    with zipfile.ZipFile(zip_path) as zf:
        assert sidecar["install_size"] == sum(i.file_size for i in zf.infolist())


def test_missing_platform_is_an_error_unless_allowed(fake_bins, tmp_path):
    shutil.rmtree(fake_bins / "bin-macos-universal")
    with pytest.raises(package.PackageError, match="macos-universal"):
        build(fake_bins, tmp_path / "out")
    zip_path, _ = build(fake_bins, tmp_path / "out", allow_missing=True)
    with zipfile.ZipFile(zip_path) as zf:
        files = {i.filename for i in zf.infolist() if not i.is_dir()}
    assert files == expected_entries(("windows-x86_64", "linux-x86_64"))


def test_missing_tool_is_an_error(fake_bins, tmp_path):
    (fake_bins / "bin-windows-x86_64" / "freerouting-gui.exe").unlink()
    with pytest.raises(package.PackageError, match="freerouting-gui.exe"):
        build(fake_bins, tmp_path / "out")


def test_explicit_platform_folders_and_plain_names(tmp_path):
    """PLATFORM=DIR works, and so does <platform>/ without the bin- prefix."""
    specs = []
    for platform in package.PLATFORMS:
        d = tmp_path / "x" / platform.upper()  # arbitrary names via PLATFORM=DIR
        d.mkdir(parents=True)
        for tool in package.TOOLS:
            (d / package.exe_name(tool, platform)).write_text(tool)
        specs.append(f"{platform}={d}")
    zip_path, _ = package.build(specs, tmp_path / "out")
    with zipfile.ZipFile(zip_path) as zf:
        assert {i.filename for i in zf.infolist() if not i.is_dir()} == expected_entries()


def test_version_mismatch_is_refused(fake_bins, tmp_path):
    meta = json.loads((PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8"))
    meta["versions"][0]["version"] = "9.9.9"
    bad = tmp_path / "metadata.json"
    bad.write_text(json.dumps(meta))
    with pytest.raises(package.PackageError, match="9.9.9.*0.1.0|0.1.0.*9.9.9"):
        build(fake_bins, tmp_path / "out", metadata_path=bad)


def test_two_version_entries_are_refused(fake_bins, tmp_path):
    meta = json.loads((PLUGIN_ROOT / "metadata.json").read_text(encoding="utf-8"))
    meta["versions"].append(dict(meta["versions"][0], version="0.0.9"))
    bad = tmp_path / "metadata.json"
    bad.write_text(json.dumps(meta))
    with pytest.raises(package.PackageError, match="exactly one"):
        build(fake_bins, tmp_path / "out", metadata_path=bad)


def test_cli_prints_the_zip_path(fake_bins, tmp_path, capsys):
    assert package.main(["--bin", str(fake_bins), "--out", str(tmp_path / "out")]) == 0
    out = capsys.readouterr()
    assert out.out.strip() == str(tmp_path / "out" / f"freerouting-rs-{version.__version__}-pcm.zip")
    assert package.main(["--bin", str(tmp_path / "empty"), "--out", str(tmp_path / "o2")]) == 1
    assert "package.py:" in capsys.readouterr().err


def test_executables_keep_their_bytes(fake_bins, tmp_path):
    zip_path, _ = build(fake_bins, tmp_path / "out")
    with zipfile.ZipFile(zip_path) as zf:
        for platform in package.PLATFORMS:
            for tool in package.TOOLS:
                exe = package.exe_name(tool, platform)
                assert zf.read(f"plugins/bin/{platform}/{exe}") == zf_bytes(fake_bins, platform, tool)
                mode = zf.getinfo(f"plugins/bin/{platform}/{exe}").external_attr >> 16
                assert stat.S_ISREG(mode) and mode & 0o777 == 0o755
