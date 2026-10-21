//! Q22.16: a footprint's own mask aperture keeps other nets out, and a pad
//! inside it stays reachable by its own net.
//!
//! Fixtures in `tests/data/quality/` (mm, KiCad's frame; the DSN negates Y):
//! `q22-fp-mask.kicad_pcb` has J1 at (20, 11) with an F.Mask polygon over
//! (14..26, 11..19) and two 2 x 6 mm pads of nets A (16..18, 9..15) and B
//! (22..24, 9..15), which reach 2 mm above it (their centres, at y = 12, lie
//! inside it); A and B come from PA (17, 3) and PB (23, 3). Net C runs from
//! PC1 (5, 17) to PC2 (35, 17), straight through the aperture below the pads.
//! U1 at (32, 25) has an F.Mask rectangle (31.3..32.7) over its one 1 mm pad
//! of net D, which comes from PD (22, 25). `q22-fp-mask.dsn` is what the
//! KiCad plugin writes for that board (`kicad-plugin/tests/test_footprint_masks.py`
//! checks it): `q22-fp-mask-base.dsn` (the export) plus a keepout over each
//! aperture (`io::specctra::parser::pad_keepouts`): J1's, over pads of two
//! nets, is `solder_mask_shared` and J1's fingers are cut back to their part
//! above it (2 x 1.999 mm, centre y = 10, outside the aperture grown by 1 µm;
//! `freerouting_kicad::pad_trim`), U1's is `solder_mask_pads`. Both have
//! `(clearance_class null)` and a 1 µm strip along each side (Q22.18), named
//! `solder_mask_shared` where it lies over the fingers.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::drc::{BoardRule, BoardRuleLimits, InputCopper, check_board_rules};
use freerouting::job::prepare::{JobInput, PreparedJob, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntBox, Shape, TileShape};

const DIR: &str = "crates/freerouting/tests/data/quality";

fn text(name: &str) -> String {
    std::fs::read_to_string(repo_root().join(DIR).join(name)).unwrap()
}

