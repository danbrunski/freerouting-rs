//! The `drills` probe (A03.5): the drill pages and drills of a search, line
//! for line against `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! `drills_match_java`: per sampled connection, ripup off, ripup on, and
//! ripup off with `attach_smd` forced on (variant `smd`: no fixture's via
//! rule allows attaching to SMD pins, so this is the only comparison of the
//! own-net SMD pin rules), on one loaded board, after `MazeSearchEngine.getInstance` (so the start rooms
//! exist, as in a real search): `DrillPage.getDrills` with `attach_smd =
//! ctrl.attach_smd_allowed` on every page of the page array in row-major
//! order, each page with its Java id, bounds and drills (Java id, location,
//! shape, layers, the room id per layer); then the room counter and the
//! autoroute tree's walk hash after `clear()`. The drills of earlier pages
//! complete rooms that later pages find, so the page order is part of the
//! comparison. Required on the boards of the `drills` row of `fixtures.tsv`:
//! `scoring-mixed-layer`, `Issue508-SMD-routing-issue-demo` (attach rules),
//! `Issue143-rpi_splitter`, `Issue143-rpi_splitter_mod`,
//! `Issue508-DAC2020_bm08` and `Issue026-J2_reference`.

mod autoroute_common;

use autoroute_common::{
    Loaded, board_name, compare_probe, kind_fixtures, load, router_control, sample, sample_line,
    writer,
};
use freerouting::autoroute::probe::{ProbeValue, ProbeWriter};
use freerouting::autoroute::session::Session;
use freerouting::autoroute::session::drills::DrillPageId;
use freerouting::board::search::ShapeSearchTree;
use freerouting_geometry::{IntBox, Point, PolylineShapeOps, TileShape, TileShapeOps};

/// Boards of the `drills` row whose drills differ from Java's, with the
/// reason. Empty: every board is required.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// `shapeJson` of the Java probe.
fn shape_value(shape: &TileShape) -> ProbeValue {
    let mut values = Vec::new();
    match shape {
        TileShape::IntOctagon(o) => {
            values.push(ProbeValue::str("oct"));
            for v in [
                o.left_x,
                o.bottom_y,
                o.right_x,
                o.top_y,
                o.upper_left_diagonal_x,
                o.lower_right_diagonal_x,
                o.lower_left_diagonal_x,
                o.upper_right_diagonal_x,
            ] {
                values.push(v.into());
            }
        }
        TileShape::IntBox(b) => {
            values.push(ProbeValue::str("box"));
            for v in [b.ll.x, b.ll.y, b.ur.x, b.ur.y] {
                values.push(v.into());
            }
        }
        TileShape::Simplex(_) => {
            let b = shape.bounding_box();
            values.push(ProbeValue::str("simplex"));
            values.push(shape.border_line_count().into());
            values.push(shape.get_id().into());
            for v in [b.ll.x, b.ll.y, b.ur.x, b.ur.y] {
                values.push(v.into());
            }
        }
    }
    ProbeValue::List(values)
}

fn box_value(b: IntBox) -> ProbeValue {
    ProbeValue::ints([b.ll.x, b.ll.y, b.ur.x, b.ur.y].map(i64::from))
}

/// `pointJson`: drill locations are integer points.
fn point_value(point: &Point) -> ProbeValue {
    match point {
        Point::Int(p) => ProbeValue::ints([i64::from(p.x), i64::from(p.y)]),
        Point::Rational(_) => panic!("a rational drill location: {point:?}"),
    }
}

fn tree_line(out: &mut ProbeWriter, tree: &ShapeSearchTree) {
    let f = tree.fingerprint();
    out.record(&[
        ("cleared", tree.key().into()),
        ("nodes", f.nodes.into()),
        ("leaves", f.leaves.into()),
        ("maxDepth", f.max_depth.into()),
        ("shapeHash", format!("{:016x}", f.shape_hash).into()),
    ]);
}

/// `drillLines`: every page's drills, row by row.
fn drill_lines(out: &mut ProbeWriter, s: &mut Session<'_>, attach_smd: bool) {
    let count = s.drill_pages().pages().len();
    for page in (0..count).map(|p| DrillPageId(p as u32)) {
        let drills = s
            .page_drills(page, attach_smd)
            .expect("DrillPage.getDrills")
            .to_vec();
        let pages = s.built_drill_pages().expect("the page array");
        out.record(&[
            ("page", pages.page(page).java_id().into()),
            ("bounds", box_value(pages.page(page).shape)),
            ("drills", drills.len().into()),
        ]);
        for drill in drills {
            let d = s.drill(drill);
            let rooms = d
                .rooms
                .iter()
                .map(|room| {
                    let room = room.expect("a room on every layer of a page drill");
                    s.room_java_id(room).expect("a room id").into()
                })
                .collect();
            out.record(&[
                ("drill", d.java_id().into()),
                ("location", point_value(&d.location)),
                ("shape", shape_value(&d.shape)),
                ("first", d.first_layer.into()),
                ("last", d.last_layer.into()),
                ("rooms", ProbeValue::List(rooms)),
            ]);
        }
    }
}

fn exception_line(out: &mut ProbeWriter, stage: &str, net: i32, exception: &str) {
    out.record(&[
        ("stage", stage.into()),
        ("net", net.into()),
        ("exception", exception.into()),
    ]);
}

fn drills_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let mut out = writer("drills", &board_name(dsn));
    sample_line(&mut out, &loaded.board, &conns);
    for variant in ["off", "on", "smd"] {
        let ripup = variant == "on";
        for conn in &conns {
            let stage = format!("{variant}:{}", conn.net);
            if let Some(state) = conn.state {
                out.record(&[
                    ("stage", stage.as_str().into()),
                    ("net", conn.net.into()),
                    ("state", state.java_name().into()),
                ]);
                continue;
            }
            let control = match router_control(&loaded, conn.net, ripup) {
                Ok(control) => control,
                Err(e) => {
                    exception_line(&mut out, &stage, conn.net, e.java_exception());
                    continue;
                }
            };
            let class = control.trace_clearance_class_index;
            {
                let Loaded { board, .. } = &mut loaded;
                let mut session = Session::new(board, &control, None, None);
                match session.init(&conn.start, &conn.dest) {
                    Ok(ok) => {
                        let attach_smd = variant == "smd" || control.attach_smd_allowed;
                        out.record(&[
                            ("stage", stage.as_str().into()),
                            ("net", conn.net.into()),
                            ("ok", ok.into()),
                            ("attachSmd", attach_smd.into()),
                        ]);
                        if ok {
                            drill_lines(&mut out, &mut session, attach_smd);
                        }
                        out.record(&[
                            ("roomCount", session.room_count().into()),
                            ("complete", session.complete_rooms().len().into()),
                        ]);
                    }
                    Err(e) => exception_line(&mut out, &stage, conn.net, e.java_exception()),
                }
                session.clear();
            }
            let tree = loaded
                .board
                .search_trees()
                .tree_by_class(class)
                .expect("the autoroute tree");
            tree_line(&mut out, tree);
        }
    }
    out.into_lines()
}

#[test]
fn drills_match_java() {
    let mut failures = Vec::new();
    for dsn in kind_fixtures("drills") {
        let board = board_name(&dsn);
        let result = compare_probe("drills", &dsn, &drills_lines(&dsn));
        let expected = EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !expected => failures.push(report),
            Ok(()) if expected => failures.push(format!(
                "{board}: listed in EXPECTED_MISMATCHES but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
