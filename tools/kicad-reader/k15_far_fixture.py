"""K15.1: generate the `far-nm` board, nets far from the bad C65.

    python3 -I tools/kicad-reader/k15_far_fixture.py [SRC] [DST]

Edits text only; needs no pcbnew. Run from the repository root. SRC defaults
to `docs/development/pcu-rev2/c65-nm-position.kicad_pcb` (bug 1's repro: C65
at y = 105.242039 mm, whose F.Fab circle places to an unbounded simplex), DST
to `crates/freerouting/tests/data/k15/far-nm.kicad_pcb`.

The board is SRC plus four copies of its C66 footprint (a 1206 capacitor),
C901 to C904, at (150, 60), (170, 62), (150, 130) and (172, 128) mm, 75 to
95 mm from C65, on the new nets /FAR_A and /FAR_B (C901, C902) and /FAR_C
and /FAR_D (C903, C904): 6 nets in all. Before K15.1 the board box was empty
and none of the 6 routed, though no new net comes near C65.

The copies' UUIDs are derived from the reference and the original UUID
(`uuid5`), so a rerun writes the same file. The investigation's
`make_far.py` (orchestrator run `pcu1`) used random UUIDs; otherwise the
output is the same.
"""
import re
import sys
import uuid

SRC = "docs/development/pcu-rev2/c65-nm-position.kicad_pcb"
DST = "crates/freerouting/tests/data/k15/far-nm.kicad_pcb"
C66_AT = "(at 75.685865 115.179824 90)"
COPIES = [
    ("C901", "150 60", "/FAR_A", "/FAR_B"),
    ("C902", "170 62", "/FAR_A", "/FAR_B"),
    ("C903", "150 130", "/FAR_C", "/FAR_D"),
    ("C904", "172 128", "/FAR_C", "/FAR_D"),
]


def main():
    src = sys.argv[1] if len(sys.argv) > 1 else SRC
    dst = sys.argv[2] if len(sys.argv) > 2 else DST
    with open(src, encoding="utf-8") as f:
        text = f.read()
    start = text.find('\t(footprint "Capacitor_SMD:C_1206_3216Metric"')
    end = text.find("\n\t(gr_rect", start)
    if start < 0 or end < 0 or C66_AT not in text[start:end]:
        sys.exit(f"{src}: no C66 footprint followed by a gr_rect")
    block = text[start:end]
    copies = []
    for ref, at, net_a, net_b in COPIES:
        copy = block.replace(C66_AT, f"(at {at})", 1).replace('"C66"', f'"{ref}"')
        copy = copy.replace('(net "/V_PD_IN")', f'(net "{net_a}")')
        copy = copy.replace('(net "BUS_GND")', f'(net "{net_b}")')
        copy = re.sub(
            r'\(uuid "([^"]+)"\)',
            lambda m: f'(uuid "{uuid.uuid5(uuid.NAMESPACE_URL, ref + m.group(1))}")',
            copy,
        )
        copies.append(copy)
    out = text[:end] + "\n" + "\n".join(copies) + text[end:]
    with open(dst, "w", encoding="utf-8", newline="") as f:
        f.write(out)
    print(f"wrote {dst}: {len(COPIES)} footprints added")


if __name__ == "__main__":
    main()
