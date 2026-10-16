# X10 — QFN and QFP escape

Opened 2026-10-15. Java has a fanout pre-pass (`BatchFanout.fanoutBoard` →
`RoutingBoard.fanout` per SMD pin, cda540f), which is parity work for the A
series. KRT has a dedicated QFN/QFP escape (`py_router/qfn_fanout/`, 2,469
lines, entry `generate_qfn_fanout`, at `0486e799`): short stubs outward from
edge pads, in the part's own frame. On Dan's boards the escapes are exactly
where wide power nets have to narrow, so this stage pairs with Q02: each stub
starts at the pin's width and widens back past the pin field.

Prerequisites: board parity (rung 3); A10 (the fanout stage slot and
Freerouting's fanout as the default); B05 (pin exits and neck-down width);
B09; Q02 (widen-back with taper, reused); F04; F03.1, F03.4.

## What exists and what this adds

| Exists | State |
|---|---|
| Java `BatchFanout`, `RoutingBoard.fanout` | the default fanout stage (A10/A11) |
| Rust component and package model (`board/structure/component.rs`, `library/package.rs`): placement, rotation, side, pin offsets | the part frame |
| KRT `qfn_fanout`: side classification by pad position in the part frame; stubs perpendicular to the side; vias only where a pad must change layer | the design |
| KRT rotated and back-side handling (`rotate_frame.py`, `flip_frame.py`; cut 18 of 51 mirror differences to 0 in its BGA code) | the test discipline |

## Decisions made here

- **Stage id `krt-qfn-escape`**, selected by `router.fanout.algorithm`. Parts
  qualify when their pins form four (QFN, QFP) or two (SOIC, SOP) edge rows in
  the package frame with a pitch of at most 0.65 mm; others go to the
  Freerouting fanout.
- **Stubs** run outward perpendicular to the pin's side, to the package
  outline plus the net's clearance plus half its class width. Width at the
  pin: `min(class width, 2·neckdown half-width)` (B05); past the pin field the
  stub widens with Q02's taper.
- **No vias at the stub end by default**; a via is added only for a plane-net
  pin (D8: its own via, never in a pad), placed at the stub end.
- **Order:** components by name, pins by pin number within a component,
  sides in the order W, S, E, N of the package frame.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X10.1 | `crates/freerouting-krt/src/fanout/mod.rs`, `fanout/frame.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X10.2 | `fanout/qfn.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X10.3 | `fixtures/krt/x10-qfn-*.dsn`, `crates/freerouting-krt/tests/qfn_escape.rs` | — |
| X10.4 | the X10 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **X10.1 — Part frame and sides.**
  Rust design: `fanout::frame::PartFrame` maps board points to the package
  frame (undoing placement, rotation and the back-side mirror) and back, in
  integer units; `sides(component) -> Option<[Vec<PinId>; 4]>`.
  - **Accept:** unit tests: a QFN placed at 0°, 90°, 37° and mirrored gives
    the same side assignment for every pin; a BGA returns `None`.

- [ ] **X10.2 — Stubs.**
  Each stub is inserted in a B09 transaction; a stub the exact DRC rejects is
  shortened by halves to the outline and then skipped.
  - **Accept:** on `x10-qfn-power.dsn` (a 0.5 mm pitch QFN with one 1.5 mm
    power pin and signal pins), every connected pin gets a stub, the power
    stub widens to 1.5 mm past the pin field, and there is no DRC item and
    no via in a pad.

- [ ] **X10.3 — Rotated and mirrored copies.**
  - **Accept:** the same part placed at 0°, 90°, 180° and on the back side
    gets the same number of stubs, and the stubs map onto each other through
    the part frame within 1 board unit.

- [ ] **X10.4 — Measurement.**
  Preregister (F03.4). Arms: Freerouting fanout and `krt-qfn-escape`, both
  followed by the full pipeline. Boards: at least three with fine-pitch
  QFN/QFP parts, plus `canary-pwr` and its variants. Signal: unrouted.
  Guards: `drc_real`, vias in pads, Q02.1 under-width length.
  - **Accept:** measured; kept as a selectable stage only if unrouted
    improves on N−1 boards and regresses on none.
