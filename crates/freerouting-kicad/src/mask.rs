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
    /// Q22.16: `(attr ... allow_soldermask_bridges)`; KiCad's DRC ignores the
    /// footprint's mask apertures ([`crate::footprint_masks`]).
    pub allow_bridges: bool,
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

/// K13.1: the entries of `list` with a side wider than `floor_nm` (the
/// smallest class clearance), each side judged alone: a side at or under the
/// floor becomes 0, and an entry with no side left is dropped. The plugin's
/// `kicad_board.mask_expansions` filters the same way. A side within the
/// floor needs no entry: `job::solder_mask` raises the pad's clearances on
/// that side to max(clearance, expansion), and every class clearance is at
/// least the floor. The one exception is a `.kicad_dru` class-pair rule
/// below the floor (K07.5 applies it as given); neither input lowers the
/// floor for it.
pub fn keep_above_floor(list: Vec<PadMaskExpansion>, floor_nm: i32) -> Vec<PadMaskExpansion> {
    let side = |e: i32| if e > floor_nm { e } else { 0 };
    list.into_iter()
        .map(|e| PadMaskExpansion {
            front_nm: side(e.front_nm),
            back_nm: side(e.back_nm),
            ..e
        })
        .filter(|e| e.front_nm > 0 || e.back_nm > 0)
        .collect()
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
            allow_bridges: fp.child("attr").is_some_and(|a| {
                a.items()
                    .skip(1)
                    .any(|t| t.text() == "allow_soldermask_bridges")
            }),
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
