"""Tests for check_version.py (R01.1). Run: python3 -m pytest tools/release
(or python3 -m unittest discover -s tools/release -p 'test_*.py')."""

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))

import check_version  # noqa: E402

URL = "https://github.com/danbrunski/freerouting-rs/releases/download/v{v}/freerouting-rs-kicad-plugin-{v}.zip"


def write_tree(root, workspace="0.1.0", plugin="0.1.0", metadata="0.1.0", url="0.1.0", with_plugin=True, packager="0.1.0"):
    root = Path(root)
    (root / "Cargo.toml").write_text(
        f'[workspace]\nmembers = ["crates/*"]\n\n[workspace.package]\nversion = "{workspace}"\n'
        '\n[profile.dev]\nopt-level = 1\n',
        encoding="utf-8",
    )
    (root / "Packager.toml").write_text(f'product-name = "x"\nversion = "{packager}"\n', encoding="utf-8")
    if with_plugin:
        pkg = root / "kicad-plugin" / "freerouting_rs"
        pkg.mkdir(parents=True)
        (pkg / "version.py").write_text(f'__version__ = "{plugin}"\n', encoding="utf-8")
        (root / "kicad-plugin" / "metadata.json").write_text(
            json.dumps({"versions": [{"version": metadata, "download_url": URL.format(v=url)}]}),
            encoding="utf-8",
        )


class GateTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def gate(self, tag=None):
        lines, ok = check_version.check(self.root, tag)
        return "\n".join(lines), ok

    def test_all_equal_passes(self):
        write_tree(self.root)
        text, ok = self.gate("v0.1.0")
        self.assertTrue(ok, text)
        for label in ("workspace", "packager", "plugin", "metadata", "download_url", "tag"):
            self.assertIn(label, text)

    def test_all_equal_without_tag_passes(self):
        write_tree(self.root)
        self.assertTrue(self.gate()[1])

    def test_each_single_mismatch_fails_and_names_the_file(self):
        cases = {
            "workspace": ({"workspace": "0.2.0"}, "v0.1.0", "Cargo.toml"),
            "packager": ({"packager": "0.2.0"}, "v0.1.0", "Packager.toml"),
            "plugin": ({"plugin": "0.2.0"}, "v0.1.0", "version.py"),
            "metadata": ({"metadata": "0.2.0"}, "v0.1.0", "metadata.json"),
            "download_url": ({"url": "0.2.0"}, "v0.1.0", "metadata.json"),
            "tag": ({}, "v0.2.0", "git tag"),
        }
        for name, (kw, tag, filename) in cases.items():
            with self.subTest(name):
                shutil.rmtree(self.root)
                self.root.mkdir()
                write_tree(self.root, **kw)
                text, ok = self.gate(tag)
                self.assertFalse(ok, text)
                error_lines = [l for l in text.splitlines() if l.startswith("ERROR")]
                self.assertTrue(any(filename in l for l in error_lines), text)

    def test_four_part_version_fails_even_when_all_agree(self):
        write_tree(self.root, "0.1.0.1", "0.1.0.1", "0.1.0.1", "0.1.0.1")
        text, ok = self.gate("v0.1.0.1")
        self.assertFalse(ok, text)
        self.assertIn("MAJOR.MINOR.PATCH", text)

    def test_pre_release_suffix_fails(self):
        write_tree(self.root, "0.1.0-rc1", "0.1.0-rc1", "0.1.0-rc1", "0.1.0-rc1")
        self.assertFalse(self.gate()[1])

    def test_missing_plugin_files_are_skipped(self):
        write_tree(self.root, with_plugin=False)
        text, ok = self.gate("v0.1.0")
        self.assertTrue(ok, text)
        self.assertIn("not yet present", text)

    def test_tag_without_v_fails(self):
        write_tree(self.root)
        self.assertFalse(self.gate("0.1.0")[1])

    def test_two_metadata_entries_fail(self):
        write_tree(self.root)
        meta = self.root / "kicad-plugin" / "metadata.json"
        entry = json.loads(meta.read_text())["versions"][0]
        meta.write_text(json.dumps({"versions": [entry, entry]}))
        self.assertFalse(self.gate()[1])

    def test_missing_cargo_toml_fails(self):
        write_tree(self.root)
        (self.root / "Cargo.toml").unlink()
        self.assertFalse(self.gate()[1])

    def test_only_the_workspace_package_version_counts(self):
        write_tree(self.root)
        cargo = self.root / "Cargo.toml"
        cargo.write_text('[package]\nversion = "9.9.9"\n' + cargo.read_text())
        self.assertTrue(self.gate()[1])


class NegativeControlTest(unittest.TestCase):
    """The script as the workflow runs it, against a copy of the real repository files."""

    def run_gate(self, root, *args):
        return subprocess.run(
            [sys.executable, str(HERE / "check_version.py"), "--root", str(root), *args],
            capture_output=True,
            text=True,
        )

    def test_real_repository_passes(self):
        r = self.run_gate(REPO)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def test_planted_mismatch_in_a_copy_of_the_repository_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            shutil.copy(REPO / "Cargo.toml", tmp / "Cargo.toml")
            shutil.copy(REPO / "Packager.toml", tmp / "Packager.toml")
            shutil.copytree(REPO / "kicad-plugin" / "freerouting_rs", tmp / "kicad-plugin" / "freerouting_rs",
                            ignore=shutil.ignore_patterns("__pycache__"))
            self.assertEqual(self.run_gate(tmp).returncode, 0)
            version_py = tmp / "kicad-plugin" / "freerouting_rs" / "version.py"
            version_py.write_text(
                version_py.read_text().replace(
                    check_version.plugin_version(version_py.read_text()), "9.9.9"
                )
            )
            r = self.run_gate(tmp)
            self.assertEqual(r.returncode, 1, r.stdout)
            self.assertIn("version.py", r.stdout)
            self.assertIn("FAILED", r.stdout)

    def test_cli_tag_mismatch_exits_one(self):
        r = self.run_gate(REPO, "--tag", "v99.0.0")
        self.assertEqual(r.returncode, 1, r.stdout)


if __name__ == "__main__":
    unittest.main()
