//! Q07: the pass-1 orderings `conflict-rounds` and `inside-out`
//! (`quality::ordering`) on loaded boards.
//!
//! The boards are synthetic one-layer DSNs written by the tests:
//! - four two-pin nets whose pin MSTs give Q07.2's hand-derived example
//!   (`quality::ordering::conflict_rounds` unit tests);
//! - a 16-pin QFN at 0.5 mm pitch (a dense part) and the same at 1.0 mm (not
//!   dense), with three nets from QFN pins to outer pads and one net between
//!   outer pads only.

use freerouting::autoroute::batch::ordering::{FreeroutingOrdering, select_items};
use freerouting::autoroute::stages::StageSelection;
use freerouting::autoroute::stages::{NetOrdering, PassReport, RouteTask, StageRegistry};
use freerouting::board::RoutingBoard;
use freerouting::drc::load_design;
use freerouting::quality::ordering::conflict_rounds::{self, conflict_rounds_order};
use freerouting::quality::ordering::conflicts::ConflictGraph;
use freerouting::quality::ordering::inside_out::{self, dense_parts, inside_out_order};
use freerouting::settings::{ResolveContext, Source, default_patch, resolve};

/// A one-layer board with single-pin `PAD` parts at `pads` (name, x, y in
/// µm), optional `QFN` parts, and `nets` (name, pins).
fn board(pads: &[(&str, i32, i32)], qfn: Option<(i32, i32, i32)>, nets: &[(&str, &str)]) -> String {
    let mut placement = String::from("    (component \"PAD\"\n");
    for (name, x, y) in pads {
        placement.push_str(&format!("      (place {name} {x} {y} front 0)\n"));
    }
    placement.push_str("    )\n");
    let mut image = String::new();
    if let Some((x, y, pitch)) = qfn {
        placement.push_str(&format!(
            "    (component \"QFN\"\n      (place U1 {x} {y} front 0)\n    )\n"
        ));
        // Pins 1-4 bottom, 5-8 right, 9-12 top, 13-16 left; on each side at
        // -1.5, -0.5, 0.5, 1.5 pitches, the sides 3 pitches from the centre.
        let s = 3 * pitch;
        let along = [-3 * pitch / 2, -pitch / 2, pitch / 2, 3 * pitch / 2];
        image.push_str("    (image \"QFN\"\n");
        for (k, a) in along.iter().enumerate() {
            let k = k as i32;
            image.push_str(&format!("      (pin Small {} {a} {})\n", 1 + k, -s));
            image.push_str(&format!("      (pin Small {} {s} {a})\n", 5 + k));
            image.push_str(&format!("      (pin Small {} {a} {s})\n", 9 + k));
            image.push_str(&format!("      (pin Small {} {} {a})\n", 13 + k, -s));
        }
        image.push_str("    )\n");
    }
    let mut network = String::new();
    let mut names = Vec::new();
    for (name, pins) in nets {
        network.push_str(&format!("    (net {name} (pins {pins}))\n"));
        names.push(*name);
    }
    format!(
        r#"(pcb q07.dsn
  (parser (string_quote ") (space_in_quoted_tokens on) (host_cad "synthetic Q07 board"))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (boundary (path pcb 0  0 0  60000 0  60000 60000  0 60000  0 0))
    (via "Via[0-0]_600:300_um")
    (rule (width 200) (clearance 150))
  )
  (placement
{placement}  )
  (library
    (image "PAD"
      (pin RectPad 1 0 0)
    )
{image}    (padstack RectPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack Small (shape (rect F.Cu -120 -120 120 120)) (attach off))
    (padstack "Via[0-0]_600:300_um" (shape (circle F.Cu 600)) (attach off))
  )
  (network
{network}    (class kicad_default "" {}
      (circuit (use_via "Via[0-0]_600:300_um"))
      (rule (width 200) (clearance 150))
    )
  )
  (wiring)
)
"#,
        names.join(" ")
    )
}

fn load(dsn: &str) -> RoutingBoard {
    load_design(dsn.as_bytes()).unwrap_or_else(|e| panic!("synthetic board: {e}"))
}

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("no net {name}"))
        .net_number
}

fn net_sequence(board: &RoutingBoard, tasks: &[RouteTask]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for task in tasks {
        let name = board.rules.nets.get(task.net).unwrap().name.clone();
        if out.last() != Some(&name) {
            out.push(name);
        }
    }
    out
}

