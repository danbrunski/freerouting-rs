"""Tests for package_archive.py and sha256sums.py (R01.4, R01.7)."""

import contextlib
import hashlib
import io
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))

import package_archive  # noqa: E402
import sha256sums  # noqa: E402


def fake_bins(directory, exe=""):
    directory.mkdir(parents=True, exist_ok=True)
    for name in ("freerouting", "freerouting-gui"):
        (directory / (name + exe)).write_bytes(b"\x7fELF fake " + name.encode())


class ArchiveTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_linux_tar_lists_binaries_plugin_and_notices(self):
        fake_bins(self.tmp / "bin")
        path = package_archive.build(REPO, self.tmp / "bin", self.tmp / "out", "linux-x86_64", "9.8.7")
        self.assertEqual(path.name, "freerouting-rs-9.8.7-linux-x86_64.tar.gz")
        with tarfile.open(path) as tar:
            names = tar.getnames()
            top = "freerouting-rs-9.8.7-linux-x86_64/"
            for want in ("freerouting", "freerouting-gui", "README.md", "LICENSE",
                         "THIRD_PARTY_NOTICES.md", "kicad-plugin/freerouting_rs/plugin.py"):
                self.assertIn(top + want, names)
            self.assertTrue(any(n.startswith(top + "LICENSES/") for n in names))
            self.assertFalse(any("__pycache__" in n or "/tests/" in n for n in names), names)
            self.assertEqual(tar.getmember(top + "freerouting").mode, 0o755)
            self.assertEqual(tar.getmember(top + "README.md").mode, 0o644)

    def test_windows_zip_has_exe_names(self):
        fake_bins(self.tmp / "bin", ".exe")
        path = package_archive.build(REPO, self.tmp / "bin", self.tmp / "out", "windows-x86_64", "9.8.7")
        self.assertEqual(path.suffix, ".zip")
        with zipfile.ZipFile(path) as z:
            names = z.namelist()
        top = "freerouting-rs-9.8.7-windows-x86_64/"
        for want in ("freerouting.exe", "freerouting-gui.exe", "THIRD_PARTY_NOTICES.md", "LICENSE"):
            self.assertIn(top + want, names)

    def test_archive_is_reproducible(self):
        fake_bins(self.tmp / "bin")
        a = package_archive.build(REPO, self.tmp / "bin", self.tmp / "a", "linux-x86_64", "1.0.0")
        b = package_archive.build(REPO, self.tmp / "bin", self.tmp / "b", "linux-x86_64", "1.0.0")
        self.assertEqual(a.read_bytes(), b.read_bytes())

    def test_missing_binary_is_an_error(self):
        (self.tmp / "bin").mkdir()
        with self.assertRaises(SystemExit):
            package_archive.build(REPO, self.tmp / "bin", self.tmp / "out", "linux-x86_64", "1.0.0")

    def test_default_version_is_the_workspace_version(self):
        fake_bins(self.tmp / "bin")
        path = package_archive.build(REPO, self.tmp / "bin", self.tmp / "out", "macos-universal")
        self.assertRegex(path.name, r"^freerouting-rs-\d+\.\d+\.\d+-macos-universal\.tar\.gz$")


def quiet_main(argv):
    with contextlib.redirect_stdout(io.StringIO()):
        return sha256sums.main(argv)


class ChecksumTest(unittest.TestCase):
    def test_sums_verify_with_sha256sum(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "b.zip").write_bytes(b"two")
            (tmp / "a.tar.gz").write_bytes(b"one")
            out = tmp / "SHA256SUMS"
            self.assertEqual(quiet_main([str(tmp)]), 0)
            lines = out.read_text().splitlines()
            self.assertEqual([l.split("  ")[1] for l in lines], ["a.tar.gz", "b.zip"])
            self.assertEqual(lines[0].split("  ")[0], hashlib.sha256(b"one").hexdigest())
            if shutil.which("sha256sum"):
                r = subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=tmp, capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
                (tmp / "a.tar.gz").write_bytes(b"tampered")
                r = subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=tmp, capture_output=True, text=True)
                self.assertNotEqual(r.returncode, 0)

    def test_a_rerun_does_not_hash_the_old_sums_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "x").write_bytes(b"x")
            quiet_main([tmp])
            first = (Path(tmp) / "SHA256SUMS").read_text()
            quiet_main([tmp])
            self.assertEqual(first, (Path(tmp) / "SHA256SUMS").read_text())

    def test_empty_directory_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(quiet_main([tmp]), 1)


if __name__ == "__main__":
    unittest.main()
