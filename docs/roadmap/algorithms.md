# Algorithms from KiCadRoutingTools

Recorded 2026-10-15. This is the plan for decision D9's second source of
algorithms: [KiCadRoutingTools](https://github.com/drandyhaas/KiCadRoutingTools)
(KRT, MIT, at `../KiCadRoutingTools`, read at commit `0486e799`, version
0.23.0). The checklists are the
[X series](tasks/X00-krt-algorithms-README.md). The source is the KRT report
in the roadmap research (sections 10 and 11 and "Proposed roadmap changes").

## What KRT is, and what that means for porting

KRT is a grid router with a Rust core. The core (`rust_router/`, crate
`grid_router` 0.23.0) is 4,777 lines: an obstacle raster
(`obstacle_map.rs`), an octilinear multi-source multi-target A*
(`router.rs`), a pose A* with a Dubins heuristic for differential-pair
centrelines (`pose_router.rs`, `dubins.rs`), and shared types. Everything else
is Python: about 176,000 lines of router orchestration, I/O, planes and
diagnostics in `py_router/`. The Python builds and maintains the raster,
inflates every obstacle by clearance, connects grid paths to off-grid pads,
and turns paths into copper.

So the Rust crate is about 3 % of the router. Vendoring it gives a grid search
and nothing else. Every KRT algorithm except that search is Python and has to
be rewritten in Rust against freerouting-rs's board model. The Python is the
specification; nothing of it runs in freerouting-rs.

KRT also lacks things Freerouting has: push-and-shove, blind and buried vias,
and a gridless shape-based model with exact clearance geometry. Freerouting
lacks things KRT has: differential pairs, length and time matching, neck-down
reporting, plane region repair, routing diagnostics, and deep BGA and QFN
escape planning. Decision D9 takes both.

## How KRT algorithms plug in

D9 makes a routing job a pipeline of stages. The A series defines the stage
traits and the registry (A10), with Freerouting's implementations as the first
ones and the defaults. The X series adds KRT-derived implementations behind
the same traits. They are selected by the settings keys A10 defines:
`router.autorouter.algorithm` for a bundle, and per-stage keys such as
`router.autorouter.connection_router`, `router.fanout.algorithm` and
`router.special.*`.

Every implementation works on the one board model and inserts traces and vias
through the board API. DRC, undo, SES writing, the GUI and the plugin then
work the same whichever algorithm ran. Because the Freerouting implementation
is never edited, results with default settings stay byte-identical by
construction.

| KRT algorithm (source at `0486e799`) | Stage | Where | Rust size | Determinism hazard |
|---|---|---|---|---|
| Grid octilinear A* (`router.rs` `GridRouter::route_multi`, `obstacle_map.rs`) | connection routing | X03 vendor, X04 raster, X05 stage, X06 with pull-tight | 4.8k vendored + 3–5k new | `f32` heuristic weight, `f64` capsule test; FxHash is seedless |
| Plane-tap verification, region repair (`plane_pad_tap.py`, `repair_planes.py`, `plane_region_connector.py`) | plane taps and repair | X02 | 2–3k | MST ties by island index; one fill source only (KRT #675) |
| Trombone meanders, time matching (`length_matching.py`, `impedance.py`) | length matching | X07 | 3–5k | KRT iterates sets of net names; use board order |
| Pose A* + Dubins, P/N offset, coupled vias (`pose_router.rs`, `diff_pair_*.py`) | differential pairs | X08, X09 | 6–10k | `sin`/`cos`/`atan2` may differ by one ulp across libms (D6) |
| QFN escape (`py_router/qfn_fanout/`) | fanout | X10 | about 1.5k | none notable |
| Bus detection and attraction (`bus_detection.py`, `bus_corridor.py`) | ordering + cost term | X11 | 1.5–3k | Python `set` over net ids; use `BTreeSet`, ties by net number |
| BGA escape planner (`py_router/bga_fanout/`) | fanout | X12 | 6–10k | about 100 `set(...)` uses to replace with ordered collections |
| Widest-path feasibility (`py_placer/placement/reachability.py`) | diagnostics | Q03 | about 500 | stable sort on `(slack, index)` |
| Blocker attribution (`blocking_analysis.py`) | diagnostics | Q06 | about 300 | sort `(rank key, net number)` |
| History and ripped-corridor costs (`history_congestion.py`, `obstacle_costs.py`) | ripup policy | Q05 | about 400 | integer accumulation on a fixed-origin lattice |
| Conflict-round and inside-out ordering (`net_queries.py`, `net_ordering.py`) | task ordering | Q07 | about 360 | tie-break `(conflicts, length, net number)` |
| Pour guard (`pour_guard.py`) | obstacle rule | Q01 | 300–500 | per net from the input board, not from run scope |
| Neck-down width policy and widen-back (`single_ended_routing.py`) | neck-down | Q02 | 1.5–2.5k | split at exact integer fractions |
| Earned budget extension (#529, `GridSearch::extend_budget`) | connection budget | Q08 | about 100 | pure search state |

The split between Q and X: a KRT idea that changes how one of Freerouting's
own stages behaves (ordering, ripup, neck-down, the plane obstacle rule,
diagnostics) is a Q item, measured as a quality change. A KRT implementation
that is a new stage or a replacement stage is an X item.

Not planned: plane pour creation (KiCad users pour in KiCad, and the DSN
carries the planes), placement (`py_placer/`, about 84,000 lines; KRT's own
results show from-scratch placement fails), `lane_search.rs` (only meaningful
with KRT's experimental `awx/` planner), stub layer swaps, and KRT's N+1
blocker ripup ladder unless Q05 leaves congestion failures.

## What the board model must provide

KRT's grid gives its stages services for free. On a shape-based model each one
is a deliberate API. These belong to other series, and the X checklists name
them as prerequisites:

| Service | Used by | Owner |
|---|---|---|
| Failure record of a maze search: blocked doors and rooms with the blocking `ItemId`s, sorted `(net, item id)`, capped | Q05, Q06 | A04 (maze search) |
| Cost hook in maze expansion, `extra_cost(layer, door or room)`, zero and not consulted in the parity path | Q01 experiment, Q05, X11 | A04 |
| Width policy in maze expansion, `half_width(room, layer)`, Java's room-local neck-down as the default; per-segment widths in insertion | Q02, X08 | A04, A06 |
| Budget hook when a work limit fires (`Stop` or `Extend`) | Q08 | A04 |
| Per-net obstacle predicate for conduction areas, `None` in the parity path | Q01 | B09 |
| Clearance-aware distance from a point to an item, per net class and layer | Q03, X04 | nothing new: `tile_shape`, `Shape::distance` and the clearance matrix exist |
| Own-net connected components over pins, traces, vias and areas | X02 | the existing incompletes code (`drc::incompletes`), exposed by B09 |
| Insert → exact DRC → commit or revert, as one board transaction | Q02, X02, X05, X07–X12 | B09, on B01's snapshots |
| Snapshot and restore of wiring | Q04, Q05 | B01 |
| Rules the DSN lacks: pair membership and gap, stackup εr and thickness, pad-to-die length | X07–X09 | K05, K07 (the `.kicad_pcb` reader) |

The fill model KRT needed for planes already exists in Rust as Java's detailed
fill (`drc::zone_islands::fill_islands`). It lacks KiCad's minimum-thickness
opening; Q01 measures whether that matters before anyone adds it.

## What gets vendored, and what gets written

**Vendored: the grid search only (X03).** `rust_router/src/{types,
obstacle_map, router, dubins, pose_router}.rs` go into their own crate,
`crates/krt-grid-router`, with `license = "MIT"` and KRT's `LICENSE` beside
them. The changes are limited to removing the Python binding:
- the pyo3 attributes on three classes and the module;
- numpy array parameters replaced by slices;
- a stats struct instead of a `HashMap<String, f64>`;
- the `#[global_allocator]` mimalloc line in `lib.rs` removed, because a
  library must not choose the binary's allocator (the allocator trial is P05);
- `lane_search.rs` and `identify_blocking_obstacles` left out.

That is 200–400 changed lines, committed as a diff against `0486e799` so
upstream fixes can be re-vendored. `rustc-hash` stays as a `NEW DEP`; pyo3,
numpy and mimalloc go.

**Written new: the glue (X04, X05), about 3–5k lines of Rust,** in a GPL
crate `crates/freerouting-krt` that depends on the vendored one:
- board items → obstacle raster: each foreign item's shape inflated by
  clearance and the routed half-width, scan-converted conservatively (a move
  between two free cells can never come within clearance), rebuilt per
  connection in a window. KRT's refcounted per-item stamps come only if
  measurement shows the rebuild is slow, and then with an audit against a
  rebuild: KRT once had balanced refcounts while 30,862 of 47,448 committed
  cells were unblocked;
- exact pin connection: grid ends joined to the pins' exact connection
  points, as KRT's `_merge_terminal_to_exact` does;
- path → copper through the board API: one trace per run of equal layer and
  width, a via with the net's padstack at each layer change;
- the exact DRC before insert. The raster is an approximation; the board's
  own clearance check is the judge, and a failing path is rejected, never
  inserted.

**Rewritten from Python: everything else in the table above,** in
`crates/freerouting-krt` (X stages) or in the engine's `quality` module
(Q items).

## Grid A* followed by pull-tight

KRT's paths are octilinear, the same as Freerouting's 45° trace style, so
Freerouting's pull-tight (O01, Java `TraceTightener45`) can take them as
input. The combination "grid A* path, then pull-tight and the via optimizer"
is plausible and is the first grid experiment (X06). Its limits are known in
advance:
- pull-tight only accepts moves that keep clearance, so it cannot repair a
  violation: every inserted path must already pass the exact DRC;
- vias come out on grid points; moving them is the via optimizer's job
  (O05, Java `ViaOptimizer`), not pull-tight's;
- a conservative raster at 0.1 mm loses corridors that the gridless search
  keeps. **Expect the grid router to lose in fine-pitch pin fields** and to do
  better, if anywhere, on open runs. X06 measures this split explicitly. A
  hybrid (grid for open runs, the room/door search near pins) is recorded as
  a follow-up, not built.

Grade after the optimizer, because the conservative raster leaves slack that
pull-tight reclaims.

## Licensing

KRT is MIT ("Copyright (c) 2026 drandyhaas"); freerouting-rs is GPL-3.0-only.
MIT code may be copied or translated into a GPL-3.0 work provided the
copyright and permission notice is kept. Nothing has to go back to KRT. GPL
code can't flow into KRT under MIT without the authors' consent.

- The MIT text lives in `LICENSES/MIT-KiCadRoutingTools.txt` (D9). F04 adds
  it, with `THIRD_PARTY_NOTICES.md`, `docs/PROVENANCE.md`, SPDX headers and a
  CI check, **before the first ported line**.
- The vendored crate keeps `license = "MIT"` and its own `LICENSE`.
- Every Rust file that carries KRT code structure names its KRT source path,
  symbol and commit `0486e799` in its header and has a row in
  `docs/PROVENANCE.md`.
- A file written only from a description of an idea gets a one-line credit in
  its module docs ("after KRT `<file>` `<function>`, MIT"), not the notice.
- Release archives and the plugin package include the notices, because
  binaries are copies too (R series).
- **KRT's boards are not MIT.** Its stress corpus (479 GitHub boards over 33
  sets) is mostly pinned to a branch rather than a commit (31 of 479 pinned),
  unhashed, and its licences are mostly recorded as "see repo". The boards in
  KRT's `kicad_files/` come from other projects whose licences KRT's MIT
  licence doesn't cover. None of them enter this repository. A board-list
  file with URLs, pinned commits, sha256 and SPDX licences is fine; that is
  F03.3's out-of-repo corpus.

## How it is verified

Every X implementation passes the same gates as every other stage (D9):
- deterministic (D2): ordered collections, ties broken by net, pad or item
  number, budgets counted in work, proven with `tools/determinism-check.sh`;
- no new clearance violations and no via in a pad (D8), checked by the exact
  DRC and a via-in-pad check on every routed board;
- results with default settings byte-identical to the Freerouting pipeline
  (`ses-diff` on `connection-parity` and `benchmark`);
- it becomes a default only by the measurement rules in
  [routing-quality.md](routing-quality.md), graded by the KiCad oracle
  (F02.6) where the board has a `.kicad_pcb`.

There is no Java result to be at parity with, so every X stage is measured as
a divergence and listed in DIVERGENCES.md if it becomes a default.

**X01 comes first and sets the order.** It routes `canary-pwr` and four KiCad
demo boards with the fork's Java jar and with KRT's own routing chain, and
grades both with KiCad's DRC: unrouted, DRC violations (with the input
board's DRC subtracted), vias, length and time. It also sorts the connections
one engine routes and the other doesn't by cause: wide nets at fine-pitch
pins, plane nets, dense escapes, other. The default order (D9: plane handling
and diagnostics first, then the grid router, then length matching and
differential pairs, then escape planners) changes only by X01's preregistered
decision table. X01 runs now; it needs KRT built in its own virtual
environment and nothing from the port.

## Risks

- **The glue is the work.** Vendoring is a day; the raster builder, terminal
  connectors and path conversion are 3–5k lines and carry all the bugs KRT
  fixed over 0.17–0.23 (grazes at 45° moves, stale stamps, off-grid pins).
- **A conservative raster wastes completion** where pitch is tight, exactly
  where Dan's boards fail. X01 and X06 measure it before more is invested.
- **Python is a specification with history.** KRT's comments record many
  measured dead ends (the "grace" budget, a via-rung retry that fired 459
  times for about 2 wins). Port the shipped behaviour and its measurement,
  not every branch.
- **Rules the DSN lacks** hold back length matching and differential pairs
  until the K reader lands.
- **Licence hygiene.** A ported file without its header, or a corpus board
  committed by accident, is a licence problem. F04's CI check and F03.3's
  licence allow-list guard both.
