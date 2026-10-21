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
//! checks it): `q22-fp-mask-base.dsn` (the export) plus a `solder_mask_pads`
//! keepout over each aperture (`io::specctra::parser::pad_keepouts`).

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

/// J1's aperture without its pads (each 0.2 mm wider, the clearance): left,
/// middle, right and below.
fn j1_without_pads() -> [TileShape; 4] {
    [
        kicad_box(14.001, 11.001, 15.799, 18.999),
        kicad_box(18.201, 11.001, 21.799, 18.999),
        kicad_box(24.201, 11.001, 25.999, 18.999),
        kicad_box(14.001, 15.201, 25.999, 18.999),
    ]
}

/// U1's aperture, 1 µm inside its edges.
fn u1() -> TileShape {
    kicad_box(31.301, 24.301, 32.699, 25.699)
}

fn assert_kept_out(board: &RoutingBoard) {
    assert_eq!(incomplete(board), 0, "A, B, C and D all connect");
    // C goes round J1's aperture; A and B enter it only along their own pad,
    // to its centre
    assert_eq!(nets_in(board, &j1()), vec!["A".to_owned(), "B".to_owned()]);
    for part in j1_without_pads() {
        assert_eq!(nets_in(board, &part), Vec::<String>::new(), "{part:?}");
    }
    assert_eq!(nets_in(board, &u1()), vec!["D".to_owned()]);
}

/// The plugin's DSN: C goes round J1's aperture, A and B reach their pads
/// from above it, D reaches its pad inside U1's aperture.
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

/// Plain keepouts (Q18.7's `solder_mask`) over the same apertures leave
/// every pad under them unrouted: the router ends a trace at the pin's
/// centre, which lies inside. The name is what lets A, B and D in.
#[test]
fn plain_keepouts_over_the_pads_leave_them_unrouted() {
    let dsn = text("q22-fp-mask.dsn").replace("\"solder_mask_pads\"", "\"solder_mask\"");
    let board = route(prepare_dsn(&dsn));
    assert_eq!(incomplete(&board), 3);
}

/// The load log says how each keepout was fitted to its pads.
#[test]
fn the_log_names_the_keepout_whose_pads_were_cut_out() {
    let prepared = prepare_dsn(&text("q22-fp-mask.dsn"));
    let lines: Vec<&str> = prepared
        .board
        .log_messages()
        .iter()
        .map(String::as_str)
        .filter(|l| l.contains("solder_mask_pads"))
        .collect();
    assert_eq!(lines.len(), 1, "{:?}", prepared.board.log_messages());
    assert!(lines[0].contains("the pads of 2 nets cut out"), "{lines:?}");
}

/// Q20's output check judges against the same keepouts: a C trace through
/// J1's aperture is a routed `keepout` breach (so `repair` removes it); a D
/// trace into U1's aperture and an A trace along its pad to its centre are
/// none.
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
    assert_eq!(keepout, vec!["C".to_owned()], "{found:?}");
}

/// The DSN writer keeps the name that carries the net, so the keepout comes
/// back the same.
#[test]
fn the_dsn_writer_keeps_the_net_of_the_keepout() {
    let prepared = prepare_dsn(&text("q22-fp-mask.dsn"));
    let written = freerouting::io::specctra::dsn_writer::write_dsn(&prepared.board, "q22");
    // U1's keepout carries D; J1's pieces are plain keepouts
    assert_eq!(written.matches("solder_mask_pads").count(), 1, "{written}");
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
        .find(|i| i.as_area().is_some_and(|a| a.is_pad_keepout()))
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