fn patch() -> SettingsPatch {
    let mut patch = SettingsPatch {
        profile: Some("legacy".to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    patch
}

fn prepare_dsn(dsn: &str) -> PreparedJob {
    let mut input = JobInput::new(dsn.as_bytes().to_vec(), "q22-fp-mask");
    input.extra_sources = vec![(Source::Testing, patch())];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn prepare_pcb() -> PreparedJob {
    use freerouting::io::kicad::{ReadOptions, read_kicad_input};
    let read = read_kicad_input(
        &repo_root().join(DIR).join("q22-fp-mask.kicad_pcb"),
        ReadOptions::default(),
    )
    .unwrap();
    let mut input = JobInput::new(Vec::new(), "q22-fp-mask");
    input.set_kicad(read);
    input.extra_sources.push((Source::Testing, patch()));
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn route(prepared: PreparedJob) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("no listener took the board")
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

/// A box in KiCad millimetres as board units (0.1 µm, Y negated).
fn kicad_box(x0: f64, y0: f64, x1: f64, y1: f64) -> TileShape {
    let u = |v: f64| (v * 10_000.0).round() as i32;
    TileShape::IntBox(IntBox::new(u(x0), u(-y1), u(x1), u(-y0)))
}

/// The nets of the traces and vias whose copper on F.Cu meets `region`
/// (shrunk by 1 µm, so copper that only touches the edge does not count).
fn nets_in(board: &RoutingBoard, region: &TileShape) -> Vec<String> {
    let area = Shape::from(region.clone());
    let mut nets: Vec<String> = board
        .overlapping_objects(region, 0)
        .into_iter()
        .filter_map(|id| board.item(id))
        .filter(|item| item.is_trace() || item.is_via())
        .filter(|item| {
            freerouting::quality::stub_contacts::pieces(board, item)
                .iter()
                .any(|(layer, shape)| *layer == 0 && shape.intersects(&area))
        })
        .flat_map(|item| item.net_numbers().to_vec())
        .filter_map(|n| board.rules.nets.get(n).map(|net| net.name.clone()))
        .collect();
    nets.sort();
    nets.dedup();
    nets
}

/// J1's aperture, 1 µm inside its edges.
fn j1() -> TileShape {
    kicad_box(14.001, 11.001, 25.999, 18.999)
}

/// U1's aperture, 1 µm inside its edges.
fn u1() -> TileShape {
    kicad_box(31.301, 24.301, 32.699, 25.699)
}

fn assert_kept_out(board: &RoutingBoard) {
    assert_eq!(incomplete(board), 0, "A, B, C and D all connect");
    // Q22.16 (option (b)): C goes round J1's aperture, and A and B reach
    // their fingers' part above it, so no trace enters it
    assert_eq!(nets_in(board, &j1()), Vec::<String>::new());
    assert_eq!(nets_in(board, &u1()), vec!["D".to_owned()]);
}

/// The plugin's DSN: C goes round J1's aperture, A and B reach their
/// fingers, cut back to their part outside it (`solder_mask_shared`), from
/// above it, D reaches its pad inside U1's aperture.
#[test]
fn the_plugins_keepouts_keep_other_nets_out_and_let_each_pads_net_in() {
    assert_kept_out(&route(prepare_dsn(&text("q22-fp-mask.dsn"))));
}

/// The `.kicad_pcb` reader writes the same keepouts.
#[test]
fn the_readers_keepouts_keep_other_nets_out_and_let_each_pads_net_in() {
    assert_kept_out(&route(prepare_pcb()));
}

/// Without the keepouts (KiCad's export as it is) C runs through J1's
/// aperture.
#[test]
fn without_the_keepouts_c_runs_through_the_aperture() {
    let board = route(prepare_dsn(&text("q22-fp-mask-base.dsn")));
    assert_eq!(incomplete(&board), 0);
    assert!(
        nets_in(&board, &j1()).contains(&"C".to_owned()),
        "{:?}",
        nets_in(&board, &j1())
    );
}

/// Unnamed keepouts over the same apertures are Java's: U1's pad, whose
/// centre lies inside its one, stays unrouted. The name is what lets D in.
#[test]
fn plain_keepouts_over_the_pads_leave_them_unrouted() {
    let dsn = text("q22-fp-mask.dsn").replace("(keepout \"solder_mask_pads\" ", "(keepout ");
    assert_ne!(dsn, text("q22-fp-mask.dsn"));
    let board = route(prepare_dsn(&dsn));
    assert_eq!(incomplete(&board), 1);
}

/// A `solder_mask_pads` keepout that the router finds over pads of two nets
/// (Q22.16's DSN, before the fingers were cut back) holds every net out, the
/// pads' own too, and the load log says so: A and B, whose pin centres lie
/// inside it, stay unrouted.
#[test]
fn a_footprint_keepout_over_pads_of_two_nets_keeps_every_net_out() {
    let keepout = "    (keepout \"solder_mask_pads\" (polygon F.Cu 0 14000 -11000 26000 -11000 \
                   26000 -19000 14000 -19000 14000 -11000))\n    (via ";
    let dsn = text("q22-fp-mask-base.dsn").replacen("    (via ", keepout, 1);
    let prepared = prepare_dsn(&dsn);
    let lines: Vec<&str> = prepared
        .board
        .log_messages()
        .iter()
        .map(String::as_str)
        .filter(|l| l.contains("solder_mask_pads"))
        .collect();
    assert_eq!(lines.len(), 1, "{:?}", prepared.board.log_messages());
    assert!(
        lines[0].contains("over the pads of 2 nets, it keeps every net out"),
        "{lines:?}"
    );
    let board = route(prepared);
    assert_eq!(incomplete(&board), 2);
    assert_eq!(nets_in(&board, &j1()), Vec::<String>::new());
}

/// Q20's output check judges against the same keepouts: a C trace through
/// J1's aperture and an A trace along its finger into it are routed
/// `keepout` breaches (so `repair` removes them); a D trace into U1's
/// aperture is none.
#[test]
fn the_output_check_reports_another_nets_trace_in_the_aperture() {
    let wiring = "(wiring\n    (wire (path F.Cu 200  5000 -17000  35000 -17000) (net C))\n    \
                  (wire (path F.Cu 200  22000 -25000  32000 -25000) (net D))\n    \
                  (wire (path F.Cu 200  17000 -3000  17000 -12000) (net A))\n  )";
    let dsn = text("q22-fp-mask.dsn").replace("(wiring)", wiring);
    assert_ne!(dsn, text("q22-fp-mask.dsn"));
    let prepared = prepare_dsn(&dsn);
    let limits = BoardRuleLimits::of(&prepared.board, &prepared.board_settings);
    let found = check_board_rules(&prepared.board, &limits, &InputCopper::none());
    let keepout: Vec<String> = found
        .iter()
        .filter(|v| v.rule == BoardRule::Keepout)
        .flat_map(|v| v.items.iter())
        .filter_map(|id| prepared.board.item(*id))
        .filter(|item| item.is_trace())
        .flat_map(|item| item.net_numbers().to_vec())
        .filter_map(|n| prepared.board.rules.nets.get(n).map(|net| net.name.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(keepout, vec!["A".to_owned(), "C".to_owned()], "{found:?}");
}

/// The DSN writer keeps the names, so the keepouts come back the same.
#[test]
fn the_dsn_writer_keeps_the_net_of_the_keepout() {
    let prepared = prepare_dsn(&text("q22-fp-mask.dsn"));
    let written = freerouting::io::specctra::dsn_writer::write_dsn(&prepared.board, "q22");
    // U1's body and strips (Q22.18) carry D; J1's body and top strip are
    // shared, its other strips go with them
    assert_eq!(written.matches("solder_mask_pads").count(), 8, "{written}");
    assert_eq!(
        written.matches("solder_mask_shared").count(),
        2,
        "{written}"
    );
}

/// The keepout over pads of one net carries the net but is no copper: an
/// obstacle for traces and vias of other nets only, never for a pin or
/// another area, so a pin of another net beside the aperture (the dev board's
/// USB-C pads, each in its own aperture 0.1 mm from the next) is no DRC
/// violation (`kicad_read.rs::dev_board_drc_equals_the_exports`).
#[test]
fn the_one_net_keepout_is_an_obstacle_for_other_nets_traces_only() {
    let wiring = "(wiring\n    (wire (path F.Cu 200  5000 -25000  35000 -25000) (net C))\n    \
                  (wire (path F.Cu 200  22000 -25000  32000 -25000) (net D))\n  )";
    let dsn = text("q22-fp-mask.dsn").replace("(wiring)", wiring);
    let board = prepare_dsn(&dsn).board;
    let all = board.overlapping_objects(&kicad_box(0.0, 0.0, 40.0, 30.0), 0);
    let items: Vec<_> = all.iter().filter_map(|id| board.item(*id)).collect();
    let keepout = items
        .iter()
        .find(|i| i.as_area().is_some_and(|a| a.is_pad_keepout()) && i.net_count() > 0)
        .expect("U1's keepout");
    let pins: Vec<_> = items.iter().filter(|i| i.is_pin()).collect();
    assert_eq!(pins.len(), 8);
    for pin in &pins {
        assert!(!board.is_obstacle(keepout, pin) && !board.is_obstacle(pin, keepout));
    }
    let net = |name: &str| board.rules.nets.get_by_name(name, 1).unwrap().net_number;
    let trace = |n: i32| {
        items
            .iter()
            .find(|i| i.is_trace() && i.net_numbers() == [n])
            .unwrap()
    };
    let (c, d) = (trace(net("C")), trace(net("D")));
    assert!(board.is_obstacle(keepout, c) && board.is_obstacle(c, keepout));
    assert!(!board.is_obstacle(keepout, d) && !board.is_obstacle(d, keepout));
}

/// J1's `solder_mask_shared` keepout carries no net: an obstacle for traces
/// of every net, A's (whose finger lies under it) too, and for no pin.
#[test]
fn the_shared_keepout_holds_every_nets_traces_out_and_no_pin() {
    let wiring = "(wiring\n    (wire (path F.Cu 200  17000 -3000  17000 -12000) (net A))\n  )";
    let dsn = text("q22-fp-mask.dsn").replace("(wiring)", wiring);
    let board = prepare_dsn(&dsn).board;
    let all = board.overlapping_objects(&kicad_box(0.0, 0.0, 40.0, 30.0), 0);
    let items: Vec<_> = all.iter().filter_map(|id| board.item(*id)).collect();
    let keepout = items
        .iter()
        .find(|i| i.as_area().is_some_and(|a| a.is_pad_keepout()) && i.net_count() == 0)
        .expect("J1's keepout");
    let a = items.iter().find(|i| i.is_trace()).expect("A's trace");
    assert!(board.is_obstacle(keepout, a) && board.is_obstacle(a, keepout));
    for pin in items.iter().filter(|i| i.is_pin()) {
        assert!(!board.is_obstacle(keepout, pin) && !board.is_obstacle(pin, keepout));
    }
}

/// The traces of `wiring` (DSN `(wire ...)` lines) that Q20's output check
/// finds in a keepout of `dsn`, by net.
fn keepout_breaches(dsn: &str, wiring: &str) -> Vec<String> {
    let dsn = dsn.replace("(wiring)", &format!("(wiring\n    {wiring}\n  )"));
    let prepared = prepare_dsn(&dsn);
    let limits = BoardRuleLimits::of(&prepared.board, &prepared.board_settings);
    let found = check_board_rules(&prepared.board, &limits, &InputCopper::none());
    let mut nets: Vec<String> = found
        .iter()
        .filter(|v| v.rule == BoardRule::Keepout)
        .flat_map(|v| v.items.iter())
        .filter_map(|id| prepared.board.item(*id))
        .filter(|item| item.is_trace())
        .flat_map(|item| item.net_numbers().to_vec())
        .filter_map(|n| prepared.board.rules.nets.get(n).map(|net| net.name.clone()))
        .collect();
    nets.dedup();
    nets
}

/// Q22.18: a mask keepout keeps no clearance of its own (`(clearance_class
/// null)`), so a C trace may run with its edge on U1's grown aperture edge
/// (DSN y = -24299, the 1 µm strip of `MASK_MARGIN_NM`; the 0.2 mm trace at
/// y = -24199) but not 50 µm into it. With the default area clearance
/// (0.2 mm) on the body, Q22.16's keepout, the edge is a breach too.
#[test]
fn another_nets_trace_may_run_at_the_apertures_edge_but_not_in_it() {
    let at_edge = "(wire (path F.Cu 200  25000 -24199  39000 -24199) (net C))";
    let inside = "(wire (path F.Cu 200  25000 -24250  39000 -24250) (net C))";
    let dsn = text("q22-fp-mask.dsn");
    assert_eq!(keepout_breaches(&dsn, at_edge), Vec::<String>::new());
    assert_eq!(keepout_breaches(&dsn, inside), vec!["C".to_owned()]);
    let default_class = dsn.replace("31300 -24300) (clearance_class null))", "31300 -24300))");
    assert_ne!(default_class, dsn);
    assert_eq!(
        keepout_breaches(&default_class, at_edge),
        vec!["C".to_owned()]
    );
}

/// Q22.18: a board-level mask shape (`solder_mask`, Q18.7) over pins of one
/// net carries that net like a footprint's: D reaches its pad under it,
/// and C still goes round J1's aperture.
#[test]
fn a_pad_of_the_shapes_own_net_under_a_board_level_mask_shape_connects() {
    let dsn = text("q22-fp-mask.dsn").replace(
        "(keepout \"solder_mask_pads\" ",
        "(keepout \"solder_mask\" ",
    );
    assert_ne!(dsn, text("q22-fp-mask.dsn"));
    let board = route(prepare_dsn(&dsn));
    assert_kept_out(&board);
}

/// Q22.18: a strip that grows an aperture has no pin under it, yet it is
/// fitted with the aperture it meets: U1's four strips carry D like its
/// body, so D still reaches its pad through them, and J1's strips keep every
/// net out like its body.
#[test]
fn an_apertures_strips_are_fitted_with_its_body() {
    let board = prepare_dsn(&text("q22-fp-mask.dsn")).board;
    let d = board.rules.nets.get_by_name("D", 1).unwrap().net_number;
    let keepouts_in = |region: &TileShape| -> Vec<Vec<i32>> {
        let mut nets: Vec<Vec<i32>> = board
            .overlapping_objects(region, 0)
            .into_iter()
            .filter_map(|id| board.item(id))
            .filter(|i| i.as_area().is_some_and(|a| a.is_pad_keepout()))
            .map(|i| i.net_numbers().to_vec())
            .collect();
        nets.sort();
        nets
    };
    assert_eq!(
        keepouts_in(&kicad_box(31.0, 24.0, 33.0, 26.0)),
        vec![vec![d]; 5]
    );
    assert_eq!(
        keepouts_in(&kicad_box(13.0, 10.5, 27.0, 19.5)),
        vec![Vec::<i32>::new(); 5]
    );
}

/// Q22.18: only a keepout with no pin under it takes the fit of those it
/// meets, so an aperture over PC2 (net C) that touches U1's corner leaves
/// U1's body carrying D and its own carrying C; the strips of U1 that touch
/// both keep both out.
#[test]
fn touching_apertures_of_two_nets_keep_their_own_nets() {
    let pc2 = "(keepout \"solder_mask_pads\" (polygon F.Cu 0 32700 -16300 35700 -16300 35700 \
               -24300 32700 -24300 32700 -16300) (clearance_class null))\n    (via ";
    let dsn = text("q22-fp-mask.dsn").replacen("(via ", pc2, 1);
    let board = prepare_dsn(&dsn).board;
    let net = |name: &str| board.rules.nets.get_by_name(name, 1).unwrap().net_number;
    let keepouts_in = |region: &TileShape| -> Vec<Vec<i32>> {
        let mut nets: Vec<Vec<i32>> = board
            .overlapping_objects(region, 0)
            .into_iter()
            .filter_map(|id| board.item(id))
            .filter(|i| i.as_area().is_some_and(|a| a.is_pad_keepout()))
            .map(|i| i.net_numbers().to_vec())
            .collect();
        nets.sort();
        nets
    };
    assert_eq!(
        keepouts_in(&kicad_box(31.9, 24.9, 32.1, 25.1)),
        vec![vec![net("D")]]
    );
    assert_eq!(
        keepouts_in(&kicad_box(34.0, 19.0, 34.2, 19.2)),
        vec![vec![net("C")]]
    );
    // U1's top and right strips meet both
    assert_eq!(
        keepouts_in(&kicad_box(32.6999, 24.2999, 32.7001, 24.3001)),
        vec![vec![], vec![], vec![net("C")], vec![net("D")]]
    );
}
