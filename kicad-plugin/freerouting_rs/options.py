"""Route options, saved per board (K02.1).

Kept free of pcbnew and wx imports.

`RouteOptions.to_args()` builds the router's argument list in a fixed order.
Only options that differ from the router's own defaults produce a flag, with
one exception: the minimum trace width is always passed, because the DSN does
not carry it (D8: the board's design rules are hard limits). Its value is the
option when set, else the board's Board Setup `min_track_width`.

Ranges follow the router's settings (C02) and Java's `GlobalSettings`:

| option | flag | range, default |
|---|---|---|
| `max_passes` | `--max-passes N` (= `-mp`, without its deprecation warning) | 0-9999, 0 = no limit (default, no flag) |
| `threads` | `--threads N` | 0-1024, 0 = automatic (default, no flag) |
| `optimizer` | `--router.optimizer.enabled=false` | on (default, no flag) |
| `improvement_threshold` | `--router.optimizer.improvement_threshold=X` | 1-100 percent, unset (default, the router's 2.5) |
| `ignore_net_classes` | `--router.autorouter.ignore_net_classes=a,b`; `-inc a\\,b,c` when a name has a comma | list, empty (default) |
| `seed` | `--seed N` (CLI only) | 0 or more; the router accepts only 0 for now |
| `min_trace_width_um` | `--router.min_trace_width_um=X` | above 0, unset = from the board |
| `timeout` | `--timeout T` | `90s`, `10m`, `2h`, `HH:MM:SS`, `MM:SS`, seconds; empty = none |
| `rule_area_fixup` | (plugin side, K01.2) | on |

The GUI's front-end mode (`for_gui=True`) takes the same flags except
`--seed`. `--threads` is used rather than `-mt`: the router reads `-mt` into a flat
setting that no stage reads headless, so it would do nothing.

The options live in `<board dir>/freerouting-rs/route-options.json` with
`"schema": 1`, not in `settings.json` (executable locations, rewritten by
install.py). Restoring is defensive (KRT's lesson): unknown keys are ignored,
a value of the wrong type or outside its range keeps the default for that
field, a file that is not JSON or has another `schema` gives the defaults, and
none of it raises.
"""

import json
import re
from dataclasses import dataclass, field, fields
from pathlib import Path

SCHEMA = 1
OPTIONS_FILE = "route-options.json"

MAX_PASSES_RANGE = (0, 9999)
THREADS_RANGE = (0, 1024)
# The router scales a value between 0 and 1 by 100 (Java's `-oit 0.05` is 5 %),
# so percentages below 1 cannot be written as a `--router.` value; start at 1.
THRESHOLD_RANGE = (1.0, 100.0)

_DURATION = re.compile(r"^(?:\d+[smh]?|\d+:\d{1,2}|\d+:\d{1,2}:\d{1,2})$", re.IGNORECASE)


def valid_timeout(text):
    """True for the durations the router's `--timeout` reads, or empty."""
    return isinstance(text, str) and (text.strip() == "" or bool(_DURATION.match(text.strip())))


def _int_in(value, lo, hi):
    return isinstance(value, int) and not isinstance(value, bool) and lo <= value <= hi


def _number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and value == value and abs(value) != float("inf")


def split_net_classes(text):
    """The names in a comma separated entry; `\\,` keeps a comma in a name."""
    names, current, i = [], "", 0
    while i < len(text):
        c = text[i]
        if c == "\\" and text[i + 1 : i + 2] == ",":
            current += ","
            i += 2
            continue
        if c == ",":
            names.append(current)
            current = ""
        else:
            current += c
        i += 1
    names.append(current)
    return [n.strip() for n in names if n.strip()]


def join_net_classes(names):
    """The inverse of `split_net_classes`."""
    return ",".join(n.replace(",", "\\,") for n in names)


