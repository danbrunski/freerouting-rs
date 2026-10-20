//! Q22.10: the board's minimum clearance as a floor under every copper
//! clearance, before routing.
//!
//! KiCad's DRC keeps copper of different nets at least Board Setup's minimum
//! clearance apart (`m_MinClearance`, `.kicad_pro` `min_clearance`; its
//! report says "board minimum clearance"), whatever the net classes say:
//! pocketsdr-v3-0's Default class is 0.09 mm and its minimum 0.127 mm, and
//! routing at the class value gave 446 KiCad `clearance` items in Q18's
//! grades. KiCad's DSN export carries only the class values.
//!
//! `router.board_minimums.clearance_um` raises every entry of the clearance
//! matrix between two classes other than `null` (on every layer) that is
//! below it to it. The classes of other KiCad rules keep their values:
//! [`BOARD_EDGE_CLEARANCE_CLASS_NAME`] (copper to edge) and
//! [`HOLE_EDGE_CLEARANCE_CLASS_NAME`] (copper to hole) are made after this
//! step by the overrides and set from their own settings. The board's items
//! are reinserted into the search trees when anything changed, as the hole
//! override does, because the trees hold shapes grown by the clearances.
//!
//! Unset or 0 (profile `legacy` with nothing supplied), nothing changes
//! (`tests/clearance_floor.rs`).

use super::overrides::{BOARD_EDGE_CLEARANCE_CLASS_NAME, HOLE_EDGE_CLEARANCE_CLASS_NAME};
use super::{LogLevel, LogLine};
use crate::board::BasicBoard;
use crate::settings::RouterSettings;
use crate::settings::text::java_double_to_string;

/// The floor of `settings` in micrometres, or `None` when unknown.
pub fn clearance_floor_um(settings: &RouterSettings) -> Option<f64> {
    settings
        .board_minimums
        .clearance_um
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Raises every clearance of `board` below the floor of `settings` to it
/// (module docs); returns the log lines (none when nothing changed).
pub fn apply_clearance_floor(board: &mut BasicBoard, settings: &RouterSettings) -> Vec<LogLine> {
    let Some(um) = clearance_floor_um(settings) else {
        return Vec::new();
    };
    let units = super::overrides::um_to_board_units(board, um);
    let matrix = &mut board.rules.clearance_matrix;
    let skip = |name: Option<&str>| {
        name.is_some_and(|n| {
            n.eq_ignore_ascii_case(BOARD_EDGE_CLEARANCE_CLASS_NAME)
                || n.eq_ignore_ascii_case(HOLE_EDGE_CLEARANCE_CLASS_NAME)
        })
    };
    let classes: Vec<usize> = (1..matrix.class_count())
        .filter(|&c| !skip(matrix.get_name(c as i32)))
        .collect();
    let mut raised = 0usize;
    for &i in &classes {
        for &j in &classes {
            for layer in 0..matrix.layer_count() {
                if matrix.get_value(i as i32, j as i32, layer as i32, false) < units {
                    matrix.set_value_on_layer(i, j, layer, units);
                    raised += 1;
                }
            }
        }
    }
    if raised == 0 {
        return Vec::new();
    }
    board.reinsert_tree_items();
    vec![LogLine {
        level: LogLevel::Info,
        text: format!(
            "Board minimum clearance {} um ({units} board units): {raised} class clearance(s) raised to it.",
            java_double_to_string(um)
        ),
    }]
}
