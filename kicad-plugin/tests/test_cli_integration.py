"""The plugin's route path against the real `freerouting` CLI (K02.1 to K02.4).

Skipped, with the reason, when no `freerouting` that routes is found: the
directory in `FREEROUTING_RS_BIN`, else `target/release` or `target/debug` of
the repository. Boards come from `fixtures/` (small ones; each test routes
for a few seconds, longer in a debug build).
"""

import json
import os
import shutil
import subprocess
import time

import options
import pytest
import routing
import summary
import tools
from conftest import REPO

SMALL = "Issue026-J2_reference.dsn"  # routes in under a second
BIG = "Issue508-DAC2020_bm01.dsn"  # still routing after its first progress event
# Its fanout spends minutes in one pin (the GND net's normalization oscillates);
# before 2026-10-18 a Stop there waited for the pin to end.
LONG_STEP = "Issue690-kit-dev-coldfire-xilinx_5213.dsn"


def find_router():
    candidates = []
    if os.environ.get("FREEROUTING_RS_BIN"):
        candidates.append(os.environ["FREEROUTING_RS_BIN"])
    candidates += [str(REPO / "target" / "release"), str(REPO / "target" / "debug")]
    for d in candidates:
        exe = os.path.join(d, "freerouting" + tools.EXE_SUFFIX)
        if os.path.isfile(exe):
            return exe
    return None


@pytest.fixture(scope="module")
def router():
    exe = find_router()
    if exe is None:
        pytest.skip("no freerouting executable (set FREEROUTING_RS_BIN or build target/release)")
    probe = subprocess.run([exe, "--capabilities"], capture_output=True, text=True, timeout=60)
    if probe.returncode != 0 or '"progress"' not in probe.stdout:
        pytest.skip(f"{exe} predates C08 (--capabilities, --progress json)")
    return tools.Tool("freerouting", exe)


@pytest.fixture
def board(tmp_path):
    def make(name):
        work = tmp_path / "freerouting-rs"
        work.mkdir(exist_ok=True)
        dsn = work / name
        shutil.copy(REPO / "fixtures" / name, dsn)
        return work, dsn, dsn.with_suffix(".ses")

    return make


def start(router, dsn, ses, work, args=()):
    run = routing.RouteRun(router, dsn, ses, list(args), routing.manifest_path(work, dsn.stem))
    run.start()
    return run


def drc(router, dsn, ses, report=None):
    """`freerouting -de <dsn> <ses> -drc [report]`: (log text, report dict)."""
    args = ["-de", str(dsn), str(ses), "-drc"] + ([str(report)] if report else [])
    done = subprocess.run([str(router.exe), *args], capture_output=True, text=True, timeout=300)
    assert done.returncode == 0, done.stdout + done.stderr
    return done.stdout + done.stderr, json.loads(report.read_text(encoding="utf-8")) if report else None


def wait_for_route_event(run, timeout=120):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        run.poll()
        if run.state.phase == "route":
            return
        assert not run.finished, "the run ended before its first route event: " + "\n".join(run.lines[-10:])
        time.sleep(0.02)
    raise AssertionError("no route event")


def test_all_options_reach_the_real_cli_and_it_exits_0(router, board):
    work, dsn, ses = board(SMALL)
    chosen = options.RouteOptions(
        max_passes=2,
        threads=2,
        optimizer=False,
        improvement_threshold=5.0,
        ignore_net_classes=["NoSuchClass"],
        seed=0,
        min_trace_width_um=150.0,
        timeout="10m",
    )
    run = start(router, dsn, ses, work, chosen.to_args(board_min_trace_width_um=100.0))
    assert run.wait(timeout=300) == 0, "\n".join(run.lines[-15:])
    assert run.state.done and run.state.state == "completed" and run.ses_written()
    assert not any("Unknown" in line or "Deprecated" in line for line in run.lines), run.lines
    manifest = run.manifest()
    snapshot = manifest["settings_snapshot"]
    assert snapshot["autorouter"]["max_passes"] == 2
    assert snapshot["autorouter"]["max_threads"] == 2 and snapshot["optimizer"]["max_threads"] == 2
    assert snapshot["optimizer"]["enabled"] is False and snapshot["optimizer"]["improvement_threshold"] == 5.0
    assert snapshot["autorouter"]["ignore_net_classes"] == ["NoSuchClass"]
    assert snapshot["job_timeout"] == "00:10:00"


