"""K15.32: the clearances a `.kicad_dru` can set, for the router's pour clearance.

KiCad's zone filler asks its rule engine for the gap between a zone and every
other item (`ZONE_FILLER`: `EvalRules( CLEARANCE_CONSTRAINT, zone, item )`), and
a custom rule wins over the zone's own clearance: the pcu board's unconditional
`(constraint clearance (min 0.127mm))` made KiCad 10.0.7 fill a 0.5 mm zone
0.127 mm from a foreign track (K15.29). The plugin therefore passes the router
the smallest clearance any rule of the file can set, conditional rules included
(the minimum is the safe side: a cleanup floor above KiCad's gap would judge a
trace dangling that KiCad counts connected).

Only the `clearance` constraint is read. `physical_clearance` and
`hole_clearance` enter the filler as `max( gap, ... )`, so they never lower the
gap. Stdlib only; a file this parser cannot read raises `RulesError` and the
caller passes no clearance.
"""

import re

_UNITS_NM = {"": 1_000_000, "mm": 1_000_000, "um": 1_000, "mil": 25_400, "in": 25_400_000}
_NUMBER = re.compile(r"^([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)(mm|um|mil|in)?$")


class RulesError(ValueError):
    """The rules file cannot be read."""


def _tokens(text):
    """`(`, `)`, quoted strings (marked by a leading `"`) and bare words; `#` starts a comment."""
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c in " \t\r\n":
            i += 1
        elif c == "#":
            while i < n and text[i] != "\n":
                i += 1
        elif c in "()":
            yield c
            i += 1
        elif c == '"':
            j, out = i + 1, []
            while j < n and text[j] != '"':
                if text[j] == "\\" and j + 1 < n:
                    j += 1
                out.append(text[j])
                j += 1
            if j >= n:
                raise RulesError("unterminated string")
            yield '"' + "".join(out)
            i = j + 1
        else:
            j = i
            while j < n and text[j] not in " \t\r\n()":
                j += 1
            yield text[i:j]
            i = j


def parse(text):
    """The S-expression list of `text`: `(a (b c))` is `["a", ["b", "c"]]`; a quoted string keeps its `"` prefix."""
    stack, top = [], []
    for tok in _tokens(text):
        if tok == "(":
            stack.append(top)
            top = []
        elif tok == ")":
            if not stack:
                raise RulesError("unbalanced )")
            done, top = top, stack.pop()
            top.append(done)
        else:
            top.append(tok)
    if stack:
        raise RulesError("unbalanced (")
    return top


def length_nm(token):
    """A rule length (`0.127mm`, `5mil`, `0.1`: bare numbers are mm) in nm."""
    m = _NUMBER.match(token) if isinstance(token, str) else None
    if not m:
        raise RulesError(f"not a length: {token!r}")
    return round(float(m.group(1)) * _UNITS_NM[m.group(2) or ""])


def clearance_minimums_nm(text):
    """`[(rule name, min nm)]` of every `(constraint clearance ...)` of the rules in `text`, in file
    order, conditional rules included. A constraint with no `(min ...)` counts 0 (KiCad reads the
    missing bound as no minimum), so the caller passes no floor for it. Raises `RulesError`."""
    out = []
    for rule in parse(text):
        if not (isinstance(rule, list) and rule[:1] == ["rule"]):
            continue
        name = rule[1].lstrip('"') if len(rule) > 1 and isinstance(rule[1], str) else ""
        for item in rule[2:]:
            if not (isinstance(item, list) and item[:2] == ["constraint", "clearance"]):
                continue
            mins = [b for b in item[2:] if isinstance(b, list) and b[:1] == ["min"]]
            if not mins:
                out.append((name, 0))
                continue
            if len(mins[0]) != 2:
                raise RulesError(f"rule {name!r}: unreadable (min ...)")
            out.append((name, length_nm(mins[0][1])))
    return out


def smallest_clearance_nm(text):
    """The smallest clearance the rules in `text` set, or None when they set none."""
    values = [nm for _, nm in clearance_minimums_nm(text)]
    return min(values) if values else None
