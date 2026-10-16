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

use autoroute_common::{
    Conn, Loaded, board_name, compare_probe, kind_fixtures, load, router_control, sample,
    sample_line, writer,
};
use freerouting::autoroute::probe::{ProbeValue, ProbeWriter};
use freerouting::autoroute::session::{DoorId, ExpandableRef, Room, RoomId, RoomKind, Session};
use freerouting::board::search::ShapeSearchTree;
use freerouting_geometry::{IntBox, PolylineShapeOps, TileShape, TileShapeOps};

/// Boards of the ladder beyond J2 whose start rooms differ from Java's,
/// with the reason. Empty: none known.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// The ladder up to and including J2 (the acceptance set).
const LAST_REQUIRED: &str = "Issue026-J2_reference";

fn shape_value(shape: Option<&TileShape>) -> ProbeValue {
    let Some(shape) = shape else {
        return ProbeValue::List(Vec::new());
    };
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

fn kind_name(kind: RoomKind) -> &'static str {
    match kind {
        RoomKind::CompleteFree => "free",
        RoomKind::Obstacle => "obstacle",
        RoomKind::Incomplete => "incomplete",
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

fn java_id(value: Result<i32, freerouting::autoroute::session::RoomError>) -> ProbeValue {
    value.expect("a Java id").into()
}

/// `startRoomLines`: the room graph of `session` after `init`.
fn start_room_lines(out: &mut ProbeWriter, stage: &str, conn: &Conn, s: &Session<'_>, ok: bool) {
    let queue_len = if ok { s.queue().len() } else { 0 };
    out.record(&[
        ("stage", stage.into()),
        ("net", conn.net.into()),
        ("ok", ok.into()),
        ("roomCount", s.room_count().into()),
        ("complete", s.complete_rooms().len().into()),
        ("incomplete", s.incomplete_rooms().len().into()),
        ("queue", queue_len.into()),
    ]);
    for &room in s.complete_rooms() {
        let Room::CompleteFree {
            id,
            shape,
            layer,
            doors,
            target_doors,
            net_dependent,
        } = s.room(room)
        else {
            panic!("a complete free room in the list");
        };
        out.record(&[
            ("room", (*id).into()),
            ("layer", (*layer).into()),
            ("shape", shape_value(Some(shape))),
            ("netDependent", (*net_dependent).into()),
            ("doors", doors.len().into()),
            ("targets", target_doors.len().into()),
        ]);
        for &door in doors {
            let other = s.other_room(door, room).expect("a door of the room");
            out.record(&[
                ("door", java_id(s.door_java_id(door))),
                ("dim", s.door(door).dimension.into()),
                ("other", kind_name(s.room(other).kind()).into()),
                ("otherId", java_id(s.room_java_id(other))),
                ("first", (s.door(door).rooms[0] == room).into()),
                (
                    "bounds",
                    box_value(s.door_shape(door).expect("door shape").bounding_box()),
                ),
            ]);
        }
        for &door in target_doors {
            let target = s.target_door(door);
            out.record(&[
                ("target", java_id(s.target_door_java_id(door))),
                ("item", target.item.0.into()),
                ("entry", target.tree_entry.into()),
                ("destination", s.is_destination_door(door).into()),
                ("bounds", box_value(target.shape.bounding_box())),
            ]);
        }
    }
    for &room in s.incomplete_rooms() {
        let Room::Incomplete {
            shape,
            contained,
            layer,
            doors,
        } = s.room(room)
        else {
            panic!("an incomplete room in the list");
        };
        out.record(&[
            ("incomplete", shape_value(shape.as_ref())),
            ("contained", shape_value(contained.as_ref())),
            ("layer", (*layer).into()),
            ("doors", doors.len().into()),
        ]);
    }
    if ok {
        for (key, element) in s.queue().iter() {
            let kind = match element.door {
                ExpandableRef::Door(_) => "ExpansionDoor",
                ExpandableRef::TargetDoor(_) => "TargetItemExpansionDoor",
                ExpandableRef::Drill(_) => "ExpansionDrill",
                ExpandableRef::DrillPage(_) => "DrillPage",
            };
            let next_room = element
                .next_room
                .map_or(0, |r| s.room_java_id(r).expect("room id"));
            out.record(&[
                (
                    "queue",
                    ProbeValue::List(vec![
                        key.sorting_value.into(),
                        key.expansion_value.into(),
                        key.door_id.into(),
                        key.section.into(),
                    ]),
                ),
                ("kind", kind.into()),
                (
                    "entry",
                    ProbeValue::bits_list([element.shape_entry.a.x, element.shape_entry.a.y]),
                ),
                ("nextRoom", next_room.into()),
            ]);
        }
    }
}

fn start_rooms_lines(dsn: &str) -> Vec<String> {
    let mut loaded = load(dsn);
    let conns = sample(&loaded.board);
    let mut out = writer("start-rooms", &board_name(dsn));
    sample_line(&mut out, &loaded.board, &conns);
    for variant in ["off", "on"] {
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
                    out.record(&[
                        ("stage", stage.as_str().into()),
                        ("net", conn.net.into()),
                        ("exception", e.java_exception().into()),
                    ]);
                    continue;
                }
            };
            let class = control.trace_clearance_class_index;
            {
                let Loaded { board, .. } = &mut loaded;
                let mut session = Session::new(board, &control, None, None);
                match session.init(&conn.start, &conn.dest) {
                    Ok(ok) => start_room_lines(&mut out, &stage, conn, &session, ok),
                    Err(e) => out.record(&[
                        ("stage", stage.as_str().into()),
                        ("net", conn.net.into()),
                        ("exception", e.java_exception().into()),
                    ]),
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
