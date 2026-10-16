# freerouting-rs: port plan

**Goal:** a full port of Freerouting to Rust: a fast, correct routing engine plus a
cross-platform desktop GUI that runs on Windows, macOS, Linux and WSL (through WSLg).

Source: `danbrunski/freerouting` at `a773a70ec` (Java 25, Gradle; pinned at `cda540f`
until J01 moved it on 2026-10-15). About 140k lines of
production Java across ~60 packages: ~75k of routing core, ~25k of Swing GUI and
interactive editing, and the rest API, job management, analytics and settings.

## What "port" means here

The Java version is slow and buggy, so the target is better behavior, not a
bit-for-bit copy. Java is still the reference: its outputs on the fixture boards are the
regression baseline, and every place the Rust result differs has to be explained as
either a bug fixed on purpose or a bug in the port. Fixed-on-purpose differences get a
fixture and a note in `docs/DIVERGENCES.md`.

## Scope

**Engine:** DSN read, SES/rules write, board model, rules, DRC and connectivity, batch
autorouter (fanout, maze routing, ripup), optimizer (pull-tight, shove, via
optimization), scoring, and interactive editing operations (route, drag, move, shove,
delete, undo/redo).

**GUI:** board view with pan/zoom and layer visibility, interactive routing and editing,
autorouter run with live progress, DRC/incompletes/violations lists, nets, net classes,
clearance matrix, via rules, and the route/autoroute parameter dialogs. Same feature set
as the Java Swing windows (`gui.windows.board`, `gui.windows.routing`), not the same
look.

**CLI:** headless routing compatible with the Java `-de/-do/-mp/-mt/-oit` flags, plus DRC.

**Later, once the above works:** KiCad importer (`io.kicad`), REST API and MCP server
(`api.*`), job management. Analytics/telemetry is dropped. Java `.frb` snapshots
(Java serialization) are replaced by a native session format.

**License:** upstream is GPL-3.0 and this is a derivative work, so freerouting-rs is
GPL-3.0 too.

## GUI toolkit

Recommendation: **egui/eframe on wgpu.** It builds natively on Windows, macOS and Linux
from one codebase, runs under WSLg, has the panels, tables and dialogs the parameter
windows need, and its painter API (or a custom wgpu callback for large boards) handles a
PCB canvas with tens of thousands of shapes. It's the fastest route to a usable editor.

Alternatives considered: **Iced** (Elm-style, nice architecture, fewer ready-made
widgets and slower to iterate on), **Slint** (polished widgets, but the canvas story for
a CAD view is weaker), **Tauri + web canvas** (big GUI ecosystem, but puts a JS layer
between the editor and the engine and complicates the hot interactive-routing path).

## Module map (Java package → Rust)

| Java package | Lines | Rust home | Notes |
| --- | ---: | --- | --- |
| `geometry.planar` | 12.0k | `freerouting-geometry` | Shape hierarchy (`Shape → PolylineShape → TileShape → IntBox/IntOctagon/Simplex`) becomes enums plus traits. `Point` is already sealed (`IntPoint`/`RationalPoint`), so it maps to an enum directly. |
| `datastructures` | 2.3k | math helpers → geometry; ShapeTree/MinAreaTree/UndoableObjects → `freerouting` | |
| `io.specctra.parser` | 9k incl. 1.9k JFlex-generated lexer | `freerouting-dsn` | Hand-written S-expression lexer replaces JFlex. Parse to a typed AST; the board is built from the AST in the engine, not inside the parser. |
| `io.specctra` | 1.8k | `freerouting-dsn` (format) + `freerouting::io` (AST ↔ board) | |
| `io.kicad` | 1.6k | `freerouting-dsn` or its own crate (later) | |
| `rules` | 2.2k | `freerouting::rules` | |
| `board.model.*`, `board.facade`, `board.state`, `board.trace` | 12k | `freerouting::board` | See ownership notes below. |
| `board.actions` (`ForcedPadRouter`, `ForcedViaInserter`, `DrillItemMover`) and `board.optimize.TraceShover` with `ShapeTraceEntries`/`ShapeEntrySide` | 2.7k | `freerouting::shove` | Push-and-shove checks and inserts (O03, O04); the autorouter is the main caller. `ComponentItemTransform`/`MoveComponent` go to `freerouting::interactive` (I02, I04). |
| `board.searchtree` | 3.6k | `freerouting::board::search` | |
| `board.optimize` tighteners and `ViaOptimizer`; `autoroute.pipeline.BatchOptimizer` | 4.4k | `freerouting::optimize`, `::optimize::batch` | O01, O02, O05, O06 |
| `drc` | 2.6k | `freerouting::drc` | |
| `autoroute.*` | 18k | `freerouting::autoroute` | Expansion rooms/doors, drill pages, maze search, path insertion, batch pipeline. |
| `core.scoring` | 2.1k | `freerouting::scoring` | |
| `gui.interactive` (states, `Route`) | 5.3k | `freerouting::interactive` | The editing logic (dynamic route, drag, move, make-space) is engine code with no UI types, so the GUI and any future API share it. Only input mapping stays in the GUI. |
| `gui.rendering`, `gui.board` | 8.3k | `freerouting-gui::view` | Board drawing, colors, layer visibility, coordinate transform. |
| `gui.windows.*`, `gui.menus`, `gui.controls`, `gui.support` | ~15k | `freerouting-gui::panels` | Rebuilt as egui panels/dialogs rather than translated line by line. |
| `gui.workspace` | 4.5k | `freerouting-gui::session` | Session state, background routing job, progress events. |
| `settings`, `startup`, `cli` | ~6k | `freerouting::settings` + `freerouting-cli` | One serde config struct (TOML file + CLI overrides) instead of the multi-source merger. |
| `api.*`, `management` | ~10k | later: `freerouting-server` | |
| `analytics` | 2.9k | dropped | |

