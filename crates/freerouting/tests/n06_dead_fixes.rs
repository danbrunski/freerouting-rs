//! N06.2: values and state corrected outright in both profiles.
mod board_probe;

use freerouting::board::AutorouteChange;
use freerouting::board::structure::FixedState;
use freerouting::geometry::{IntPoint, Point, Polyline};
use freerouting::optimize::{PullTightParams, Tightener};
use std::collections::BTreeSet;

#[test]
fn keep_point_smoothing_restores_contact_pins() {
    let mut board = board_probe::load(&board_probe::fixture("scoring-perfect-two-pin"));
    let net = board.items().find(|i| i.is_pin()).unwrap().net_numbers()[0];
    let saved_pins: BTreeSet<_> = board.pins().into_iter().collect();
    let p = |x, y| Point::from(IntPoint::new(x, y));
    let id = board
        .insert_trace_without_cleaning(
            Polyline::from_points(&[p(1000, 1000), p(2000, 2000)]),
            0,
            100,
            &[net],
            0,
            FixedState::Unfixed,
        )
        .unwrap();
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&[p(1000, 1000), p(3000, 1000)]),
            0,
            100,
            &[net],
            0,
            FixedState::Unfixed,
        )
        .unwrap();
    let mut params = PullTightParams::new(500);
    params.keep_point = Some((p(1500, 1500), 0));
    let mut tightener = Tightener::new(&board, &params);
    tightener.set_contact_pins(Some(saved_pins.clone()));
    board.set_autoroute_change_log(true);
    assert!(board.smoothen_end_corners_at_trace(id, &mut tightener));
    assert_eq!(tightener.take_contact_pins(), Some(saved_pins));
    // The kept interior point really caused a split, exercising the early return.
    let contacts: Vec<_> = board
        .traces()
        .into_iter()
        .filter(|id| {
            let trace = board.item(*id).unwrap().as_trace().unwrap();
            trace.first_corner() == p(1500, 1500) || trace.last_corner() == p(1500, 1500)
        })
        .collect();
    assert_eq!(contacts.len(), 2);
    let removed = board
        .take_autoroute_changes()
        .into_iter()
        .filter(|change| matches!(change, AutorouteChange::Removed(removed) if *removed == id))
        .count();
    assert_eq!(removed, 1);
}
