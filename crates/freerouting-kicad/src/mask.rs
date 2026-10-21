//! Q18.7: the pads' solder-mask expansions, for
//! [`crate::read::Extras::solder_mask`] (the engine's
//! `router.solder_mask_expansions`, `freerouting::job::solder_mask`).
//!
//! `PAD::GetSolderMaskExpansion` (`pcbnew/pad.cpp`, tag `10.0.0`): the pad's
//! `solder_mask_margin`, else its footprint's, else the board's
//! `(setup (pad_to_mask_clearance ...))` (0 when absent); a negative margin
//! opens no wider than the pad, so it raises nothing. A pad has an aperture
//! on a side when its layers name that side's mask (`F.Mask`, `B.Mask`,
//! `*.Mask`), as the file has them (a back footprint's pads already say
//! `B.Mask`). The DSN carries none of this.
//!
//! Q22.12: KiCad's DRC (`solder_mask_bridge`) also keeps copper of another
//! net `solder_mask_to_copper_clearance` (`m_SolderMaskToCopperClearance`,
//! [`crate::project::Project::solder_mask_to_copper`], default 0) from an
//! aperture. [`expansion`] folds it in: a side with an aperture expands by
//! margin + clearance (at least 0), so the router (`job::solder_mask`) and
//! Q20's output check (`drc::board_rules`, which reads the same
//! `router.solder_mask_expansions`) both work on the aperture grown by the
//! clearance, with no setting of its own. The plugin folds it the same way
//! (`kicad_board.mask_expansions`). With 0 nothing changes.

use crate::sexpr::NodeRef;
use crate::units::nm;

/// One pad's mask facts as the file has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadMask {
    pub front: bool,
    pub back: bool,
    /// The pad's own `solder_mask_margin`, nm.
    pub margin: Option<i32>,
}

/// A footprint's pads (file order) and its own margin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FootprintMasks {
    pub margin: Option<i32>,
    pub pads: Vec<PadMask>,
}

/// A pad whose aperture is wider than its copper, per outer side (nm, 0 for
/// no aperture).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadMaskExpansion {
    pub component: String,
    pub pin: String,
    pub front_nm: i32,
    pub back_nm: i32,
}

fn margin_of(node: NodeRef<'_>) -> Option<i32> {
    node.child("solder_mask_margin")
        .and_then(|m| m.atom(1))
        .and_then(nm)
        .and_then(|v| i32::try_from(v).ok())
}

/// The board's `pad_to_mask_clearance`, nm (0 when absent).
pub fn board_expansion(board: NodeRef<'_>) -> i32 {
    board.child("setup").and_then(margin_of_setup).unwrap_or(0)
}

fn margin_of_setup(setup: NodeRef<'_>) -> Option<i32> {
    setup
        .child("pad_to_mask_clearance")
        .and_then(|m| m.atom(1))
        .and_then(nm)
        .and_then(|v| i32::try_from(v).ok())
}

/// The mask facts of every footprint of `board`, in the order of
/// [`crate::footprints::read_footprints`].
pub fn read_masks(board: NodeRef<'_>) -> Vec<FootprintMasks> {
    board
        .items()
        .filter(|c| c.is_list() && matches!(c.head(), Some("footprint") | Some("module")))
        .map(|fp| FootprintMasks {
            margin: margin_of(fp),
            pads: fp
                .items()
                .filter(|c| c.is_list() && c.head() == Some("pad"))
                .map(|pad| {
                    let mut m = PadMask {
                        margin: margin_of(pad),
                        ..PadMask::default()
                    };
                    if let Some(layers) = pad.child("layers") {
                        for t in layers.items().skip(1) {
                            match t.text().trim_matches('"') {
                                "F.Mask" => m.front = true,
                                "B.Mask" => m.back = true,
                                "*.Mask" | "F&B.Mask" => (m.front, m.back) = (true, true),
                                _ => {}
                            }
                        }
                    }
                    m
                })
                .collect(),
        })
        .collect()
}

/// The expansion per side of pad `pad` of `fp` (nm, 0 for no aperture or a
/// margin at or below 0), grown by `clearance_nm` (Q22.12: the board's
/// `solder_mask_to_copper_clearance`; module docs).
pub fn expansion(fp: &FootprintMasks, pad: usize, board_nm: i32, clearance_nm: i32) -> (i32, i32) {
    let Some(p) = fp.pads.get(pad) else {
        return (0, 0);
    };
    let margin = p.margin.or(fp.margin).unwrap_or(board_nm);
    let e = margin.saturating_add(clearance_nm.max(0)).max(0);
    (if p.front { e } else { 0 }, if p.back { e } else { 0 })
}
