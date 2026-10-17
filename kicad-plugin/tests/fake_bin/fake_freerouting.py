"""A stand-in for the freerouting executable, used by the plugin tests.

    fake_freerouting.py <mode> [args...]

Modes (the first argument after the script):
  ok         `--version` prints "freerouting 0.1.0 (abc1234)"
  newer      `--version` prints "freerouting 0.2.0 (abc1234)"
  patch      `--version` prints "freerouting 0.1.9 (abc1234)"
  garbage    `--version` prints text that is not a version line
  fail       prints a version line but exits 3
  sleep      sleeps 30 s before answering (to hit the timeout)
  echo       prints its arguments, one per line
  route      routes like the real CLI with `--progress json`: JSON events on
             stderr, the SES (temp file, then rename) and the `--result-json`
             manifest, exit 0. `--version` as in `ok`.
  route-slow like `route`, but keeps emitting events every 0.1 s until it reads
             `stop` on stdin (then writes the SES and exits 0); killed, it
             leaves nothing
  route-fail prints an error log event and exits 1 without output
  route-noise like `route`, with a plain text line and a cut-off JSON line
"""

import json
import os
import sys
import threading
import time


def option(name):
    return args[args.index(name) + 1] if name in args else None


def emit(event):
    print(json.dumps(event, separators=(",", ":")), file=sys.stderr, flush=True)


def write_outputs(incomplete=0):
    ses = option("-do")
    if ses:
        tmp = ses + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            f.write("(session fake)\n")
        os.replace(tmp, ses)
    result = option("--result-json")
    if result:
        manifest = {
            "final_state": "COMPLETED",
            "board_statistics": {
                "connections": {"maximum_count": 10, "incomplete_count": incomplete},
                "vias": {"total_count": 2},
                "traces": {"total_length_mm": 12.34},
                "clearance_violations": {"total_count": 0},
            },
            "phases": {"autorouter": {"duration_seconds": 0.5}},
        }
        with open(result, "w", encoding="utf-8") as f:
            json.dump(manifest, f)


mode = sys.argv[1]
args = sys.argv[2:]

if mode in ("route", "route-slow", "route-fail", "route-noise") and "--version" in args:
    print("freerouting 0.1.0 (abc1234)")
elif mode == "route-fail":
    emit({"event": "start", "schema_version": 1, "mode": "route"})
    emit({"event": "log", "level": "error", "text": "Couldn't load the input file"})
    emit({"event": "done", "exit_code": 1, "state": "terminated", "elapsed_ms": 3})
    sys.exit(1)
elif mode in ("route", "route-noise"):
    emit({"event": "start", "schema_version": 1, "mode": "route"})
    if mode == "route-noise":
        print("a plain line from the router", file=sys.stderr, flush=True)
        print('{"event":"progress","phase":"ro', file=sys.stderr, flush=True)
    for i in range(1, 4):
        emit({"event": "progress", "phase": "route", "pass": i, "routed": 3 * i, "failed": 0, "ripped": 0, "incomplete": 10 - 3 * i, "violations": 0, "score": 100.0 * i, "pass_end": True, "elapsed_ms": 50 * i})
        time.sleep(0.03)
    write_outputs()
    emit({"event": "done", "exit_code": 0, "state": "completed", "elapsed_ms": 200})
elif mode == "route-slow":
    stopped = threading.Event()

    def read_stdin():
        for line in sys.stdin:
            if line.strip().lower() == "stop":
                stopped.set()
                return

    threading.Thread(target=read_stdin, daemon=True).start()
    emit({"event": "start", "schema_version": 1, "mode": "route"})
    i = 0
    while not stopped.wait(0.1):
        i += 1
        emit({"event": "progress", "phase": "route", "pass": i, "routed": i, "failed": 0, "ripped": 0, "incomplete": 5, "violations": 0, "score": 1.0, "pass_end": False, "elapsed_ms": 100 * i})
        if i > 300:  # a safety net: never run for ever
            sys.exit(3)
    write_outputs(incomplete=5)
    emit({"event": "done", "exit_code": 0, "state": "completed", "elapsed_ms": 100 * i})
elif mode == "echo":
    print("\n".join(args))
elif mode == "sleep":
    time.sleep(30)
    print("freerouting 0.1.0 (abc1234)")
elif mode == "fail":
    print("freerouting 0.1.0 (abc1234)")
    sys.exit(3)
elif mode == "garbage":
    print("Usage: freerouting [OPTIONS]")
else:
    print({"ok": "0.1.0", "newer": "0.2.0", "patch": "0.1.9"}[mode].join(["freerouting ", " (abc1234)"]))
