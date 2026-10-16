# The autorouter

Recorded 2026-10-15. This is the roadmap area for the autorouter and for the board
model work it needs first. The checklists are in [A00](tasks/A00-autorouter-README.md)
(the router) and [B00](tasks/B00-board-gaps-README.md) (the board model). Decisions
D1–D9 are in [port-completion.md](port-completion.md); this document explains how they
shape this area and does not restate them.

## What the Java autorouter does

Java Freerouting routes one connection at a time with a maze search over free-space
*rooms*, not over a grid. At `cda540f` the search core (`autoroute.*` without
`pipeline`) is about 11,300 lines, and the batch pipeline about 3,500 more.

1. **Control.** `AutorouteControl` gathers what one net needs: trace half widths per
   layer, active layers, via masks and via costs, trace costs per layer and direction,
   ripup settings and fanout fields.
2. **Rooms.** The search starts from the start items' shapes. An *incomplete room* is a
   region known to be free near a point. *Room completion* grows it to a maximal convex
   shape that touches no obstacle (`ShapeSearchTree45Degree.completeShape`). The
   completed room is sorted against its neighbours. Where a neighbour touches along an
   edge, a *door* joins the two rooms. A neighbour can be another free room, or an
   *obstacle room*: one tree shape of a foreign trace or via, which the search may rip
   up or shove aside. Gaps between neighbours become new incomplete rooms.
3. **Drills.** For layer changes the board is cut into *drill pages*. Each page computes
   candidate via locations for the current net by cutting obstacles out of the page and
   taking the centre of each convex piece.
4. **Search.** A priority queue holds door sections, keyed by an estimated total cost:
   the cost so far plus a lower bound to the destination (`DestinationDistance`). Popping
   a section completes the rooms behind it and queues their doors. Entering a foreign
   trace's room asks the ripup resolver for a cost, and the shover whether the trace can
   be pushed aside. Shoving is only checked here, never done.
5. **Locate.** From the destination door the search backtracks to the start, collecting
   the items it decided to rip, and turns the chain of door sections into trace corners
   that obey the 45° rule.
6. **Insert.** The ripped connections are removed. Then vias and traces are inserted
   with the forced insert, which shoves for real and pulls the new trace tight.
7. **Batch.** The pipeline runs a fanout pre-pass for SMD pins (on by default), then
   passes over every unrouted item. The ripup cost grows with the pass number, and the
   pipeline keeps a history of the best boards to restore when a pass makes things
   worse. Stagnation rules end the run.

## What the board must provide first

The router reads more of the board than DRC or the viewer does. These pieces are
missing in Rust today and are owned by other series:

- **Board identity (B01).** Java takes an item id for every item it constructs, even
  temporaries, and ids order every `TreeSet<Item>`. A restore brings back the ids, the id
  generator and the failure log. The batch loop compares boards by hash.
- **Search-tree parity (B02).** Rooms live in the autoroute search tree beside items,
  ordered rooms first. Room completion visits obstacles in the tree's depth-first order
  while its bound shrinks, so the tree's *shape* decides the rooms (D5). B02 ports
  Java's leaf-reuse operations, proves the shape with fingerprints, and adds the
  WorkMeter that counts tree-node visits, the change hook and the autoroute-info side
  table.
- **Room completion (B03).** `completeShape` with its 45°, 90° and plain variants.
- **Checks and picks (B04)**, **item operations and pin exits (B05)**, **scores (B06).**
- **Shove and pull-tight (O series).** The search needs the read-only shove checks (O03)
  inside its inner loop. Path insertion needs the forced insert (O04) and pull-tight
  (O01). The pull-tight after every routed connection also runs the via optimizer (O05),
  because Java passes the trace costs.
- **Settings (C02, C03).** Costs come from `RouterSettings` after
  `applyBoardSpecificOptimizations`. Load-time overrides, such as the copper-to-edge
  clearance, change the clearance matrix the router sees. The SES writer (C05) is needed
  to compare routed boards.
- **Deterministic Java (J01)** and the **harness (F02)**: `probe.sh`, `ses-diff`,
  `bench`, the determinism check and the board sets.

The B00 README holds the order and the details. The short version: B01 → B02 → B03 and
B04 come before A02; O03 before A04; O04, O01 and B05 before A06; O05 before A11.

## Why the Rust design looks the way it does

**Results, not structure (D1).** Java allocates an object per room, door, drill and queue
entry, and hangs per-item scratch data off `Item`. The port keeps one *session* per
connection: arenas of rooms, doors, target doors and drills with `u32` handles, door
lists in insertion order, section state in one flat array reset by a generation stamp,
and per-item data in a side table keyed by `ItemId`. None of that changes a result.
What stays Java-shaped is what Java's structure turns into behaviour:

- the queue key and its tie-breaks, including Java's silent drop of a fully tied entry;
- door and room ids, which are Java hash formulas with `int` overflow and decide ties;
- the order in which doors, target doors, drill pages and drills are created;
- the left-to-right order of every floating-point cost (D6);
- identity comparisons of points, which the port turns into explicit enums;
- quirks with side effects, such as `getSectionSegments` replacing a door's section array
  and losing its search state.

Each of these is marked `// PARITY:` in the code, so the later speed work (P02) knows
what it may not touch.

**One search core with sinks.** The maze search is generic over a sink trait and
monomorphized. A no-op sink compiles to nothing. Other sinks count work (the D5 units
plus per-search statistics), record why a search failed (the doors and rooms it could
not enter and the items that blocked them, for Q03 and Q05), send GUI progress events
(D7), or write the probe log. The GUI's live view steps the same core through the same
`step()` call as the router. KiCadRoutingTools learned this the hard way: its visualizer
drifted into a different search from production. A cost hook, zero by default and
compiled out, lets Q05 add congestion costs without touching the parity path.

