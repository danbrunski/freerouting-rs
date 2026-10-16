//! The plane owner rules (decision D8 in docs/roadmap/port-completion.md)
//! as tests: ports of `io/specctra/PlanePinClearanceTest` and
//! `io/specctra/SignalLayerPourTest` (Java `cda540f`), with the same
//! inline DSN text.
//!
//! - A plane is exported as its outline; the EDA tool relieves its fill
//!   around foreign pins, so a foreign through-hole pin inside a plane is
//!   not a clearance violation unless the plane is an obstacle.
//! - A pour on a signal layer does not stand in for the plane of a net
//!   that has one on a power layer; it is dropped. On a board without a
//!   power layer it is kept.
//!
//! All four Java tests are ported.

use std::collections::BTreeMap;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::specctra::{BoardReadResult, read_board};

/// Java `DsnTestFixtures.loadBoard(byte[])`: `DsnReader.readBoard`
/// without post-processing, accepting a board without an outline.
fn load_board(dsn: &str) -> RoutingBoard {
    match read_board(dsn.as_bytes()) {
        BoardReadResult::Success { board, .. } => *board,
        BoardReadResult::OutlineMissing {
            board: Some(board), ..
        } => *board,
        _ => panic!("the DSN must load"),
    }
}

// ---------------------------------------------------------------------------
// io.specctra.PlanePinClearanceTest

const PLANE_PIN_DSN: &str = r#"
(pcb test.dsn (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10) (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer Gnd.Cu (type power) (property (index 1)))
    (layer B.Cu (type signal) (property (index 2)))
    (boundary (path pcb 0  0 0  20000 0  20000 -20000  0 -20000  0 0))
    (plane GND (polygon Gnd.Cu 0  0 0  20000 0  20000 -20000  0 -20000))
    (rule (width 250) (clearance 200)))
  (placement (component TP (place TP1 10000 -10000 front 0)))
  (library
    (image TP (pin THT 1 0 0))
    (padstack THT
      (shape (circle F.Cu 1500)) (shape (circle Gnd.Cu 1500)) (shape (circle B.Cu 1500))
      (attach off)))
  (network (net SIG (pins TP1-1)) (net GND)))
"#;

/// The first conduction area of the board (Java's `plane(board)`).
fn plane(board: &RoutingBoard) -> ItemId {
    board
        .items()
        .find(|item| item.as_area().is_some_and(|a| a.is_conduction_area()))
        .map(|item| item.id())
        .expect("the board has a plane")
}

/// The first pin of the board (Java `board.getPins().iterator().next()`).
fn first_pin(board: &RoutingBoard) -> ItemId {
    *board.pins().first().expect("the board has a pin")
}

#[test]
fn foreign_pin_inside_plane_is_not_a_violation() {
    let board = load_board(PLANE_PIN_DSN);
    let plane = plane(&board);
    let pin = first_pin(&board);
    // The pin really lies inside the plane, on its layer.
    let plane_item = board.item(plane).unwrap();
    let pin_item = board.item(pin).unwrap();
    assert_eq!(board.layer_structure.layers[1].name, "Gnd.Cu");
    assert_eq!(plane_item.as_area().unwrap().layer(), 1);
    assert!(pin_item.first_layer() <= 1 && 1 <= pin_item.last_layer());
    assert!(!pin_item.shares_net(plane_item));
    assert!(!plane_item.as_area().unwrap().conduction_is_obstacle());

    assert!(
        board.clearance_violations(plane).is_empty(),
        "plane reports the foreign pin"
    );
    assert!(
        board.clearance_violations(pin).is_empty(),
        "pin reports the plane"
    );
}

#[test]
fn foreign_pin_inside_obstacle_plane_is_still_a_violation() {
    let mut board = load_board(PLANE_PIN_DSN);
    let plane = plane(&board);
    let pin = first_pin(&board);
    board.set_conduction_is_obstacle(plane, true);
    assert!(
        board
            .item(plane)
            .unwrap()
            .as_area()
            .unwrap()
            .conduction_is_obstacle()
    );

    // The plane is stored as several convex tiles; each one near the pin
    // reports it.
    let violations = board.clearance_violations(plane);
    assert!(
        !violations.is_empty(),
        "obstacle plane must report the foreign pin"
    );
    assert!(violations.iter().all(|v| v.second_item == pin));
}

// ---------------------------------------------------------------------------
// io.specctra.SignalLayerPourTest

const BOUNDARY: &str = "(path pcb 0  0 0  20000 0  20000 -20000  0 -20000  0 0)";

fn load(layers: &str, planes: &str, nets: &str) -> RoutingBoard {
    let dsn = format!(
        "(pcb test.dsn (parser (string_quote \") (space_in_quoted_tokens on)) \
         (resolution um 10) (unit um) \
         (structure {layers} (boundary {BOUNDARY}) {planes} \
         (rule (width 250) (clearance 200))) \
         (network {nets}))"
    );
    load_board(&dsn)
}

fn layer(name: &str, kind: &str, index: i32) -> String {
    format!("(layer {name} (type {kind}) (property (index {index}))) ")
}

fn pour(net: &str, layer: &str) -> String {
    format!("(plane {net} (polygon {layer} 0  0 0  20000 0  20000 -20000  0 -20000)) ")
}

/// Each net name mapped to the layer of its conduction area; panics if a
/// net has more than one (Java `conductionAreasByNetAndLayer`).
fn conduction_areas_by_net_and_layer(board: &RoutingBoard) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for item in board.items() {
        let Some(area) = item.as_area().filter(|a| a.is_conduction_area()) else {
            continue;
        };
        let net = board
            .rules
            .nets
            .get(item.net_number(0))
            .unwrap()
            .name
            .clone();
        let layer = board.layer_structure.layers[area.layer() as usize]
            .name
            .clone();
        let previous = result.insert(net.clone(), layer);
        assert_eq!(
            previous, None,
            "net {net} has more than one conduction area"
        );
    }
    result
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[test]
fn signal_layer_pour_is_ignored_when_net_has_power_layer_plane() {
    let board = load(
        &(layer("F.Cu", "signal", 0)
            + &layer("Gnd.Cu", "power", 1)
            + &layer("Pwr.Cu", "power", 2)
            + &layer("B.Cu", "signal", 3)),
        &(pour("GND", "F.Cu")
            + &pour("GND", "B.Cu")
            + &pour("GND", "Gnd.Cu")
            + &pour("VCC", "Pwr.Cu")
            + &pour("SIG", "B.Cu")),
        "(net GND) (net VCC) (net SIG)",
    );
    assert_eq!(
        conduction_areas_by_net_and_layer(&board),
        map(&[("GND", "Gnd.Cu"), ("VCC", "Pwr.Cu"), ("SIG", "B.Cu")]),
        "only GND's signal-layer pours may be dropped; SIG has no power-layer plane"
    );
}

#[test]
fn signal_layer_pour_is_kept_on_two_layer_board() {
    let board = load(
        &(layer("F.Cu", "signal", 0) + &layer("B.Cu", "signal", 1)),
        &pour("GND", "B.Cu"),
        "(net GND)",
    );
    assert_eq!(
        conduction_areas_by_net_and_layer(&board),
        map(&[("GND", "B.Cu")])
    );
    assert!(
        board
            .rules
            .nets
            .get_by_name("GND", 1)
            .unwrap()
            .contains_plane()
    );
}
