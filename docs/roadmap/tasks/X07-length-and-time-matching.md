# X07 — Length and time matching

Opened 2026-10-15. Java Freerouting has per-net-class minimum and maximum
trace lengths, and it reports violations (`drc/NetIncompletes.calcLengthViolation`
at cda540f, `WindowLengthViolations`), but its autorouter never lengthens a
trace. KRT tunes lengths with trombone meanders
(`py_router/length_matching.py`, 4,182 lines: `generate_trombone_meander`,
`apply_meanders_to_route`, `get_bump_segments`, `get_safe_amplitude_at_point`,
`apply_time_matching_to_group`; delay maths in `impedance.py`; at
`0486e799`). This checklist adds a post-route stage that inserts meanders as
ordinary traces, each bump checked by the exact DRC. It is gridless by
construction: KRT's algorithm uses its grid only for a clearance margin.

Prerequisites: board parity (rung 3); O06 (the optimizer, which runs before
this stage); B09 (insert → DRC → commit or revert); K05.1 (stackup thickness
and εr, pad-to-die lengths) for X07.2's via and pad terms and for X07.7; K07.5
for KiCad length rules if they are exposed (unverified); F04; F03.1, F03.4.

## What exists and what this adds

| Exists | State |
|---|---|
| Rust `rules::NetClass::{minimum_trace_length, maximum_trace_length, length_constraint}`, `rules::NetLengthConstraint` | DSN class length rules, parsed |
| Rust `drc::incompletes::NetIncompletes::calc_length_violation`, `drc::checker` `length_violation_count` | the Java length-violation check, ported |
| Java `Route.java` "length matching area" ellipse (interactive only) | not a router feature |
| KRT: target = the longest net in a group; meanders on straight runs, spilling across runs; tolerance `--length-match-tolerance` 0.1 mm; via-barrel length and pad-to-die counted; time matching with ps/mm from εr, `(εr + 1)/2` for microstrip, `εr` for stripline, FR4 default 4.3 | the design |
| KRT hazard: groups built from Python sets of net **names** (around `length_matching.py` lines 2032, 2225, 2628), whose order varies per process | not copied |

## Decisions made here

- **Stage id `krt-meander`**, selected by `router.special.length_matching`,
  run last: after the optimizer, so pull-tight can't undo the meanders in a
  batch run. An interactive optimize later may; that is documented.
- **Targets:**
  - per net, the class's `[min, max]` window from the DSN: a net shorter than
    its minimum is lengthened to the minimum;
  - groups from a setting, `router.length_matching.groups`: a list of
    `{ name, nets: [glob], tolerance_um }`; the target is the longest member,
    measured after routing. KiCad rules from K07.5 feed the same list once they
    exist.
- **Length** = trace length + via barrel spans (stackup thickness between the
  layers, from K05.1) + pad-to-die (K05.1). Without K05.1 the last two are 0 and
  the report says "stackup unknown".
- **Meander geometry:** a bump replaces part of a straight segment with a
  U of amplitude A and pitch p: `p ≥ 2·hw + c`, A from the largest that
  passes the clearance check, stepped down by halves to `2·hw + c`; corners
  at 45° where the board's trace angle restriction allows, else 90°.
  Integer board units; bump ends at exact integer points on the segment.
- **Order:** groups by name, nets by net number, a net's straight runs by
  length descending (ties by item id), bumps along a run from its start.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X07.1 | `crates/freerouting-krt/src/length/mod.rs`, `length/measure.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X07.2 | `length/targets.rs` | the C02 settings struct (one field) |
| X07.3 | `length/meander.rs` | — |
| X07.4 | `length/matching.rs`, `fixtures/krt/x07-*.dsn`, `crates/freerouting-krt/tests/length.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X07.5 | `length/timing.rs` | — |
| X07.6 | the X07 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **X07.1 — Length measurement.**
  Source: KRT's measurement (trace + via barrel + pad-to-die).
  - **Accept:** on a fixture with known geometry, measured lengths equal hand
    values within 1 µm; without stackup data the report flags it; the value
    matches `NetIncompletes`' length for a net with no vias.

- [ ] **X07.2 — Targets and groups.**
  - **Accept:** unit tests: a class min length of 30 mm makes a 25 mm net's
    target 30 mm; a group of three nets targets the longest; glob patterns
    match in net-number order; an empty group is a settings error.

- [ ] **X07.3 — Meander bumps.**
  Source: KRT `generate_trombone_meander`, `get_safe_amplitude_at_point`,
  `get_bump_segments`.
  Rust design: `length::meander::bump(segment, at, amplitude, pitch, side,
  angle_style) -> Polyline` and the added length as an exact function of A
  and p.
  - **Accept:** unit tests: added length matches the formula; bumps on a
    45° segment stay 45°-legal; both sides are tried, left first.

- [ ] **X07.4 — Matching loop with the DRC gate.**
  Source: KRT `apply_meanders_to_route`.
  Each bump goes in through a B09 transaction; a bump the exact DRC rejects
  is retried at half the amplitude, then skipped. Stop when the net is within
  tolerance or no run has room; report the shortfall.
  - **Accept:**
    - on `x07-group.dsn` (three nets of different lengths with room), all
      three end within 0.1 mm of the target;
    - on `x07-tight.dsn` (no room), the shortfall is reported and the board
      has no new DRC item;
    - running the stage twice is idempotent (the second run changes nothing);
    - Java's length-violation count does not rise;
    - `tools/determinism-check.sh` passes.

- [ ] **X07.5 — Time matching.**
  Source: KRT `apply_time_matching_to_group`.
  Delay per layer from K05.1's εr: `sqrt(εr_eff)/c` with `εr_eff = (εr + 1)/2`
  on outer layers and `εr` on inner layers; via delay from barrel length at
  the inner εr. Targets in ps through `router.length_matching.groups`
  (`tolerance_ps`).
  - **Accept:** on a fixture with a two-layer stackup, a group matched in
    time differs in length by the expected ratio; without K05.1 the stage
    refuses time targets with a clear message. If K05.1 has not landed, mark
    `[~]` with that remainder.

- [ ] **X07.6 — Measurement.**
  Preregister (F03.4). Boards: at least three with length rules or groups
  (fixtures with class min lengths, and corpus boards from F03.3 with
  matched groups). Signal: nets within tolerance. Guards: unrouted, `drc_real`,
  vias in pads, Java length violations.
  - **Accept:** on every board, every group is within tolerance or each
    net's shortfall is reported; no new DRC items; the stage is kept as a
    selectable stage if that holds (it adds no change when no target exists).
