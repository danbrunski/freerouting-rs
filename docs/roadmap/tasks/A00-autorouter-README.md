# A00 — Autorouter

Opened 2026-10-15. The A series ports Java Freerouting's autorouter: the room-and-door
maze search that routes one connection (`autoroute.*`, about 11,300 Java lines at
`cda540f`), and the batch pipeline that runs fanout and ripup passes over the whole
board (`autoroute.pipeline`, about 3,500 lines without the optimizer). The narrative,
the design reasons and the risks are in [autorouter.md](../autorouter.md). The board
prerequisites are in [B00](B00-board-gaps-README.md). Decisions D1–D9 in
[port-completion.md](../port-completion.md) apply throughout; this series implements
them and never reopens them.

**The Java target.** Java line numbers in the A checklists are at `cda540f`, where the
code was read. Route-level parity is against `30baad7e2` ("Make routing independent of
machine load"), the commit J01 re-pins to. Its port ledger line says "Port: yes". What it
changes for this series:
- Every former `TimeLimit` is a `WorkLimit`: 64,000 units per nominal millisecond, one
  unit per search-tree node popped on the current thread (`WorkMeter.tick()` in
  `MinAreaTree.overlaps` and in the three `completeShape` traversals), and a limit is
  exceeded when `units - start > budget`. The maze search, the fanout per pin,
  pull-tight, `TraceShover`, `DrillItemMover` and `ForcedPadRouter` use it. B02.7 and B03.7
  port the meter and its counting points; this series creates the limits where Java does.
- `NetIncompletes` groups items in input order, as Rust already does, so the airline order
  and the last-mile blocker ripup are reproducible.
- New Java test `fixtures/WorkLimitDeterminismTest` (ported in A12.6).
The ledger's `de7a4fd2c` ("the GUI batch loop saves the best board") is also "Port: yes":
A08 and A13 make the board the run reports the board that is handed back and saved.
The optimizer stays aggregates-only until a later ledger line says otherwise.

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [A01](archive/A01-control-costs-and-probes.md) ✅ | `AutorouteControl`, `DestinationDistance`, the Java `getId` formulas, and the probe harness every later A checklist uses | M / low | C02, C03.6, C03.7, F02.1, B04.7 |
| [A02](archive/A02-room-graph-45.md) ✅ | 45° room graph: room and door arenas, room database, neighbour sorting, door sections, search init; `start-rooms` probe equal | L / high | A01, B02, B03, B04.7 |
| [A03](archive/A03-drill-pages.md) ✅ | Drill pages and drills; `drills` probe equal | M / medium | A02 |
| [A04](archive/A04-maze-search.md) ✅ | The maze search as one core generic over sinks: queue, expansion, layer changes, ripup, shove-room checks; `maze-log` probe equal pop for pop | L / high | A03, O03, B04, B05, B02.7, B03.7 |
| [A05](archive/A05-path-location.md) ✅ | Backtrack and 45° corner location; `locate` probe equal | M / medium | A04 |
| [A06](archive/A06-connection-insertion.md) ✅ | `FoundConnectionInserter`, `route_connection`; `insert` probe equal on one connection (rung 2) | L / high | A05, O04, O01, B05, B04, B01 |
| [A07](archive/A07-angle-variants.md) ✅ | 90° and any-angle room graphs and locator, with a `TreeMap`-faithful set; probes equal on synthetic fixtures | M–L / medium | A06, B03 (90°, plain), O02 |
| [A08](archive/A08-support-classes.md) ✅ | `BoardHistory`, `RoutingFailureLog`, `ItemRouteResult`, router events, two-level cancel token | S / low | B01, B06 for the real-board tests; none for the rest |
| [A10](archive/A10-stage-traits-and-batch-control.md) ✅ | D9 stage traits and registry; pure pass controller, item ordering and shuffle, ripup policy, fanout rules | M / medium | C02, C03.7, A08 |
| [A11](archive/A11-connection-routing-in-passes.md) ✅ | Connection router stage, strict DRC, the pass runner, the optimizer entry point; `pass` probe equal | L / high | A06, A08, A10, O05, B01, J01; P02.1 for A11.8 |
| [A12](archive/A12-fanout.md) ✅ | `RoutingBoard.fanout` and `BatchFanout` as the fanout stage; D8 plane-via tests | M / medium | A11, O05, B01 |
| [A13](A13-batch-loop-and-pipeline.md) | The batch loop with snapshots and restores, last-mile ripup, `run_pipeline`; determinism with the audit | M–L / high | A12, B01, B06, P02.1 |
| [A14](A14-board-parity-gate.md) | Rung 3: route every fixture, `ses-diff` against `fixtures/reference/route`; ported Java fixture tests | L / high | A13, J01, C05, F02.2, F02.4, F02.5 |

A09 is unused. The series map reserved A01–A08 for the search core and A10+ for the
pipeline; the gap keeps the two blocks apart.

## Order and critical path

```
A01 ─> A02 ─> A03 ─> A04 ─> A05 ─> A06 ─┬─> A11 ─> A12 ─> A13 ─> A14
                                        └─> A07 (off the critical path)
A08 ─> A10 ─> (joins at A11)
```

What each step waits for from other series:

| Before | Needs |
|---|---|
| A01 | C02, C03.6, C03.7, F02.1, B04.7 |
| A02 | B01, B02.1, B02.6, B02.8, B03 |
| A04 | O03, B04, B05, B02.7, B03.7 |
| A06 | O04, O01, B05 |
| A07 | B03.2 (plain), B03.5 (90°), O01, O02 |
| A10 | C02, C03.7 |
| A11 | O05, B01.6, J01 (and P02.1 for A11.8) |
| A13 | B01.3, B01.5, B06, P02.1 |
| A14 | J01, C05, F02.2, F02.4, F02.5 |

- The search core (A01–A06) is strictly sequential: each checklist's probe compares a
  layer that the next one builds on.
- A08 and A10 are pure logic. They can run as soon as C02 exists, in parallel with the
  search core.
- A07 was off the critical path: no fixture uses 90° or any-angle routing. Its synthetic
  boards live in `crates/freerouting/tests/data/autoroute-variants/`, outside `fixtures/`.
- Fanout is on by default in Java (`DefaultSettings.java:182`), so every board with SMD
  pins runs it before the passes. A12 therefore comes before the batch loop (A13), and
  both before the gate (A14).

## Parallel lanes and file ownership

Rust paths are under `crates/freerouting/src/` unless they start with `crates/`,
`tools/`, `fixtures/` or `docs/`. "Append" means add lines without changing others.

| Checklist | Creates | May edit |
|---|---|---|
| A01 | `autoroute/{mod.rs, control.rs, distance.rs, java_ids.rs, probe.rs}`, `tools/java-reference/probes/autoroute/{AutorouteProbe.java, generate.sh, fixtures.tsv, README.md}`, `crates/freerouting/tests/autoroute_common/mod.rs`, `crates/freerouting/tests/autoroute_probe_control.rs`, `crates/freerouting/tests/autoroute_probe_harness.rs`, `fixtures/reference/probe/{selftest,ids,connections,control,distance}/` | `lib.rs` (append `pub mod autoroute;`) |
| A02 | `autoroute/session/{mod.rs, rooms.rs, doors.rs, neighbours45.rs, init.rs, queue.rs}`, `tests/autoroute_probe_start_rooms.rs`, `fixtures/reference/probe/start-rooms/` | `autoroute/mod.rs` (append), `AutorouteProbe.java` and `fixtures.tsv` (append a kind) |
| A03 | `autoroute/session/drills.rs`, `tests/autoroute_probe_drills.rs`, `fixtures/reference/probe/drills/` | `session/mod.rs`, `AutorouteProbe.java`, `fixtures.tsv` |
| A04 | `autoroute/session/{search.rs, layers.rs, ripup.rs, shove_room.rs, sink.rs}`, `tests/autoroute_probe_maze.rs`, `tests/autoroute_search_sinks.rs`, `fixtures/reference/probe/maze-log/` | `session/{mod.rs, queue.rs}`, `crates/freerouting-geometry/src/jmath.rs` (append `next_double`), `crates/freerouting/Cargo.toml` (append a `[lints.rust]` check-cfg line) |
| A05 | `autoroute/path/{mod.rs, backtrack.rs, locate.rs, locate45.rs}`, `tests/autoroute_probe_locate.rs`, `fixtures/reference/probe/locate/` | `autoroute/mod.rs` |
| A06 | `autoroute/path/insert.rs`, `autoroute/connection.rs`, `tests/autoroute_probe_insert.rs`, `crates/freerouting/examples/route_connections.rs`, `fixtures/reference/probe/insert/` | `autoroute/mod.rs` |
| A07 | `autoroute/session/{neighbours90.rs, neighbours_any.rs, java_tree_set.rs}`, `autoroute/path/locate_any.rs`, `tests/data/autoroute-variants/`, `tests/autoroute_probe_variants.rs` | the angle dispatch in `session/mod.rs` and `path/locate.rs` |
| A08 | `autoroute/{history.rs, failure_log.rs, item_route_result.rs, events.rs}`, `tests/autoroute_history.rs` | `board/routing_board.rs` (append the `failure_log` field and its accessor only), `board/snapshot.rs` (B01.3: carry the failure log in `BoardSnapshot`) |
| A10 | `autoroute/stages.rs`, `autoroute/batch/{mod.rs, control.rs, ordering.rs, ripup_policy.rs, fanout_rules.rs, report.rs}`, `tests/autoroute_batch_control.rs` | `settings/fields.rs` (append the stage keys, A10.2), `autoroute/mod.rs` |
| A11 | `autoroute/batch/{connection.rs, strict_drc.rs, pass.rs}`, `tests/autoroute_pass.rs`, `crates/freerouting/examples/route_pass.rs`, `fixtures/reference/probe/pass/` | `batch/mod.rs` |
| A12 | `autoroute/fanout_pin.rs`, `autoroute/batch/fanout.rs`, `tests/autoroute_fanout.rs`, `fixtures/reference/probe/fanout/` | `batch/mod.rs` |
| A13 | `autoroute/batch/{batch_loop.rs, last_mile.rs, snapshot.rs}`, `autoroute/pipeline.rs`, `tests/autoroute_pipeline.rs`, `crates/freerouting/examples/route_board.rs` | `batch/mod.rs`, `autoroute/mod.rs` |
| A14 | `tests/autoroute_board_parity.rs`, `tests/autoroute_fixture_ports.rs` | `docs/DIVERGENCES.md` (append) |

Every checklist may append to `docs/DIVERGENCES.md`, and every checklist that adds a
probe kind appends it to `AutorouteProbe.java` and `fixtures.tsv`. Lanes that can run at
the same time on disjoint files: {A01..A06 in order}, {A08 then A10}, and A07 after A06 beside
A11.

Other series touch A files in three places only: P01.2 may append a statistics sink to
`autoroute/session/sink.rs`; P02.1 adds its audit call sites under `autoroute/` behind
`router.debug.audit_incremental`; O06 appends one registry entry (`freerouting-optimizer`)
to `autoroute/stages.rs`.

## Rules for this series

These add to the shared rules in [README.md](README.md).

- **The parity ladder is the acceptance backbone.**
  - Rung 1, component parity: A01–A05 compare probes bit for bit on small fixtures
    (control values, start rooms, every maze pop, drills, located paths). The tree
    fingerprints (B02) and complete-shape probes (B03) sit under them.
  - Rung 2, connection parity: A06's `insert` probe (one connection on the loaded board),
    then A11's `pass` probe and A12's `fanout` probe.
  - Rung 3, board parity: A14 against `fixtures/reference/route` after J01.
- **Java-observable orders are reproduced exactly and marked `// PARITY:`** in the
  code, with the Java file and line. Known ones:
  - the maze queue key `(sortingValue, expansionValue, door.getId(), section)` compared
    with `<`/`>`, where a full tie drops the newcomer (`MazeListElement.java:80-113`);
  - door and room ids from Java's hash formulas, with `int` overflow (A01.2);
  - the any-angle neighbour comparator, which is not transitive; it needs a set that makes
    exactly `java.util.TreeMap.put`'s comparisons (A07);
  - identity comparisons of points (`FoundConnectionLocator.java:415,432`,
    `FoundConnectionInserter`), reproduced with explicit enums, never value equality;
  - insertion order of door lists, target doors, drill pages and drills;
  - descending item-id order of every `TreeSet<Item>`, and rooms before items in mixed
    sets (B02's `TreeObject`);
  - the left-to-right order of every floating-point cost sum (D6).
- **Java quirks stay during the parity phase**, marked `// PARITY(quirk):`. Examples:
  the section-array reallocation in `ExpansionDoor.getSectionSegments`, ripped items of
  a failed attempt being ripped later, `ripupPassNo` never set, exceptions caught as
  `FAILED`. Fixes come after rung 3, each as a measured divergence.
- **Determinism (D2).**
  - Seeds: the maze search's `JavaRandom` is reseeded with `ctrl.ripup_costs` for every
    search (`MazeSearchEngine.java:86-87`); the pass shuffle uses
    `JavaRandom::new(pass_no * 1000003 + max_net_number)` (`AutoroutePassRunner.java:278`).
    `BatchAutorouter.random = new Random(0)` (`BatchAutorouter.java:142`) is never read at
    `cda540f` or `30baad7e2`, so Rust creates none (a field nobody reads would also fail
    clippy); if a later pin reads it, port it then.
  - Budgets are work counts only: B02.7's `WorkMeter` and `WorkLimit`, Java's nominal
    milliseconds times 64,000 units. A wall-clock timeout may only stop a run.
  - The maze search stops only on a full stop (`isStopRequested`), never on a
    router-only stop; the pass loop checks the router-only stop between items.
  - Every checklist that routes runs `tools/determinism-check.sh` (F02.4) in its
    acceptance.
  - From A11 on, determinism tests run with the debug-only audit of P02.1 switched on:
    after each ripup, each restore and each pass it compares the live search trees and
    the autoroute room cache with a build from scratch, checking balance and content.
    KRT once had balanced refcounts with 30,862 of 47,448 committed cells left
    unblocked. The audit itself is P02.1's.
- **Board history.** A restore rolls back exactly what Java's serialized board holds:
  items and their ids, the id generator and the failure log. Transient state resets as
  in `BasicBoard.readObject`. Use B01's snapshots, never a plain `Clone`. Every board
  replacement is announced to listeners, and the board handed back at the end is the one
  the run reports (`de7a4fd2c`).
- **One maze-search core** (A04). The search is generic over a sink trait and
  monomorphized, so unused hooks compile out. Every caller steps the same core: the
  batch router, the probes, Q-series diagnostics and the GUI's live view.
- **Probes** go through `tools/java-reference/probe.sh` (F02.1), from one Java file,
  `tools/java-reference/probes/autoroute/AutorouteProbe.java`, in package
  `app.freerouting.autoroute.maze`. It reads package-private maze state directly and
  private state elsewhere by reflection (the Java repo has no `module-info.java`).
  - Format: one JSON object per line, keys in a fixed order, integers in decimal,
    every `double` as `"0x"` + 16 hex digits of `Double.doubleToRawLongBits`. Rust
    writes the same text, so a comparison is line equality. The first line is a header
    with the probe kind, board, Java commit and format version.
  - A board whose output exceeds 1 MiB is committed as `<board>.digest`: one line per
    sampled connection with its line count and FNV-1a 64 hash. On a mismatch the Rust
    test writes its own full log under `target/probe/` and prints the `probe.sh` command
    that writes Java's, for a diff.
  - A Java exception is a record (`{"exception":…,"at":…}`); Rust reproduces it at the
    same point with an explicit error, or the test lists it as an expected mismatch
    with its reason.
  - Probes are generated from a clean worktree at `30baad7e2`, J01's pin (only there does
    `WorkMeter` exist, and the probes compare work units). Between `cda540f` and
    `30baad7e2` the search core changed only in `AutorouteEngine`'s limit field, so the
    `cda540f` line numbers in these checklists still find the code. When a later re-pin
    moves the pin, rerun `generate.sh <checkout> all` and fix what changes before any
    other A work.
- **No retained autoroute database.** Java's `maintainDatabase` mode is off unless a
  system property is set (`BatchAutorouter.java:67`). The port creates a fresh session
  per connection, as Java does by default.
- **No new dependency.** Hashes in probes are FNV-1a, written by hand on both sides.
- **Owner rules (D8)** are tested where they act: plane-net pads get their own via (A11,
  A12), no via in a pad (A12, A14), `(attach off)` honoured (A01's via masks).

## Open questions for Dan

1. **Unknown stage ids.** Java replaces an unknown `router.autorouter.algorithm` with
   `freerouting-router` and warns (`RoutingPipeline.java:140-148`); the KRT report
   proposed an error instead.
   *Recommendation:* keep Java's warn-and-default for the two Java keys
   (`router.autorouter.algorithm`, `router.optimizer.algorithm`), and make an unknown id
   in the new per-stage keys a settings error. Java never sees those keys, and a typo
   there should not silently route with the default.
2. **Tolerance for boards outside exact parity.** These are boards with Java work-limit
   cut-offs, until B02 proves tree-shape parity. (At `30baad7e2` the Java A/B found a
   limit firing only on `Issue145-smoothieboard` among its four boards.)
   *Recommendation:* per board, incomplete connections and clearance violations no
   worse than Java's, via count and total trace length within ±5 %, and no worse summed
   over the set.
3. **`ripupPassNo` is never set** (`AutorouteControl.java:192`), so the ripup random
   factor (`MazeRipupResolver.java:158-162`) is dead.
   *Recommendation:* port it exactly as it is. If you fix it in Java, follow at the next
   pin; otherwise list it as a post-parity candidate.
4. **Last-mile airline order.** Resolved by `30baad7e2`: `NetIncompletes` now keeps input
   order. *Recommendation:* A13.3 confirms with a probe that Java's and Rust's airline
   orders agree on the boards where the ripup fires; nothing else changes.
5. **J01's route JSON should record the per-pass score lines** (`Auto-routing pass #N …
   with score S (U unrouted and V violations)`). A13.7 compares them pass by pass.
   *Recommendation:* add them to J01.3's fields.
6. **Probe output in the repository.** A full `maze-log` of J2 is probably tens of MB
   (an estimate: 33 connections, two variants, thousands of pops each; not measured).
   *Recommendation:* commit full JSONL up to 1 MiB per board and digests above that, as
   the rules say.
7. **Fanout `pin_sorting_order` doc names** (`distance_to_closest_on_net`,
   `surroundings_density`) silently fall back to pin-index order in Java
   (`BatchFanout.java:796-830`).
   *Recommendation:* keep Java's behaviour through rung 3, then accept both spellings as
   a listed divergence. The default `outer_first` is unaffected.