@dataclass
class RouteOptions:
    max_passes: int = 0
    threads: int = 0
    optimizer: bool = True
    improvement_threshold: float = None
    ignore_net_classes: list = field(default_factory=list)
    seed: int = 0
    min_trace_width_um: float = None
    timeout: str = ""
    rule_area_fixup: bool = True

    # --- the argument list ---------------------------------------------------

    def to_args(self, board_min_trace_width_um=None, for_gui=False):
        """The router's option flags (without files, `--progress` and
        `--result-json`), in a fixed order. `board_min_trace_width_um` is the
        board's minimum track width, used when `min_trace_width_um` is unset.
        `for_gui` leaves out `--seed`, which the GUI does not read."""
        args = []
        if self.max_passes:
            args += ["--max-passes", str(self.max_passes)]
        if self.threads:
            args += ["--threads", str(self.threads)]
        if not self.optimizer:
            args.append("--router.optimizer.enabled=false")
        if self.improvement_threshold is not None:
            args.append(f"--router.optimizer.improvement_threshold={_fmt(self.improvement_threshold)}")
        if self.ignore_net_classes:
            if any("," in name for name in self.ignore_net_classes):
                args += ["-inc", join_net_classes(self.ignore_net_classes)]
            else:
                args.append("--router.autorouter.ignore_net_classes=" + ",".join(self.ignore_net_classes))
        if self.seed and not for_gui:
            args += ["--seed", str(self.seed)]
        width = self.min_trace_width_um if self.min_trace_width_um is not None else board_min_trace_width_um
        if width is not None and width > 0:
            args.append(f"--router.min_trace_width_um={_fmt(width)}")
        if self.timeout.strip():
            args += ["--timeout", self.timeout.strip()]
        return args

    # --- persistence ---------------------------------------------------------

    def to_dict(self):
        d = {"schema": SCHEMA}
        for f in fields(self):
            v = getattr(self, f.name)
            d[f.name] = list(v) if isinstance(v, list) else v
        return d

    @classmethod
    def from_dict(cls, data):
        """Never raises. See the module docstring for what is kept."""
        out = cls()
        if not isinstance(data, dict) or data.get("schema") != SCHEMA:
            return out
        v = data.get("max_passes")
        if _int_in(v, *MAX_PASSES_RANGE):
            out.max_passes = v
        v = data.get("threads")
        if _int_in(v, *THREADS_RANGE):
            out.threads = v
        v = data.get("optimizer")
        if isinstance(v, bool):
            out.optimizer = v
        v = data.get("improvement_threshold")
        if _number(v) and THRESHOLD_RANGE[0] <= v <= THRESHOLD_RANGE[1]:
            out.improvement_threshold = float(v)
        v = data.get("ignore_net_classes")
        if isinstance(v, list) and all(isinstance(n, str) for n in v):
            out.ignore_net_classes = [n.strip() for n in v if n.strip()]
        v = data.get("seed")
        if _int_in(v, 0, 2**63 - 1):
            out.seed = v
        v = data.get("min_trace_width_um")
        if _number(v) and 0 < v <= 100000:
            out.min_trace_width_um = float(v)
        v = data.get("timeout")
        if isinstance(v, str) and valid_timeout(v):
            out.timeout = v.strip()
        v = data.get("rule_area_fixup")
        if isinstance(v, bool):
            out.rule_area_fixup = v
        return out


def _fmt(number):
    """`2.5`, `150`, `0.05`: no trailing zeros, no exponent."""
    text = f"{float(number):.6f}".rstrip("0").rstrip(".")
    return text or "0"


def load(path):
    """Options from `path`; the defaults when it is absent or unreadable."""
    try:
        with open(path, encoding="utf-8") as f:
            return RouteOptions.from_dict(json.load(f))
    except (OSError, ValueError):
        return RouteOptions()


def save(path, options):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(options.to_dict(), f, indent=2)
        f.write("\n")


def options_path(work_dir):
    return Path(work_dir) / OPTIONS_FILE


def load_for_board(work_dir, legacy_settings=None):
    """The options of the board whose files are in `work_dir`. Without a
    saved file, `rule_area_fixup` comes from the K01 setting `fix_rule_areas`
    of `plugin.json` (`legacy_settings`, a dict), if it has one."""
    path = options_path(work_dir)
    options = load(path)
    if not path.is_file() and isinstance(legacy_settings, dict):
        v = legacy_settings.get("fix_rule_areas")
        if isinstance(v, bool):
            options.rule_area_fixup = v
    return options


def from_form(values):
    """Options from the dialog's text boxes. `values` maps the field names to
    strings (or ints/bools for the numeric and checkbox controls). Returns
    `(options, errors)`; a field with an error keeps its default and the
    dialog shows the messages instead of routing."""
    options, errors = RouteOptions(), []

    def number(name, kind, lo, hi):
        text = str(values.get(name, "")).strip()
        if text == "":
            return None
        try:
            v = kind(text)
        except ValueError:
            errors.append(f"{name.replace('_', ' ')}: '{text}' is not a number")
            return None
        if not lo <= v <= hi:
            errors.append(f"{name.replace('_', ' ')}: {v} is outside {lo} to {hi}")
            return None
        return v

    v = number("max_passes", int, *MAX_PASSES_RANGE)
    options.max_passes = 0 if v is None else v
    v = number("threads", int, *THREADS_RANGE)
    options.threads = 0 if v is None else v
    options.optimizer = bool(values.get("optimizer", True))
    options.improvement_threshold = number("improvement_threshold", float, *THRESHOLD_RANGE)
    options.ignore_net_classes = split_net_classes(str(values.get("ignore_net_classes", "")))
    v = number("seed", int, 0, 2**63 - 1)
    options.seed = 0 if v is None else v
    options.min_trace_width_um = number("min_trace_width_um", float, 0.001, 100000.0)
    timeout = str(values.get("timeout", "")).strip()
    if valid_timeout(timeout):
        options.timeout = timeout
    else:
        errors.append(f"timeout: '{timeout}' is not a duration (use 90s, 10m, 2h or HH:MM:SS)")
    options.rule_area_fixup = bool(values.get("rule_area_fixup", True))
    return options, errors
