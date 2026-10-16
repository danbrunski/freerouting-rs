# A02 — The 45° room graph and search init

Opened 2026-10-15. Before the maze search pops anything, Java completes the start rooms,
sorts their neighbours, creates doors and new incomplete rooms, and queues the start
doors. This checklist ports that machinery for the 45° tree, which every fixture uses
(118 top-level fixtures; three state `snap_angle`, all `fortyfive_degree`). It ends with
the `start-rooms` probe: every room, door and initial queue entry equal to Java's for
every sampled connection. The 90° and any-angle variants are A07.

**Prerequisites:**
- A01 (control, ids, probe harness, connection sampler).
- B02.1 (`TreeObject` keys with rooms before items, room insert and remove, tree
  fingerprints), B02.6 (`reduce_trace_shape_at_tie_pin`, `tree_shape_on_layer`), B02.8
  (`ItemSideTable<T>`).
- B03: the room types and the 45° `complete_shape` (B03.1, B03.3, B03.4), and completion
  against trees that hold rooms (B03.6).
- B04.7: `trace_connection_shape`.
- Decisions D1, D5, D6.

## What exists and what this adds

| Exists | State |
|---|---|
| `board/search/mod.rs` `get_autoroute_tree` | Present; inserts items in descending id order like `SearchTreeManager.getAutorouteTree` |
| B02.1, B02.6, B02.8: rooms in the tree, `TreeObject` order, tie-pin reduction, the side table | Prerequisite |
| B03: `complete_shape` 45° returning candidate rooms with contained shapes | Prerequisite |
| `autoroute/maze/AutorouteEngine.java:321-700` (room database, id counter, `clear`) | Missing |
| `autoroute/expansion/*` (rooms, doors, target doors, `Sorted45DegreeRoomNeighbours` 972 lines, `SortedRoomNeighbours.insertDoorOk`) | Missing |
| `MazeSearchEngine.getInstance` and `init` (`:147-183, :991-1125`) | Missing |
| `MazeListElement` and its `compareTo` (`MazeListElement.java:80-113`) | Missing |

What this adds: `autoroute/session/` with the arenas, the room database, 45° neighbour
sorting, door geometry, search init and the queue type; the `start-rooms` probe.

## Decisions made here

- **One session per connection.** `Session<'b>` holds a borrow of the board and of the
  net's autoroute tree, the control, the arenas and the queue. Java's per-object graph
  becomes:
  - `RoomId(u32)` into `rooms: Vec<Room>`, where `Room` is
    `Incomplete { shape, contained, layer, doors }`,
    `CompleteFree { id: i32, shape, layer, doors, target_doors, net_dependent }` or
    `Obstacle { item, index, doors, doors_calculated }`;
  - `DoorId(u32)` into `doors: Vec<Door { rooms: [RoomId; 2], dimension, sections }>`;
  - `TargetDoorId(u32)` into `target_doors: Vec<TargetDoor { item, tree_entry, room,
    is_destination, section }>`;
  - per-room door lists as `Vec<DoorId>` (no new dependency).
  - Removed rooms are tombstoned, never reused within a session, so handles stay stable.
- **Door lists keep Java's `ArrayList` semantics.** Append on add; `remove(Object)`
  removes the first occurrence and shifts the rest. Iteration over a snapshot where Java
  copies the list (`MazeSearchEngine.java:573` builds a `LinkedList` copy).
- **Section state is flat.** `sections: Vec<SectionState>` indexed by a per-door offset
  range. A door's range is replaced when Java's `allocateSections` would replace the
  array, losing the old state (A02.5).
- **The complete room id is the engine counter**, `generate_room_id_no` = `++count`
  (`AutorouteEngine.java:696`), advanced on every `calculate` call, including obstacle
  rooms and edge-removal restarts. `clear` resets it to 0 (`:321-332`).
- **Obstacle rooms live in the side table.** `ItemAutorouteInfo.getExpansionRoom(index,
  tree)` (`ItemAutorouteInfo.java:54-82`) creates the obstacle room lazily and resizes the
  per-item array when the tree-shape count changed. The session owns a B02.8
  `ItemSideTable<ItemAutorouteInfo>` whose payload is this crate's: the start flag, a
  `Vec<Option<RoomId>>` of obstacle rooms, the cached `Connection` (A04.6) and the via's
  drill (A03.4).
