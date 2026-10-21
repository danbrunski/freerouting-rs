"""Q22.16: a footprint's own F.Mask / B.Mask shapes as keepouts (decided
2026-10-20: routing never breaks the board's manufacturing rules).

KiCad's DRC (`solder_mask_bridge`) flags copper of two nets in one mask
aperture. A footprint can open the mask with a shape of its own instead of its
pads' mask layers: an edge connector's polygon over all its fingers, or one
polygon per pad. `edge_graphics.mask_polygons` makes each such shape keepouts
on its side's copper, grown by the solder-mask-to-copper clearance like the
board's own mask shapes (Q18.7, Q22.12), named `keepout_name`:
`FOOTPRINT_MASK_KEEPOUT_NAME`. The router reads the name
(`crates/freerouting/src/io/specctra/parser/pad_keepouts.rs`): over pads of one
net the keepout lets that net through, over pads of several nets (`pad_trim`
renames it `solder_mask_shared` and cuts those pads back to their part outside
it) it keeps every net out. Q22.18: with `(clearance_class null)`, no clearance
on top of the grown aperture. A footprint KiCad lets bridge
(`allow_soldermask_bridges`, a solder jumper) has none: KiCad's DRC ignores its
apertures. The `.kicad_pcb` reader does the same
(`crates/freerouting-kicad/src/footprint_masks.rs`).
"""

FOOTPRINT_MASK_KEEPOUT_NAME = "solder_mask_pads"


def keepout_name(footprint):
    """The keepout name of a mask shape of the footprint `footprint`
    (`kicad_board.footprint_mask_facts`), or None for no keepout."""
    if footprint.get("allow_bridges"):
        return None
    return FOOTPRINT_MASK_KEEPOUT_NAME
