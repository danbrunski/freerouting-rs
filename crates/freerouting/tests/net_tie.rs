//! Q22.19: a net tie's copper lets its own nets in
//! (`io::specctra::parser::pad_keepouts`, `NET_TIE_KEEPOUT_NAME`).
//!
//! Fixture `tests/data/kicad/q22-net-tie.kicad_pcb` (mm, KiCad's frame; the
//! DSN negates Y): the net-tie footprint NT1 at (20, 15) with a filled F.Cu
//! `fp_poly` (16..24, 13.5..16.5) and two 1 mm round F.Cu pads wholly inside
//! it, NT1-1 of net A at (17.5, 15) and NT1-2 of net B at (22.5, 15); A comes
//! from PA (5, 15), B from PB (35, 15), and net C runs from PC1 (20, 5) to
//! PC2 (20, 25), straight across the tie. `q22-net-tie-base.dsn` is KiCad
//! 10.0.7's export of it, which carries no shape; `q22-net-tie.dsn` is what
//! the KiCad plugin's `copper_graphics.apply` writes for it
//! (`kicad-plugin/tests/test_copper_graphics.py` checks it): the base plus
//! one `keepout "net_tie"` over the polygon, at the default clearance.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, NET_TIE_KEEPOUT_NAME};
use freerouting::drc::{BoardRule, BoardRuleLimits, InputCopper, check_board_rules};
use freerouting::job::prepare::{JobInput, PreparedJob, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const DIR: &str = "crates/freerouting/tests/data/kicad";
/// The tie's polygon, board units (0.1 µm, Y up): x0, y0, x1, y1.
const TIE: [f64; 4] = [160_000.0, -165_000.0, 240_000.0, -135_000.0];
const EMPTY_WIRING: &str = "(wiring\n  )";

fn text(name: &str) -> String {
    std::fs::read_to_string(repo_root().join(DIR).join(name)).unwrap()
}

fn patch(profile: &str) -> SettingsPatch {
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    patch
}

fn prepare_dsn(dsn: &str, profile: &str) -> PreparedJob {
    let mut input = JobInput::new(dsn.as_bytes().to_vec(), "q22-net-tie");
    input.extra_sources = vec![(Source::Testing, patch(profile))];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn prepare_pcb(profile: &str) -> PreparedJob {
    use freerouting::io::kicad::{ReadOptions, read_kicad_input};
    let read = read_kicad_input(
        &repo_root().join(DIR).join("q22-net-tie.kicad_pcb"),
        ReadOptions::default(),
    )
    .unwrap();
    let mut input = JobInput::new(Vec::new(), "q22-net-tie");
    input.set_kicad(read);
    input.extra_sources.push((Source::Testing, patch(profile)));
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

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board.rules.nets.get_by_name(name, 1).unwrap().net_number
}

fn net_names(board: &RoutingBoard, item: &Item) -> Vec<String> {
    item.net_numbers()
        .iter()
        .filter_map(|n| board.rules.nets.get(*n).map(|net| net.name.clone()))
        .collect()
}

/// The distance from `p` to the box `b`, -1 inside it.
fn box_distance(b: [f64; 4], p: (f64, f64)) -> f64 {
    let dx = (b[0] - p.0).max(p.0 - b[2]).max(0.0);
    let dy = (b[1] - p.1).max(p.1 - b[3]).max(0.0);
    if dx == 0.0 && dy == 0.0 {
        -1.0
    } else {
        dx.hypot(dy)
    }
}

/// The smallest gap between `name`'s F.Cu traces and vias and the box `b`,
/// board units (negative: they overlap); infinite without any.
fn gap(board: &RoutingBoard, name: &str, b: [f64; 4]) -> f64 {
    let n = net(board, name);
    let mut gap = f64::INFINITY;
    for item in board.items().filter(|i| i.contains_net(n)) {
        if let Some(t) = item.as_trace().filter(|t| t.layer == 0) {
            let pts: Vec<(f64, f64)> = t
                .polyline
                .corner_approx_arr()
                .iter()
                .map(|p| (p.x, p.y))
                .collect();
            for w in pts.windows(2) {
                for k in 0..=200 {
                    let s = f64::from(k) / 200.0;
                    let p = (
                        w[0].0 + s * (w[1].0 - w[0].0),
                        w[0].1 + s * (w[1].1 - w[0].1),
                    );
                    gap = gap.min(box_distance(b, p) - f64::from(t.half_width));
                }
            }
        } else if item.is_via() {
            let drill = item.drill().unwrap();
            let c = (f64::from(drill.center.x), f64::from(drill.center.y));
            let radius = drill
                .shape_on_layer(0)
                .map(|s| f64::from(s.bounding_box().width()) / 2.0)
                .unwrap_or(0.0);
            gap = gap.min(box_distance(b, c) - radius);
        }
    }
    gap
}

/// The `net_tie` keepouts, in id order: their nets' names.
fn tie_keepouts(board: &RoutingBoard) -> Vec<Vec<String>> {
    board
        .items()
        .filter(|i| {
            i.as_area().is_some_and(|a| {
                a.is_pad_keepout() && a.name.as_deref() == Some(NET_TIE_KEEPOUT_NAME)
            })
        })
        .map(|i| net_names(board, i))
        .collect()
}

/// The keepout is fitted to the tie's pads: it carries A and B.
#[test]
fn the_net_tie_keepout_carries_the_nets_of_the_tie_pads() {
    let plugin = prepare_dsn(&text("q22-net-tie.dsn"), "enhanced").board;
    assert_eq!(tie_keepouts(&plugin), [["A", "B"]]);
    let reader = prepare_pcb("enhanced").board;
    assert_eq!(tie_keepouts(&reader), [["A", "B"]]);
    // the export alone has no keepout
    let base = prepare_dsn(&text("q22-net-tie-base.dsn"), "enhanced").board;
    assert!(base.items().all(|i| i.as_area().is_none()));
}

fn assert_tie_routed(board: &RoutingBoard, what: &str) {
    assert_eq!(incomplete(board), 0, "{what}: A, B and C all connect");
    // A and B reach their pads, wholly inside the tie's copper, through it
    for name in ["A", "B"] {
        let g = gap(board, name, TIE);
        assert!(g < 0.0, "{what}: {name} enters the tie: {g}");
    }
    // C keeps the 0.2 mm clearance (1 unit of rounding)
    let c = gap(board, "C", TIE);
    assert!(c >= 1999.0, "{what}: C's gap {c}");
}

/// The plugin's DSN: A and B reach their pads through the tie's copper, C
/// keeps its clearance from it, in both profiles.
#[test]
fn the_plugins_net_tie_lets_its_nets_in_and_keeps_c_out() {
    for profile in ["legacy", "enhanced"] {
        let board = route(prepare_dsn(&text("q22-net-tie.dsn"), profile));
        assert_tie_routed(&board, profile);
    }
}

/// The `.kicad_pcb` reader's keepout routes the same way.
#[test]
fn the_readers_net_tie_lets_its_nets_in_and_keeps_c_out() {
    for profile in ["legacy", "enhanced"] {
        assert_tie_routed(&route(prepare_pcb(profile)), profile);
    }
}

/// Q22.4's model, a `copper_graphic` keepout for every net, leaves the tie's
/// pads unreachable (slice's J1-10 to NT1-2): the regression this fixes.
#[test]
fn with_a_keepout_for_every_net_the_ties_pads_stay_unrouted() {
    let dsn = text("q22-net-tie.dsn").replace("\"net_tie\"", "\"copper_graphic\"");
    assert_ne!(dsn, text("q22-net-tie.dsn"));
    let board = route(prepare_dsn(&dsn, "enhanced"));
    assert_eq!(incomplete(&board), 2);
    assert!(tie_keepouts(&board).is_empty());
}

/// The rule breaches Q20's output check finds for `wires` (DSN `(wire ...)`
/// lines) on the plugin's DSN: (rule, the nets of the pair), sorted.
fn breaches(wires: &[&str]) -> Vec<(String, Vec<String>)> {
    let wiring = format!("(wiring\n    {}\n  )", wires.join("\n    "));
    let dsn = text("q22-net-tie.dsn").replace(EMPTY_WIRING, &wiring);
    assert_ne!(dsn, text("q22-net-tie.dsn"));
    let prepared = prepare_dsn(&dsn, "enhanced");
    let board = &prepared.board;
    let limits = BoardRuleLimits::of(board, &prepared.board_settings);
    let mut out: Vec<(String, Vec<String>)> =
        check_board_rules(board, &limits, &InputCopper::none())
            .iter()
            .filter(|v| matches!(v.rule, BoardRule::Keepout | BoardRule::Clearance))
            .map(|v| {
                let nets = v
                    .items
                    .iter()
                    .filter_map(|id| board.item(*id))
                    .map(|item| net_names(board, item).join(","))
                    .collect();
                (v.rule.name().to_owned(), nets)
            })
            .collect();
    out.sort();
    out.dedup();
    out
}

/// A trace of A may run through the tie's copper, but not closer to B's pad
/// than the normal pad clearance; a trace of C keeps the board's clearance
/// (0.2 mm) from the copper.
#[test]
fn the_output_check_lets_the_ties_nets_in_and_keeps_the_clearance() {
    // A from PA into the tie, ending 0.4 mm from B's pad: no breach
    let a_in = "(wire (path F.Cu 200  5000 -15000  21500 -15000) (net A))";
    assert_eq!(breaches(&[a_in]), []);
    // ending 0.1 mm from B's pad: a clearance breach with the pad, not the tie
    let a_near_b = "(wire (path F.Cu 200  5000 -15000  21800 -15000) (net A))";
    assert_eq!(
        breaches(&[a_near_b]),
        [("clearance".to_owned(), vec!["A".to_owned(), "B".to_owned()])]
    );
    // C 0.25 mm from the copper: none; 0.1 mm: a keepout breach; across it too
    let c_clear = "(wire (path F.Cu 200  10000 -16850  30000 -16850) (net C))";
    assert_eq!(breaches(&[c_clear]), []);
    let c_near = "(wire (path F.Cu 200  10000 -16700  30000 -16700) (net C))";
    let c_across = "(wire (path F.Cu 200  20000 -5000  20000 -25000) (net C))";
    let keepout_c = ("keepout".to_owned(), vec!["C".to_owned(), "A,B".to_owned()]);
    for c in [c_near, c_across] {
        assert_eq!(breaches(&[c]), std::slice::from_ref(&keepout_c), "{c}");
    }
}

/// Pieces of one shape go together: a piece touching the body lets A and B
/// in although no pin lies under it; a `net_tie` keepout touching nothing
/// with a pin is Java's keepout for every net.
#[test]
fn a_ties_pieces_share_its_nets_and_a_piece_with_no_pin_is_a_plain_keepout() {
    let strip = "    (keepout \"net_tie\" (polygon F.Cu 0 24000 -13500 25000 -13500 25000 -16500 24000 -16500 24000 -13500))\n";
    let far = "    (keepout \"net_tie\" (polygon F.Cu 0 30000 -4000 31000 -4000 31000 -5000 30000 -5000 30000 -4000))\n";
    let base = text("q22-net-tie.dsn");
    let at = base.find("    (via ").unwrap();
    let dsn = format!("{}{strip}{far}{}", &base[..at], &base[at..]);
    let board = prepare_dsn(&dsn, "enhanced").board;
    assert_eq!(tie_keepouts(&board), [["A", "B"], ["A", "B"]]);
    let plain: Vec<_> = board
        .items()
        .filter_map(|i| i.as_area().map(|a| (i, a)))
        .filter(|(i, a)| !a.is_pad_keepout() && i.net_count() == 0)
        .collect();
    assert_eq!(plain.len(), 1, "the far piece");
}

/// The DSN writer keeps the name, so the keepout reads back with the same
/// nets.
#[test]
fn the_dsn_writer_keeps_the_net_tie_keepout() {
    let prepared = prepare_dsn(&text("q22-net-tie.dsn"), "enhanced");
    let written = freerouting::io::specctra::dsn_writer::write_dsn(&prepared.board, "q22");
    assert_eq!(written.matches("(keepout net_tie").count(), 1, "{written}");
    let back = prepare_dsn(&written, "enhanced").board;
    assert_eq!(tie_keepouts(&back), [["A", "B"]]);
}
