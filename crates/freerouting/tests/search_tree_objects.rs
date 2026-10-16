//! Rooms as search-tree objects and tree fingerprints (B02.1).
//!
//! Java's trees hold `SearchTreeObject`s: items and the autorouter's
//! `CompleteFreeSpaceExpansionRoom`s. `ShapeTree.Leaf.compareTo` orders the
//! leaves by object (rooms before items, each by id descending), then by
//! shape index.

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::{ObjectEntry, RoomId, ShapeSearchTree, TreeKind, TreeObject};
use freerouting::board::structure::{FixedState, Layer, LayerStructure};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{IntBox, IntPoint, Point, Polyline, PolylineShape, TileShape};
use freerouting::rules::{BoardRules, ClearanceMatrix};

fn board() -> RoutingBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", true), Layer::new("Bottom", true)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 1_000_000, 1_000_000);
    let mut board = RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Default::default(),
    );
    let class = board.rules.default_net_class();
    board.rules.nets.add("A", 1, false, class);
    board
}

fn trace(board: &mut RoutingBoard, corners: &[(i32, i32)]) -> ItemId {
    let points: Vec<Point> = corners
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
        .collect();
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&points),
            0,
            1_000,
            &[1],
            0,
            FixedState::Unfixed,
        )
        .expect("trace inserted")
}

fn bx(x1: i32, y1: i32, x2: i32, y2: i32) -> TileShape {
    TileShape::IntBox(IntBox::new(x1, y1, x2, y2))
}

#[test]
fn overlap_order_rooms_then_items_descending() {
    let mut board = board();
    // Items up to id 9; only 5 and 9 lie at the query, 9 with two shapes.
    let mut ids = Vec::new();
    let mut i = 0;
    while board.communication.id_generator.max_generated_id() < 9 {
        i += 1;
        let id = match board.communication.id_generator.max_generated_id() + 1 {
            5 => trace(&mut board, &[(10_000, 10_000), (20_000, 10_000)]),
            9 => trace(
                &mut board,
                &[(12_000, 5_000), (12_000, 12_000), (18_000, 12_000)],
            ),
            _ => trace(
                &mut board,
                &[
                    (500_000 + i * 10_000, 900_000),
                    (500_000 + i * 10_000, 950_000),
                ],
            ),
        };
        ids.push(id);
    }
    assert!(ids.contains(&ItemId(5)) && ids.contains(&ItemId(9)));
    let ctx = board.shape_context();
    let mut tree = ShapeSearchTree::new(TreeKind::Plain, 0);
    tree.insert_room(RoomId(3), bx(0, 0, 30_000, 30_000), 0);
    for id in &ids {
        tree.insert(board.item(*id).expect("item"), &ctx);
    }
    tree.insert_room(RoomId(7), bx(11_000, 9_000, 19_000, 13_000), 0);
    // A room on the other layer is filtered out by layer.
    tree.insert_room(RoomId(8), bx(11_000, 9_000, 19_000, 13_000), 1);

    let query = bx(11_000, 9_000, 19_000, 13_000);
    let mut found = Vec::new();
    tree.overlapping_object_entries(&query, 0, &[1], board.item_list(), &mut found);
    let expected = vec![
        ObjectEntry {
            object: TreeObject::Room(RoomId(7)),
            shape_index: 0,
        },
        ObjectEntry {
            object: TreeObject::Room(RoomId(3)),
            shape_index: 0,
        },
        ObjectEntry {
            object: TreeObject::Item(ItemId(9)),
            shape_index: 0,
        },
        ObjectEntry {
            object: TreeObject::Item(ItemId(9)),
            shape_index: 1,
        },
        ObjectEntry {
            object: TreeObject::Item(ItemId(5)),
            shape_index: 0,
        },
    ];
    // Rooms pass every net filter; the items are of net 1, which is
    // ignored, so only the rooms remain.
    let rooms: Vec<_> = expected[..2].to_vec();
    assert_eq!(found, rooms);

    found.clear();
    tree.overlapping_object_entries(&query, 0, &[], board.item_list(), &mut found);
    assert_eq!(found, expected);

    let objects: Vec<TreeObject> = tree
        .overlapping_objects(&query, -1, &[], board.item_list())
        .into_iter()
        .collect();
    assert_eq!(
        objects,
        vec![
            TreeObject::Room(RoomId(8)),
            TreeObject::Room(RoomId(7)),
            TreeObject::Room(RoomId(3)),
            TreeObject::Item(ItemId(9)),
            TreeObject::Item(ItemId(5)),
        ]
    );

    // The item-only query skips the rooms.
    let mut items = Vec::new();
    tree.overlapping_tree_entries(&query, 0, &[], board.item_list(), &mut items);
    let item_ids: Vec<(i32, usize)> = items.iter().map(|e| (e.item.0, e.shape_index)).collect();
    assert_eq!(item_ids, vec![(9, 0), (9, 1), (5, 0)]);

    tree.remove_room(RoomId(7));
    assert!(tree.room(RoomId(7)).is_none());
    assert_eq!(tree.room_count(), 2);
}

/// Five rooms in a 90 degree tree (box bounds), inserted in a fixed order.
/// The expected walk follows `MinAreaTree.positionLocate` by hand: each new
/// leaf goes down the child whose area grows least (ties to the first).
fn hand_built(order: &[usize]) -> ShapeSearchTree {
    let rooms = [
        (1, bx(0, 0, 10, 10)),
        (2, bx(100, 0, 110, 10)),
        (3, bx(0, 100, 10, 110)),
        (4, bx(5, 5, 20, 20)),
        (5, bx(100, 100, 110, 110)),
    ];
    let mut tree = ShapeSearchTree::new(TreeKind::NinetyDegree, 1);
    for &i in order {
        let (id, shape) = &rooms[i];
        tree.insert_room(RoomId(*id), shape.clone(), 0);
    }
    tree
}