- **Exceptions become values.** Java's `completeExpansionRoom` catches everything and
  returns an empty list (`:540-544`); `getInstance` failures are caught by
  `autorouteConnection`. Rust returns `Result<_, RoomError>`, and the caller maps an error
  exactly where Java's catch would. Each `RoomError` variant names the Java exception it
  stands for.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A02.1 | `autoroute/session/mod.rs`, `autoroute/session/rooms.rs` | `autoroute/mod.rs` (append) |
| A02.2 | — | `session/rooms.rs` |
| A02.3 | `autoroute/session/neighbours45.rs` | — |
| A02.4 | — | `session/neighbours45.rs` |
| A02.5 | `autoroute/session/doors.rs` | — |
| A02.6 | `autoroute/session/queue.rs`, `autoroute/session/init.rs` | `session/mod.rs` |
| A02.7 | `crates/freerouting/tests/autoroute_probe_start_rooms.rs`, `fixtures/reference/probe/start-rooms/` | `AutorouteProbe.java`, `fixtures.tsv` |

## Checklist

- [ ] **A02.1 — Session and arenas.** Types as in the decisions. Add
  `TreeKind { Plain, Ninety, FortyFive }` dispatch for neighbour calculation, matching
  `SortedRoomNeighbours.selectCalculationMode` (`SortedRoomNeighbours.java:80-93`);
  `Ninety` and `Plain` return `RoomError::UnsupportedAngle` until A07.
  - **Accept:** port `autoroute/expansion/SortedRoomNeighboursFactoryTest` (dispatch by
    tree kind); a unit test that door removal keeps the order of the remaining doors.

- [ ] **A02.2 — Room database.** Port `AutorouteEngine.java:355-700`:
  - `add_incomplete` (append), `first_incomplete` (first in list), `remove_incomplete`
    (`remove_all_doors`, then remove by identity);
  - `remove_complete` (`:391-430`): for each door, a dimension-1 intersection gives a new
    incomplete room on the neighbour (a `Simplex` from the opposite border line), then
    `remove_all_doors`, tree removal, list removal, drill-page invalidation (A03 fills the
    page array; leave a call it can hook);
  - `complete_expansion_room` (`:432-545`): the first door to a complete free room with
    dimension 2 gives `ignore_object` and `from_door_shape`; call B03's `complete_shape`;
    remove the incomplete room; the **first** two-dimensional candidate goes through
    `add_complete_room`; every later candidate is completed again with `complete_shape`
    (the first room may now overlap it) and added the same way;
  - `add_complete_room` (`:547-569`), `calculate_doors` (`:583-589`),
    `complete_neighbour_rooms` (`:591-620`: restart the door iteration after each
    completion; compute the doors of neighbouring obstacle rooms), `remove_all_doors`
    (`:627-642`: cascades to incomplete neighbours), `clear` (`:321-332`: remove every
    complete room from the tree **in list order**, drop both lists, reset the counter,
    clear the item autoroute data).
  - Orders that decide results: list orders, tree insert and remove order (they shape the
    tree, D5), the counter.
  - **Accept:** unit tests on a synthetic board for each operation's list effects and its
    tree inserts and removals (in order); the tree's fingerprint after `clear` is compared
    with Java's in A02.7, not with the state before the session, because removals
    recompute inner bounds and need not restore them.

- [ ] **A02.3 — 45° neighbours.** Port `Sorted45DegreeRoomNeighbours.calculate`
  (`:45-171`) and its neighbour type:
  - take a room id from the counter, create the complete room;
  - `overlapping_tree_entries(room_shape, layer)` in tree order, then a **stable** sort by
    `getId()` and then shape index (`:104-112`), so an item and a room with the same id keep
    tree order (rooms first);
  - own-net objects get target doors (`CompleteFreeSpaceExpansionRoom.calculateTargetDoors`,
    `:132-152`), in that sorted order;
  - other objects become `SortedRoomNeighbour`s (`:815-906`). The constructor sets the
    room's edge-touch flags **even when the set then drops the neighbour as a duplicate**;
    keep that side effect;
  - the comparator (`:913-970`) is an exact integer total order, so a `BTreeSet` with
    first-wins insertion gives Java's contents;
  - neighbours touching on a line get a door to the complete room or to the item's
    obstacle room, filtered by `insert_door_ok` (both forms,
    `SortedRoomNeighbours.java:331-388`); obstacle rooms get overlap doors
    (`ObstacleExpansionRoom.createOverlapDoor`, `:77-102`);
  - `remove_not_touching_border_lines` (`:173-242`).
  - **Accept:** covered by A02.7; plus a unit test that a duplicate neighbour still sets the
    edge flags.

