"""Option presets: named sets of route options kept in JSON files.

A preset is a file in the plugin's options format (options.py, the same as
`route-options.json`) with a `name` and an optional `description`. It may
hold only the options it wants to set; applying it changes those fields and
leaves the rest of the dialog as it is. The plugin ships no preset: a board
or a user brings its own, so the tool carries no board-specific behaviour.

Presets are listed from two places, the board's first:

1. `<board dir>/freerouting-rs/presets/*.json` (presets that live with the
   design; `save` writes here);
2. the user's presets folder, if it exists: `$FREEROUTING_RS_PRESETS`, else
   `~/.config/freerouting-rs/presets` (`%APPDATA%\\freerouting-rs\\presets` on
   Windows). It is never created.

A board preset hides a user preset of the same name. A file that is not JSON,
is not an object, has no `name`, or has a schema `options.py` cannot read is
skipped and named in the warnings; nothing here raises. The values are checked
by `RouteOptions.from_dict`, the rules of the options file: a bad value keeps
the default.

Pure Python (no pcbnew, no wx).
"""

import json
import os
import re
from dataclasses import dataclass, fields, replace
from pathlib import Path

try:  # inside the plugin package
    from . import options
except ImportError:  # tests and headless scripts put this folder on sys.path
    import options

PRESET_DIR = "presets"
USER_ENV = "FREEROUTING_RS_PRESETS"
_META_KEYS = ("schema", "name", "description")
_FIELDS = {f.name for f in fields(options.RouteOptions)}
# Options whose "not set" is null in a preset file.
_NULLABLE = ("improvement_threshold", "min_trace_width_um", "start_ripup_costs", "sweep_passes", "pass_hint")
# Left out of a saved preset: the pass hint is a memory of the last run on one board.
_NOT_SAVED = ("pass_hint",)


@dataclass
class Preset:
    name: str
    description: str
    data: dict  # the options it sets, as stored (without the meta keys)
    path: Path
    source: str  # "board" or "user"

    def label(self):
        return f"{self.name} ({self.source})"


def board_dir(work_dir):
    return Path(work_dir) / PRESET_DIR


def user_dir(env=None, home=None, windows=None):
    """The user's presets folder when it exists, else None."""
    env = os.environ if env is None else env
    if env.get(USER_ENV):
        path = Path(env[USER_ENV])
    else:
        windows = os.name == "nt" if windows is None else windows
        if windows and env.get("APPDATA"):
            path = Path(env["APPDATA"]) / "freerouting-rs" / PRESET_DIR
        else:
            path = Path(home or Path.home()) / ".config" / "freerouting-rs" / PRESET_DIR
    return path if path.is_dir() else None


def parse(text, path, source):
    """`(preset, warning)`: one of them is None."""
    try:
        data = json.loads(text)
    except ValueError as e:
        return None, f"preset {path.name} skipped: not JSON ({e})"
    if not isinstance(data, dict):
        return None, f"preset {path.name} skipped: not a JSON object"
    name = data.get("name")
    if not isinstance(name, str) or not name.strip():
        return None, f"preset {path.name} skipped: no \"name\""
    if data.get("schema") not in options.READABLE_SCHEMAS or isinstance(data.get("schema"), bool):
        return None, f"preset {path.name} skipped: \"schema\" must be one of {list(options.READABLE_SCHEMAS)}"
    description = data.get("description")
    description = description if isinstance(description, str) else ""
    body = {k: v for k, v in data.items() if k not in _META_KEYS}
    return Preset(name.strip(), description, body, Path(path), source), None


def _scan(folder, source, warnings):
    found = []
    if folder is None or not Path(folder).is_dir():
        return found
    for path in sorted(Path(folder).glob("*.json")):
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, ValueError) as e:
            warnings.append(f"preset {path.name} skipped: unreadable ({e})")
            continue
        preset, warning = parse(text, path, source)
        if warning:
            warnings.append(warning)
        else:
            found.append(preset)
    return found


def discover(work_dir, user=None):
    """`(presets, warnings)`: the board's presets, then the user's that the
    board does not hide, each in file-name order."""
    warnings = []
    mine = _scan(board_dir(work_dir), "board", warnings)
    names = {p.name for p in mine}
    theirs = [p for p in _scan(user, "user", warnings) if p.name not in names]
    return mine + theirs, warnings


def apply(preset, current):
    """`current` (a `RouteOptions`) with the preset's options on top. Each
    value is checked as the options file checks it; a bad one keeps the value
    `current` has. Returns `(options, warnings)`; warnings name unknown keys."""
    warnings = []
    out = current
    for key, value in preset.data.items():
        if key not in _FIELDS:
            warnings.append(f"preset {preset.name}: unknown option {key!r} ignored")
            continue
        if value is None and key in _NULLABLE:  # "unset": back to the profile's default
            out = replace(out, **{key: getattr(options.profile_defaults(current.profile), key)})
            continue
        checked = options.RouteOptions.from_dict({"schema": options.SCHEMA, "profile": current.profile, key: value})
        good = getattr(checked, key)
        if key == "profile":
            good = value if value in options.PROFILES else current.profile
        if good == getattr(options.profile_defaults(current.profile), key) and value != good and key != "profile":
            warnings.append(f"preset {preset.name}: {key}={value!r} is not valid, ignored")
            continue
        out = replace(out, **{key: good})
    return out, warnings


def slug(name):
    """A file name stem for `name`: letters, digits, `-` and `_` (other
    characters, including non-ASCII letters, are kept when they are word
    characters); empty when nothing is left."""
    text = re.sub(r"[^\w-]+", "-", name.strip()).strip("-_")
    return text[:64]


def save(work_dir, name, opts, description="", stage_defaults=None):
    """Writes the current options as a preset in the board's presets folder.
    Every option is written, so applying it restores them all; a preset of the same file name is replaced. Returns the path; raises
    `ValueError` for a name with nothing usable in it."""
    stem = slug(name)
    if not stem:
        raise ValueError("a preset needs a name with at least one letter or digit")
    path = board_dir(work_dir) / (stem + ".json")
    path.parent.mkdir(parents=True, exist_ok=True)
    data = {"name": name.strip(), "description": description}
    # Every option, so applying the preset restores them all; a hand-written
    # preset may hold fewer.
    stored = opts.to_dict(stage_defaults)
    full = {f.name: getattr(opts, f.name) for f in fields(options.RouteOptions) if f.name not in _NOT_SAVED}
    full["stages"] = stored.get("stages", {})
    data = {"schema": stored["schema"], **data, **full}
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(data, f, indent=2)
        f.write("\n")
    return path