def test_an_escaped_comma_in_a_net_class_name_reaches_the_cli(router, board):
    work, dsn, ses = board(SMALL)
    chosen = options.RouteOptions(max_passes=1, ignore_net_classes=["Power,Default", "GND"])
    run = start(router, dsn, ses, work, chosen.to_args())
    assert run.wait(timeout=300) == 0, "\n".join(run.lines[-15:])
    assert run.manifest()["settings_snapshot"]["autorouter"]["ignore_net_classes"] == ["Power,Default", "GND"]


def test_stop_after_the_first_route_event_keeps_a_loadable_board(router, board):
    work, dsn, ses = board(BIG)
    run = start(router, dsn, ses, work, ["--max-passes", "50"])
    wait_for_route_event(run)
    assert not run.finished
    assert run.stop() is True
    assert run.wait(timeout=600) == 0, "\n".join(run.lines[-15:])
    assert run.stopped and run.state.state == "completed" and run.ses_written()
    # `freerouting -de dsn ses -drc` loads the session with no import error.
    log, _ = drc(router, dsn, ses)
    assert "SES file loaded for DRC:" in log and "Failed to load" not in log and "rror" not in log, log
    # The plugin never leaves a temp file behind.
    assert sorted(p.name for p in work.iterdir()) == sorted([dsn.name, ses.name, f"{dsn.stem}.result.json"])


def test_abort_at_the_same_point_leaves_no_ses_and_no_temp_file(router, board):
    work, dsn, ses = board(BIG)
    run = start(router, dsn, ses, work, ["--max-passes", "50"])
    wait_for_route_event(run)
    run.abort()
    run.wait(timeout=60)
    assert run.aborted and run.returncode != 0
    assert not ses.exists()
    leftover = sorted(p.name for p in work.iterdir())
    assert not any(n.endswith((".ses", ".tmp")) or ".tmp" in n for n in leftover), leftover
    assert set(leftover) <= {dsn.name, f"{dsn.stem}.result.json"}, leftover


def test_the_summary_matches_the_drc_of_the_written_session(router, board, tmp_path):
    """The manifest's incomplete count against `-drc` on the SES. The DRC
    report has no incomplete count of its own (its `unconnectedItems` lists
    split nets, at most one entry per net), but its `qualityScore` is the
    router's V2 score computed from that count and the violations, so equal
    scores mean equal counts; on a complete board the list is empty too."""
    for name, passes in ((SMALL, "2"), (BIG, "1")):
        work, dsn, ses = board(name)
        run = start(router, dsn, ses, work, ["--max-passes", passes])
        assert run.wait(timeout=600) == 0, "\n".join(run.lines[-15:])
        manifest = run.manifest()
        _, report = drc(router, dsn, ses, tmp_path / f"{dsn.stem}.drc.json")
        assert manifest["normalized_score"] == pytest.approx(report["qualityScore"], abs=0.005), name
        incomplete = summary.incomplete_count(manifest)
        assert (incomplete == 0) == (report["unconnectedItems"] == []), (name, incomplete)
        text = run.summary_text()
        assert f"{incomplete} incomplete" in text
        assert ("WARNING" in text) == (incomplete > 0)
        shutil.rmtree(work)
        os.makedirs(work)


def test_progress_events_arrive_while_the_router_runs(router, board):
    work, dsn, ses = board(BIG)
    run = start(router, dsn, ses, work, ["--max-passes", "2"])
    phases = []
    while not run.finished:
        for kind, payload in run.poll():
            if kind == "event" and payload["event"] == "progress" and payload["phase"] not in phases:
                phases.append(payload["phase"])
        time.sleep(0.02)
    assert run.returncode == 0
    assert phases[0] == "load" and "route" in phases and phases[-1] == "write", phases