/// Q07.2's example in board units (×100 µm, offset 5 mm).
fn four_nets() -> RoutingBoard {
    let at = |x: i32, y: i32| (5000 + 100 * x, 5000 + 100 * y);
    let pads = [
        ("A1", at(0, 50)),
        ("A2", at(100, 50)),
        ("B1", at(20, 0)),
        ("B2", at(20, 80)),
        ("C1", at(60, 30)),
        ("C2", at(60, 70)),
        ("D1", at(70, 0)),
        ("D2", at(130, 0)),
    ];
    let pads: Vec<(&str, i32, i32)> = pads.iter().map(|(n, (x, y))| (*n, *x, *y)).collect();
    load(&board(
        &pads,
        None,
        &[
            ("N1", "A1-1 A2-1"),
            ("N2", "B1-1 B2-1"),
            ("N3", "C1-1 C2-1"),
            ("N4", "D1-1 D2-1"),
        ],
    ))
}

#[test]
fn conflict_rounds_give_the_hand_derived_order_on_a_board() {
    let board = four_nets();
    let java = select_items(&board);
    assert_eq!(java.len(), 8, "one task per unconnected pin");
    let graph = ConflictGraph::build(&board, &java);
    assert_eq!(graph.len(), 4);
    let (n1, n2, n3, n4) = (
        net(&board, "N1"),
        net(&board, "N2"),
        net(&board, "N3"),
        net(&board, "N4"),
    );
    assert!(graph.conflict(n1, n2) && graph.conflict(n1, n3));
    assert!(!graph.conflict(n2, n3) && !graph.conflict(n1, n4));
    assert_eq!(graph.edge_count(), 2);
    let ordered = conflict_rounds_order(&board, java.clone());
    assert_eq!(net_sequence(&board, &ordered), ["N3", "N4", "N2", "N1"]);
    // A permutation of Java's list, each net's tasks in Java's order.
    let mut sorted = ordered.clone();
    sorted.sort_by_key(|t| t.item.0);
    let mut java_sorted = java.clone();
    java_sorted.sort_by_key(|t| t.item.0);
    assert_eq!(sorted, java_sorted);
    for n in [n1, n2, n3, n4] {
        let of = |list: &[RouteTask]| {
            list.iter()
                .filter(|t| t.net == n)
                .copied()
                .collect::<Vec<_>>()
        };
        assert_eq!(of(&ordered), of(&java));
    }
    // Deterministic.
    assert_eq!(conflict_rounds_order(&board, java), ordered);
}

#[test]
fn the_orderings_replace_pass_one_only() {
    let board = four_nets();
    let java = select_items(&board);
    let mut ordering = conflict_rounds::ordering();
    assert_eq!(ordering.id(), "conflict-rounds");
    let pass1 = ordering.select(&board);
    assert_eq!(pass1, conflict_rounds_order(&board, java.clone()));
    assert_ne!(pass1, java);
    let report = PassReport {
        pass: 1,
        continue_routing: true,
        incomplete: 3,
        ..PassReport::default()
    };
    ordering.on_pass_end(&report);
    // After pass 1: Java's selection, and Java's reorder for pass 2.
    assert_eq!(ordering.select(&board), java);
    let mut freerouting = FreeroutingOrdering::new();
    freerouting.on_pass_end(&report);
    assert_eq!(
        ordering.order(java.clone(), &board, 2),
        freerouting.order(java.clone(), &board, 2)
    );
    // A restore resets the anti-oscillation state, not pass 1.
    ordering.reset();
    assert_eq!(ordering.select(&board), java);
}

#[test]
fn the_orderings_are_registered_and_selected_by_the_ordering_key() {
    let registry = StageRegistry::builtin();
    for id in ["conflict-rounds", "inside-out"] {
        assert!(registry.ordering.contains_key(id), "{id}");
        let mut patch = default_patch(1);
        patch.autorouter.ordering = Some(id.into());
        let settings = resolve(&[(Source::Defaults, patch)], &ResolveContext { cores: 1 }).settings;
        let selection = StageSelection::resolve(&settings, &registry);
        let pipeline = registry.build(&selection, &settings).expect(id);
        assert_eq!(pipeline.ordering.id(), id);
    }
    // The default stays Freerouting's.
    let settings = resolve(
        &[(Source::Defaults, default_patch(1))],
        &ResolveContext { cores: 1 },
    )
    .settings;
    let selection = StageSelection::resolve(&settings, &registry);
    assert_eq!(selection.ordering.id, "freerouting-ordering");
}

