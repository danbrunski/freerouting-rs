"""K15.49: the board snapshot is the same file on every run.

KiCad 10.0.7 writes a teardrop zone without a `(uuid ...)`, so `LoadBoard`
gives each one a new random KIID, and `SaveBoard` writes the zones sorted by
`BOARD_ITEM::ptr_cmp` (type, layer set, then uuid). The teardrops therefore
land at another place among the zones of their layer set on every load, and
the router, which reads zones in file order, routed the same board
differently from run to run (K15.43, K15.47: `wisweep-driver-board`,
`complex_hierarchy`). `snapshot.take` puts the zones without a uuid in a fixed
order after saving. Pure Python: SaveBoard is a fake that writes the zones in
KiCad's order with fresh random uuids for the teardrops."""

import random
import uuid

import pytest
import snapshot

HEAD = '(kicad_pcb\n\t(version 20260206)\n\t(generator "pcbnew")\n\t(gr_text "a (zone) \\" (uuid x)"\n\t\t(at 1 2)\n\t)\n'
TAIL = '\t(embedded_fonts no)\n)\n'

# (net, layer declaration, priority, uuid or None for a teardrop)
ZONES = [
    ("VM", '(layer "F.Cu")', 6, "246c1667-1a35-4c53-9ee6-b84e47060288"),
    ("/OUT1B", '(layer "F.Cu")', 30004, None),
    ("CPO", '(layer "F.Cu")', 30000, None),
    ("VM", '(layer "F.Cu")', 4, "776fc9ac-6a7f-45b7-8ee5-7434ef401574"),
    ("VCP", '(layer "F.Cu")', 30002, None),
    ("/OUT2A", '(layer "F.Cu")', 30006, None),
    ("GND", '(layers "F.Cu" "B.Cu")', 2, "8eb345ae-ebd2-4aae-a076-c0648b7a1070"),
    ("+3V3", '(layer "B.Cu")', 3, "17787945-32d9-4af2-91c5-8b5853834bf2"),
    ("CPI", '(layer "B.Cu")', 30001, None),
    ("CPI", '(layer "B.Cu")', 30001, None),  # two equal teardrops are interchangeable
]
LAYER_RANK = {'(layer "F.Cu")': 0, '(layers "F.Cu" "B.Cu")': 1, '(layer "B.Cu")': 2}


def zone_text(net, layer, priority, uid):
    lines = ["\t(zone", f'\t\t(net "{net}")', f"\t\t{layer}"]
    if uid:
        lines.append(f'\t\t(uuid "{uid}")')
    lines += [f"\t\t(priority {priority})"]
    if uid is None:
        lines += ["\t\t(attr", "\t\t\t(teardrop", "\t\t\t\t(type padvia)", "\t\t\t)", "\t\t)"]
    lines += ["\t\t(polygon", "\t\t\t(pts", "\t\t\t\t(xy 0 0) (xy 1 0) (xy 1 1)", "\t\t\t)", "\t\t)", "\t)"]
    return "\n".join(lines) + "\n"


def kicad_save_text(seed):
    """The board as KiCad's saver writes it after a load that gave the
    teardrops the random uuids of `seed`: zones sorted by layer set, then
    uuid."""
    rng = random.Random(seed)
    keyed = []
    for net, layer, priority, uid in ZONES:
        key = uid or str(uuid.UUID(int=rng.getrandbits(128), version=4))
        keyed.append(((LAYER_RANK[layer], key), zone_text(net, layer, priority, uid)))
    return HEAD + "".join(text for _, text in sorted(keyed)) + TAIL


class SavingPcbnew:
    """SaveBoard writes `text`; no live project (the disk copy is used)."""

    def __init__(self, text):
        self.text = text

        class Manager:
            def SaveProjectCopy(self, path, project):
                raise RuntimeError("no live project in this test")

        self.GetSettingsManager = Manager

    def SaveBoard(self, path, board, *rest):
        with open(path, "w", encoding="utf-8", newline="") as f:
            f.write(self.text)
        return True


class Board:
    def GetProject(self):
        return object()


def take(tmp_path, text, name="run"):
    work = tmp_path / name
    ok, lines = snapshot.take(Board(), SavingPcbnew(text), work, "b", tmp_path)
    assert ok
    return (work / "b.kicad_pcb").read_bytes(), lines


def test_the_saved_order_of_the_teardrops_changes_with_their_random_uuids():
    # The premise: without a fixed order the saver's text differs between loads.
    assert len({kicad_save_text(seed) for seed in range(8)}) > 1


def test_every_load_of_the_board_gives_one_snapshot(tmp_path):
    files = {take(tmp_path, kicad_save_text(seed), f"run{seed}")[0] for seed in range(8)}
    assert len(files) == 1


def test_the_zones_with_a_uuid_keep_their_order_and_the_teardrops_follow_them_by_priority(tmp_path):
    data, lines = take(tmp_path, kicad_save_text(3))
    text = data.decode("utf-8")
    blocks = [b for b in text.split("\t(zone\n")[1:]]
    order = [(b.split('(net "')[1].split('"')[0], "uuid" in b) for b in blocks]
    assert order == [
        ("VM", True),  # F.Cu: the uuid zones in KiCad's (uuid) order
        ("VM", True),
        ("CPO", False),  # then its teardrops by priority
        ("VCP", False),
        ("/OUT1B", False),
        ("/OUT2A", False),
        ("GND", True),  # the F.Cu + B.Cu set
        ("+3V3", True),  # B.Cu
        ("CPI", False),
        ("CPI", False),
    ]
    assert text.startswith(HEAD) and text.endswith(TAIL)
    assert sorted(text.split("\t(zone\n")) == sorted(kicad_save_text(3).split("\t(zone\n"))  # nothing lost
    assert "snapshot: 6 zone(s) without a uuid (teardrops) put in a fixed order" in lines


def test_a_board_without_teardrops_is_written_byte_for_byte_as_kicad_saved_it(tmp_path):
    text = HEAD + "".join(zone_text(n, la, p, u) for n, la, p, u in ZONES if u) + TAIL
    data, lines = take(tmp_path, text)
    assert data == text.encode("utf-8")
    assert not any("fixed order" in line for line in lines)
    assert snapshot.order_zones(text) == (text, 0)


@pytest.mark.parametrize("text", ["(kicad_pcb\n\t(zone\n", '(kicad_pcb\n\t(zone "x)\n)\n', ""])
def test_a_file_the_scan_cannot_read_is_left_as_saved(text):
    assert snapshot.order_zones(text) == (text, 0)

