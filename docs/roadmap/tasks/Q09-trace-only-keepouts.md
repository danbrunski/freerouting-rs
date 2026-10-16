# Q09 — Trace-only keepouts

Opened 2026-10-15 from a request by the CANary layout session (canary-09).
On a 4-layer board whose inner plane layer is mostly GND but carries power
islands, opening that layer to routing for a crowded area (an MCU under a QFN)
completes far more. On `canary-pwr` it went from 47 to 30 unconnected. But the
router then ran 166 mm of track through the +12 V island. A plain `keepout`
over the island would also bar the GND and power vias that must pass through
it. What's wanted is an area that bars traces but allows vias.

Specctra has exactly that, `wire_keepout`, and KiCad exports a rule area that
bans only tracks as one ([K06](K06-kicad-pcb-rule-areas-and-copper.md)). But
Java lexes `wire_keepout` as a plain `keepout`
(`SpecctraFileDescription.flex:201` at `cda540f`), and so does freerouting-dsn
(`canonical_keyword` in `src/parse/mod.rs`). A Java probe at `cda540f`
confirmed it: `Issue754`'s board contains `ObstacleArea layer=Ground`. Java
does not drop the scope; a report saying it did was checked and is wrong.
Changing that changes routing
results on fixtures: `Issue754-avionics_hub.dsn` has a `wire_keepout` over its
whole Ground layer, which today blocks every via there. So this is a
deliberate divergence, kept behind a setting until measured under the Q rules
([routing-quality.md](../routing-quality.md)).

**Prerequisites:**
- board parity (rung 3);
- B09.4 (the conduction-area obstacle hook);
- Q01 (the per-class plane policy this extends);
- C02 (settings);
- K06 (the KiCad reader keeps tracks-only areas apart);
- F03 (the measurement gate and preregistration).

**Fixture needed:** a `canary-pwr` variant with the inner plane layer opened
to routing and a `wire_keepout` drawn over the +12 V island. canary-09 has
been asked for it.

## What exists and what this adds

| Exists | State |
|---|---|
| freerouting-dsn `ast::KeepoutKind::{Keepout, ViaKeepout, PlaceKeepout}`; `canonical_keyword` | `wire_keepout` becomes `Keepout` at the lexer; the AST can't tell the two apart (`tests/syntax.rs` asserts this) |
| Board areas (`board::items::AreaKind`) | `Obstacle` (blocks traces and vias of foreign nets), `ViaObstacle` (vias only), `ComponentObstacle`, `Conduction { is_obstacle, … }` |
| `BasicBoard::is_trace_obstacle` | The trace-obstacle predicate the search uses; its conduction branch gets Q01's hook through B09.4 |
| Q01 | Pours of listed classes block foreign nets that have no pin or via inside them |

What this adds:
- `wire_keepout` kept as its own keyword;
- a trace-only area kind;
- a setting that chooses Java's reading or the trace-only one;
- a trace-only mode for Q01's plane policy;
- a measurement that decides the defaults.

## Decisions made here

- **Parse faithfully, build Java's way by default.** The AST keeps
  `wire_keepout` distinct and writes it back unchanged. The board builder maps
  it to a full keepout unless `router.wire_keepout = "traces_only"`. With the
  default, every route is byte-identical to today's.
- **A new area kind, `AreaKind::TraceObstacle`.** It is the counterpart of
  Java's `ViaObstacleArea`: it blocks traces of every net and no vias.
  `is_trace_obstacle` is true for it, and the via-obstacle predicate is
  false.
