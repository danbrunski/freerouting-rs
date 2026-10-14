# freerouting-rs: port plan

Source: `danbrunski/freerouting` at `fc4bd20` (Java 25, Gradle). About 140k lines of
production Java across ~60 packages. The routing core that matters for a headless
autorouter is roughly 75k lines; the rest is GUI, REST/MCP API, analytics and job
management, which this port leaves out at first.

## Scope

**In:** DSN read, SES/rules write, board model, rules, DRC/connectivity, batch
autorouter (fanout, maze routing, ripup), optimizer (pull-tight, shove, via
optimization), and a CLI compatible with the Java `-de/-do/-mp/-mt/-oit` flags.

**Out for now:** Swing GUI (`gui.*`, ~25k lines), REST API and MCP server (`api.*`),
`management`, `analytics`, binary `.frb` board snapshots (Java serialization), the KiCad
importer (`io.kicad`), and the multi-source settings merger (replaced by one TOML/CLI
config struct).

**License:** upstream is GPL-3.0 and this is a derivative work, so freerouting-rs must
be GPL-3.0 too.

## Module map (Java package → Rust)

| Java package | Lines | Rust home | Notes |
| --- | ---: | --- | --- |
| `geometry.planar` | 12.0k | `freerouting-geometry` | Shape hierarchy (`Shape → PolylineShape → TileShape → IntBox/IntOctagon/Simplex`) becomes enums plus traits. `Point` is already sealed (`IntPoint`/`RationalPoint`), so it maps to an enum directly. |
| `datastructures` (Signum, BigIntAux, ShapeTree, MinAreaTree, Delaunay, UndoableObjects) | 2.3k | split: math helpers → geometry; trees/undo → `freerouting` | |
| `io.specctra.parser` | 9k incl. 1.9k JFlex-generated lexer | `freerouting-dsn` | Hand-written S-expression lexer replaces JFlex. Parse to a typed AST; do not build the board inside the parser (Java does via callbacks). |
| `io.specctra` (DsnReader/Writer, SesReader/Writer, Rules*) | 1.8k | `freerouting-dsn` (format) + `freerouting::io` (AST ↔ board) | |
| `rules` | 2.2k | `freerouting::rules` | Nets, net classes, clearance matrix, via rules. |
| `board.model.*`, `board.facade`, `board.state`, `board.trace`, `board.actions` | 13k | `freerouting::board` | See ownership notes below. |
| `board.searchtree` | 3.6k | `freerouting::board::search` | ShapeSearchTree over the board arena. |
| `board.optimize` | 4.0k | `freerouting::optimize` | Pull-tight, shove, via optimization. |
| `drc` | 2.6k | `freerouting::drc` | Clearance violations, incompletes, airlines. |
| `autoroute.expansion`, `.drill`, `.maze`, `.path` | 13k | `freerouting::autoroute` | Expansion rooms/doors, maze search engine, path insertion. |
| `autoroute.pipeline` | 4.9k | `freerouting::autoroute::pipeline` | BatchAutorouter, BatchFanout, BatchOptimizer, pass runner. |
| `core.scoring` | 2.1k | `freerouting::scoring` | Needed to compare results with Java. |
| `cli`, `startup`, parts of `settings` | ~2.5k | `freerouting-cli` | clap-based, same flag names. |

## Crate layout (Cargo workspace)

```
freerouting-rs/
  Cargo.toml                 # workspace
  crates/
    freerouting-geometry/    # planar geometry, exact integer/rational math
    freerouting-dsn/         # Specctra DSN/SES/rules lexer, parser, AST, writer
    freerouting/             # engine library: board, rules, drc, autoroute, optimize
    freerouting-cli/         # `freerouting` binary
  fixtures/                  # DSN boards copied from upstream (~170 boards)
  docs/PORT_PLAN.md
```

Dependency direction mirrors the Java ArchUnit rules: `geometry` depends on nothing in
the workspace, `dsn` on nothing (it only knows the file format), `freerouting` on both,
`cli` on `freerouting`. Splitting crates gets the compiler to enforce what ArchUnit
enforces in Java.

