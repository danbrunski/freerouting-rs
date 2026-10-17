import io
import json

import progress
import pytest
from conftest import TESTS

DATA = TESTS / "data"
BOARDS = ["j2", "bm01"]


def replay(name):
    state = progress.ProgressState()
    logs, events = [], []
    for line in (DATA / f"progress-{name}.jsonl").read_text(encoding="utf-8").splitlines():
        event = progress.parse_line(line)
        if event is None:
            logs.append(line)
        else:
            events.append(event)
            state.update(event)
    return state, events, logs


@pytest.mark.parametrize("name", BOARDS)
def test_a_recording_replays_to_the_state_of_its_done_event(name):
    state, events, logs = replay(name)
    done = events[-1]
    assert done["event"] == "done" and events[0]["event"] == "start"
    assert logs == [], "every line of a --progress json run is an event"
    assert state.started and state.done
    assert (state.exit_code, state.state, state.elapsed_ms) == (done["exit_code"], done["state"], done["elapsed_ms"])
    # The counters are those of the last progress event that had them.
    last = [e for e in events if e["event"] == "progress" and e["incomplete"] is not None][-1]
    assert state.incomplete == last["incomplete"]
    assert state.phase == "write"


def test_the_recorded_runs_end_complete_and_incomplete():
    j2, _, _ = replay("j2")
    bm01, _, _ = replay("bm01")
    assert j2.incomplete == 0 and j2.routed is not None
    assert bm01.incomplete == 49
    assert j2.state == bm01.state == "completed"


def test_pass_counters_follow_the_events():
    state = progress.ProgressState()
    for line in (DATA / "progress-bm01.jsonl").read_text(encoding="utf-8").splitlines()[:6]:
        event = progress.parse_line(line)
        if event:
            state.update(event)
    assert state.phase in ("load", "fanout") and not state.done
    assert "Fanout" in state.text() or "Loading" in state.text()


def test_null_figures_keep_the_last_known_values():
    state = progress.ProgressState()
    state.update({"event": "progress", "phase": "route", "pass": 2, "routed": 10, "failed": 1, "incomplete": 4, "score": 5})
    state.update({"event": "progress", "phase": "write", "pass": None, "routed": None, "incomplete": 3, "elapsed_ms": 1500})
    assert (state.phase, state.pass_number, state.routed, state.failed, state.incomplete) == ("write", 2, 10, 1, 3)
    assert state.text() == "Writing: 10 routed, 1 failed, 3 incomplete  (1.5 s)"


def test_text_of_a_routing_pass_and_of_the_end():
    state = progress.ProgressState()
    assert state.text() == "Starting"
    state.update({"event": "progress", "phase": "route", "pass": 3, "routed": 120, "failed": 0, "incomplete": 5, "elapsed_ms": 5400})
    assert state.text() == "Routing, pass 3: 120 routed, 0 failed, 5 incomplete  (5.4 s)"
    state.update({"event": "done", "exit_code": 0, "state": "completed", "elapsed_ms": 70000})
    assert state.text().startswith("Finished (completed)") and "1 min 10 s" in state.text()


@pytest.mark.parametrize(
    "line",
    [
        "",
        "   ",
        "Auto-routing pass #3 ...",
        "{",
        '{"event":"progress","phase":"ro',  # cut off mid-line
        '{"event":"progress"',  # no closing brace
        "[1, 2]",
        '"event"',
        '{"phase":"route"}',  # an object without `event`
        '{"event":3}',
        "\x00\x01\x02",
    ],
)
def test_malformed_and_partial_lines_are_skipped_without_an_exception(line):
    assert progress.parse_line(line) is None


def test_none_is_not_a_line():
    assert progress.parse_line(None) is None


def test_bad_values_in_an_event_are_ignored():
    state = progress.ProgressState()
    state.update({"event": "progress", "phase": 7, "pass": "x", "routed": -1, "failed": True, "incomplete": 2.5, "score": "hi", "elapsed_ms": []})
    state.update({"event": "done", "exit_code": "0", "state": None})
    state.update({"event": "mystery"})
    state.update({})
    assert (state.phase, state.pass_number, state.routed, state.failed, state.incomplete, state.score) == (None,) * 6
    assert state.done and state.exit_code is None and state.state is None


def test_log_events_format_like_the_router():
    event = progress.parse_line('{"event":"log","level":"warn","text":"careful"}')
    assert progress.format_log_event(event) == "[warn] careful"


def test_pump_lines_puts_every_line_and_the_end():
    stream = io.BytesIO("one\r\ntwo \u00e4\n".encode("utf-8") + b"\xff bad\nlast without newline")
    got = []
    progress.pump_lines(stream, got.append)
    assert got[-1] == ("eof", None)
    texts = [t for k, t in got if k == "line"]
    assert texts[0] == "one" and texts[1] == "two \u00e4" and texts[-1] == "last without newline"
    assert "\ufffd bad" in texts  # bytes that are not UTF-8 do not stop the reader


def test_pump_lines_removes_nuls_only_when_asked():
    plain, stripped = [], []
    progress.pump_lines(io.BytesIO(b"x\x00y\n"), plain.append, strip_nul=False)
    progress.pump_lines(io.BytesIO(b"x\x00y\n"), stripped.append, strip_nul=True)
    assert plain[0] == ("line", "x\x00y")
    assert stripped[0] == ("line", "xy")


def test_pump_lines_survives_a_closed_pipe():
    stream = io.BytesIO(b"x\n")
    stream.close()
    got = []
    progress.pump_lines(stream, got.append)
    assert got == [("eof", None)]


def test_format_seconds():
    assert progress.format_seconds(0.04) == "0.0 s"
    assert progress.format_seconds(6.84) == "6.8 s"
    assert progress.format_seconds(125) == "2 min 05 s"
    assert progress.format_seconds(3720) == "1 h 02 min"


def test_every_recorded_line_is_valid_json():
    for name in BOARDS:
        for line in (DATA / f"progress-{name}.jsonl").read_text(encoding="utf-8").splitlines():
            json.loads(line)
