"""R01.2: build.rs of the engine crate, compiled and run on its own (seconds, no
cargo build of the engine). Checks the cases a unit test inside the crate
cannot reach: a source tarball without .git, a tarball unpacked inside some
other repository, the CI override, a real checkout, a linked worktree."""

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BUILD_RS = REPO / "crates" / "freerouting" / "build.rs"
HEX12 = re.compile(r"^[0-9a-f]{12}$")


def git(cwd, *args):
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@t", *args],
        cwd=cwd, check=True, capture_output=True, env=env,
    )


class BuildStampTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls._bin_dir = tempfile.TemporaryDirectory()
        cls.exe = Path(cls._bin_dir.name) / "stamp_build"
        subprocess.run(
            ["rustc", "--edition", "2024", "-o", str(cls.exe), str(BUILD_RS)],
            check=True, capture_output=True,
        )

    @classmethod
    def tearDownClass(cls):
        cls._bin_dir.cleanup()

    def stamp(self, root, env_sha=None, profile=None):
        """Runs build.rs as cargo would: CARGO_MANIFEST_DIR = <root>/crates/freerouting."""
        manifest = Path(root) / "crates" / "freerouting"
        manifest.mkdir(parents=True, exist_ok=True)
        env = {k: v for k, v in os.environ.items() if k != "FREEROUTING_GIT_SHA" and not k.startswith("GIT_")}
        env["CARGO_MANIFEST_DIR"] = str(manifest)
        if env_sha is not None:
            env["FREEROUTING_GIT_SHA"] = env_sha
        if profile is not None:
            env["PROFILE"] = profile
        r = subprocess.run([str(self.exe)], capture_output=True, text=True, env=env, cwd=manifest)
        self.assertEqual(r.returncode, 0, r.stderr)
        m = re.search(r"^cargo:rustc-env=FREEROUTING_GIT_SHA=(.*)$", r.stdout, re.M)
        self.assertIsNotNone(m, r.stdout)
        return m.group(1), r.stdout

    def test_tarball_without_git_is_unknown(self):
        with tempfile.TemporaryDirectory() as tmp:
            sha, out = self.stamp(tmp)
            self.assertEqual(sha, "unknown")
            self.assertNotIn("rerun-if-changed=" + tmp + "/crates/freerouting/../../.git", out)

    def test_tarball_inside_another_repository_is_unknown(self):
        with tempfile.TemporaryDirectory() as tmp:
            git(tmp, "init", "-q")
            (Path(tmp) / "f").write_text("x")
            git(tmp, "add", "f")
            git(tmp, "commit", "-q", "-m", "x")
            sha, _ = self.stamp(Path(tmp) / "vendor" / "freerouting-rs")
            self.assertEqual(sha, "unknown")

    def test_checkout_gives_twelve_hex_digits_matching_head(self):
        with tempfile.TemporaryDirectory() as tmp:
            git(tmp, "init", "-q")
            (Path(tmp) / "f").write_text("x")
            git(tmp, "add", "f")
            git(tmp, "commit", "-q", "-m", "x")
            head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=tmp, capture_output=True, text=True).stdout.strip()
            sha, out = self.stamp(tmp)
            self.assertRegex(sha, HEX12)
            self.assertEqual(sha, head[:12])
            self.assertIn("rerun-if-changed=" + str(Path(tmp) / "crates/freerouting/../..") + "/.git/HEAD", out)

    def test_debug_profile_does_not_watch_head(self):
        with tempfile.TemporaryDirectory() as tmp:
            git(tmp, "init", "-q")
            (Path(tmp) / "f").write_text("x")
            git(tmp, "add", "f")
            git(tmp, "commit", "-q", "-m", "x")
            sha, out = self.stamp(tmp, profile="debug")
            self.assertRegex(sha, HEX12)
            self.assertNotIn(".git/HEAD", out)
            self.assertIn(".git/HEAD", self.stamp(tmp, profile="release")[1])

    def test_linked_worktree_is_stamped_and_watches_its_head(self):
        with tempfile.TemporaryDirectory() as tmp:
            main = Path(tmp) / "main"
            main.mkdir()
            git(main, "init", "-q")
            (main / "f").write_text("x")
            git(main, "add", "f")
            git(main, "commit", "-q", "-m", "x")
            wt = Path(tmp) / "wt"
            git(main, "worktree", "add", "-q", "--detach", str(wt))
            sha, out = self.stamp(wt)
            self.assertRegex(sha, HEX12)
            self.assertIn("rerun-if-changed=", out)

    def test_ci_override_is_truncated_to_twelve(self):
        with tempfile.TemporaryDirectory() as tmp:
            full = "0123456789abcdef0123456789abcdef01234567"
            self.assertEqual(self.stamp(tmp, full)[0], full[:12])

    def test_bad_override_is_ignored(self):
        with tempfile.TemporaryDirectory() as tmp:
            for bad in ("", "nothex", "abc123"):
                self.assertEqual(self.stamp(tmp, bad)[0], "unknown", bad)

    def test_real_checkout(self):
        if not (REPO / ".git").exists():
            self.skipTest("not a git checkout")
        sha, _ = self.stamp(REPO)
        self.assertRegex(sha, HEX12)


if __name__ == "__main__":
    unittest.main()
