# Autoroute probes

`AutorouteProbe.java` is the Java side of the A series' probe tests
(`crates/freerouting/tests/autoroute_probe_*.rs`): one file in package
`app.freerouting.autoroute.maze`, so it reads package-private maze state directly (private
state elsewhere by reflection), with one kind per comparison. Later A checklists append a kind
and a `fixtures.tsv` row; none rewrites another's. The format and the rules are in
`docs/roadmap/tasks/A00-autorouter-README.md` ("Probes") and
`crates/freerouting/src/autoroute/probe.rs`, which writes the same text on the Rust side.

```sh
git -C ../freerouting worktree add --detach <scratch>/java-pin a773a70ec
TMPDIR=<scratch> tools/java-reference/probes/autoroute/generate.sh <scratch>/java-pin all
# one board, full output (also what a failing digest comparison prints):
TMPDIR=<scratch> tools/java-reference/probe.sh <scratch>/java-pin \
  tools/java-reference/probes/autoroute/AutorouteProbe.java distance fixtures/Issue026-J2_reference.dsn out.jsonl
```

Use a private `REFERENCE_WORK_DIR` when the shared `target/java-reference` cache is at another
commit; `probe.sh` refuses a cache built from a different commit than the worktree's HEAD.

- `fixtures.tsv`: the `ladder` row (the fixture ladder of the A series, the 17 small boards of
  the autoroute research's table) and one row per kind: its fixtures, `@ladder`, or `-` for a
  kind without a board.
- `generate.sh <checkout> <kind|all>` writes `fixtures/reference/probe/<kind>/<board>.jsonl`;
  output above 1 MiB becomes `<board>.digest` (the header, then `<section> <lines> <FNV-1a 64>`
  per section, a section starting at each `"stage"` line). Two runs are byte-identical.
- The loader is J01's `route` mode: `DefaultSettings` and `DsnFileSettings`, then
  `RoutingJobScheduler`'s load sequence (headless load, post-load wait, second merge with an
  adjacent rules file, board-specific tuning, net-class exclusions). Rust uses `prepare_job`.
  The route harness's own source (pass and time limits) changes nothing the probes print.
- The connection sampler (A01 decisions): nets from 1 to `maxNetNumber`, the first item of
  `getConnectableItems(net)` with a non-empty unconnected set and no ignored nets, at most 200
  connections, start and destination as `AutorouteConnectionRouter.route` computes them; the
  variants run ripup off, then ripup on with `startRipupCosts`.

