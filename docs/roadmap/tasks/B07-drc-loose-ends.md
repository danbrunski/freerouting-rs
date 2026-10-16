# B07 — DRC loose ends

Opened 2026-10-15. The DRC stage is ported and matches Java's reports on every
fixture (`tests/reference_drc.rs`). [port-completion.md](../port-completion.md)
lists two loose ends: some Java DRC unit tests are not ported, or are ported
with weaker assertions; and copper islands are neither compared with Java nor
computed in the reference test. This checklist closes both, plus two
owner-rule tests about planes (D8). None of it is on the routing critical
path. It can run at any time, and B07.3 waits only for B01.1's probe support.

**Prerequisites:** B01.1 for B07.3. None for the rest.

## What exists and what this adds

| Rust today (`crates/freerouting/`, `70389b7`) | Java at `cda540f` (`src/test/java/app/freerouting/` and `src/main/java/app/freerouting/`) |
|---|---|
| `tests/drc.rs:175` `clearance_violation_categories`: one aggregate test on a fixture, asserting only `PIN_TO_PIN` and `POTENTIALLY_FIXABLE` | `drc/ClearanceViolationTest.java` (131 lines, 8 tests with mocks: pin-to-pin, pin-to-outline, pin-to-keepout, pin-to-component-outline, fixed route (trace and via), routable trace, two routable traces, other unfixable) |
| `tests/drc.rs:197` `issue575_unconnected_items`: lower bounds (`>= 4` dangling vias, `>= 9` groups) | `drc/UnconnectedItemsReproductionTest.java` (210 lines) on `Issue575-drc_Natural_Tone_Preamp_7_unconnected_items.dsn`: items 2402 and 2411 dangling, exactly 9 unconnected groups, 24 dangling tracks, 4 dangling vias |
| `src/drc/zone_islands.rs`: Rust's own fill and island decomposition (header `:1-13`); `fill_islands` (`:899`), `zone_island_violations` (`:917`); unit tests `tests/drc.rs:375-452`. `tests/reference_drc.rs` turns islands off (`set_check_zone_islands(false)`, `:14-15`) | `drc/DesignRulesChecker.getZoneIslandViolations` (`:192-308`), `mergeOrAddIsland` (`:370-398`), `ZoneIslandViolation.java`; the fill is `ConductionArea.getDetailedFillArea` (`board/model/items/ConductionArea.java:105-200`) |
| No tests for the plane owner rules | `io/specctra/PlanePinClearanceTest.java` (a foreign pin inside a plane is not a violation; inside an obstacle plane it still is); `io/specctra/SignalLayerPourTest.java` (a signal-layer pour is ignored when the net has a power-layer plane; kept on a two-layer board) |

`docs/DIVERGENCES.md` ("Design rule check") explains why islands differ: Java
pairs holes with the wrong outer subpath of the AWT area, orders islands in
AWT path order, and measures dead copper by bounding box.

## Decisions made here

- **Real items, not mocks.** The `ClearanceViolationTest` cases become small
  synthetic boards built through the board API (as `tests/drc.rs` already does
  for zones), so they test the Rust category logic on real geometry.
- **Islands are compared as sets, with tolerances, and an allowlist.** Exact
  comparison is impossible by design (DIVERGENCES). A finding is keyed by
  (conduction area id, type, sorted item-id set). Bounds and area are compared
  within a tolerance. A board where Java's hole pairing is provably wrong goes
  on an allowlist, with the proof computed, not asserted.
- **The islands dump is a probe, not a `ReferenceDump` mode.** C01 owns new
  `ReferenceDump` modes. A probe keeps B07 independent.

## File ownership

| Item | New files | May edit |
|---|---|---|
| B07.1 | — | `tests/drc.rs` (append) |
| B07.2 | — | `tests/drc.rs` (`issue575_unconnected_items` only) |
| B07.3 | `tools/java-reference/probes/board/IslandsProbe.java`, `tests/reference_islands.rs`, `fixtures/reference/probe/islands/*` | `tools/java-reference/probes/board/{fixtures.tsv, README.md}`, `docs/DIVERGENCES.md` |
| B07.4 | `tests/plane_owner_rules.rs` | — |

## Checklist

- [ ] **B07.1 — The `ClearanceViolationTest` cases.**
  - Java: `drc/ClearanceViolationTest.java:21-131`; the logic is
    `ClearanceViolation.isUnfixable` and `getCategory` (`drc/ClearanceViolation.java`),
    ported as `is_unfixable` and `category` (`src/drc/clearance.rs:61-100`).
  - Rust: one test per Java case on a synthetic board: two overlapping pins of
    different nets; a pin crossing the outline; a pin inside a keepout; a pin
    inside a component outline; a `USER_FIXED` trace, and a fixed via, against a
    foreign pin; a routable trace against a pin; two routable traces; an
    unfixable pair that fits no other category (follow the Java test's items).
  - **Accept:** 8 tests, one per Java test name (snake case), each asserting
    `is_unfixable` and `category` exactly as Java does. The old aggregate test
    stays.

- [ ] **B07.2 — Exact counts for Issue575.**
  - Java: `UnconnectedItemsReproductionTest.java:52-130`.
  - Rust: replace the lower bounds with Java's exact values: items 2402 and 2411
    are dangling traces; 9 unconnected groups; 24 dangling tracks; 4 dangling vias;
    via 2522's assertions. Where the representative choice differs
    (`docs/DIVERGENCES.md`: a trace that represents its group isn't reported as
    dangling), assert the exact Rust count and comment the difference with the
    representative's id.
  - **Accept:** exact assertions green; any number that differs from Java's names
    the representative item that explains it.

- [ ] **B07.3 — Copper islands compared with Java.**
  - Java `IslandsProbe.java`: for every `fixtures.tsv` row with a conduction area,
    load (and import the session), call `getZoneIslandViolations()`, and write each
    finding: conduction area id, type (`isolated_island_unconnected` or
    `isolated_island_dead_copper`), layer, net, island bounds, the AWT island area
    (the shoelace area of the flattened path), and the sorted ids of the items
    assigned to it. Also write, per conduction area, every island's bounds and
    whether it contains a hole that lies outside its own outer boundary (the
    evidence of Java's hole pairing bug).
  - Rust: `tests/reference_islands.rs` runs `zone_island_violations` with islands
    on, and compares keyed on (area id, type, sorted item ids). Bounds may differ by
    at most 1% of the area's bounding-box side, and areas by at most 1% (the 16-step
    Bézier flattening and the sliver rule in DIVERGENCES).
  - The allowlist names each board where a mismatch comes from a Java island with
    a misplaced hole. It cites the probe's evidence line, and the test re-checks
    that line, so an allowlisted board whose evidence disappears fails.
  - **Accept:**
    - `reference_islands.rs` green on every fixture with conduction areas.
    - The done note gives the number of fixtures compared, findings matched and
      boards allowlisted.
    - The DIVERGENCES copper-island entry points to the test.

- [ ] **B07.4 — Plane owner rules as tests.**
  - Java: `io/specctra/PlanePinClearanceTest.java` (2 tests, inline DSN) and
    `io/specctra/SignalLayerPourTest.java` (2 tests, inline DSN). Both are owner
    rules (D8: plane-net pads, signal-layer pours ignored when the net has a
    power-layer plane).
  - Rust: `tests/plane_owner_rules.rs` with the same inline DSN text, loaded with
    `freerouting::io::specctra::read_board`, asserting the same item-level results
    with `BasicBoard::clearance_violations(id)` (`src/drc/clearance.rs:157`) and the
    conduction areas by net and layer.
  - **Accept:** 4 tests green, named after the Java tests.
