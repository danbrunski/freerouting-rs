"""A stand-in for freerouting-gui in front-end mode, used by the plugin tests.

    fake_gui.py <mode> [args...]

It records its arguments as JSON in `<-do file>.argv` (next to the output),
then:
  write    writes the `-do` session (as the GUI does when routing ends) and exits 0
  nowrite  exits 0 without writing anything
  hang     waits (up to 60 s) so the test can terminate it
"""

import json
import sys
import time

mode = sys.argv[1]
args = sys.argv[2:]
out = args[args.index("-do") + 1] if "-do" in args else None
if out:
    with open(out + ".argv", "w", encoding="utf-8") as f:
        json.dump(args, f)
if mode == "write":
    with open(out, "w", encoding="utf-8") as f:
        f.write("(session fake-gui)\n")
elif mode == "hang":
    time.sleep(60)