- **Q01 gains a block mode.** A plane policy entry can block foreign
  `traces` only, or `traces_and_vias` (Q01's current behaviour). Trace-only
  is canary-09's "no foreign traces, foreign vias OK".
- **Defaults change only by measurement**, under the Q rules: at least three
  boards, paired, better on N−1 and worse on none, graded by the KiCad
  oracle. The guards hold: no new clearance violations, no via in a pad, no
  copper narrower than its net class (D8).

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q09.1 | — | `crates/freerouting-dsn/src/parse/mod.rs`, `src/ast.rs`, `src/write/*.rs`, `tests/syntax.rs` |
| Q09.2 | `crates/freerouting/tests/trace_obstacle.rs` | `crates/freerouting/src/board/items/mod.rs`, `board/basic_board.rs`, `drc/clearance.rs` |
| Q09.3 | — | `crates/freerouting/src/io/specctra/` (board build), C02's settings table (append) |
| Q09.4 | — | Q01's area-policy module (B09.4) |
| Q09.5 | `tools/ab/q09-trace-only-keepouts.json` | `docs/DIVERGENCES.md` |
| Q09.6 | — | `crates/freerouting-dsn/src/parse/mod.rs` (`skip_unknown`), `tests/syntax.rs` |

## Checklist

- [ ] **Q09.1 — Keep `wire_keepout` in the DSN syntax tree.**
  Add `KeepoutKind::WireKeepout`, read `wire_keepout` into it, and write it
  back as `wire_keepout`.
  - **Accept:**
    - `tests/syntax.rs` asserts that `wire_keepout` reads as `WireKeepout`,
      replacing today's assertion that it reads as `Keepout`.
    - `Issue754-avionics_hub.dsn` round-trips with its `wire_keepout`
      intact.
    - `reference_load` and `reference_drc` are unchanged on every fixture,
      because the board builder still maps it to a keepout.

- [ ] **Q09.2 — A trace-only area kind.**
  `AreaKind::TraceObstacle`: an obstacle to traces of every net, never to
  vias. Wire it into the search-tree predicates and the DRC. A trace inside it
  is a violation; a via inside it is not.
  - **Accept:**
    - unit tests on a synthetic board: a trace through the area is refused
      by `is_trace_obstacle` and reported by the DRC;
    - a via inside it is accepted and not reported;
    - an existing `Obstacle` area still blocks both.

- [ ] **Q09.3 — The `router.wire_keepout` setting.**
  `"keepout"` (default, Java's reading) or `"traces_only"`. The board builder
  maps `WireKeepout` to `Obstacle` or to `TraceObstacle` accordingly. Append
  the key to C02's settings table.
  - **Accept:**
    - with the default, `ses-diff` is identical on the whole `benchmark` set
      and on `Issue754`;
    - with `traces_only`, `Issue754`'s Ground layer accepts vias, and a test
      shows it.

- [ ] **Q09.4 — Trace-only mode for Q01's plane policy.**
  Extend Q01's policy with `block = "traces" | "traces_and_vias"`. With
  `traces`, a pour of a listed class bars foreign traces and lets foreign
  vias through, still with full clearance.
  - **Accept:**
    - a synthetic board with a +12 V island on an inner layer: a foreign GND
      via passes through the island with correct clearance;
    - a foreign trace doesn't;
    - the `traces_and_vias` mode keeps Q01's behaviour, byte-identical.

- [ ] **Q09.5 — Measure and decide.**
  Preregister the A/B (F03.4) before running it:
  - **arms:** the default; `router.wire_keepout = "traces_only"`; and the
    Q01 `traces` mode;
  - **boards:** `Issue754`, the requested `canary-pwr` inner-layer variant,
    and the `benchmark` set;
  - **metrics:** unrouted connections, and track length inside foreign-net
    islands. The 166 mm through the +12 V island is the number to beat.
  Grade through the KiCad oracle (F02.6).
  - **Accept:**
    - the F03.1 gate result is in the done note with every board's numbers;
    - if it passes, the default changes and DIVERGENCES gets a line with
      the numbers;
    - if it fails, the setting stays opt-in, and the numbers stay in the
      note. The setting stays either way, because the KiCad plugin can pass
      it for boards that need it.

- [ ] **Q09.6 — Warn on every skipped scope.**
  Today an unknown named scope is skipped silently. `skip_unknown` warns only
  for a scope with no keyword, as Java's `skipScope` does. A silently ignored
  constraint is how a board gets routed through an area its author barred
  (canary-09's lesson). Warn once per unknown keyword per file, naming the
  keyword, the enclosing scope and the first position. This doesn't change
  what is read, so it isn't a routing divergence. Keep these warnings in the
  parser's warning list, apart from the wiring warnings that `reference_load`
  compares with Java.
  - **Accept:**
    - a test DSN with an invented `(foo_keepout ...)` in the structure scope
      gets one warning naming `foo_keepout` and `structure`;
    - the done note lists the unknown keywords found across all fixtures,
      with counts;
    - `reference_load` and `reference_drc` are unchanged.

