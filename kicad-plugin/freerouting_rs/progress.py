"""The router's `--progress json` stream (C08.3), as the plugin reads it (K02.2).

Kept free of pcbnew and wx imports.

The router writes one JSON object per line on stderr:

    {"event":"start","schema_version":1,"mode":"route"}
    {"event":"progress","phase":"route","pass":3,"routed":120,"failed":0,
     "ripped":14,"incomplete":5,"violations":0,"score":812.5,"pass_end":true,
     "elapsed_ms":5436}
    {"event":"log","level":"info","text":"..."}
    {"event":"done","exit_code":0,"state":"completed","elapsed_ms":6841}

`parse_line` reads one such line and gives `None` for anything else, which
the dialog shows as it is in its log pane. The plugin never parses the human
log text (K02 decision). A figure that is `null` (the router's counters while
loading) keeps the last known value in `ProgressState`.
"""

import json

PHASE_NAMES = {
    "load": "Loading",
    "fanout": "Fanout",
    "route": "Routing",
    "optimize": "Optimizing",
    "write": "Writing",
}

_FIGURES = ("routed", "failed", "ripped", "incomplete", "violations")


def parse_line(text):
    """The event of one stream line: a dict with a string `event`, or None for
    a line that is not JSON, not an object, truncated, or has no `event`."""
    text = (text or "").strip()
    if not text.startswith("{"):
        return None
    try:
        event = json.loads(text)
    except ValueError:
        return None
    if isinstance(event, dict) and isinstance(event.get("event"), str):
        return event
    return None


def _count(value):
    return value if isinstance(value, int) and not isinstance(value, bool) and value >= 0 else None


class ProgressState:
    """The last phase, pass, counters and time of a run, and its end."""

    def __init__(self):
        self.started = False
        self.phase = None
        self.pass_number = None
        self.routed = None
        self.failed = None
        self.ripped = None
        self.incomplete = None
        self.violations = None
        self.score = None
        self.elapsed_ms = None
        self.done = False
        self.exit_code = None
        self.state = None
        self.pass_ends = []  # (pass, incomplete) at the end of each routing pass (K09.7)

    def best_pass(self):
        """The routing pass that ended with the fewest incomplete connections
        (the earliest of equals), or None. K09.7's pass-limit hint."""
        if not self.pass_ends:
            return None
        return min(self.pass_ends, key=lambda p: (p[1], p[0]))[0]

    def update(self, event):
        """Applies one event (from `parse_line`); unknown events and bad
        values are ignored."""
        kind = event.get("event")
        if kind == "start":
            self.started = True
        elif kind == "progress":
            phase = event.get("phase")
            if isinstance(phase, str):
                self.phase = phase
            number = _count(event.get("pass"))
            if number is not None:
                self.pass_number = number
            for name in _FIGURES:
                value = _count(event.get(name))
                if value is not None:
                    setattr(self, name, value)
            score = event.get("score")
            if isinstance(score, (int, float)) and not isinstance(score, bool):
                self.score = float(score)
            if event.get("pass_end") is True and phase == "route" and number and self.incomplete is not None:
                self.pass_ends.append((number, self.incomplete))
            self._time(event)
        elif kind == "done":
            self.done = True
            code = event.get("exit_code")
            if isinstance(code, int) and not isinstance(code, bool):
                self.exit_code = code
            if isinstance(event.get("state"), str):
                self.state = event["state"]
            self._time(event)

    def _time(self, event):
        ms = _count(event.get("elapsed_ms"))
        if ms is not None:
            self.elapsed_ms = ms

    def text(self):
        """One line for the dialog: `Routing, pass 3: 120 routed, 0 failed,
        5 incomplete (5.4 s)`."""
        if self.done:
            head = f"Finished ({self.state or 'unknown state'})"
        elif self.phase:
            head = PHASE_NAMES.get(self.phase, self.phase)
            if self.pass_number is not None and self.phase in ("fanout", "route", "optimize"):
                head += f", pass {self.pass_number}"
        else:
            head = "Starting"
        figures = [
            f"{getattr(self, n)} {label}"
            for n, label in (("routed", "routed"), ("failed", "failed"), ("incomplete", "incomplete"))
            if getattr(self, n) is not None
        ]
        text = head + (": " + ", ".join(figures) if figures else "")
        if self.elapsed_ms is not None:
            text += f"  ({format_seconds(self.elapsed_ms / 1000)})"
        return text


def format_seconds(seconds):
    """`6.8 s`, `2 min 05 s`, `1 h 02 min`."""
    if seconds < 60:
        return f"{seconds:.1f} s"
    whole = int(round(seconds))
    if whole < 3600:
        return f"{whole // 60} min {whole % 60:02d} s"
    return f"{whole // 3600} h {whole % 3600 // 60:02d} min"


def format_log_event(event):
    """`[warn] text` for a `log` event."""
    return f"[{event.get('level', 'info')}] {event.get('text', '')}"


def pump_lines(stream, put, strip_nul=False):
    """Reads `stream` (a binary pipe) line by line, calling `put(("line",
    text))` for each and `put(("eof", None))` at the end. This is the body of
    the reader thread: it touches neither pcbnew nor wx. `strip_nul` removes
    the NULs of the UTF-16 text `wsl.exe` prints for its own errors."""
    try:
        for raw in iter(stream.readline, b""):
            text = raw.decode("utf-8", errors="replace")
            if strip_nul:
                text = text.replace("\x00", "")
            put(("line", text.rstrip("\r\n")))
    except (OSError, ValueError):  # the pipe was closed under us
        pass
    finally:
        put(("eof", None))