- [ ] **A02.4 — Edge removal and new incomplete rooms.** Port `try_remove_edge_line`
  (`:315-430`: when an edge has no neighbour, remove that line, call `complete_shape` on
  the larger octagon ignoring the largest 2-D door to a complete room, and if the area
  grew, `remove_all_doors` and start over with a new id), `calculate_new_incomplete_rooms`
  (`:610-788`), the obstacle-room variants (`:254-309`, `:474-608`) and
  `insert_incomplete_room` (`:437-472`). Then the room goes into the autoroute tree.
  - Orders: neighbour iteration order, the restart rule, room creation order.
  - **Accept:** covered by A02.7.

- [ ] **A02.5 — Door geometry.** Port `ExpansionDoor` (`ExpansionDoor.java`):
  - `shape()` recomputes the intersection of the two room shapes on every call (`:43-52`).
    Cache it only if the cache is invalidated by every room shape change; in doubt,
    recompute (P02 may cache later);
  - `dimension` from the intersection; `other_room`;
  - `section_segments(offset)` (`:105-147`) with `allocate_sections` (`:193-201`):
    **when the section count differs, the section array is replaced and its occupation
    and backtrack data are lost.** Keep it, `// PARITY(quirk):`;
  - `calc_door_line_segment` (`:149-174`), `reset`;
  - `TargetItemExpansionDoor` (one section, `is_destination` from the item's start flag).
  - **Accept:** unit tests: a door whose section count changes loses its occupied flag; the
    segment endpoints on a hand-built pair of octagons equal values computed in a probe
    kind `door-sections` (100 doors of J2's first sampled connection).

- [ ] **A02.6 — Queue entry and search init.**
  - `session/queue.rs`: `MazeListElement` with `door: ExpandableRef` (door, target door,
    drill or drill page), `section`, `backtrack: Option<(ExpandableRef, section)>`,
    `expansion_value`, `sorting_value`, `next_room`, `shape_entry`, `room_ripped`,
    `adjustment`, `already_checked`, `ripup_cost`. Key: `(sorting_value,
    expansion_value, door_id, section)` compared with `<` and `>` as Java does; a full
    tie is equal, and insertion keeps the first. `-0.0` is normalized to `0.0` on entry;
    `debug_assert!` that no NaN enters. A `BTreeSet<Key>` plus a side `Vec` of payloads is
    enough here; A04 adds the fanout filter and may change the container.
  - `session/init.rs`: `MazeSearch::init` (`MazeSearchEngine.java:991-1125`) in Java's
    order: tie-pin reductions for start then destination items (B02 helper); destination
    items get `start_info = false` and join `DestinationDistance` (A01); the fanout case
    without destinations joins the board box on the first and last layer; start items get
    `start_info = true` and one incomplete room per tree shape (contained shape =
    `trace_connection_shape`, B04.7); complete each start room; for every target door of
    the completed start rooms that is not a destination door, queue an element with entry
    point = centre of gravity of (connection shape ∩ room shape), `sorting_value =
    destination_distance(point, layer)` and everything else zero.
  - The stop checks at Java's points (`:997, :1031, :1057, :1073`) test the full stop on a
    `&dyn Stoppable` (the existing trait; A08.5's `CancelToken` implements it) and the
    connection's B02.7 `WorkLimit`, as `AutorouteEngine.isStopRequested` does at
    `30baad7e2`. The limit is optional (`None` in the probes).
  - **Accept:** port `autoroute/maze/MazeListElementTest` (both tests) and add tie tests:
    by expansion value, by door id, by section, and a full tie that keeps the first.

- [ ] **A02.7 — `start-rooms` probe.** Java kind `start-rooms`: after
  `MazeSearchEngine.getInstance` for each sampled connection (both variants), print:
  - every complete room in creation order: id, layer, octagon coordinates, net-dependent
    flag (read `completeExpansionRooms` by reflection);
  - each room's doors in list order: door id, the other room's kind and id, dimension,
    door shape bounds;
  - target doors: item id, tree entry, destination flag;
  - incomplete rooms left: shape, contained shape, layer;
  - the initial queue in key order: the four key fields (doubles as bits) and the door
    kind;
  - the room id counter, and B02's autoroute-tree fingerprint after `clear()`.
  - **Accept:** `start_rooms_match_java` equal for every sampled connection on the fixture
    ladder up to and including `Issue026-J2_reference`, in both ripup variants.
