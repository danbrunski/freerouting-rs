import json

import progress
import summary
from conftest import TESTS

DATA = TESTS / "data"


def manifest(name):
    return summary.load_manifest(DATA / f"result-{name}.json")


def test_a_complete_run_has_no_warning():
    # Elapsed time from the recorded `done` event of progress-j2.jsonl (396 ms).
    assert summary.format_summary(manifest("j2"), elapsed_ms=396) == (
        "Profile: Enhanced\n"
        "Connections: 33 of 33 routed, 0 incomplete\n"
        "Vias: 16\n"
        "Total trace length: 371.2 mm\n"
        "Time: 0.4 s"
    )


def test_an_incomplete_run_ends_with_a_warning():
    assert summary.format_summary(manifest("bm01"), elapsed_ms=6841) == (
        "Profile: Enhanced\n"
        "Connections: 146 of 195 routed, 49 incomplete\n"
        "Vias: 120\n"
        "Total trace length: 2726.7 mm\n"
        "Time: 6.8 s\n"
        "\n"
        "WARNING: 49 connections are not routed. Check the board before you use it."
    )


def test_one_incomplete_connection_is_singular():
    m = manifest("j2")
    m["board_statistics"]["connections"]["incomplete_count"] = 1
    assert "WARNING: 1 connection is not routed." in summary.format_summary(m, 1000)


def test_the_time_falls_back_to_the_phase_durations():
    m = manifest("j2")
    seconds = sum(p["duration_seconds"] for p in m["phases"].values() if p["duration_seconds"] is not None)
    assert summary.elapsed_seconds(m) == seconds
    assert f"Time: {progress.format_seconds(seconds)}" in summary.format_summary(m)


def test_the_recordings_agree_with_the_progress_streams():
    for name in ("j2", "bm01"):
        last = None
        for line in (DATA / f"progress-{name}.jsonl").read_text(encoding="utf-8").splitlines():
            event = progress.parse_line(line)
            if event and event["event"] == "progress" and event["incomplete"] is not None:
                last = event
        assert summary.incomplete_count(manifest(name)) == last["incomplete"]


def test_other_states_and_violations_are_named():
    m = manifest("j2")
    m["final_state"] = "TIMED_OUT"
    m["board_statistics"]["clearance_violations"]["total_count"] = 3
    text = summary.format_summary(m, 1000)
    assert "Run state: TIMED_OUT" in text and "Clearance violations: 3" in text


def test_a_damaged_manifest_reads_as_unknown_never_as_an_exception():
    assert summary.format_summary(None).startswith("No result summary")
    assert summary.format_summary({}) == (
        "Profile: Enhanced\nConnections: unknown\nVias: unknown\nTotal trace length: unknown\nTime: unknown"
    )
    text = summary.format_summary({"board_statistics": {"connections": "x", "vias": [], "traces": None}, "phases": 3})
    assert "Connections: unknown" in text and "Vias: unknown" in text


def test_load_manifest_handles_missing_and_bad_files(tmp_path):
    assert summary.load_manifest(tmp_path / "none.json") is None
    bad = tmp_path / "bad.json"
    bad.write_text("[1]", encoding="utf-8")
    assert summary.load_manifest(bad) is None
    bad.write_text("{oops", encoding="utf-8")
    assert summary.load_manifest(bad) is None
    good = tmp_path / "good.json"
    good.write_text(json.dumps({"final_state": "COMPLETED"}), encoding="utf-8")
    assert summary.load_manifest(good) == {"final_state": "COMPLETED"}


def test_the_summary_names_the_profile():
    m = manifest("j2")
    # Asked for, the manifest silent: the plugin's profile.
    assert summary.format_summary(m, 396, profile="legacy").startswith("Profile: Legacy\n")
    assert summary.format_summary(m, 396, profile="enhanced").startswith("Profile: Enhanced\n")
    # The manifest's settings snapshot wins (the router's own word).
    m["settings_snapshot"] = {"profile": "legacy"}
    assert summary.format_summary(m, 396, profile="enhanced").startswith("Profile: Legacy\n")
    m["settings_snapshot"] = {"profile": 3}
    assert summary.format_summary(m, 396).startswith("Profile: Enhanced\n")
