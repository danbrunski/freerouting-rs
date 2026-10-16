//! The line writers of the `start-rooms` probes, moved out of
//! `tests/autoroute_probe_start_rooms.rs` so that `tests/autoroute_probe_variants.rs` (A07.6)
//! runs them on the 90 degree and any-angle boards. Both include
//! `tests/autoroute_common/mod.rs` as `crate::autoroute_common`.

#![allow(dead_code)]

use crate::autoroute_common::{
    Conn, Loaded, board_name, load, router_control, sample, sample_line, writer,
};
use freerouting::autoroute::probe::{ProbeValue, ProbeWriter};
use freerouting::autoroute::session::{ExpandableRef, Room, RoomKind, Session};
use freerouting::board::search::ShapeSearchTree;
use freerouting_geometry::{IntBox, PolylineShapeOps, TileShape, TileShapeOps};

pub fn shape_value(shape: Option<&TileShape>) -> ProbeValue {
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

pub fn box_value(b: IntBox) -> ProbeValue {
    ProbeValue::ints([b.ll.x, b.ll.y, b.ur.x, b.ur.y].map(i64::from))
}

pub fn kind_name(kind: RoomKind) -> &'static str {
    match kind {
        RoomKind::CompleteFree => "free",
        RoomKind::Obstacle => "obstacle",
        RoomKind::Incomplete => "incomplete",
    }
}

pub fn tree_line(out: &mut ProbeWriter, tree: &ShapeSearchTree) {
    let f = tree.fingerprint();
    out.record(&[
        ("cleared", tree.key().into()),
        ("nodes", f.nodes.into()),
        ("leaves", f.leaves.into()),
        ("maxDepth", f.max_depth.into()),
        ("shapeHash", format!("{:016x}", f.shape_hash).into()),
    ]);
}

pub fn java_id(value: Result<i32, freerouting::autoroute::session::RoomError>) -> ProbeValue {
    value.expect("a Java id").into()
}

/// `startRoomLines`: the room graph of `session` after `init`.
pub fn start_room_lines(
    out: &mut ProbeWriter,
    stage: &str,
    conn: &Conn,
    s: &Session<'_>,
    ok: bool,
) {
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

pub fn start_rooms_lines(dsn: &str) -> Vec<String> {
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
