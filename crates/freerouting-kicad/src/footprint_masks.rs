//! Q22.16: a footprint's own F.Mask and B.Mask shapes as keepouts on the
//! copper of their side (decided 2026-10-20: routing never breaks the
//! board's manufacturing rules).
//!
//! KiCad's DRC (`solder_mask_bridge`) flags copper of two nets in one mask
//! aperture. A footprint can open the mask with a shape of its own instead of
//! its pads' mask layers: an edge connector's `fp_poly` over all its fingers
//! (fc-rom's J1, 72 nets), or one polygon per pad (a sensor's land pattern).
//! Q18.7 left these shapes to the pads, so tracks of other nets ran through
//! the aperture. [`footprint_mask_keepouts`] makes each one keepouts on its
//! side's copper (F.Mask on the top copper layer, B.Mask on the bottom), the
//! shapes of [`crate::edge_keepouts::mask_polygons`]: grown by the board's
//! `solder_mask_to_copper_clearance` (Q22.12) like the board's own mask
//! shapes, and named [`FOOTPRINT_MASK_KEEPOUT_NAME`]. The router reads the
//! name (`freerouting::io::specctra::parser::pad_keepouts`): over pads of
//! one net the keepout lets that net through, over pads of several nets
//! ([`crate::pad_trim`] renames it [`SHARED_MASK_KEEPOUT_NAME`] and cuts
//! those pads back to their part outside it) it keeps every net out. Q22.18:
//! with `(clearance_class null)`, no clearance on top of the grown aperture.
//!
//! A footprint KiCad lets bridge (`(attr ... allow_soldermask_bridges)`, a
//! solder jumper) has no keepout: KiCad's DRC ignores its apertures. The
//! KiCad plugin does the same to KiCad's export
//! (`kicad-plugin/freerouting_rs/footprint_masks.py`). `docs/DIVERGENCES.md`
//! lists the keepouts.

use freerouting_dsn::ast::Keepout;

use crate::edge_keepouts::{F, mask_keepout, mask_polygons};
use crate::footprints::Footprint;
use crate::mask::FootprintMasks;

/// The name of a footprint's mask-aperture keepouts: the router's
/// `PAD_KEEPOUT_NAME`.
pub const FOOTPRINT_MASK_KEEPOUT_NAME: &str = "solder_mask_pads";

/// The name of a footprint's mask-aperture keepout over pads of two or more
/// nets ([`crate::pad_trim`]): the router's `SHARED_PAD_KEEPOUT_NAME`.
pub const SHARED_MASK_KEEPOUT_NAME: &str = "solder_mask_shared";

/// The keepout polygons (nm, KiCad's frame) of every footprint's F.Mask /
/// B.Mask shapes, each with its side (`true` for B.Mask), in footprint then
/// file order, from the footprints as filed (`footprints`, before the flip)
/// and their mask facts (`masks`, same order). `clearance_nm` grows them
/// (Q22.12).
pub fn footprint_mask_polygons(
    footprints: &[Footprint],
    masks: &[FootprintMasks],
    max_error: i32,
    clearance_nm: i32,
) -> Vec<(bool, Vec<F>)> {
    footprints
        .iter()
        .enumerate()
        .filter(|(i, _)| !masks.get(*i).is_some_and(|m| m.allow_bridges))
        .flat_map(|(_, fp)| mask_polygons(&fp.graphics, max_error, clearance_nm))
        .collect()
}

/// [`footprint_mask_polygons`] as DSN keepouts on `front` (F.Mask) or `back`
/// (B.Mask).
pub fn footprint_mask_keepouts(
    polygons: &[(bool, Vec<F>)],
    front: &str,
    back: &str,
) -> Vec<Keepout> {
    polygons
        .iter()
        .map(|(is_back, poly)| {
            mask_keepout(
                FOOTPRINT_MASK_KEEPOUT_NAME,
                if *is_back { back } else { front },
                poly,
            )
        })
        .collect()
}
