//! K06.1: `wire_keepout` keeps its own syntax-tree kind, and the board builds it as a
//! `keepout` (blocking vias too), as Java does.

use freerouting::board::items::AreaKind;
use freerouting::board::structure::FixedState;
use freerouting::io::specctra::{BoardReadResult, build_board};
use freerouting_dsn::Pcb;
use freerouting_dsn::ast::KeepoutKind;
use freerouting_geometry::IntPoint;

fn issue754() -> Pcb {
    let text = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/Issue754-avionics_hub.dsn"),
    )
    .unwrap();
    Pcb::parse(&String::from_utf8_lossy(&text)).unwrap()
}

/// FNV-1a over `id type layers nets` of every item, highest id first.
fn id_sequence_hash(pcb: &Pcb) -> (usize, u64) {
    let BoardReadResult::Success { board, .. } = build_board(pcb) else {
        panic!("build_board failed");
    };
    let mut h: u64 = 0xcbf29ce484222325;
    let mut n = 0;
    for item in board.items() {
        let line = format!(
            "{} {} {} {} {:?};",
            item.id().0,
            item.type_name(),
            item.first_layer(),
            item.last_layer(),
            item.net_numbers()
        );
        for b in line.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
        n += 1;
    }
    (n, h)
}

/// The tree keeps the kind, and the id sequence of the built board is the one the reader
/// produced when `wire_keepout` was still an alias of `keepout` (2,384 items; the hash was
/// taken on the tree before K06.1).
#[test]
fn issue754_item_ids_do_not_change() {
    let pcb = issue754();
    let kinds: Vec<KeepoutKind> = pcb
        .structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .map(|k| k.kind)
        .collect();
    assert_eq!(kinds, [KeepoutKind::WireKeepout]);
    assert_eq!(id_sequence_hash(&pcb), (2384, 0xa2275f6b2b485322));
    // the same tree with the area as a plain keepout builds the same ids
    let mut plain = pcb.clone();
    plain.structure.as_mut().unwrap().keepouts[0].kind = KeepoutKind::Keepout;
    assert_eq!(id_sequence_hash(&plain), (2384, 0xa2275f6b2b485322));
}

/// The Ground area is still an `AreaKind::Obstacle` and blocks a via inside it; the build
/// logs one log line per `wire_keepout`.
#[test]
fn issue754_ground_area_still_blocks_vias() {
    let BoardReadResult::Success {
        mut board,
        warnings,
    } = build_board(&issue754())
    else {
        panic!("build_board failed");
    };
    let ground = board.layer_structure.get_no("Ground");
    let area_id = board
        .items()
        .find(|i| {
            i.as_area()
                .is_some_and(|a| a.kind == AreaKind::Obstacle && a.layer() == ground)
        })
        .expect("the wire_keepout on Ground")
        .id();
    // logged, not a caller warning: the warning list stays what Java's is
    assert_eq!(
        board
            .log_messages()
            .iter()
            .filter(|w| w.starts_with("wire_keepout on layer Ground is treated as a keepout"))
            .count(),
        1
    );
    assert!(warnings.iter().all(|w| !w.contains("wire_keepout")));
    // a via of an existing padstack, in the middle of the area
    let padstack = board
        .items()
        .find_map(|i| i.as_via().map(|v| v.drill.padstack))
        .expect("the board has vias");
    let bb = board.item(area_id).unwrap().bounding_box();
    let center = IntPoint::new((bb.ll.x + bb.ur.x) / 2, (bb.ll.y + bb.ur.y) / 2);
    let via = board.insert_via(padstack, center, &[], 0, FixedState::Unfixed, true);
    let (via_item, area_item) = (board.item(via).unwrap(), board.item(area_id).unwrap());
    assert!(board.is_obstacle(via_item, area_item));
    assert!(board.is_obstacle(area_item, via_item));
}