| Kind | What | Boards | Rust test |
| --- | --- | --- | --- |
| `selftest` | fixed values through the writer (`-0.0`, `0.1`, `Double.MIN_VALUE`, `Integer.MIN_VALUE`, NaN, escapes, empty and nested lists, FNV-1a) | none | `autoroute_probe_harness.rs::probe_selftest_matches_java` |
| `connections` | the sampled connections per variant: net, item, plane flag, state, ripup values, start and destination ids | ladder | `autoroute_probe_harness.rs::probe_connections_match_java` |
| `java-ids` | the six `getId` formulas on 1,000 `new Random(7).nextInt()` inputs each, then real `ObstacleExpansionRoom`, `IncompleteFreeSpaceExpansionRoom`, `ExpansionDrill` and `DrillPage` ids on the first 200 default-tree shapes | J2 | `autoroute_probe_control.rs::java_ids_match_java` |
| `control` | every `AutorouteControl` field of every net as the batch router builds it (ripup on), then after a second `rebuildViaInfo` | ladder | `autoroute_probe_control.rs::control_matches_java` |
| `distance` | per sampled connection a `DestinationDistance` joined as `MazeSearchEngine.init` joins it (no tie-pin reduction, no rooms), at 1,000 points and 1,000 boxes from `new Random(11)` on every layer | ladder | `autoroute_probe_control.rs::distance_matches_java` |
| `start-rooms` | per sampled connection, ripup off then on, after `MazeSearchEngine.getInstance`: the complete rooms in list order with their doors and target doors, the incomplete rooms left, the maze queue in order, the room counter, and the autoroute tree's walk hash after `AutorouteEngine.clear()` (A02.7) | ladder | `autoroute_probe_start_rooms.rs::start_rooms_match_java` |
| `door-sections` | `ExpansionDoor.getSectionSegments` of a hand-built pair of doors at four offsets, then of the first 100 doors of the routable sampled connections (A02.5) | J2 | `autoroute_probe_start_rooms.rs::door_sections_match_java` |
| `drills` | per sampled connection, ripup off, ripup on, and ripup off with `attachSmd` forced on (`smd`; no fixture's via rule attaches to SMD pins), after `MazeSearchEngine.getInstance`: `DrillPage.getDrills` on every page of the engine's page array in row-major order, each page's id, bounds and drills (id, location, shape, layers, the room id per layer), then the room counter and the tree's walk hash after `clear()` (A03.5) | `scoring-mixed-layer`, `Issue508-SMD-routing-issue-demo`, `Issue143-rpi_splitter`, `Issue143-rpi_splitter_mod`, `Issue508-DAC2020_bm08`, J2 | `autoroute_probe_drills.rs::drills_match_java` |
| `maze-log` | per sampled connection, ripup off then on: `MazeSearchEngine.getInstance`, then `occupyNextElement` call by call (at most 200,000): before each call the element it expands (the first queued one whose section is unoccupied) with door kind and id, section, key bits, next room, backtrack door and flags; the occupied ones popped before it, whether its section is occupied after the call, the WorkMeter units since the search was created; at the end the destination, room counter, queue size and units, then the tree's walk hash after `clear()`. No `WorkLimit` (A04.8) | ladder | `autoroute_probe_maze.rs::maze_log_matches_java` |
| `maze-routed` | `maze-log` through pre-routed traces: the board's traces and vias unfixed (`Item.unfix`), then for up to 12 nets with route items the net's route items removed in item order and its sampled connection searched as in `maze-log`; the removed items stay removed. Exercises ripup and the shove checks of obstacle trace rooms (A04.6, A04.7) | `Issue413-test`, `Issue957-smoke-ldo-led`, `Issue676-ch32v-tx118s`, `Issue069-TestSensel/TestSensel`, `Issue027-zMRETestFixture`, `Issue313-FastTest`, `Issue229-display-8-digit-hc595` | `autoroute_probe_maze.rs::maze_routed_matches_java` |
| `locate` | per sampled connection, ripup off then on: `MazeSearchEngine.getInstance`, `findConnection`, then `FoundConnectionLocator.getInstance` with a `TreeSet` of ripped items and a `LinkedHashMap` of costs, as `autorouteConnection` calls it: the backtrack array (door kind and id, section, next room), the result items (corners as integer points, layer), the ripped ids in set order with their costs in map order, the start and target items and layers; `{"result":"none"}` for a failed search; then the tree's walk hash after `clear()` (A05.5) | ladder | `autoroute_probe_locate.rs::locate_matches_java` |
| `locate-routed` | `locate` through pre-routed traces, walked as `maze-routed` walks them; compares the ripped items and their costs (A05.1, A05.5) | the `maze-routed` boards | `autoroute_probe_locate.rs::locate_routed_matches_java` |
| `insert` | per sampled connection, on a fresh load of the board: the control as the batch router builds it in pass 1 (ripup on, the start ripup costs) and `AutorouteEngine.autorouteConnection` on a new engine with the router's maze-search `WorkLimit`: the state and details, the ripped ids and costs, every item changed (same id, other geometry), added (traces: nets, layer, half width, class, fixed state, corners; vias: nets, padstack, centre, class, fixed state) and removed in id order, `maxGeneratedId`, the `BoardStatistics` deltas (incomplete connections, vias, total trace length before and after) and the `WorkMeter` units of the call (A06.7) | ladder | `autoroute_probe_insert.rs::insert_matches_java` |
| `insert-routed` | `insert` through pre-routed traces, walked as `maze-routed` walks them on one board (each connection keeps what it inserted and ripped); compares the rip of `ripConnections` (A06.5, A06.7) | the `maze-routed` boards | `autoroute_probe_insert.rs::insert_routed_matches_java` |
| `pass` | passes 1 and 2 of a `BatchAutorouter(job)` on the loaded board (no fanout), replicating `runSingleThread`'s item loop around the real `autorouteItem` with Java's `getAutorouteItems` and `reorderSingleThreadItems` (reflection): per pass the item list; per connection item, net, state, ripped ids, `maxGeneratedId` before and after, the count and FNV-1a 64 of the added traces and vias, the FNV-1a 64 of all traces and vias, the failure count and the `WorkMeter` units from before `autorouteItem` to after the failure handling; per pass the tail removal (result, units), the routing ledger's incomplete count and nets, the result and the board hash; then whether the real `autoroutePass(1)` and `(2)` on a second load left the replica's boards (A11.7) | `connection-parity.txt` and J2 | `autoroute_pass.rs::pass_matches_java` |
| `treemap` | 200 sequences of 50 points (x, y from `nextInt(32)` of one `new Random(13)`) added to a `TreeSet` whose comparator is not transitive (distances within 1.0 fall through to y, then x): per sequence the points, each `add` result and the iteration order (A07.3) | none | `autoroute_probe_variants.rs::treemap_matches_java` |
| `start-rooms`, `drills`, `maze-log`, `locate`, `insert` on the variant boards | the five kinds above on the 90 degree and any-angle copies of J2, `Issue143-rpi_splitter` and `scoring-mixed-layer` listed in `variants.txt` (under `crates/freerouting/tests/data/autoroute-variants/`, outside `fixtures/`), written by `generate-variants.sh <checkout> [kind...]` to `fixtures/reference/probe/<kind>/crates/freerouting/tests/data/autoroute-variants/` (A07.6) | `variants.txt` | `autoroute_probe_variants.rs::variants_match_java` |

A Java exception ends the output with `{"exception":…,"at":…}`. Where `AutorouteControl`'s
constructor throws (a board without via rules, `scoring-perfect-two-pin`), `control` and
`distance` write `{"stage":…,"exception":"NullPointerException"}` for that net and go on; the
Rust `ControlError::java_exception` gives the same name.