**Stages (D9).** The batch pipeline is a set of traits with registered implementations:
fanout, net ordering, connection routing, ripup policy, pass control and the optimizer,
plus special stages (plane taps, neck-down, diagnostics, and later stages such as diff
pairs and length matching). The Freerouting implementations are the defaults and the
parity baseline. The KiCadRoutingTools ports in the X series plug in behind the same
traits, inserting through the same board API, so DRC, SES writing, undo and the GUI work
whichever algorithm ran. The connection-router trait takes a task (an item and a net),
the board, the cost parameters and a work budget. It returns an outcome with the ripped
items and, when the search failed, its failure record.

**Determinism (D2).** Java's randomness is already seeded: the maze search's `Random` is
reseeded with the ripup cost for every connection, and its random factor never fires at
`cda540f` because `ripupPassNo` stays 1. The pass shuffle is seeded with
`pass * 1000003 + maxNetNumber`. Java's `30baad7e2` (included in `a773a70ec`, the commit J01 re-pins to) turned
every wall-clock limit into a work count: 64,000 units per nominal millisecond, one unit
per search-tree node popped (in `MinAreaTree` queries and the three `completeShape`
traversals). The Rust router creates its limits at the same points with the same budgets,
and B02 and B03 count the same nodes. A limit then fires at the same point in both, as
long as the trees have the same shape. Everything else is ordered collections and ordered
merges.

## How it is verified

The verification ladder in port-completion.md maps onto the A series:

| Rung | What is compared | Checklists |
|---|---|---|
| 1. Component parity | Bit-for-bit probes on small fixtures: tree fingerprints (B02), completed room shapes (B03), control values (A01), start rooms (A02), drills (A03), every maze pop (A04), located paths (A05) | B02, B03, A01–A05 |
| 2. Connection parity | One connection inserted on the loaded board (A06); then pass 1 connection by connection (A11) and the fanout pre-pass pin by pin (A12) | A06, A11, A12 |
| 3. Board parity | Every fixture routed with J01's work bounds, `ses-diff` against `fixtures/reference/route` | A14 |

**Probes.** A Java probe at the pin and a Rust test write the same canonical lines: keys
in a fixed order, doubles as raw bits. The comparison is line equality, and a failure
shows the first differing line with the five before it. The probes share one *connection
sampler*: load the board as J01's route mode does, then take one unconnected item per
net in ascending net order and route it on the unmodified board, with ripup off and then
on. Each sampled connection is independent of insertion, so the whole search core is
checked before the shove and insert stack exists. The fixtures run from a two-pin board
to `Issue026-J2_reference` (33 connections), in the order of the autoroute research's
fixture table.

**Board parity** compares exactly on boards where Java logged no work-limit cut-off.
Boards with cut-offs join the exact set once B02 proves the tree shape matches. (At
`cda540f` Java's airline order followed identity hashes, which made the last-mile blocker
ripup irreproducible; `30baad7e2` fixed that.) Every routing acceptance runs the
determinism check:
idle against loaded, byte for byte. From A11 on it also runs with P02.1's audit, which
compares the incrementally maintained trees and room cache with a fresh build.

## Risks

- **Small order differences cascade.** One different tie-break in the queue changes the
  route, then every later connection. That is why rung 1 compares every pop, and why the
  probes run on small boards before J2.
- **Load-bearing Java behaviour.** Java catches every exception in the search and
  returns `FAILED`. A Java `NullPointerException` can decide a route. The probes record
  exceptions so Rust reproduces them on purpose rather than by accident.
- **The non-transitive any-angle comparator.** Its set contents depend on the exact
  comparisons `java.util.TreeMap` makes. A07 ports `TreeMap.put` for that one set. No
  fixture uses any-angle routing, so A07 is off the critical path.
- **Work counts depend on tree shape.** A limit fires at the same point only when the
  Rust trees pop the same nodes as Java's. Until B02 proves the shape on routed boards,
  boards with cut-offs are compared on aggregates. The probes compare the units step by
  step, so a drift shows where it starts.
- **Probe volume.** A full maze log of J2 is probably tens of megabytes (an estimate).
  Large boards commit digests and regenerate the full log on a mismatch.
- **Settings coupling.** The control values depend on settings applied at load (C03).
  A wrong edge clearance shows up as a wrong room, far from its cause. The `control`
  probe catches this first.
- **Speed during parity.** The data layout is chosen for speed from the start, but
  speed-ups that change visit order wait for P04, after rung 3.

## Facts corrected by this area

- The maze `Random` is seeded (`MazeSearchEngine.java:86-87`), and its only use is dead
  at `cda540f`. PORT_PLAN, engine.md and the reference README say otherwise; J01.6
  fixes them.
- `BatchAutorouter.random = new Random(0)` is assigned and never read, at `cda540f` and at
  `30baad7e2` (the port ledger's note that the router "uses `Random(0)`" means only that
  the field exists).
- `PlannedConnection.Segment` is used by `FoundConnectionInserter.getInstance`; only the
  rest of `PlannedConnection` is dead.
- The fanout escape filter's defaults are 2.5 mm minimum and 4.5 mm maximum
  (`DefaultSettings.java:186-187`). The 0.5 mm and 3 mm in `MazeSearchEngine` apply only
  when the settings are null.
