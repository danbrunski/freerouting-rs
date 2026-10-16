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
"""

import sys
import time

mode = sys.argv[1]
args = sys.argv[2:]

if mode == "echo":
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