/// The QFN board at `pitch` µm: nets FAR (pin 12, top right), NEAR_T (pin
/// 10, top), NEAR_B (pin 2, bottom) to outer pads, and OUT between two outer
/// pads. The QFN is at (30 mm, 30 mm).
fn qfn_board(pitch: i32) -> RoutingBoard {
    let pads = [
        ("P1", 50000, 50000),
        ("P2", 25000, 50000),
        ("P3", 30000, 8000),
        ("P4", 5000, 5000),
        ("P5", 5000, 55000),
    ];
    load(&board(
        &pads,
        Some((30000, 30000, pitch)),
        &[
            ("FAR", "U1-12 P1-1"),
            ("NEAR_T", "U1-10 P2-1"),
            ("NEAR_B", "U1-2 P3-1"),
            ("OUT", "P4-1 P5-1"),
        ],
    ))
}

#[test]
fn inside_out_routes_a_fine_pitch_qfn_from_its_centre() {
    let board = qfn_board(500);
    let parts = dense_parts(&board);
    assert_eq!(parts.len(), 1, "{parts:?}");
    assert_eq!(parts[0].pins, 16);
    let java = select_items(&board);
    let ordered = inside_out_order(&board, java.clone());
    // NEAR_T and NEAR_B are equally near (0.5 pitch along, 3 pitches out):
    // the lower net number first; FAR (1.5 pitches along) next; OUT keeps
    // Java's place after them.
    let (t, b) = (net(&board, "NEAR_T"), net(&board, "NEAR_B"));
    let near = if t < b {
        ["NEAR_T", "NEAR_B"]
    } else {
        ["NEAR_B", "NEAR_T"]
    };
    assert_eq!(
        net_sequence(&board, &ordered),
        [near[0], near[1], "FAR", "OUT"]
    );
    let mut ordering = inside_out::ordering();
    assert_eq!(ordering.id(), "inside-out");
    assert_eq!(ordering.select(&board), ordered);
}

#[test]
fn inside_out_keeps_java_order_without_a_dense_part() {
    let board = qfn_board(1000);
    assert!(dense_parts(&board).is_empty());
    let java = select_items(&board);
    assert_eq!(inside_out_order(&board, java.clone()), java);
}

/// The pass-1 lists of both orderings on a real board are permutations of
/// Java's with the plane-net tasks first and unmoved.
#[test]
fn the_orders_permute_java_s_list_on_a_real_board() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/Issue555-CNH_Functional_Tester_1.dsn");
    let board = load_design(&std::fs::read(&path).unwrap()).unwrap();
    let java = select_items(&board);
    let planes = java.iter().take_while(|t| t.is_plane).count();
    assert!(java[planes..].iter().all(|t| !t.is_plane));
    let key = |list: &[RouteTask]| {
        let mut v: Vec<i32> = list.iter().map(|t| t.item.0).collect();
        v.sort_unstable();
        v
    };
    for ordered in [
        conflict_rounds_order(&board, java.clone()),
        inside_out_order(&board, java.clone()),
    ] {
        assert_eq!(key(&ordered), key(&java));
        assert_eq!(ordered[..planes], java[..planes]);
    }
}

/// Prints, per board of `Q07_BOARDS` (paths separated by `:`), the pass-1
/// list size, the conflict graph, the rounds, the dense parts and whether
/// each order differs from Java's. Run in release:
/// `Q07_BOARDS=a.dsn:b.dsn cargo test --release --test quality_ordering -- --ignored --nocapture`.
#[test]
#[ignore = "a report over boards named in Q07_BOARDS"]
fn report_pass_one_orders() {
    let boards = std::env::var("Q07_BOARDS").unwrap_or_default();
    println!(
        "board\ttasks\tplane_tasks\tsignal_nets\tconflicts\trounds\tdense_parts\tdense_nets\tconflict_rounds_differs\tinside_out_differs\tms"
    );
    for path in boards.split(':').filter(|p| !p.is_empty()) {
        let Ok(bytes) = std::fs::read(path) else {
            println!("{path}\tmissing");
            continue;
        };
        let board = load_design(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        let start = std::time::Instant::now();
        let java = select_items(&board);
        let graph = ConflictGraph::build(&board, &java);
        let rounds = conflict_rounds::rounds(&graph).len();
        let cr = conflict_rounds_order(&board, java.clone());
        let parts = dense_parts(&board);
        let nets: std::collections::BTreeSet<i32> =
            java.iter().filter(|t| !t.is_plane).map(|t| t.net).collect();
        let dense = inside_out::dense_net_order(&board, &parts, &nets).len();
        let io = inside_out_order(&board, java.clone());
        let ms = start.elapsed().as_millis();
        println!(
            "{path}\t{}\t{}\t{}\t{}\t{rounds}\t{}\t{dense}\t{}\t{}\t{ms}",
            java.len(),
            java.iter().filter(|t| t.is_plane).count(),
            graph.len(),
            graph.edge_count(),
            parts.len(),
            cr != java,
            io != java
        );
    }
}
