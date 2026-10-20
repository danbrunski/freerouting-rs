"""The text shown after a routing run (K02.4), from the router's
`--result-json` manifest (C08.4).

Kept free of pcbnew and wx imports. Reads the manifest defensively: a key that
is missing or of another type reads as unknown, never as an exception.

Fields used (Java's `RoutingResultManifest` names):
`board_statistics.connections.maximum_count` / `incomplete_count`,
`board_statistics.vias.total_count`, `board_statistics.traces.total_length_mm`,
`board_statistics.clearance_violations.total_count`, `final_state`, and for
the time the sum of `phases.*.duration_seconds` unless the caller has the
`done` event's `elapsed_ms`.

The first line names the routing profile (N01.3): `settings_snapshot.profile`
of the manifest when the router wrote it, else the profile the plugin asked
for, else the router's default, Enhanced.
"""

import json

try:  # as part of the plugin package
    from . import progress
except ImportError:  # top-level import, as the tests do
    import progress


def load_manifest(path):
    """The manifest at `path`, or None when it is absent or not a JSON object."""
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def _get(data, *keys):
    for key in keys:
        if not isinstance(data, dict):
            return None
        data = data.get(key)
    return data


def _int(value):
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def _number(value):
    return float(value) if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def incomplete_count(manifest):
    """The manifest's incomplete connections, or None."""
    return _int(_get(manifest, "board_statistics", "connections", "incomplete_count"))


def elapsed_seconds(manifest, elapsed_ms=None):
    if isinstance(elapsed_ms, (int, float)) and not isinstance(elapsed_ms, bool):
        return elapsed_ms / 1000
    phases = _get(manifest, "phases")
    if not isinstance(phases, dict):
        return None
    times = [_number(_get(p, "duration_seconds")) for p in phases.values()]
    times = [t for t in times if t is not None]
    return sum(times) if times else None


PROFILE_LABELS = {"legacy": "Legacy", "enhanced": "Enhanced"}


def profile_label(manifest, profile=None):
    """`Legacy` or `Enhanced`: the manifest's profile, else `profile`, else
    the default."""
    named = _get(manifest, "settings_snapshot", "profile")
    for candidate in (named, profile):
        if isinstance(candidate, str) and candidate.strip().lower() in PROFILE_LABELS:
            return PROFILE_LABELS[candidate.strip().lower()]
    return PROFILE_LABELS["enhanced"]


def _rules_text(rules):
    """`clearance 1, track_width 2` for a `{rule: count}` object, in key order."""
    if not isinstance(rules, dict):
        return ""
    return ", ".join(f"{rule} {count}" for rule, count in sorted(rules.items()) if _int(count))


def output_check_lines(manifest):
    """The summary lines of the manifest's `output_check` (Q20.3), none when the
    router did not run the check (`--router.output_check=off`, profile Legacy)."""
    check = _get(manifest, "output_check")
    if not isinstance(check, dict):
        return []
    mode = check.get("mode")
    found = _int(check.get("found")) or 0
    lines = []
    if found == 0:
        lines.append("Rule check: no routed copper breaks a board rule")
    else:
        detail = _rules_text(check.get("by_rule"))
        line = f"Rule check: {found} breach(es) of the board's rules in routed copper ({detail})"
        if mode == "repair":
            line += (
                f"; removed {_int(check.get('removed')) or 0}, retried "
                f"{_int(check.get('retried')) or 0} connection(s), repaired "
                f"{_int(check.get('repaired')) or 0}, left open {_int(check.get('left_open')) or 0}"
            )
        lines.append(line)
    input_rules = check.get("input")
    input_total = sum(_int(v) or 0 for v in input_rules.values()) if isinstance(input_rules, dict) else 0
    if input_total:
        lines.append(
            f"The board already had {input_total} rule violation(s), left as they were: "
            f"{_rules_text(input_rules)}"
        )
    remaining = _int(check.get("remaining")) or 0
    if remaining:
        lines.append(f"WARNING: {remaining} routed breach(es) of the board's rules were saved.")
    return lines


def format_summary(manifest, elapsed_ms=None, profile=None):
    """The summary text, several lines. `manifest` None gives a one-line note.
    `profile` is the profile the run was started with (see the module docs)."""
    if manifest is None:
        return "No result summary: the router wrote no result file."
    maximum = _int(_get(manifest, "board_statistics", "connections", "maximum_count"))
    incomplete = incomplete_count(manifest)
    vias = _int(_get(manifest, "board_statistics", "vias", "total_count"))
    length = _number(_get(manifest, "board_statistics", "traces", "total_length_mm"))
    violations = _int(_get(manifest, "board_statistics", "clearance_violations", "total_count"))
    seconds = elapsed_seconds(manifest, elapsed_ms)
    state = _get(manifest, "final_state")

    lines = [f"Profile: {profile_label(manifest, profile)}"]
    if maximum is not None and incomplete is not None:
        routed = max(maximum - incomplete, 0)
        lines.append(f"Connections: {routed} of {maximum} routed, {incomplete} incomplete")
    else:
        lines.append("Connections: unknown")
    lines.append(f"Vias: {vias if vias is not None else 'unknown'}")
    lines.append(f"Total trace length: {length:.1f} mm" if length is not None else "Total trace length: unknown")
    lines.append(f"Time: {progress.format_seconds(seconds)}" if seconds is not None else "Time: unknown")
    if isinstance(state, str) and state != "COMPLETED":
        lines.append(f"Run state: {state}")
    if violations:
        lines.append(f"Clearance violations: {violations}")
    lines.extend(output_check_lines(manifest))
    if incomplete:
        noun = "connection is" if incomplete == 1 else "connections are"
        lines.append("")
        lines.append(f"WARNING: {incomplete} {noun} not routed. Check the board before you use it.")
    return "\n".join(lines)
