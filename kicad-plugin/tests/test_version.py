import re

import tools
import version
from conftest import REPO


def test_plugin_version_equals_workspace_version():
    cargo = (REPO / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'^version\s*=\s*"([^"]+)"', cargo, re.M)  # first = [workspace.package]
    assert m, "no version in Cargo.toml"
    assert version.__version__ == m.group(1)


def test_parse_router_version():
    assert version.parse_router_version("freerouting 0.1.0 (abc1234)\n") == (0, 1, 0, "abc1234")
    assert version.parse_router_version("freerouting 1.22.3") == (1, 22, 3, None)
    assert version.parse_router_version("\nfreerouting 0.1.0-rc1 (deadbee)") == (0, 1, 0, "deadbee")
    assert version.parse_router_version("freerouting 0.1.0 (abc1234)\nmore") == (0, 1, 0, "abc1234")
    # C08's real line: "(git <sha>)".
    line = "freerouting 0.1.0 (git 813663cea882)"
    assert version.parse_router_version(line) == (0, 1, 0, "813663cea882")
    for bad in ("", None, "Usage: freerouting", "freerouting 0.1 (x)", "other 0.1.0 (x)", "freerouting x.y.z"):
        assert version.parse_router_version(bad) is None


def test_check_accepts_same_major_minor_any_patch():
    assert version.check((0, 1, 0, "abc"), "0.1.0")[0] == version.OK
    assert version.check((0, 1, 9, "abc"), "0.1.0")[0] == version.OK


def test_check_refuses_mismatch_and_names_both_versions():
    status, msg = version.check((0, 2, 0, "abc1234"), "0.1.0")
    assert status == version.MISMATCH
    assert "0.2.0" in msg and "0.1.0" in msg and "reinstall the plugin" in msg and "Rebuild" in msg
    assert version.check((1, 1, 0, None), "0.1.0")[0] == version.MISMATCH


def test_check_unknown_is_allowed_with_warning():
    status, msg = version.check(None, "0.1.0")
    assert status == version.UNKNOWN and "did not report a version" in msg


# --- through fake executables -------------------------------------------------


def query(make_fake_router, mode, **kw):
    return tools.query_version(tools.Tool("freerouting", make_fake_router(mode)), **kw)


def test_fake_matching_version_accepted(make_fake_router):
    parsed = query(make_fake_router, "ok")
    assert parsed == (0, 1, 0, "abc1234")
    assert version.check(parsed, "0.1.0")[0] == version.OK


def test_fake_newer_minor_refused_message_names_both(make_fake_router):
    parsed = query(make_fake_router, "newer")
    status, msg = version.check(parsed, "0.1.0")
    assert status == version.MISMATCH
    assert "0.2.0 (abc1234)" in msg and "0.1.0" in msg


def test_fake_garbage_is_unknown(make_fake_router):
    parsed = query(make_fake_router, "garbage")
    assert parsed is None
    assert version.check(parsed)[0] == version.UNKNOWN


def test_fake_nonzero_exit_is_unknown(make_fake_router):
    parsed = query(make_fake_router, "fail")
    assert parsed is None
    assert version.check(parsed)[0] == version.UNKNOWN


def test_fake_sleeping_past_timeout_is_unknown(make_fake_router):
    assert query(make_fake_router, "sleep", timeout=0.5) is None


def test_missing_executable_is_unknown(tmp_path):
    assert tools.query_version(tools.Tool("freerouting", tmp_path / "nope")) is None
