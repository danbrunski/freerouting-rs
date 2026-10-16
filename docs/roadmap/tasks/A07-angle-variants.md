# A07 — 90° and any-angle variants

Opened 2026-10-15. Java has three room-graph variants, selected by the board's angle
restriction: 45° (A02), 90° (`SortedOrthogonalRoomNeighbours`, box rooms) and any-angle
(`SortedRoomNeighbours`, general convex rooms). No fixture uses 90° or any-angle
routing, so this checklist builds synthetic fixtures for them and is off the critical
path ([A00](A00-autorouter-README.md)). The any-angle neighbour set has a comparator that
is not transitive, so its contents depend on the exact comparisons
`java.util.TreeMap.put` makes. This checklist ports that comparison sequence for that
one set.

**Prerequisites:**
- A06 (the whole 45° chain, so every probe kind exists).
- B03: `complete_shape` for the 90° tree and the plain tree.
- O01 (90° pull-tight) and O02 (any-angle pull-tight) for the `insert` probe.
- Decisions D1, D6.

## What exists and what this adds

| Exists | State |
|---|---|
| A02–A06 for 45°; the `TreeKind` dispatch returns `UnsupportedAngle` for the others | Prerequisite |
| `expansion/SortedOrthogonalRoomNeighbours.java` (727 lines) | Missing |
| `expansion/SortedRoomNeighbours.java` (806 lines): `calculate` (`:95-136`), `calculateIncompleteRoomsWithEmptyNeighbours` (`:138-156`), `calculateNeighbours` (`:187-329`), `tryRemoveEdge` (`:414-507`), `calculateNewIncompleteRooms` (`:509-658`), `SortedRoomNeighbour` (`:664-805`; constructor `:697`, comparator `:719-761`, tolerance `c_dist_tolerance = 1` at `:666`) | Missing |
| `path/FoundConnectionLocatorAnyAngle.java` (455 lines; a reference comparison at `:329`) | Missing |
| `FoundConnectionLocator45Degree` handles the 90° case too (`FoundConnectionLocator.java:185-211`) | A05 |

What this adds: the two room-graph variants, a `JavaTreeSet` with `TreeMap.put`'s
comparison sequence, the any-angle locator, synthetic fixtures and their probes.

## Decisions made here

- **Synthetic fixtures** live in `fixtures/autoroute-variants/`: copies of
  `Issue026-J2_reference.dsn`, `Issue143-rpi_splitter.dsn` and `scoring-mixed-layer.dsn`
  with `(snap_angle ninety_degree)` and `(snap_angle none)` added to the structure, six
  files. Each file's first line is a comment naming its source and the one change. Java
  must load all six (check with the `load` reference mode before committing).
- **`JavaTreeSet<T, F>`** in `session/java_tree_set.rs`: a red-black tree that ports
  `java.util.TreeMap.put` and `fixAfterInsertion` (JDK 25 source, the runtime the harness
  uses), with the comparator as a closure. It makes the same comparisons, in the same
  order, against the same nodes, so a non-transitive comparator gives the same contents
  and the same iteration order. It is used only for the any-angle neighbour set; every
  other set in the router has a total order and uses `BTreeSet`.
- **The any-angle locator's reference comparison** (`resultCorner !=
  this.currentFromPoint`, `FoundConnectionLocatorAnyAngle.java:329`) uses A05's
  `Corner::{Unchanged, New}` enum.
- **Fallback if the emulation proves costly**: the operator may instead accept any-angle
  without `TreeMap` emulation as a listed divergence (A00 open questions). This checklist
  assumes the emulation.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A07.1 | `fixtures/autoroute-variants/*.dsn` | `fixtures.tsv` |
| A07.2 | `autoroute/session/neighbours90.rs` | `session/mod.rs` (dispatch) |
| A07.3 | `autoroute/session/java_tree_set.rs` | `AutorouteProbe.java` (kind `treemap`) |
| A07.4 | `autoroute/session/neighbours_any.rs` | `session/mod.rs` (dispatch) |
| A07.5 | `autoroute/path/locate_any.rs` | `path/locate.rs` (dispatch) |
| A07.6 | `crates/freerouting/tests/autoroute_probe_variants.rs`, `fixtures/reference/probe/*/` for the six boards | `autoroute/connection.rs` (remove the unsupported-angle failure) |

## Checklist

- [ ] **A07.1 — Synthetic fixtures.** The six files from the decisions. Run the Java
  `load` reference on them and commit `fixtures/reference/load/autoroute-variants/*.json`
  only if `reference_load.rs` picks them up without changes; otherwise check the load in
  the probe header.
  - **Accept:** Java loads all six (`success`); `reference_load.rs` (or the probe header)
    shows the Rust load matching.

- [ ] **A07.2 — 90° neighbours.** Port `SortedOrthogonalRoomNeighbours.java`: `calculate`
  (`:43-77`), `calculateIncompleteRoomsWithEmptyNeighbours` (`:79-112`),
  `calculateNeighbours` (`:114-208`), `removeBorderLine` (`:210-221`),
  `calculateNewIncompleteRooms` (`:223-400`), `insertIncompleteRoom` (`:402-423`),
  `tryRemoveEdge` (`:425-584`), and the comparator (`:673-725`, an exact integer order with
  an id tie-break: a `BTreeSet` with first-wins insertion is enough).
  - **Accept:** covered by A07.6.

- [ ] **A07.3 — `JavaTreeSet`.** The type from the decisions, with `insert(T) -> bool`
  (false when the comparator returns 0 for an existing node, as `TreeMap.put` keeps the
  old key) and in-order iteration.
  - **Accept:** probe kind `treemap`: 200 sequences of 50 random points from
    `new Random(13)`, inserted into a Java `TreeSet` with a comparator that treats
    distances within 1.0 as equal (the same shape as `SortedRoomNeighbour.compareTo`).
    The Rust in-order sequence is equal for every sequence, and differs from a
    `BTreeSet` on at least one (proving the test can fail).

- [ ] **A07.4 — Any-angle neighbours.** Port the `SortedRoomNeighbours` calculation:
  `calculate`, `calculateNeighbours` with the stable id sort, the neighbour constructor
  and the tolerance comparator in a `JavaTreeSet`, `tryRemoveEdge`,
  `calculateNewIncompleteRooms` and `calculateIncompleteRoomsWithEmptyNeighbours`.
  The plain-tree `complete_shape` sorts its leaves (B03).
  - **Accept:** covered by A07.6.

- [ ] **A07.5 — Any-angle locator.** Port `FoundConnectionLocatorAnyAngle.java`: the
  tangent-point corner construction and the reference comparison via the identity enum.
  - **Accept:** covered by A07.6.

- [ ] **A07.6 — Probes on the variants.** Regenerate `start-rooms`, `drills`, `maze-log`,
  `locate` and `insert` for the six synthetic boards, and remove the unsupported-angle
  failure from `route_connection`.
  - **Accept:** `variants_match_java`: all five probe kinds equal on all six boards, both
    ripup variants. The `insert` probe on the any-angle boards waits for O02 and is `[~]`
    until then.
