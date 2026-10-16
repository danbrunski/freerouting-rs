# X02 — Plane-tap verification and pour region repair

Opened 2026-10-15. A pour can end up split into islands after routing, and a
plane-net pin can look connected while its via lands on a floating island.
KRT handles both: its tap code checks that a tap lands on the plane's main
connected component (`py_router/plane_component_oracle.py`, used by
`plane_pad_tap.py`), and its plane finalize re-joins a net's disconnected pour
regions with straps along a Kruskal MST (`py_router/repair_planes.py`,
`plane_region_connector.py`: `find_disconnected_zone_regions`,
`find_region_connection_points`, `route_plane_connection_wide`, at
`0486e799`). Freerouting has neither. This checklist adds the verification as
a report, then region repair as a post-route stage.

Q01 tries to stop routing from cutting pours; X02 repairs what is cut anyway.
They are measured together.

Prerequisites: board parity (rung 3); Q01.4 (the pour audit and its use of
`fill_islands`); A06 (a connection-routing entry point with a trace-width
override); A10 (the plane-repair stage slot); B09 (insert → DRC → commit or
revert); F04; F03.1, F03.4, F02.6.

## What exists and what this adds

| Exists | State |
|---|---|
| Rust `drc::zone_islands::fill_islands` (port of Java `ConductionArea.getDetailedFillArea`) and `zone_island_violations` | islands per area; Java DRC's isolated-island findings |
| Rust `drc::incompletes::NetIncompletes`, `drc::unconnected` | own-net connectivity, the same code DRC uses |
| Java plane-net routing at cda540f: plane-net tasks first; plane targets with a fallback (pipeline report §3.5) | ported by A; D8: plane-net pads get their own via |
| KRT `plane_component_oracle.py` (267 lines): a tap may only target copper on the main component | the verification |
| KRT region repair: flood-fill regions from anchors (vias, through-hole pads), Kruskal MST over regions, multi-source to multi-target routing per MST edge in both directions, strap widths stepping 2.0 → 1.0 → 0.5 → … → minimum | the repair |
| KRT #675: its fill source depends on whether KiCad is installed | the hazard not to copy |

## Decisions made here

- **One fill source:** Java's detailed fill through `fill_islands`. No
  KiCad-dependent fallback.
- **Main component** of a plane net on a layer: the island touching the most
  anchors of that net (pins and vias), ties by the lowest anchor `ItemId`
  inside it.
- **D8 holds:** a plane-net pin counts as connected to the plane only through
  its own via, and that via must lie on the main component. Via reuse and
  via-in-pad from KRT's tap ladder are not ported.
- **Repair is a stage**, id `krt-region-repair`, selected by
  `router.special.plane_repair`, run after the last routing pass and before
  the optimizer. Default off.
- **Straps are routed by the Freerouting connection router** with a width
  override (A06), from the anchors of one island to the anchors of the other.
  Widths step down 2.0, 1.0, 0.5 mm, then the net class width. Each strap is
  inserted in a B09 transaction and kept only if the exact DRC adds nothing.
- **MST order:** edge weight is the smallest anchor-to-anchor distance
  between two islands (squared, `i64`), ties by the pair of island indices;
  islands are indexed by their lowest anchor `ItemId`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X02.1 | `crates/freerouting-krt/{Cargo.toml, README.md, src/lib.rs}` if the crate doesn't exist yet; `crates/freerouting-krt/src/planes/mod.rs`, `planes/components.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X02.2 | `planes/verify.rs`, `fixtures/krt/x02-floating-island.dsn` | — |
| X02.3 | `planes/repair.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X02.4 | `fixtures/krt/x02-split-pour.dsn`, `crates/freerouting-krt/tests/planes.rs` | — |
| X02.5 | the X02 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **X02.1 — Crate skeleton; islands, anchors and main components.**
  If `crates/freerouting-krt` doesn't exist, create it as X04's decisions
  describe (`license = "GPL-3.0-only AND MIT"`, `publish = false`, depends on
  `freerouting` only, `register()` in `lib.rs`, a README stating the licence
  split).
  Rust design: `planes::components::PlaneComponents::build(board)`: for each
  filled conduction area in ascending `ItemId`, its islands (`fill_islands`),
  each island's anchors (pins and vias of the area's net whose shape centre
  lies inside the island) sorted by `ItemId`, and the main island.
  - **Accept:** the gate passes with the crate; unit tests: an area cut in
    two by a foreign trace has two islands with the expected anchors; the main island is the one with more
    anchors; a tie picks the lower anchor id.

- [ ] **X02.2 — Tap verification (report only).**
  For each plane-net pin: the via that connects it (D8), and whether that via
  lies on the main component of the plane on each plane layer it reaches.
  A pin attached only to a non-main island is reported as
  `floating_tap`. Emitted in the C08 result JSON.
  - **Accept:**
    - `x02-floating-island.dsn` (two plane pins whose vias land on a
      same-net island cut off from the main pour) reports both pins;
    - the incomplete count Rust's DRC reports for the net equals the count
      of reported pins' connections;
    - the SES is byte-identical with the report on and off.

- [ ] **X02.3 — Region repair stage.**
  Rust design: `planes::repair::RegionRepair` implements A10's plane-repair
  trait. For each plane net with more than one island on a layer, in
  ascending net number: build the MST; for each edge in MST order, try the
  width ladder; route with A06's width override; insert in a B09
  transaction; keep the first width whose insertion adds no DRC item. A
  strap that fails at every width is reported, not retried.
  - **Orders that decide results:** nets ascending; MST edges in Kruskal
    order with the tie-break above; widths in ladder order.
  - **Accept:** unit test of the MST on a synthetic set of four islands;
    with the stage off, `ses-diff` shows identical sessions on `benchmark`.

- [ ] **X02.4 — Repair fixture.**
  `x02-split-pour.dsn`: a GND pour on B.Cu cut in two by a pre-routed signal
  trace, with room for a strap around the trace's end.
  - **Accept:** with the stage on, the pour has one island after routing
    (`fill_islands`), Rust's DRC reports no new item, no via is in a pad,
    and `tools/determinism-check.sh` passes.

- [ ] **X02.5 — Measurement, with Q01.**
  Preregister (F03.4). Arms: default; `krt-region-repair`; Q01's rule plus
  repair. Boards: Q01's `q01-pours.txt` set and `canary-pwr` with its
  variants. Signal: islands lost (Q01.4). Guards: KiCad `unconnected_items`
  for zone nets (must not rise), unrouted, `drc_real`, vias in pads.
  - **Accept:** measured. Kept only if islands lost fall on N−1 boards and
    nothing regresses on any; otherwise X02.3–X02.4's code is removed and
    the numbers stay here. X02.2's report may stay if Dan wants it.
