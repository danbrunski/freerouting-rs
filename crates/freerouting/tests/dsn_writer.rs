//! C06: the DSN writer, section by section. Every test writes each loaded fixture, reads the
//! text back and compares one section of the two boards (`tests/dsn_support/mod.rs`).

mod dsn_support;

use dsn_support::*;

#[test]
fn frame_survives_the_round_trip() {
    for_every_board("frame", |_, a, b| diff_lines("frame", &frame(a), &frame(b)));
}

#[test]
fn clearance_matrix_survives_the_round_trip() {
    for_every_board("clearance", |_, a, b| {
        diff_lines("clearance", &clearance(a), &clearance(b))
    });
}

#[test]
fn areas_survive_the_round_trip() {
    for_every_board("areas", |_, a, b| diff_lines("areas", &areas(a), &areas(b)));
}

#[test]
fn placement_survives_the_round_trip() {
    for_every_board("placement", |_, a, b| {
        diff("placement", &placement(a), &placement(b))
    });
}

#[test]
fn library_survives_the_round_trip() {
    for_every_board("library", |_, a, b| {
        diff_lines("library", &library(a), &library(b))
    });
}

#[test]
fn network_survives_the_round_trip() {
    for_every_board("network", |_, a, b| {
        diff_lines("network", &network(a), &network(b))
    });
}

#[test]
fn wiring_survives_the_round_trip() {
    for_every_board("wiring", |_, a, b| {
        let (ta, va) = wiring(a);
        let (tb, vb) = wiring(b);
        let mut out = diff("traces", &ta, &tb);
        out.extend(diff("vias", &va, &vb));
        out
    });
}

// ---------------------------------------------------------------------------
// C06.6: the routed boards

use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::dsn::ast::{KeepoutKind, Rule, WireType, WiringItem};
use freerouting::geometry::{Area, IntBox, Shape};
use freerouting::io::read_session;
use freerouting::io::specctra::dsn_writer::pcb_from_board;

fn load(name: &str) -> BasicBoard {
    loaded_boards()
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("fixture {name} not found or not loaded"))
        .1
}

/// The route reference of a fixture (`fixtures/reference/route/<stem>.ses`), if Java made one.
fn route_reference(name: &str) -> Option<String> {
    let stem = std::path::Path::new(name).file_stem()?.to_str()?;
    std::fs::read_to_string(
        fixtures_dir()
            .join("reference/route")
            .join(format!("{stem}.ses")),
    )
    .ok()
}

