//! The `start-rooms` and `door-sections` probes (A02.5, A02.7): the room
//! graph after `MazeSearchEngine.getInstance`, line for line against
//! `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! - `start_rooms_match_java` (kind `start-rooms`): per sampled connection,
//!   ripup off and then on, on one loaded board: every complete room in list
//!   order with its doors and target doors, the incomplete rooms left, the
//!   maze queue in key order, the room counter, and the autoroute tree's walk
//!   hash after `clear()`. Required on the fixture ladder up to and including
//!   `Issue026-J2_reference`; the rest of the ladder is compared too and
//!   listed in `EXPECTED_MISMATCHES` where it differs.
//! - `door_sections_match_java` (kind `door-sections`): the section segments
//!   of a hand-built pair of doors at four offsets, then of the first 100
//!   doors of J2's routable sampled connections.

mod autoroute_common;
#[path = "support/start_rooms_lines.rs"]
mod start_rooms_lines;

use autoroute_common::{
    Loaded, board_name, compare_probe, kind_fixtures, load, router_control, sample, writer,
};
use freerouting::autoroute::probe::{ProbeValue, ProbeWriter};
use freerouting::autoroute::session::{DoorId, RoomId, Session};
use freerouting_geometry::{IntBox, TileShape};

/// Boards of the ladder beyond J2 whose start rooms differ from Java's,
/// with the reason. Empty: none known.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// The ladder up to and including J2 (the acceptance set).
const LAST_REQUIRED: &str = "Issue026-J2_reference";

use start_rooms_lines::*;

#[test]
fn start_rooms_match_java() {
    let mut failures = Vec::new();
    let mut required = true;
    for dsn in kind_fixtures("start-rooms") {
        let board = board_name(&dsn);
        let result = compare_probe("start-rooms", &dsn, &start_rooms_lines(&dsn));
        let expected = EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if required || !expected => failures.push(report),
            Ok(()) if expected => failures.push(format!(
                "{board}: listed in EXPECTED_MISMATCHES but matches Java; remove it"
            )),
            _ => {}
        }
        if board == LAST_REQUIRED {
            required = false;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// door-sections

fn door_section_line(out: &mut ProbeWriter, s: &mut Session<'_>, door: DoorId, offset: f64) {
    let segments = s.section_segments(door, offset).expect("section segments");
    let values = segments
        .iter()
        .map(|l| ProbeValue::bits_list([l.a.x, l.a.y, l.b.x, l.b.y]))
        .collect();
    out.record(&[
        ("door", java_id(s.door_java_id(door))),
        ("dim", s.door(door).dimension.into()),
        ("offset", offset.into()),
        ("count", segments.len().into()),
        ("sections", s.door_sections(door).len().into()),
        ("segments", ProbeValue::List(values)),
    ]);
}

fn octagon(llx: i32, lly: i32, urx: i32, ury: i32) -> TileShape {
    TileShape::IntOctagon(IntBox::new(llx, lly, urx, ury).to_int_octagon())
}

fn door_sections_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let mut out = writer("door-sections", &board_name(dsn));
    let routable: Vec<_> = conns
        .iter()
        .filter(|c| c.state.is_none())
        .filter_map(|c| {
            router_control(&loaded, c.net, false)
                .ok()
                .map(|ctl| (c, ctl))
        })
        .collect();
    let Loaded { board, .. } = &mut loaded;
    let Some((_, first_control)) = routable.first() else {
        return out.into_lines();
    };
    // The hand-built pair (rooms outside any list; the session only holds
    // them).
    let mut s = Session::new(board, first_control, None, None);
    let a = s.detached_free_room(octagon(0, 0, 1000, 1000), 0, 1);
    let b = s.detached_free_room(octagon(500, 200, 2000, 800), 0, 2);
    let c = s.detached_free_room(octagon(-800, 0, 0, 600), 0, 3);
    let ab = s.new_door_of_shapes(a, b).expect("door");
    let ac = s.new_door_of_shapes(a, c).expect("door");
    for offset in [0.0, 10.0, 50.0, 200.0] {
        out.record(&[("stage", format!("hand:ab:{offset:?}").into())]);
        door_section_line(&mut out, &mut s, ab, offset);
        out.record(&[("stage", format!("hand:ac:{offset:?}").into())]);
        door_section_line(&mut out, &mut s, ac, offset);
    }
    s.clear();
    drop(s);
    // The first 100 doors of the routable connections, in room list order.
    let mut printed = 0;
    for (conn, control) in &routable {
        if printed >= DOOR_SECTION_DOORS {
            break;
        }
        let mut s = Session::new(board, control, None, None);
        s.init(&conn.start, &conn.dest).expect("init");
        let mut doors: Vec<DoorId> = Vec::new();
        for &room in s.complete_rooms() {
            for &door in s.room(room).doors() {
                if !doors.contains(&door) && printed + doors.len() < DOOR_SECTION_DOORS {
                    doors.push(door);
                }
            }
        }
        out.record(&[
            ("stage", format!("conn:{}", conn.net).into()),
            ("net", conn.net.into()),
            ("doors", doors.len().into()),
        ]);
        for &door in &doors {
            let first_room: RoomId = s.door(door).rooms[0];
            let layer = s.room(first_room).layer() as usize;
            let offset = f64::from(control.compensated_trace_half_width[layer]);
            door_section_line(&mut out, &mut s, door, offset);
        }
        printed += doors.len();
        s.clear();
    }
    out.into_lines()
}

/// The doors of the `door-sections` probe.
const DOOR_SECTION_DOORS: usize = 100;

#[test]
fn door_sections_match_java() {
    for dsn in kind_fixtures("door-sections") {
        let lines = door_sections_lines(&dsn);
        if let Err(report) = compare_probe("door-sections", &dsn, &lines) {
            panic!("{report}");
        }
    }
}
