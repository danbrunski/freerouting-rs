"""RouteRun and GuiRun against the fake router and GUI (fake_bin/)."""

import json
import time

import pytest
import routing
import tools


def tool_for(path):
    return tools.Tool("freerouting", path)


def make_run(make_fake_router, tmp_path, mode, options=(), name="board"):
    work = tmp_path / "freerouting-rs"
    work.mkdir(exist_ok=True)
    dsn, ses = work / f"{name}.dsn", work / f"{name}.ses"
    dsn.write_text("(pcb x)", encoding="utf-8")
    run = routing.RouteRun(
        tool_for(make_fake_router(mode)), dsn, ses, list(options), routing.manifest_path(work, name)
    )
    return run, work


def test_a_run_reports_progress_and_writes_the_files(make_fake_router, tmp_path):
    run, work = make_run(make_fake_router, tmp_path, "route", options=["--max-passes", "3"])
    run.start()
    code = run.wait(timeout=20)
    assert code == 0 and run.state.done and run.state.state == "completed"
    assert (run.state.phase, run.state.pass_number, run.state.routed, run.state.incomplete) == ("route", 3, 9, 1)
    assert run.ses_written() and not run.stopped and not run.aborted
    assert "Connections: 10 of 10 routed, 0 incomplete" in run.summary_text()
    assert "Time: 0.2 s" in run.summary_text()  # from the done event
    assert not list(work.glob("*.tmp"))


def test_the_command_line_has_files_progress_result_and_options(make_fake_router, tmp_path):
    run, work = make_run(make_fake_router, tmp_path, "echo", options=["--threads", "2"])
    run.start()
    run.wait(timeout=20)
    # `echo` prints its arguments, one per line; they arrive as plain log lines.
    assert run.lines == [
        "-de", str(run.dsn), "-do", str(run.ses),
        "--progress", "json",
        "--result-json", str(work / "board.result.json"),
        "--threads", "2",
    ]  # fmt: skip


def test_non_json_and_truncated_lines_go_to_the_log(make_fake_router, tmp_path):
    run, _ = make_run(make_fake_router, tmp_path, "route-noise")
    run.start()
    assert run.wait(timeout=20) == 0
    assert "a plain line from the router" in run.lines
    assert '{"event":"progress","phase":"ro' in run.lines
    assert run.state.done


def test_stop_keeps_the_board(make_fake_router, tmp_path):
    run, _ = make_run(make_fake_router, tmp_path, "route-slow")
    run.start()
    deadline = time.monotonic() + 20
    while run.state.pass_number is None and time.monotonic() < deadline:
        run.poll()
        time.sleep(0.02)
    assert run.state.pass_number is not None and not run.finished
    assert not run.ses.exists()
    assert run.stop() is True
    assert run.wait(timeout=20) == 0
    assert run.stopped and not run.aborted and run.ses_written()
    assert "5 incomplete" in run.summary_text()


def test_abort_kills_and_leaves_no_output(make_fake_router, tmp_path):
    run, work = make_run(make_fake_router, tmp_path, "route-slow")
    run.start()
    while run.state.pass_number is None:
        run.poll()
        time.sleep(0.02)
    run.abort()
    assert run.aborted and run.proc.poll() is not None
    run.wait(timeout=20)
    assert not run.ses.exists() and not list(work.glob("*.tmp"))
    assert run.returncode != 0


def test_a_failed_run_explains_itself(make_fake_router, tmp_path):
    run, _ = make_run(make_fake_router, tmp_path, "route-fail")
    run.start()
    assert run.wait(timeout=20) == 1
    assert not run.ses_written()
    assert run.failure_text() == "Routing failed (exit code 1).\n\n[error] Couldn't load the input file"


def test_stop_after_the_process_ended_is_harmless(make_fake_router, tmp_path):
    run, _ = make_run(make_fake_router, tmp_path, "route")
    run.start()
    run.wait(timeout=20)
    assert run.stop() in (True, False)  # no exception either way


def test_a_missing_router_raises_oserror(tmp_path):
    run = routing.RouteRun(tool_for(tmp_path / "nothing"), tmp_path / "a.dsn", tmp_path / "a.ses")
    with pytest.raises(OSError):
        run.start()


def test_the_manifest_is_optional(make_fake_router, tmp_path):
    work = tmp_path / "w"
    work.mkdir()
    run = routing.RouteRun(tool_for(make_fake_router("route")), work / "a.dsn", work / "a.ses")
    run.start()
    run.wait(timeout=20)
    assert run.manifest() is None
    assert run.summary_text().startswith("No result summary")


# --- the GUI (K02.5) ---------------------------------------------------------


def make_gui(make_fake_gui, tmp_path, mode, options=()):
    work = tmp_path / "freerouting-rs"
    work.mkdir(exist_ok=True)
    dsn, ses = work / "board.dsn", work / "board.ses"
    dsn.write_text("(pcb x)", encoding="utf-8")
    return routing.GuiRun(tools.Tool("freerouting-gui", make_fake_gui(mode)), dsn, ses, list(options)), ses


def wait_exit(run, timeout=20):
    deadline = time.monotonic() + timeout
    while not run.finished and time.monotonic() < deadline:
        time.sleep(0.02)
    assert run.finished


def test_the_gui_gets_the_front_end_command_line(make_fake_gui, tmp_path):
    run, ses = make_gui(make_fake_gui, tmp_path, "write", options=["--max-passes", "5"])
    run.start()
    wait_exit(run)
    argv = json.loads((ses.parent / "board.ses.argv").read_text(encoding="utf-8"))
    assert argv == ["-de", str(run.dsn), "-do", str(ses), "--max-passes", "5"]
    assert run.should_import()


def test_a_gui_that_exits_without_a_session_imports_nothing(make_fake_gui, tmp_path):
    run, ses = make_gui(make_fake_gui, tmp_path, "nowrite")
    run.start()
    wait_exit(run)
    assert not run.should_import()
    assert "without writing board.ses" in run.outcome_text()


def test_terminate_kills_the_gui_and_imports_nothing(make_fake_gui, tmp_path):
    run, ses = make_gui(make_fake_gui, tmp_path, "hang")
    run.start()
    assert not run.finished
    run.terminate()
    assert run.finished and not run.should_import()
    assert run.outcome_text() == "Terminated; the board was not changed."


def test_an_old_session_is_not_imported(make_fake_gui, tmp_path):
    run, ses = make_gui(make_fake_gui, tmp_path, "nowrite")
    ses.write_text("(session old)", encoding="utf-8")
    import os

    old = time.time() - 3600
    os.utime(ses, (old, old))
    run.start()
    wait_exit(run)
    assert ses.is_file() and not run.should_import()