#[test]
fn fingerprint_of_hand_built_tree() {
    let tree = hand_built(&[0, 1, 2, 3, 4]);
    let walk = tree.walk_lines();
    let expected = "\
0 I B 0 0 110 110
1 I B 0 0 20 110
2 I B 0 0 20 20
3 L B 0 0 10 10 R 1 0
3 L B 5 5 20 20 R 4 0
2 L B 0 100 10 110 R 3 0
1 I B 100 0 110 110
2 L B 100 0 110 10 R 2 0
2 L B 100 100 110 110 R 5 0
";
    let text: String = walk.iter().map(|l| format!("{l}\n")).collect();
    assert_eq!(text, expected);
    let fingerprint = tree.fingerprint();
    assert_eq!(fingerprint.nodes, 9);
    assert_eq!(fingerprint.leaves, 5);
    assert_eq!(fingerprint.max_depth, 3);
    assert_eq!(fingerprint.shape_hash, fnv1a64(expected));
}

#[test]
fn content_digest_ignores_insertion_order() {
    let a = hand_built(&[0, 1, 2, 3, 4]);
    let b = hand_built(&[4, 3, 2, 1, 0]);
    assert_ne!(a.fingerprint().shape_hash, b.fingerprint().shape_hash);
    assert_eq!(a.content_digest(), b.content_digest());
    let expected = "\
R 5 0 B 100 100 110 110
R 4 0 B 5 5 20 20
R 3 0 B 0 100 10 110
R 2 0 B 100 0 110 10
R 1 0 B 0 0 10 10
";
    assert_eq!(a.content_digest(), fnv1a64(expected));
}

fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// `ShapeSearchTree.reuseEntriesAfterCutout` (`ShapeSearchTree.java:362-405`)
/// on a trace of 7 lines (5 shapes) cut into a start piece of 5 lines and an
/// end piece of 5 lines (3 shapes each). Java's sequence: the start piece
/// takes `fromTraceEntries[0..1]` as its shapes 0 and 1 and inserts its shape
/// 2; the end piece inserts its shape 0 and takes `fromTraceEntries[3..4]`
/// as its shapes 1 and 2. Only `fromTraceEntries[2]` is left for the
/// removal of the old trace.
#[test]
fn reuse_after_cutout_leaf_sequence() {
    let mut board = board();
    let from = trace(
        &mut board,
        &[
            (0, 0),
            (10_000, 0),
            (10_000, 10_000),
            (20_000, 10_000),
            (20_000, 20_000),
            (30_000, 20_000),
        ],
    );
    let start = trace(
        &mut board,
        &[(0, 0), (10_000, 0), (10_000, 10_000), (15_000, 10_000)],
    );
    let end = trace(
        &mut board,
        &[
            (17_000, 10_000),
            (20_000, 10_000),
            (20_000, 20_000),
            (30_000, 20_000),
        ],
    );
    let ctx = board.shape_context();
    let mut tree = ShapeSearchTree::new(TreeKind::Plain, 0);
    tree.insert(board.item(from).expect("from"), &ctx);
    let labels = |tree: &ShapeSearchTree| {
        let shape_tree = tree.shape_tree();
        let mut labels: Vec<_> = shape_tree
            .to_array()
            .into_iter()
            .map(|leaf| (leaf, shape_tree.leaf(leaf)))
            .collect();
        labels.sort();
        labels
    };
    let before = labels(&tree);
    assert_eq!(before.len(), 5);
    let leaf_of = |k: usize| {
        before
            .iter()
            .find(|(_, (_, index))| *index == k)
            .map(|(leaf, _)| *leaf)
            .expect("leaf")
    };
    assert!(tree.reuse_entries_after_cutout(
        from,
        board.item(start).expect("start"),
        board.item(end).expect("end"),
        &ctx,
    ));
    let after = labels(&tree);
    let label = |leaf| after.iter().find(|(l, _)| *l == leaf).map(|(_, v)| *v);
    let (f, s, e) = (
        TreeObject::Item(from),
        TreeObject::Item(start),
        TreeObject::Item(end),
    );
    // The kept leaves are the same leaves, relabelled.
    assert_eq!(label(leaf_of(0)), Some((s, 0)));
    assert_eq!(label(leaf_of(1)), Some((s, 1)));
    assert_eq!(label(leaf_of(2)), Some((f, 2)));
    assert_eq!(label(leaf_of(3)), Some((e, 1)));
    assert_eq!(label(leaf_of(4)), Some((e, 2)));
    // Two new leaves: the start piece's last shape and the end piece's
    // first.
    let new: Vec<_> = after
        .iter()
        .filter(|(leaf, _)| !before.iter().any(|(b, _)| b == leaf))
        .map(|(_, v)| *v)
        .collect();
    assert_eq!(new.len(), 2);
    assert!(new.contains(&(s, 2)) && new.contains(&(e, 0)));
    // Removing the old trace removes only its middle leaf.
    tree.remove(from);
    let mut last: Vec<_> = labels(&tree).into_iter().map(|(_, v)| v).collect();
    last.sort();
    let mut expected = vec![(s, 0), (s, 1), (s, 2), (e, 0), (e, 1), (e, 2)];
    expected.sort();
    assert_eq!(last, expected);
}
