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

A Java exception ends the output with `{"exception":…,"at":…}`. Where `AutorouteControl`'s
constructor throws (a board without via rules, `scoring-perfect-two-pin`), `control` and
`distance` write `{"stage":…,"exception":"NullPointerException"}` for that net and go on; the
Rust `ControlError::java_exception` gives the same name.
