"""Which input the router reads, and the fallback to the DSN (K13.3).

`board_input = kicad_pcb` asks the router to read a snapshot of the open board
(`snapshot.py`). When the router's `.kicad_pcb` reader refuses that file, the
run uses KiCad's DSN export instead and says so. A run never fails because of
the input choice. Two refusals are recognised:

* **Before routing, from the snapshot** (`snapshot_refusal`): the file's
  `(version N)` is below 20201115, the first format the reader accepts (KiCad 6
  development; KiCad 5 is 20171130). Costs nothing. KiCad writes its own
  version, so this only fires on a KiCad older than 6.
* **At load time, from the router** (`load_refusal`): the real CLI prints
  `There was a parse error while reading board file at '<name>': <reason>` as
  an error log line and exits 1 (`crates/freerouting-cli/src/cli.rs`). That
  covers a construct the reader does not support. The plugin exports the DSN
  and routes again once. A routing failure without that line is never retried,
  and neither is a failure to read the file from disk.

Pure Python like `snapshot.py`; `prepare` calls the `export_flow` functions
(which take pcbnew as an argument) and runs on KiCad's UI thread.
"""

import dataclasses
import re
from collections import namedtuple

try:  # inside the plugin package
    from . import export_flow, kicad_board, options, snapshot
except ImportError:  # tests and headless scripts put this folder on sys.path
    import export_flow
    import kicad_board
    import options
    import snapshot

# `crates/freerouting-kicad/src/header.rs` FIRST_SUPPORTED_VERSION.
FIRST_SUPPORTED_VERSION = 20201115

_VERSION = re.compile(r"\(\s*version\s+(\d+)\s*\)")
_REFUSAL = re.compile(r"There was a parse error while reading board file at '.*?': (.*)$")

# What `prepare` returns. `opts` are the options the run uses (the DSN fallback
# turns `board_input` to `dsn`); `input` is `kicad_pcb` or `dsn`; `reason` is
# why a fallback happened (None when none); `design` the file the router reads.
Prepared = namedtuple("Prepared", "ok opts input reason design args lines locked")


def snapshot_refusal(path):
    """The reason the reader will refuse the snapshot at `path` from its header,
    or None. A file that cannot be read, or has no version, is not refused here
    (the router's own message then decides)."""
    try:
        with open(path, "rb") as f:
            head = f.read(4096).decode("utf-8", errors="replace")
    except OSError:
        return None
    found = _VERSION.search(head)
    if not found:
        return None
    version = int(found.group(1))
    if version >= FIRST_SUPPORTED_VERSION:
        return None
    return f"the board file is in an older format than KiCad 6's (version {version}, the reader needs {FIRST_SUPPORTED_VERSION} or later)"


def load_refusal(lines):
    """The reader's reason from the router's log `lines` when it refused the
    board at load time, else None."""
    for line in lines:
        found = _REFUSAL.search(line)
        if found:
            return found.group(1).strip()
    return None


def input_line(name, reason):
    """The one run-log line saying which input ran and why."""
    if name == options.INPUT_KICAD_PCB:
        return "input: kicad_pcb (the board snapshot, read directly)"
    if reason:
        return f"input: dsn (KiCad's DSN export; fallback from kicad_pcb because {reason})"
    return "input: dsn (KiCad's DSN export, as chosen)"


def prepare(board, pcbnew, work_dir, stem, board_dir, dsn, opts, export, fallback_reason=None):
    """Prepares the input `opts` asks for: the snapshot for `kicad_pcb`
    (`export_flow.prepare_snapshot`), checked against `snapshot_refusal`, else
    the DSN export (`export_flow.prepare`). With `fallback_reason` (the router
    refused the snapshot already) the DSN is prepared at once. A failed
    snapshot (`SaveBoard`) is not a refusal and is reported as it was before.
    Returns a `Prepared`; its `lines` end with the `input_line`."""
    chosen = opts.board_input
    reason = fallback_reason
    lines = []
    if chosen == options.INPUT_KICAD_PCB and reason is None:
        ok, args, lines, locked = export_flow.prepare_snapshot(board, pcbnew, work_dir, stem, board_dir, opts)
        design = snapshot.design_path(work_dir, stem, chosen)
        if not ok:
            return Prepared(False, opts, chosen, None, design, [], lines, [])
        reason = snapshot_refusal(design)
        if reason is None:
            lines = lines + [input_line(chosen, None)]
            return Prepared(True, opts, chosen, None, design, args, lines, locked)
        kicad_board.restore_locks(locked)
        note = options.input_ignored_note(opts)  # the DSN run applies that option
        lines = [line for line in lines if line != note] + [f"WARNING: the snapshot is not usable ({reason}); the DSN export is used instead"]
    elif reason is not None:
        lines = [f"the router refused the snapshot ({reason}); the DSN export is used instead"]
    used = dataclasses.replace(opts, board_input=options.INPUT_DSN)
    ok, args, more, locked = export_flow.prepare(board, pcbnew, dsn, used, export)
    design = snapshot.design_path(work_dir, stem, options.INPUT_DSN)
    if not ok:
        return Prepared(False, used, options.INPUT_DSN, reason, design, [], lines + more, [])
    return Prepared(True, used, options.INPUT_DSN, reason, design, args, lines + more + [input_line(options.INPUT_DSN, reason)], locked)
