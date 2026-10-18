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
| `board_input` | (plugin side, K10) what the router reads: `dsn` (KiCad's export, default) or `kicad_pcb` (a snapshot of the open board, read directly) | `dsn` |
| `apply_kicad_rules` | `--apply-design-rules`, only with `board_input = kicad_pcb` (K10.3) | off |
| `profile` | `--profile legacy` (N01.3) | `legacy` or `enhanced` (default, no flag) |
| `stages` | `--router.<key>=<id>`, sorted by key (N01.4) | the "Advanced" stage choices that differ from the profile's |

`profile` is the routing profile: `legacy` gives results identical to Java
Freerouting, `enhanced` (the router's default) the accepted improvements.
`stages` maps a stage slot's settings key (`autorouter.connection_router`) to
the stage id picked in the dialog's "Advanced" group; the slots and the
profile's choices come from the router's `--list-stages=json`
(`parse_stage_listing`). A choice equal to the profile's is not kept, so a
board follows the profile until the user picks something else.

The GUI's front-end mode (`for_gui=True`) takes the same flags except
`--seed`. `--threads` is used rather than `-mt`: the router reads `-mt` into a flat
setting that no stage reads headless, so it would do nothing.

The options live in `<board dir>/freerouting-rs/route-options.json` with
`"schema": 2`, not in `settings.json` (executable locations, rewritten by
install.py). The file stores the profile and only the values that differ
from it: an option at its default and a stage choice equal to the profile's
are left out. A schema 1 file (every option written out, no profile) still
loads, as `enhanced`. Restoring is defensive (KRT's lesson): unknown keys are
ignored, a value of the wrong type or outside its range keeps the default for
that field, a file that is not JSON or has another `schema` gives the
defaults, and none of it raises.
"""

import json
import re
from dataclasses import dataclass, field, fields, replace
from pathlib import Path

SCHEMA = 4
# Schema 1 (K02.1) wrote every option and had no profile; schema 2 (N01) had
# no K09 fields; schema 3 (K09) had no K10 fields. All still load: a field a
# file leaves out takes the profile's default (`profile_defaults`).
READABLE_SCHEMAS = (1, 2, 3, 4)
OPTIONS_FILE = "route-options.json"

# The router's routing profiles (N01): `legacy` is Java Freerouting's results,
# `enhanced` the router's default.
PROFILES = ("legacy", "enhanced")
DEFAULT_PROFILE = "enhanced"
PROFILE_LABELS = {"legacy": "Legacy", "enhanced": "Enhanced"}
_STAGE_KEY = re.compile(r"^[a-z_]+(?:\.[a-z_]+)+$")
_STAGE_ID = re.compile(r"^[A-Za-z0-9_.-]+$")

AUTO = "auto"  # a fix-up list derived from the board (dsn_fixups.resolve)
POUR_MODES = ("wires", "engine")
# K10: what the router reads. `dsn` is KiCad's export (the default: the Java
# comparison and K02/K09's graded flow); `kicad_pcb` a snapshot of the open
# board read by the router's own reader.
INPUT_DSN, INPUT_KICAD_PCB = "dsn", "kicad_pcb"
BOARD_INPUTS = (INPUT_DSN, INPUT_KICAD_PCB)
BOARD_INPUT_LABELS = {INPUT_DSN: "KiCad's DSN export", INPUT_KICAD_PCB: "Board snapshot (experimental)"}
SWEEPS = ("", "fixed", "free")
RIPUP_RANGE = (1, 1000000)
SWEEP_PASSES_RANGE = (1, 100)
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
    profile: str = DEFAULT_PROFILE
    stages: dict = field(default_factory=dict)
    # K09: DSN fix-ups (dsn_fixups.py) and the router settings of canary's flow.
    class_widths: object = field(default_factory=dict)  # {class: mm} or "auto"
    fixed_pour_classes: object = field(default_factory=list)  # [class] or "auto"
    fixed_pour_mode: str = "wires"  # "wires" (K09.2 arm b) or "engine" (arm a, Q01)
    drop_pour_nets: object = field(default_factory=list)  # [net] or "auto"
    edge_and_pad_keepouts: bool = True  # board rules: on under both profiles (K09.4, K09.9)
    route_on_plane_layers: list = field(default_factory=list)
    fix_existing: bool = False
    start_ripup_costs: int = None
    sweep: str = ""  # "", "fixed" or "free" (Q10)
    sweep_passes: int = None
    use_pass_hint: bool = False
    pass_hint: int = None  # the pass that won last time (K09.7), written after a run
    # K10: the direct read of the board (snapshot.py).
    board_input: str = INPUT_DSN
    apply_kicad_rules: bool = False  # --apply-design-rules; kicad_pcb only

    # --- the argument list ---------------------------------------------------

    def to_args(self, board_min_trace_width_um=None, for_gui=False, extra_args=()):
        """The router's option flags (without files, `--progress` and
        `--result-json`), in a fixed order. `board_min_trace_width_um` is the
        board's minimum track width, used when `min_trace_width_um` is unset.
        `for_gui` leaves out `--seed`, which the GUI does not read.
        `extra_args` (the fix-ups' engine flags, `export_flow.prepare`) go
        before the stage choices."""
        args = []
        if self.profile != DEFAULT_PROFILE:
            args += ["--profile", self.profile]
        passes = self.max_passes or (self.pass_hint if self.use_pass_hint and self.pass_hint else 0)
        if passes:
            args += ["--max-passes", str(passes)]
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
        if self.start_ripup_costs is not None:
            args.append(f"--router.scoring.start_ripup_costs={self.start_ripup_costs}")
        if self.sweep in SWEEPS[1:]:
            args.append(f"--router.autorouter.sweep={self.sweep}")
            if self.sweep_passes is not None:
                args.append(f"--router.autorouter.sweep_passes={self.sweep_passes}")
        if self.apply_kicad_rules and self.board_input == INPUT_KICAD_PCB and not for_gui:
            # The GUI reads the rules file without applying it (it has no such switch).
            args.append("--apply-design-rules")
        args += list(extra_args)
        for key in sorted(self.stages):
            args.append(f"--router.{key}={self.stages[key]}")
        return args

    # --- persistence ---------------------------------------------------------

    def to_dict(self, stage_defaults=None):
        """The stored form: the schema, the profile, and only the values that
        differ from it. `stage_defaults` maps a slot key to the profile's id
        (`parse_stage_listing`); a stage choice equal to it is left out."""
        d = {"schema": SCHEMA, "profile": self.profile}
        defaults = profile_defaults(self.profile)
        for f in fields(self):
            if f.name in ("profile", "stages"):
                continue
            v = getattr(self, f.name)
            if v != getattr(defaults, f.name):
                d[f.name] = list(v) if isinstance(v, list) else dict(v) if isinstance(v, dict) else v
        stages = differing_stages(self.stages, stage_defaults)
        if stages:
            d["stages"] = stages
        return d

    @classmethod
    def from_dict(cls, data):
        """Never raises. See the module docstring for what is kept."""
        if not isinstance(data, dict) or data.get("schema") not in READABLE_SCHEMAS:
            return profile_defaults(DEFAULT_PROFILE)
        if isinstance(data.get("schema"), bool):
            return profile_defaults(DEFAULT_PROFILE)
        v = data.get("profile")
        out = profile_defaults(v if v in PROFILES else DEFAULT_PROFILE)
        v = data.get("stages")
        if isinstance(v, dict):
            out.stages = {
                k: s for k, s in v.items() if isinstance(k, str) and isinstance(s, str) and valid_stage(k, s)
            }
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
        _restore_k09(out, data)
        return out


def _names(value):
    return isinstance(value, list) and all(isinstance(n, str) and n.strip() for n in value)


def _restore_k09(out, data):
    """The K09 fields of a stored dict; a bad value keeps the default."""
    v = data.get("class_widths")
    if v == AUTO or (
        isinstance(v, dict) and all(isinstance(k, str) and k and _number(w) and 0 < w <= 100 for k, w in v.items())
    ):
        out.class_widths = v if v == AUTO else {k: float(w) for k, w in v.items()}
    for name in ("fixed_pour_classes", "drop_pour_nets"):
        v = data.get(name)
        if v == AUTO or _names(v):
            setattr(out, name, v if v == AUTO else [n.strip() for n in v])
    v = data.get("fixed_pour_mode")
    if v in POUR_MODES:
        out.fixed_pour_mode = v
    v = data.get("route_on_plane_layers")
    if _names(v):
        out.route_on_plane_layers = [n.strip() for n in v]
    for name in ("edge_and_pad_keepouts", "fix_existing", "use_pass_hint"):
        v = data.get(name)
        if isinstance(v, bool):
            setattr(out, name, v)
    v = data.get("start_ripup_costs")
    if _int_in(v, *RIPUP_RANGE):
        out.start_ripup_costs = v
    v = data.get("sweep")
    if v in SWEEPS:
        out.sweep = v
    v = data.get("sweep_passes")
    if _int_in(v, *SWEEP_PASSES_RANGE):
        out.sweep_passes = v
    v = data.get("pass_hint")
    if _int_in(v, 1, MAX_PASSES_RANGE[1]):
        out.pass_hint = v
    v = data.get("board_input")
    if v in BOARD_INPUTS:
        out.board_input = v
    v = data.get("apply_kicad_rules")
    if isinstance(v, bool):
        out.apply_kicad_rules = v


# The plugin's defaults per profile (K09.9): what each profile changes from the
# dataclass defaults above, each entry naming the measurement that accepted
# it. `legacy` keeps the plugin's behaviour before K09, except the board's
# own clearance rules (`edge_and_pad_keepouts`, on in the dataclass), which
# hold under both profiles. `enhanced` adds Q10's add-only completion sweep
# (`free`, 2 passes) after the router's passes: the keep rule of
# docs/roadmap/prereg/K09.9-rerun.json (2026-10-18, engine 571d06d, the
# plugin's headless path on canary-pwr and three KiCad demos) passed `+sweep`
# (KiCad-unconnected total 95 -> 74 against a set noise of 17.14, no board
# worse, no new clearance error, no via in a pad, no power-layer trace) and
# `+cli-flow` (73, with -mp 1, ripup 1500 and the auto pour fix-ups), and gave
# the tie to `+sweep` by wall time (282 s against 439 s over the four boards).
# The arm passed `sweep_passes=2`; the default leaves `sweep_passes` unset,
# which is the router's 2 (`DEFAULT_SWEEP_PASSES`; the same session byte for
# byte on interf_u) and keeps a stored file free of a null pass count.
# `-mp 1` alone (125), `-mp 1` with the sweep (81) and the CLI flow's flags
# (83, a new clearance error on CM5_MINIMA_3) did not pass. K09.json's first
# round (2026-10-17, no candidate passed) is in K09.9's note. `enhanced` keeps
# the Board Setup class widths (no `class_widths`): the engine's own
# `enhanced` profile necks a connection down only where it fails at full
# width (`router.neck_width_um = 300`), which N05.3 measured against class
# narrowing (docs/roadmap/prereg/N05-neck-down.json: no-regress, copper under
# the Board Setup width 848.9 -> 310.4 mm), so the plugin passes no flag for it.
PROFILE_DEFAULTS = {
    "legacy": {},
    "enhanced": {"sweep": "free"},
}


def profile_defaults(profile=DEFAULT_PROFILE):
    """The options a board starts from under `profile`."""
    return replace(RouteOptions(), profile=profile, **PROFILE_DEFAULTS.get(profile, {}))


def dsn_settings(opts):
    """The `dsn_fixups.apply` settings of `opts`. With `fixed_pour_mode`
    `engine` the pours stay planes and the classes go to the router."""
    return {
        "class_widths": opts.class_widths,
        "fixed_pour_classes": opts.fixed_pour_classes if opts.fixed_pour_mode == "wires" else [],
        "drop_pour_nets": opts.drop_pour_nets,
        "edge_and_pad_keepouts": opts.edge_and_pad_keepouts,
        "route_on_plane_layers": opts.route_on_plane_layers,
    }


def format_class_widths(widths):
    """`CLASS=mm, CLASS=mm`, or `auto`."""
    if widths == AUTO:
        return AUTO
    return ", ".join(f"{k}={_fmt(v)}" for k, v in sorted(widths.items()))


def parse_class_widths(text):
    """`(widths, error)` from `CLASS=mm, CLASS=mm` or `auto`."""
    text = text.strip()
    if text.lower() == AUTO:
        return AUTO, None
    out = {}
    for part in filter(None, (p.strip() for p in text.split(","))):
        name, sep, value = part.partition("=")
        try:
            mm = float(value) if sep else None
        except ValueError:
            mm = None
        if not name.strip() or mm is None or not 0 < mm <= 100:
            return {}, f"class widths: '{part}' is not CLASS=mm (0 to 100 mm)"
        out[name.strip()] = mm
    return out, None


def format_names(names):
    return AUTO if names == AUTO else ", ".join(names)


def parse_names(text, allow_auto=True):
    text = text.strip()
    if allow_auto and text.lower() == AUTO:
        return AUTO
    return [n.strip() for n in text.split(",") if n.strip()]


def valid_stage(key, stage_id):
    """True for a slot key such as `autorouter.connection_router` and a
    stage id such as `freerouting-connection`."""
    return bool(_STAGE_KEY.match(key)) and bool(_STAGE_ID.match(stage_id))


def differing_stages(stages, stage_defaults=None):
    """The stage choices that differ from the profile's, sorted by key."""
    defaults = stage_defaults or {}
    return {k: stages[k] for k in sorted(stages) if stages[k] != defaults.get(k)}


def parse_stage_listing(data):
    """The stage slots of the router's `--list-stages=json` (a dict): a list
    of `(key, label, ids, default)` for the slots that take one stage, the key
    without `router.`, `default` the profile's id ("" for none). Slots that
    take a list are left out. A damaged listing gives what can be read."""
    slots = []
    if not isinstance(data, dict) or not isinstance(data.get("slots"), list):
        return slots
    for slot in data["slots"]:
        if not isinstance(slot, dict) or slot.get("arity") not in ("one", "optional"):
            continue
        key, label = slot.get("key"), slot.get("label")
        if not isinstance(key, str) or not key.startswith("router.") or not isinstance(label, str):
            continue
        ids = [i for i in slot.get("ids", []) if isinstance(i, str) and _STAGE_ID.match(i)]
        if slot["arity"] == "optional":
            ids = [""] + ids
        default = slot.get("default")
        default = default[0] if isinstance(default, list) and default and isinstance(default[0], str) else ""
        slots.append((key[len("router."):], label, ids, default))
    return slots


EXPERIMENTAL_MARK = " (experimental)"


def experimental_stage_ids(data):
    """`{key: {ids}}` of the slots' `experimental_ids` in the router's
    `--list-stages=json` (N02.4): stage ids that are off in every profile and
    meet N02's keep rule; the Advanced group marks them. Keys are without
    `router.`; a damaged or older listing gives what can be read."""
    out = {}
    if not isinstance(data, dict) or not isinstance(data.get("slots"), list):
        return out
    for slot in data["slots"]:
        if not isinstance(slot, dict):
            continue
        key, found = slot.get("key"), slot.get("experimental_ids")
        if not isinstance(key, str) or not key.startswith("router.") or not isinstance(found, list):
            continue
        ids = {i for i in found if isinstance(i, str) and _STAGE_ID.match(i)}
        if ids:
            out[key[len("router."):]] = ids
    return out


def stage_label(stage_id, experimental_ids=()):
    """The text of a stage id in the Advanced group: `(none)` for no stage,
    the id marked `(experimental)` when the router says so."""
    if not stage_id:
        return "(none)"
    return stage_id + EXPERIMENTAL_MARK if stage_id in experimental_ids else stage_id


def stage_defaults(slots):
    """`{key: default id}` of `parse_stage_listing`'s slots."""
    return {key: default for key, _, _, default in slots}


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


def save(path, options, stage_defaults=None):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(options.to_dict(stage_defaults), f, indent=2)
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
    dialog shows the messages instead of routing. A field `values` leaves
    out keeps the profile's default."""
    errors = []
    profile = str(values.get("profile", DEFAULT_PROFILE)).strip().lower()
    options = profile_defaults(profile if profile in PROFILES else DEFAULT_PROFILE)

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
    _form_k09(options, values, errors, number)
    _form_k10(options, values, errors)
    if profile not in PROFILES:
        errors.append(f"profile: '{profile}' is not a profile (use {' or '.join(PROFILES)})")
    stages = values.get("stages", {})
    if isinstance(stages, dict):
        for key, stage_id in stages.items():
            if stage_id in ("", None):
                continue
            if valid_stage(str(key), str(stage_id)):
                options.stages[str(key)] = str(stage_id)
            else:
                errors.append(f"{key}: '{stage_id}' is not a stage id")
    return options, errors


def _form_k09(options, values, errors, number):
    """The K09 fields of the form; a field the form leaves out keeps the
    profile's default."""
    if "class_widths" in values:
        widths, error = parse_class_widths(str(values["class_widths"]))
        options.class_widths = widths
        if error:
            errors.append(error)
    for name in ("fixed_pour_classes", "drop_pour_nets"):
        if name in values:
            setattr(options, name, parse_names(str(values[name])))
    if "route_on_plane_layers" in values:
        options.route_on_plane_layers = parse_names(str(values["route_on_plane_layers"]), allow_auto=False)
    mode = str(values.get("fixed_pour_mode", options.fixed_pour_mode)).strip().lower()
    if mode in POUR_MODES:
        options.fixed_pour_mode = mode
    else:
        errors.append(f"fixed pour mode: '{mode}' is not one of {', '.join(POUR_MODES)}")
    for name in ("edge_and_pad_keepouts", "fix_existing", "use_pass_hint"):
        if name in values:
            setattr(options, name, bool(values[name]))
    if "start_ripup_costs" in values:
        options.start_ripup_costs = number("start_ripup_costs", int, *RIPUP_RANGE)
    sweep = str(values.get("sweep", options.sweep)).strip().lower()
    sweep = "" if sweep in ("off", "none") else sweep
    if sweep in SWEEPS:
        options.sweep = sweep
    else:
        errors.append(f"sweep: '{sweep}' is not off, fixed or free")
    if "sweep_passes" in values:
        options.sweep_passes = number("sweep_passes", int, *SWEEP_PASSES_RANGE)
    if "pass_hint" in values:
        options.pass_hint = number("pass_hint", int, 1, MAX_PASSES_RANGE[1])


# The K09 options that rewrite the DSN text, by the name the dialog shows
# (K10.4). A `.kicad_pcb` input has no DSN text to rewrite, and the reader's
# tree has no form of these yet, so `board_input = kicad_pcb` refuses them.
DSN_REWRITE_LABELS = {
    "class_widths": "Class widths",
    "fixed_pour_classes": "Fixed pour classes",
    "drop_pour_nets": "Drop pours of nets",
    "edge_and_pad_keepouts": "Keep the board-edge and pad clearances",
    "route_on_plane_layers": "Route on plane layers",
}


def input_conflicts(opts):
    """The dialog labels of the options that need the DSN text and are on, as
    a list, for `opts.board_input == "kicad_pcb"`; empty for a DSN input or
    when none is on.

    `fixed_pour_classes` in `engine` mode is allowed when the classes are
    named: it is the engine flag `--router.plane_obstacle_net_classes`, which
    does not touch the input. `auto` needs the DSN text to find the classes."""
    if opts.board_input != INPUT_KICAD_PCB:
        return []
    out = []
    if opts.class_widths:
        out.append(DSN_REWRITE_LABELS["class_widths"])
    if opts.fixed_pour_classes and (opts.fixed_pour_mode == "wires" or opts.fixed_pour_classes == AUTO):
        out.append(DSN_REWRITE_LABELS["fixed_pour_classes"])
    if opts.drop_pour_nets:
        out.append(DSN_REWRITE_LABELS["drop_pour_nets"])
    if opts.edge_and_pad_keepouts:
        out.append(DSN_REWRITE_LABELS["edge_and_pad_keepouts"])
    if opts.route_on_plane_layers:
        out.append(DSN_REWRITE_LABELS["route_on_plane_layers"])
    return out


def _form_k10(options, values, errors):
    """The K10 fields of the form; a field the form leaves out keeps the
    profile's default. Refuses a board snapshot while a DSN-rewriting option
    is on, naming each (K10.4)."""
    chosen = str(values.get("board_input", options.board_input)).strip().lower()
    if chosen in BOARD_INPUTS:
        options.board_input = chosen
    else:
        errors.append(f"board input: '{chosen}' is not one of {', '.join(BOARD_INPUTS)}")
    if "apply_kicad_rules" in values:
        options.apply_kicad_rules = bool(values["apply_kicad_rules"])
    for label in input_conflicts(options):
        errors.append(
            f"board input: the board snapshot is read without a DSN, so '{label}' cannot apply. "
            f"Turn it off, or choose {BOARD_INPUT_LABELS[INPUT_DSN]}."
        )