#[test]
fn wiring_survives_with_the_route_reference_imported() {
    let mut routed = 0;
    let mut failures = Vec::new();
    for (name, mut board) in loaded_routing_boards() {
        let Some(session) = route_reference(&name) else {
            continue;
        };
        if read_session(&mut board, &session).is_err() {
            continue;
        }
        // The reader normalizes the traces when it has read the wiring (Java's
        // `normalizeAllTraces`), which merges the overlapping wires of a design that has
        // wiring and a session. The board is normalized the same way first.
        board.normalize_all_traces();
        // The writer writes every trace that has a net and every via, normalized or not.
        let pcb = pcb_from_board(&board, &name);
        let written = &pcb.wiring.as_ref().unwrap().items;
        let wires = written
            .iter()
            .filter(|i| matches!(i, WiringItem::Wire(w) if w.net.is_some() && matches!(w.shapes.first(), Some(freerouting::dsn::ast::Shape::PolylinePath { .. }))))
            .count();
        let written_vias = written
            .iter()
            .filter(|i| matches!(i, WiringItem::Via(_)))
            .count();
        let netted = board
            .traces()
            .iter()
            .filter(|id| board.item(**id).is_some_and(|i| i.net_count() > 0))
            .count();
        assert_eq!(
            (wires, written_vias),
            (netted, board.vias().len()),
            "{name}"
        );
        let (traces, vias) = wiring(&board);
        let again = round_trip(&board, &name);
        let (traces_again, vias_again) = wiring(&again);
        // A session can state a via that the design has already: the board has both, and the
        // reader skips the second ("duplicate via skipped", Java's `viaExists`).
        let mut keys: Vec<String> = vias
            .iter()
            .map(|v| v.split(" fixed=").next().unwrap().to_string())
            .collect();
        keys.dedup();
        let keys_again: Vec<String> = vias_again
            .iter()
            .map(|v| v.split(" fixed=").next().unwrap().to_string())
            .collect();
        failures.extend(
            diff_lines("traces", &traces, &traces_again)
                .into_iter()
                .map(|f| format!("{name}: {f}")),
        );
        failures.extend(
            diff_lines("vias", &keys, &keys_again)
                .into_iter()
                .map(|f| format!("{name}: {f}")),
        );
        routed += usize::from(!traces.is_empty());
    }
    assert!(routed >= 50, "only {routed} routed fixtures");
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// The differences from Java's writer (docs/DIVERGENCES.md), each pinned.

fn area_of(shape: IntBox) -> Area {
    Area::from(Shape::from(shape))
}

/// Java writes `(clearance v (type name))`, which its reader skips; Rust writes the pairs.
#[test]
fn named_clearance_classes_are_written_as_pairs() {
    let pcb = pcb_from_board(&load("Issue015-StackOverflow.dsn"), "x");
    let rules = pcb.structure.unwrap().rules;
    let pairs = rules
        .iter()
        .filter(|r| matches!(r, Rule::Clearance { types, .. } if types.len() == 2))
        .count();
    assert!(pairs >= 1, "{pairs} pair rules");
    assert!(
        rules.iter().all(|r| match r {
            Rule::Clearance { types, .. } => types.len() != 1 || types[0] == "smd_to_turn_gap",
            _ => true,
        }),
        "a single named type is a rule Java's reader skips"
    );
}

/// A layer's rule carries the layer's own trace width (Java repeats layer 0's).
#[test]
fn layer_rules_carry_the_width_of_their_layer() {
    let mut board = load("Issue413-test.dsn");
    board.rules.set_default_trace_half_width(1, 777);
    let pcb = pcb_from_board(&board, "x");
    let layer = &pcb.structure.as_ref().unwrap().layers[1];
    let scale = board.communication.coordinate_transform.scale_factor();
    assert!(
        layer.rules.contains(&Rule::Width(2.0 * 777.0 / scale)),
        "{:?}",
        layer.rules
    );
    let again = round_trip(&board, "x");
    assert_eq!(clearance(&again), clearance(&board));
    assert_eq!(
        again
            .rules
            .net_classes
            .get(again.rules.default_net_class_id().unwrap())
            .trace_half_width(1),
        777
    );
}

/// Keepouts of all three kinds, with classes the reader would not infer, a fixed conduction
/// area and a shove-fixed trace survive; Java writes a place keepout as `keepout`.
#[test]
fn keepout_kinds_classes_and_fixed_states_are_written() {
    let mut board = load("Issue413-test.dsn");
    let b = || area_of(IntBox::new(0, 0, 2000, 2000));
    let default_area = board.rules.default_net_class_id().map(|id| {
        board
            .rules
            .net_classes
            .get(id)
            .default_item_clearance_classes
            .get(freerouting::rules::ItemClass::Area)
    });
    // Class 0 ("null") and class 1 ("default") are not what the reader infers for a keepout.
    let null_class: i32 = 0;
    assert!(default_area != Some(null_class));
    board.insert_obstacle(b(), 0, null_class, FixedState::SystemFixed);
    board.insert_via_obstacle(b(), 1, 1, FixedState::SystemFixed);
    board.insert_component_obstacle(b(), 0, 1, FixedState::SystemFixed);
    let net = board.rules.nets.iter().next().unwrap().net_number;
    let area = board.insert_conduction_area(b(), 1, &[net], 1, false, FixedState::UserFixed);
    let _: ItemId = area;
    if let Some(trace) = board.traces().first().copied() {
        board.set_fixed_state(trace, FixedState::ShoveFixed);
    }
    let pcb = pcb_from_board(&board, "x");
    let kinds: Vec<_> = pcb
        .structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .map(|k| k.kind)
        .collect();
    for kind in [
        KeepoutKind::Keepout,
        KeepoutKind::ViaKeepout,
        KeepoutKind::PlaceKeepout,
    ] {
        assert!(kinds.contains(&kind), "{kind:?} missing");
    }
    let wiring_items = &pcb.wiring.as_ref().unwrap().items;
    assert!(wiring_items.iter().any(|i| matches!(i, WiringItem::Wire(w) if w.wire_type == Some(WireType::Protect) && w.shapes.iter().all(|s| !matches!(s, freerouting::dsn::ast::Shape::PolylinePath { .. })))));
    let again = round_trip(&board, "x");
    assert_eq!(areas(&again), areas(&board));
    assert_eq!(wiring(&again), wiring(&board));
}

/// The reader enlarges the `pcb` box by 1000, so the writer shrinks it: a board does not grow
/// with every write (Java's does).
#[test]
fn the_bounding_box_does_not_grow() {
    let board = load("Issue413-test.dsn");
    let again = round_trip(&round_trip(&board, "x"), "x");
    assert_eq!(again.bounding_box(), board.bounding_box());
}

/// Items are written oldest first, so that the reader (which inserts in file order) rebuilds
/// the item list in the order the board had; `BasicBoard::items` lists the newest first.
#[test]
fn items_are_written_oldest_first() {
    let board = load("Issue413-test.dsn");
    let pcb = pcb_from_board(&board, "x");
    let written = pcb.structure.unwrap().keepouts;
    let listed: Vec<_> = board
        .items()
        .filter_map(|i| i.as_area())
        .filter(|a| !a.is_conduction_area() && a.name.is_none())
        .collect();
    assert!(written.len() >= 5 && written.len() == listed.len());
    for (w, a) in written.iter().zip(listed.iter().rev()) {
        let Shape::Polyline(_) = Shape::from(IntBox::new(0, 0, 1, 1)) else {
            unreachable!()
        };
        assert_eq!(
            w.area.shape.layer().name(),
            board.layer_structure.layers[a.layer() as usize].name
        );
    }
    assert_eq!(areas(&round_trip(&board, "x")), areas(&board));
}

/// The net classes that make the reader create their clearance class carry
/// `(rule (clearance v))` and a `use_via`, and no via rule is written; the structure lists
/// the via padstacks without the ones the classes append.
#[test]
fn net_classes_are_written_the_way_the_reader_makes_them() {
    let board = load("Issue015-StackOverflow.dsn");
    let pcb = pcb_from_board(&board, "x");
    let network = pcb.network.unwrap();
    assert!(network.via_rules.is_empty());
    let with_rule = network
        .classes
        .iter()
        .filter(|c| {
            c.rules
                .iter()
                .any(|r| matches!(r, Rule::Clearance { types, .. } if types.is_empty()))
        })
        .count();
    assert!(with_rule >= 7, "{with_rule} classes with a clearance rule");
    assert!(
        network
            .classes
            .iter()
            .all(|c| c.circuit.as_ref().is_some_and(|c| !c.use_via.is_empty()))
    );
    let structure_vias = pcb.structure.unwrap().via.unwrap().padstacks.len();
    assert!(structure_vias < board.library.via_padstack_count());
    // A net class that routes on the signal layers only has no `use_layer`.
    assert!(
        network
            .classes
            .iter()
            .all(|c| c.circuit.as_ref().unwrap().use_layer.is_empty())
    );
}

/// Power layers list the nets of their planes (`use_net`), and a fixed area of a plane net on a
/// signal layer is a `plane`: both are how the reader learns that a net is a plane net.
#[test]
fn plane_nets_survive_through_use_net_and_plane_scopes() {
    let board = load("Issue145-smoothieboard.dsn");
    let pcb = pcb_from_board(&board, "x");
    let structure = pcb.structure.unwrap();
    assert!(structure.layers.iter().any(|l| !l.use_net.is_empty()));
    let board = load("Issue367-Charger.dsn");
    let pcb = pcb_from_board(&board, "x");
    let structure = pcb.structure.unwrap();
    assert!(
        structure
            .planes
            .iter()
            .any(|p| p.area.shape.layer().name() == "F.Cu")
    );
    assert!(board.rules.nets.iter().any(|n| n.contains_plane()));
    assert_eq!(network(&round_trip(&board, "x")), network(&board));
}