## Rust design decisions

- **Board items live in an arena.** Java `Item` holds a back-reference to its
  `BasicBoard`, and items reference each other through the board. In Rust the board owns
  a `SlotMap<ItemId, Item>` and everything else stores `ItemId`. The class tree
  (`Item → DrillItem → Pin/Via`, `Trace → PolylineTrace`, `ObstacleArea → ConductionArea…`)
  becomes `enum ItemKind` with shared fields in `Item`.
- **Undo/snapshots.** `UndoableObjects` keeps per-object version chains. Port it as an
  arena with generation stamps first; consider a persistent map (`im`) only if
  profiling says so.
- **Exact arithmetic.** Coordinates are bounded by `Limits.CRIT_INT = 2^25`, so a
  product of four coordinates fits in `i128`. Java uses `BigInteger` in `Line`,
  `RationalPoint`, `BigIntDirection` and friends. Start with `i128` and fall back to
  `num-bigint` only at call sites where the bound can't be proven; each such site gets
  a test at the coordinate limits.
- **Floating point.** Java doubles are IEEE strict, same as Rust `f64`. Bit-identical
  results are realistic as long as `Math.sin/cos/atan2` calls are matched (Java `Math`
  may differ from `StrictMath` by 1 ulp; `libm` matches `StrictMath`/fdlibm).
- **Determinism.** `MazeSearchEngine` uses an unseeded `new Random()`, and
  `AutoroutePassRunner` shuffles with a seed. For parity testing, port
  `java.util.Random`'s LCG exactly (`JavaRandom`) and add a seed option on the Java side.
  Also watch for `HashMap`/`HashSet` iteration order feeding routing order; use
  `IndexMap` or sorted collections where Java's order matters.
- **Threads.** Start single-threaded. The Java batch loop runs on a worker thread with
  cancellation (`Stoppable`, `TimeLimit`); port those as a cancel token plus deadline.

## Porting order and milestones

1. **Scaffold and harness.** Workspace, CI (fmt, clippy, test), fixtures copied over,
   and a small Java-side script that dumps golden outputs (parsed AST summary, DRC
   counts, routed SES, score) for each fixture.
2. **Geometry** (`geometry.planar` + math helpers). Port the Java unit tests with it and
   add proptests against the integer/rational invariants. Done when all ported tests pass.
3. **DSN/SES I/O** (`freerouting-dsn`). Lexer, typed AST, writer. Done when all
   fixtures parse and a DSN → AST → DSN round-trip re-parses to the same AST.
4. **Board model and rules.** Arena, items, layers, components, padstacks, nets,
   clearance matrix, search trees. Build the board from the DSN AST. Done when item
   counts and net assignments match Java on every fixture.
5. **DRC and connectivity.** Clearance violations, incompletes, airlines. Done when
   unrouted-connection and violation counts match Java's DRC on every fixture. This gives
   a useful `freerouting --drc` before any routing exists.
6. **Autorouter.** Expansion rooms and doors, drill pages, maze search, path insertion,
   trace shoving, ripup, then BatchFanout and BatchAutorouter. Done when small fixtures
   route to completion; then compare completion rate, via count, trace length and score
   with Java across the fixture set, aiming for identical SES under a fixed seed.
7. **Optimizer.** Pull-tight, shove, via optimization, BatchOptimizer passes.
8. **CLI parity and performance.** Java-compatible flags, timing vs the Java benchmark
   scripts in `scripts/benchmark`, then multithreading if worth it.

Steps 2 and 3 are independent and can run in parallel.

## Risks

- The maze router and shove code are the bulk of the behavior and the hardest to get
  bit-identical; small ordering differences cascade. The golden-output harness in step 1
  is what keeps this tractable.
- Arena IDs change how "same object" is tested; Java code that relies on reference
  identity or `equals` on items needs care.
- Upstream keeps moving (recent commits are refactors and nightly benchmarks). Pin the
  port to one upstream commit and track deltas deliberately.
