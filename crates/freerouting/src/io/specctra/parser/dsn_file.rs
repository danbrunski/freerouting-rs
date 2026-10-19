//! Board adjustments after reading.
//!
//! Port of `io.specctra.parser.DsnFile.adjustPlaneAutorouteSettings`.

use freerouting_geometry::TileShapeOps;

use crate::board::BasicBoard;
use crate::board::structure::FixedState;

/// On boards with only signal layers, marks the nets of conduction areas
/// that cover at least 30% of the board as plane nets and fixes the areas.
/// Returns true if a net changed.
pub(crate) fn adjust_plane_autoroute_settings(board: &mut BasicBoard) -> bool {
    let layers = &board.layer_structure.layers;
    if layers.is_empty() || layers.iter().any(|l| !l.is_signal) {
        return false;
    }
    let mut board_area = 0.0;
    if let Some(outline) = board.outline().and_then(|o| o.as_board_outline()) {
        for shape in outline.shapes() {
            if let Some(pieces) = shape.split_to_convex() {
                board_area += pieces.iter().map(|p| p.area()).sum::<f64>();
            }
        }
    }
    if board_area <= 0.0 {
        return false;
    }
    let layer_count = board.layer_count();
    let mut changed_layers = vec![false; layer_count];
    let mut nothing_changed = true;
    let split = board.java_quirks().split();
    for id in board.conduction_areas() {
        let Some(item) = board.item(id) else {
            continue;
        };
        let Some(area) = item.as_area() else {
            continue;
        };
        let layer = area.layer();
        if layer < 0 || layer as usize >= layer_count {
            continue;
        }
        if !board.layer_structure.layers[layer as usize].is_signal {
            continue;
        }
        // PARITY(quirk, java_quirks.octagon_box_cutout_branch3): the board's split
        // switches (all four); Java's split when they are `true`.
        let Some(pieces) = area.area().split_to_convex_with(None, split) else {
            continue;
        };
        let current_area: f64 = pieces.iter().map(|p| p.area()).sum();
        if current_area < 0.3 * board_area {
            continue;
        }
        let nets: Vec<i32> = item.net_numbers().to_vec();
        let fixed_state = item.fixed_state();
        for net in nets {
            if let Some(net) = board.rules.nets.get_mut(net) {
                net.set_contains_plane(true);
                nothing_changed = false;
            }
        }
        changed_layers[layer as usize] = true;
        if fixed_state < FixedState::UserFixed {
            board.set_fixed_state(id, FixedState::UserFixed);
        }
    }
    for (i, changed) in changed_layers.iter().enumerate() {
        if *changed {
            let message = format!(
                "Layer '{}' contains a power plane / large copper pour covering >=30% of the \
                 board; associated net(s) configured for plane routing.",
                board.layer_structure.layers[i].name
            );
            board.log.push(message);
        }
    }
    !nothing_changed
}

#[cfg(test)]
mod n06_tests {
    use super::*;
    use crate::board::split_switches::test_support::{
        board, border_area, pieces_total, split_fixed,
    };
    use crate::settings::JavaQuirks;
    use freerouting_geometry::IntBox;

    /// The conduction area's share of the board follows the board's split
    /// switches: Java's border-crossing cutout loses half the area, so a plane
    /// that covers 30% under `false` stays a signal net under Java.
    #[test]
    fn plane_share_follows_the_split_switches() {
        let area = border_area();
        let java = pieces_total(&area, JavaQuirks::JAVA);
        let fixed = pieces_total(&area, split_fixed());
        assert!(java < 0.6 * fixed, "{java} {fixed}");
        // An outline whose 30% lies halfway between the two totals.
        let height = ((java + fixed) / 2.0 / 0.3 / 200_000.0) as i32;
        let outline = IntBox::new(-100_000, -100_000, 100_000, height - 100_000);
        for (quirks, plane) in [(JavaQuirks::JAVA, false), (split_fixed(), true)] {
            let mut board = board(outline, quirks);
            board.insert_conduction_area(area.clone(), 0, &[1], 0, false, FixedState::Unfixed);
            assert_eq!(adjust_plane_autoroute_settings(&mut board), plane);
            assert_eq!(board.rules.nets.get(1).unwrap().contains_plane(), plane);
        }
    }
}