def test_the_stage_listing_has_every_registered_id(router):
    """N01.4: the plugin's Advanced choices, read from the real router, name
    every stage id the router registers (`--capabilities` lists them)."""
    probe = subprocess.run(router.command(["--capabilities"]), capture_output=True, text=True, timeout=60)
    registered = json.loads(probe.stdout).get("stages")
    if not isinstance(registered, dict):
        pytest.skip("this router lists no stages in --capabilities")
    for profile in options.PROFILES:
        listing = tools.query_stages(router, profile)
        if listing is None:
            pytest.skip("this router predates --list-stages (N01.4)")
        assert listing["profile"] == profile
        offered = {i for _, _, ids, _ in options.parse_stage_listing(listing) for i in ids}
        for kind, ids in registered.items():
            for stage_id in ids:
                assert stage_id in offered, f"{kind}: {stage_id}"


@pytest.mark.parametrize("profile, label", [("legacy", "Legacy"), ("enhanced", "Enhanced")])
def test_both_profiles_route_and_the_summary_names_them(router, board, profile, label):
    work, dsn, ses = board(SMALL)
    result = work / "r.json"
    chosen = options.RouteOptions(profile=profile)
    run = routing.RouteRun(router, dsn, ses, chosen.to_args(), result, profile)
    run.start()
    assert run.wait(timeout=300) == 0, run.failure_text()
    assert run.ses_written()
    assert run.summary_text().startswith(f"Profile: {label}\n")


def test_the_k09_flags_and_a_fixed_up_dsn_reach_the_real_cli(router, tmp_path):
    """K09: the canary preset file's router flags are read by the CLI (its settings snapshot), and a DSN
    with every fix-up (class width, fixed pour wires, dropped plane, edge and pad keepouts,
    an opened plane layer) loads and routes."""
    import dsn_fixups
    from test_dsn_fixups import BOARD, FOUR

    text, _log = dsn_fixups.apply(
        FOUR,
        BOARD,
        {
            "class_widths": {"Power": 0.3},
            "fixed_pour_classes": ["Power"],
            "drop_pour_nets": ["GND"],
            "edge_and_pad_keepouts": True,
            "route_on_plane_layers": ["In2.Cu"],
        },
    )
    work = tmp_path / "freerouting-rs"
    work.mkdir()
    dsn, ses = work / "k09.dsn", work / "k09.ses"
    dsn.write_text(text, encoding="utf-8")
    from conftest import canary_preset_options

    chosen = canary_preset_options()
    run = start(router, dsn, ses, work, chosen.to_args())
    assert run.wait(timeout=300) == 0, "\n".join(run.lines[-15:])
    assert run.ses_written()
    assert not any("ignored" in line or "Unknown" in line or "could not" in line for line in run.lines), run.lines
    snapshot = run.manifest()["settings_snapshot"]
    assert snapshot["autorouter"]["max_passes"] == 1
    assert snapshot["autorouter"]["sweep"] == "free" and snapshot["autorouter"]["sweep_passes"] == 2
    assert snapshot["scoring"]["start_ripup_costs"] == 1500
    assert snapshot["min_trace_width_um"] == 150.0


def test_stop_inside_a_long_fanout_step_ends_within_seconds(router, board):
    """Stop and keep 3 s into coldfire's long fanout pin (its last progress
    event before the pin is pass 2 with 155 pins routed): the router stops,
    writes the board and exits 0 within seconds. Before the normalization
    checked the stop (2026-10-18) the pin ran on for about 280 s."""
    work, dsn, ses = board(LONG_STEP)
    run = start(router, dsn, ses, work)
    deadline = time.monotonic() + 300
    state = run.state
    while not (state.phase == "fanout" and state.pass_number == 2 and (state.routed or 0) >= 155):
        run.poll()
        assert not run.finished, "the run ended before the long pin: " + "\n".join(run.lines[-10:])
        assert time.monotonic() < deadline, "no fanout event of pass 2 with 155 pins routed"
        time.sleep(0.02)
    in_pin = time.monotonic()
    while time.monotonic() - in_pin < 3:
        run.poll()
        time.sleep(0.05)
    assert not run.finished, "the run ended in its fanout: " + "\n".join(run.lines[-10:])
    stopped_at = time.monotonic()
    assert run.stop() is True
    code = run.wait(timeout=60)
    latency = time.monotonic() - stopped_at
    if code is None:
        run.abort()
    assert code == 0, f"no exit {latency:.0f} s after Stop: " + "\n".join(run.lines[-15:])
    assert latency < 15, f"the stop took {latency:.1f} s"
    assert run.stopped and run.state.state == "completed" and run.ses_written()
