"""Plugin version and the check against the router's version.

Kept free of pcbnew and wx imports. `__version__` equals the workspace
version in the repository's `Cargo.toml` (tests/test_version.py checks it;
R01.1's release gate keeps the two equal).

The router prints `freerouting <x.y.z> (<sha>)` for `--version` (C08's
format). The plugin and the router must agree on major.minor.
"""

import re

__version__ = "0.1.0"

_VERSION_LINE = re.compile(
    r"^freerouting\s+(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)(?P<pre>[-+][0-9A-Za-z.+-]*)?"
    r"(?:\s+\((?:git\s+)?(?P<sha>[0-9A-Za-z]+)\))?$"
)

OK = "ok"
UNKNOWN = "unknown"
MISMATCH = "mismatch"


def parse_router_version(text):
    """Parses the output of `freerouting --version`. Returns
    (major, minor, patch, sha_or_None) or None when it is not that format.
    Only the first non-empty line counts."""
    for line in (text or "").splitlines():
        line = line.strip()
        if not line:
            continue
        m = _VERSION_LINE.match(line)
        if not m:
            return None
        return int(m["major"]), int(m["minor"]), int(m["patch"]), m["sha"]
    return None


def format_router_version(parsed):
    if parsed is None:
        return "unknown"
    major, minor, patch, sha = parsed
    return f"{major}.{minor}.{patch}" + (f" ({sha})" if sha else "")


def check(router, plugin_version=__version__):
    """Compares a parsed router version (or None) with the plugin's.

    Returns (status, message): OK, UNKNOWN (Route allowed, with the warning
    in `message`) or MISMATCH (Route refused; `message` names both versions
    and the fix)."""
    plugin = tuple(int(x) for x in plugin_version.split(".")[:3])
    if router is None:
        return UNKNOWN, (
            "The router did not report a version (it may predate --version). "
            f"Plugin {plugin_version}; routing is allowed, but a mismatch cannot be ruled out."
        )
    if router[:2] != plugin[:2]:
        return MISMATCH, (
            f"Router {format_router_version(router)} does not match plugin {plugin_version} "
            f"(major.minor must agree). Rebuild the router from the same source "
            f"(cargo build --release -p freerouting-cli) or reinstall the plugin with install.py."
        )
    return OK, f"Router {format_router_version(router)}, plugin {plugin_version}."