## Crate layout (Cargo workspace)

```
freerouting-rs/
  Cargo.toml                 # workspace
  crates/
    freerouting-geometry/    # planar geometry, exact integer/rational math
    freerouting-dsn/         # Specctra DSN/SES/rules lexer, parser, AST, writer
    freerouting/             # engine library: board, rules, drc, autoroute,
                             #   optimize, interactive editing, settings
    freerouting-cli/         # `freerouting` headless binary
    freerouting-gui/         # `freerouting-gui` desktop app (eframe/egui)
  fixtures/                  # DSN boards copied from upstream (~170)
  docs/PORT_PLAN.md
```

Dependency direction: `geometry` and `dsn` depend on nothing in the workspace,
`freerouting` on both, `cli` and `gui` only on `freerouting`. The engine never depends
on a GUI crate, which is the same rule the Java project enforces with ArchUnit, but here
the compiler enforces it.

## Engine design decisions

- **Board items live in an arena.** Java `Item` holds a back-reference to its
  `BasicBoard`, and items reference each other through the board. In Rust the board owns
  a `SlotMap<ItemId, Item>` and everything else stores `ItemId`. The class tree
  (`Item → DrillItem → Pin/Via`, `Trace → PolylineTrace`, `ObstacleArea → ConductionArea…`)
  becomes `enum ItemKind` with shared fields in `Item`.
- **Undo/snapshots.** `UndoableObjects` keeps per-object version chains. Port it as an
  arena with generation stamps; the GUI's undo/redo and the autorouter's ripup both use it.
- **Exact arithmetic.** Coordinates are bounded by `Limits.CRIT_INT = 2^25`, so a
  product of four coordinates fits in `i128`. Java uses `BigInteger` in `Line`,
  `RationalPoint`, `BigIntDirection` and friends. Use `i128` and fall back to
  `num-bigint` only where the bound can't be proven; each such site gets a test at the
  coordinate limits.
- **Determinism.** Java routing at the pin is reproducible: with the same JDK, input and
  settings it writes a byte-identical session at any machine load (J01.4). Its randomness
  is seeded: `MazeSearchEngine` reseeds its `Random` with the ripup cost before each
  connection (and its random factor never fires, because `ripupPassNo` never exceeds 1),
  the pass shuffle is seeded with `passNo * 1000003 + maxNetNumber`, and the polygon split
  and the Delaunay triangulation use a fresh seed-99 generator per call. Its cut-offs are
  work budgets (`WorkLimit`: 64,000 search-tree node visits per nominal millisecond), not
  wall-clock time; only the job timeout is wall-clock, and it only stops work. The Rust
  engine takes the same seeds, counts the same work units (B02.7, B03.7) and avoids
  hash-order-dependent iteration (`IndexMap` or sorted collections), so the same input
  and seed always give the same output.
- **Speed.** Profile against the Java benchmark set (`scripts/benchmark`) from the first
  routing milestone. Parallelism candidates: per-net maze searches on disjoint regions,
  optimizer passes, and DRC. Start single-threaded and correct, then parallelize with
  rayon where profiles say it pays.
- **Cancellation and progress.** Java's `Stoppable` and job timeout become a cancel token
  plus deadline (its `WorkLimit` budgets become work counters, never time), and routing emits progress events over a channel that both the CLI and
  GUI consume.

## Porting order and milestones

1. **Scaffold and harness.** Workspace, CI on Windows, macOS and Linux (fmt, clippy,
   test), fixtures copied over, and a Java-side script that dumps reference outputs
   (parsed board summary, DRC counts, routed SES, score, run time) for each fixture.
2. **Geometry.** Port the Java unit tests with it and add proptests for the
   integer/rational invariants.
3. **DSN/SES I/O.** Lexer, typed AST, writer. All fixtures parse; DSN → AST → DSN
   round-trips.
4. **Board model and rules.** Build the board from the AST. Item counts and net
   assignments match Java on every fixture.
5. **GUI viewer.** Open a DSN, draw the board with pan/zoom and layer toggles, show
   nets and item info. Gives a visual check for everything after this point.
6. **DRC and connectivity.** Violations and incompletes match Java (or differ for a
   documented reason); `freerouting --drc` works; the GUI lists violations and draws
   airlines.
7. **Autorouter.** Expansion rooms, drill pages, maze search, path insertion, shove,
   ripup, fanout, batch loop. GUI runs it in the background with live progress. Compare
   completion rate, via count, length, score and time with Java across the fixtures.
8. **Optimizer.** Pull-tight, shove, via optimization, optimizer passes.
9. **Interactive editing.** Dynamic route with push-and-shove, drag/move, delete,
   undo/redo, and the routing parameter dialogs.
10. **CLI parity, performance and packaging.** Java-compatible flags, parallelism,
    release builds for Windows (.msi/zip), macOS (.dmg, universal) and Linux
    (AppImage/tarball).
11. **Later:** KiCad importer, REST/MCP server.

Steps 2 and 3 can run in parallel, and GUI work (5, then the panels for 6 to 9) can run
alongside the engine once the board model exists.

## Risks

- The maze router and shove code are most of the behavior and the hardest part to get
  right; small ordering differences cascade. The reference-output harness in step 1 is
  what keeps this tractable.
- Some Java bugs will be load-bearing (results look good because of them). The
  divergence log keeps those decisions explicit.
- A PCB canvas at full board size needs a custom renderer path (wgpu batches or a
  tessellation cache), not per-frame egui shapes, once boards get large.
- Upstream keeps moving. Pin to one upstream commit and track deltas deliberately.
