"""A routing run and a GUI routing run, as the dialog drives them (K02.3, K02.5).

Kept free of pcbnew and wx imports. The dialog calls these on the UI thread;
the only other thread is the reader of the router's output (`progress.
pump_lines`), which puts lines on a queue that `RouteRun.poll` drains. Nothing
that touches pcbnew or wx runs off the UI thread (tests/test_threads.py).

**Stop keeps, Abort discards.** `stop` writes `stop` to the router's standard
input: it ends the step it is in, writes the best board and exits 0. `abort`
kills the process. The router writes its output to a temporary file and
renames it, so an aborted run leaves no `.ses` (and no temporary file) behind.
"""

import queue
import threading
import time
from pathlib import Path

try:  # as part of the plugin package
    from . import progress, summary, tools
except ImportError:  # top-level import, as the tests do
    import progress
    import summary
    import tools

# How long the pipe may stay open after the process has exited (a child of a
# wrapper script can hold it) before the run counts as finished.
PIPE_GRACE_SECONDS = 2.0
KEEP_LINES = 400


def manifest_path(work_dir, board_stem):
    return Path(work_dir) / (board_stem + ".result.json")


class RouteRun:
    """One headless routing run of `dsn` into `ses`."""

    def __init__(self, tool, dsn, ses, option_args=(), result_json=None, profile=None):
        self.tool = tool
        self.profile = profile  # the routing profile asked for, for the summary
        self.dsn = Path(dsn)
        self.ses = Path(ses)
        self.option_args = list(option_args)
        self.result_json = Path(result_json) if result_json else None
        self.state = progress.ProgressState()
        self.lines = []  # human-readable lines for the log pane and error messages
        self.proc = None
        self.stopped = False
        self.aborted = False
        self._queue = queue.Queue()
        self._reader = None
        self._eof = False
        self._exited_at = None

    # --- control --------------------------------------------------------------

    def start(self):
        """Starts the router. Raises OSError when it cannot be started."""
        self.proc = tools.start_route(
            self.tool, self.dsn, self.ses, self.option_args, self.result_json, progress=True
        )
        self._reader = threading.Thread(
            target=progress.pump_lines,
            args=(self.proc.stdout, self._queue.put, self.tool.via_wsl),
            daemon=True,
        )
        self._reader.start()

    def stop(self):
        """Stop and keep: the router finishes its step and writes the board."""
        self.stopped = True
        return tools.stop_route(self.proc)

    def abort(self):
        """Abort and discard: kills the router. No output is written."""
        self.aborted = True
        try:
            self.proc.kill()
        except OSError:
            pass
        self.proc.wait()
        if self.proc.stdin:
            try:
                self.proc.stdin.close()
            except OSError:
                pass

    # --- reading ----------------------------------------------------------------

    def poll(self):
        """Drains what the router has printed since the last call. Returns a
        list of `("event", dict)` and `("log", text)` and updates `state`. A
        line that is not a JSON event, including a truncated one, is `log`."""
        items = []
        while True:
            try:
                kind, payload = self._queue.get_nowait()
            except queue.Empty:
                break
            if kind == "eof":
                self._eof = True
                continue
            event = progress.parse_line(payload)
            if event is None:
                self._remember(payload)
                items.append(("log", payload))
                continue
            self.state.update(event)
            if event["event"] == "log":
                self._remember(progress.format_log_event(event))
            items.append(("event", event))
        return items

    def _remember(self, text):
        self.lines.append(text)
        del self.lines[:-KEEP_LINES]

    @property
    def finished(self):
        """True once the process has exited and its output is read. Call
        `poll` first so the queue is drained."""
        if self.proc is None or self.proc.poll() is None:
            return False
        if self._eof:
            return True
        if self._exited_at is None:
            self._exited_at = time.monotonic()
        return time.monotonic() - self._exited_at > PIPE_GRACE_SECONDS

    def wait(self, timeout=None):
        """Blocks until the run is finished (for tests and scripts). Returns
        the exit code, or None on timeout."""
        deadline = None if timeout is None else time.monotonic() + timeout
        while True:
            self.poll()
            if self.finished:
                return self.proc.returncode
            if deadline is not None and time.monotonic() > deadline:
                return None
            time.sleep(0.02)

    # --- result -----------------------------------------------------------------

    @property
    def returncode(self):
        return self.proc.returncode if self.proc else None

    def ses_written(self):
        return self.ses.is_file() and self.ses.stat().st_size > 0

    def manifest(self):
        return summary.load_manifest(self.result_json) if self.result_json else None

    def summary_text(self):
        return summary.format_summary(self.manifest(), self.state.elapsed_ms, self.profile)

    def failure_text(self):
        """What to show when the run failed: the exit code and the last
        warning and error lines (or the last lines of output)."""
        important = [l for l in self.lines if l.startswith(("[error]", "[warn]"))]
        tail = (important or self.lines)[-12:]
        return f"Routing failed (exit code {self.returncode}).\n\n" + ("\n".join(tail) or "No output.")


class GuiRun:
    """One run of the GUI in front-end mode (K02.5)."""

    MESSAGE = "Freerouting GUI is running...\nComplete routing and close the window,\nor press Terminate to cancel."

    def __init__(self, tool, dsn, ses, option_args=()):
        self.tool = tool
        self.dsn = Path(dsn)
        self.ses = Path(ses)
        self.option_args = list(option_args)
        self.proc = None
        self.launched_at = None
        self.terminated = False

    def start(self):
        self.launched_at = time.time()
        self.proc = tools.start_gui(self.tool, self.dsn, self.ses, self.option_args)

    @property
    def finished(self):
        return self.proc is not None and self.proc.poll() is not None

    def terminate(self):
        """Kills the GUI; nothing is imported."""
        self.terminated = True
        try:
            self.proc.kill()
        except OSError:
            pass
        self.proc.wait()

    def should_import(self):
        """True if the session exists, has content and is newer than the
        launch (two seconds of slack for coarse file times), and the GUI was
        not terminated."""
        if self.terminated or not self.ses.is_file():
            return False
        stat = self.ses.stat()
        return stat.st_size > 0 and stat.st_mtime >= (self.launched_at or 0) - 2.0

    def outcome_text(self):
        """A message for when nothing is imported."""
        if self.terminated:
            return "Terminated; the board was not changed."
        code = self.proc.returncode if self.proc else None
        return (
            f"The GUI closed (exit code {code}) without writing {self.ses.name}.\n"
            "Route in the window and close it after routing, or the board is not changed."
        )
