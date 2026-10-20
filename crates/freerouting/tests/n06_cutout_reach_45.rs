//! N06.14: how a 45 degree board reaches `Simplex::cutout_from`, the home of the
//! `cutout_hole_crosses_border` and `simplex_octagon_half_corner` switches.
//!
//! Its own test binary, because the run statistics are process-wide and this test
//! resets them (as `n06_cutout_reach.rs` does for its one test).
//!
//! The 45 degree search tree (Java `ShapeSearchTree45Degree`) replaces the shapes of
//! pins, vias, areas and the outline by bounding octagons, but not those of traces:
//! `calculateTreeShapes(PolylineTrace)` is the base class's `Polyline.offsetShape`, which
//! simplifies to an octagon only when every line of the polyline is a multiple of 45
//! degrees (`Simplex.isIntOctagon`). A trace with a segment at another angle, such as
//! one in the board's loaded wiring, keeps a simplex tree shape. A drill page holding
//! it gets a simplex hole, so `TileShape::cutout_counted` runs `Simplex::cutout_from`
//! for that hole and, as every piece cut by it is a simplex, for the later holes of
//! the page too. The test checks this on three 45 degree boards: canary-pwr (5 such
//! fixed segments), Issue756-tomu-fpga8 (protected any-angle wiring) and
//! Issue026-J2_reference (no wiring), by computing every drill page of one net's
//! session with the run statistics on.
mod autoroute_common;

use autoroute_common::{load, router_control, sample};
use freerouting::autoroute::session::Session;
use freerouting::autoroute::session::drills::DrillPageId;
use freerouting::board::items::ItemKind;
use freerouting::run_stats::{self, Counter, Counts};
use freerouting_geometry::{IntBox, PolylineShapeOps, TileShape};

const CANARY_PWR: &str = "fixtures/canary-pwr/canary-pwr-9511d9a.dsn";
const TOMU_FPGA8: &str = "fixtures/Issue756-tomu-fpga8.dsn";
const J2_REFERENCE: &str = "fixtures/Issue026-J2_reference.dsn";

/// What one board's 45 degree autoroute tree and drill pages show.
#[derive(Debug, Default)]
struct Reach45 {
    /// Traces whose polyline has a line that is not a multiple of 45 degrees.
    any_angle_traces: usize,
    /// Simplex tree shapes of traces; of other items (pins, vias, areas, outline).
    simplex_trace_shapes: usize,
    simplex_other_shapes: usize,
    /// Pins of more than one net (the tie-pin cutout's precondition).
    tie_pins: usize,
    /// Drill pages computed; of them, pages whose box meets a simplex tree shape's box.
    pages: usize,
    pages_with_simplex: usize,
    /// (`cutout_hole_crosses_border`, `simplex_octagon_half_corner`) reached on pages
    /// with and without a simplex tree shape.
    with_simplex: (u64, u64),
    without_simplex: (u64, u64),
}

fn reach_45(dsn: &str) -> Reach45 {
    let mut loaded = load(dsn);
    let net = sample(&loaded.board)
        .iter()
        .find(|c| c.state.is_none())
        .map(|c| c.net)
        .expect("a routable connection");
    let control = router_control(&loaded, net, false).expect("control");
    let class = control.trace_clearance_class_index;
    let mut out = Reach45::default();
    let _ = loaded.board.autoroute_search_tree(class);
    let mut simplex_boxes: Vec<IntBox> = Vec::new();
    {
        let board = &loaded.board;
        let tree = board.search_trees().tree_by_class(class).expect("tree");
        assert_eq!(
            format!("{:?}", tree.kind()),
            "FortyfiveDegree",
            "{dsn}: a 45 degree board"
        );
        let ctx = board.shape_context();
        for item in board.items() {
            if let ItemKind::Trace(trace) = &item.kind {
                if !trace.polyline.is_multiple_of_45_degree() {
                    out.any_angle_traces += 1;
                }
            }
            if matches!(item.kind, ItemKind::Pin(_)) && item.net_count() > 1 {
                out.tie_pins += 1;
            }
            for shape in tree.tree_shapes(item, &ctx).into_iter().flatten() {
                if let TileShape::Simplex(_) = shape {
                    if matches!(item.kind, ItemKind::Trace(_)) {
                        out.simplex_trace_shapes += 1;
                    } else {
                        out.simplex_other_shapes += 1;
                    }
                    simplex_boxes.push(shape.bounding_box());
                }
            }
        }
    }
    let mut session = Session::new(&mut loaded.board, &control, None, None);
    let page_boxes: Vec<IntBox> = session
        .drill_pages()
        .pages()
        .iter()
        .map(|p| p.shape)
        .collect();
    run_stats::enable();
    for (index, page_box) in page_boxes.iter().enumerate() {
        run_stats::reset();
        let before = Counts::now();
        session
            .page_drills(DrillPageId(index as u32), false)
            .expect("drills");
        let counts = Counts::now().since(&before);
        let reached = (
            counts.get(Counter::CutoutHoleCrossesBorder),
            counts.get(Counter::SimplexOctagonHalfCorner),
        );
        out.pages += 1;
        let target = if simplex_boxes.iter().any(|b| {
            let common = b.intersection_box(page_box);
            common.ll.x < common.ur.x && common.ll.y < common.ur.y
        }) {
            out.pages_with_simplex += 1;
            &mut out.with_simplex
        } else {
            &mut out.without_simplex
        };
        target.0 += reached.0;
        target.1 += reached.1;
    }
    run_stats::disable();
    out
}

#[test]
fn a_45_degree_board_reaches_the_simplex_cutout_only_through_any_angle_traces() {
    let canary = reach_45(CANARY_PWR);
    let tomu = reach_45(TOMU_FPGA8);
    let j2 = reach_45(J2_REFERENCE);
    for (dsn, r) in [
        (CANARY_PWR, &canary),
        (TOMU_FPGA8, &tomu),
        (J2_REFERENCE, &j2),
    ] {
        println!("{dsn}: {r:?}");
        // Only trace shapes are simplexes in the 45 degree tree, and exactly those of
        // traces with an any-angle line.
        assert_eq!(r.simplex_other_shapes, 0, "{dsn}");
        assert_eq!(
            r.simplex_trace_shapes > 0,
            r.any_angle_traces > 0,
            "{dsn}: {r:?}"
        );
        // A page without a simplex tree shape never reaches the two switches.
        assert_eq!(r.without_simplex, (0, 0), "{dsn}: {r:?}");
    }
    // canary-pwr's 5 fixed any-angle segments put both switches in reach.
    assert_eq!(canary.any_angle_traces, 5, "{canary:?}");
    assert!(canary.with_simplex.1 > 0, "{canary:?}");
    // J2 has no wiring: no simplex, no reach.
    assert_eq!(
        (j2.any_angle_traces, j2.with_simplex),
        (0, (0, 0)),
        "{j2:?}"
    );
}
