//! Q22.11: the board's minimum hole-to-hole distance, set on the board's
//! rules before routing.
//!
//! KiCad's DRC keeps any two drilled holes at least Board Setup's
//! hole-to-hole distance apart, edge to edge, whatever their nets
//! (`m_HoleToHoleMin`, `.kicad_pro` `min_hole_to_hole`, 0.25 mm by default).
//! KiCad's DSN export does not carry it. `router.board_minimums.hole_to_hole_um`
//! becomes [`BoardRules::hole_to_hole`](crate::rules::BoardRules::hole_to_hole)
//! in board units, which the via placement and the output check read
//! ([`crate::board::hole_spacing`]).
//!
//! Unset or 0 (profile `legacy` with nothing supplied), nothing changes
//! (`tests/hole_to_hole.rs`).

use super::{LogLevel, LogLine};
use crate::board::BasicBoard;
use crate::settings::RouterSettings;
use crate::settings::text::java_double_to_string;

/// The distance of `settings` in micrometres, or `None` when unknown.
pub fn hole_to_hole_um(settings: &RouterSettings) -> Option<f64> {
    settings
        .board_minimums
        .hole_to_hole_um
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Sets the board's hole-to-hole distance from `settings` (module docs);
/// returns the log line (none when unset).
pub fn apply_hole_to_hole(board: &mut BasicBoard, settings: &RouterSettings) -> Vec<LogLine> {
    let Some(um) = hole_to_hole_um(settings) else {
        return Vec::new();
    };
    // Rounded up: a hole at the rounded distance keeps the minimum.
    let units = crate::autoroute::control::um_to_units(board, um)
        .ceil()
        .min(f64::from(i32::MAX)) as i32;
    board.rules.set_hole_to_hole(units);
    vec![LogLine {
        level: LogLevel::Info,
        text: format!(
            "Board minimum hole to hole {} um ({units} board units): no routed via hole comes closer to another drill.",
            java_double_to_string(um)
        ),
    }]
}
