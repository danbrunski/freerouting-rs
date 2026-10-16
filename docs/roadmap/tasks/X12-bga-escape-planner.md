# X12 — Dense-BGA escape planner

Opened 2026-10-15. Java's fanout pre-pass escapes pins one at a time. KRT has
a full BGA escape planner (`py_router/bga_fanout/`, 14,527 lines:
`__init__.py` `generate_bga_fanout`, `underpad.py`, `reroute.py`; README in
that directory; at `0486e799`): channel-based escape planning with layer
assignment and balancing, via size chosen by pitch (#158), plane-ball drops
(#424), an under-pad engine for fully populated arrays, coupled escapes for
pairs, and parts solved in their own frame. This checklist ports the core of
it as a fanout stage that runs before batch routing and emits ordinary traces
and vias: channel planning, dog-bone vias, layer balancing, plane-ball drops.
The under-pad engine and coupled pair escapes are follow-ups. It is the
largest X item and the last in the default order.

Prerequisites: board parity (rung 3); A10 (fanout stage slot); K02 (pad
arrays in the footprint frame, rotation and side); X10.1 (the part frame);
B09; F04; F03.1, F03.4.

## What exists and what this adds

| Exists | State |
|---|---|
| Java `BatchFanout` (cda540f) | the default fanout; parity |
| X10.1 `PartFrame` | board ↔ package frame |
| KRT channel planning and layer balancing; via size by pitch; plane balls dropped to the plane with their own via; mirror handling by flipping the board in memory | the design |
| KRT hazard: about 100 `set(...)` uses over ids and tuples | replaced by ordered collections |
| D8 | no via in a pad: KRT's via-in-pad option is excluded |

## Decisions made here

- **Stage id `krt-bga-escape`**, selected by `router.fanout.algorithm`;
  applies to parts whose pads form a grid of at least 6 × 6 positions in the
  package frame; other parts go to the next fanout (X10 or Freerouting's).
- **Dog-bone vias** at the centre of the square between four balls, so a via
  is never in a pad; its padstack is the smallest via in the net's via rule
  whose diameter plus clearance fits that square. If none fits, the ball is
  reported as not escapable by vias.
- **Channel capacity** between two ball rows on a layer:
  `floor((pitch − pad − c) / (w + c))` traces of width w and clearance c
  (n traces need `n·w + (n + 1)·c` of the gap), in integer units. Rings
  are escaped from the outside in; each ring's nets are assigned to the
  layer with the most remaining capacity, ties by layer index.
- **Plane balls** (nets with a plane layer) get a dog-bone via only (D8: their
  own via); signal balls get a via plus a stub to the channel.
- **Order:** components by name, rings outside in, balls in ring order
  clockwise from the package frame's top-left.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X12.1 | `crates/freerouting-krt/src/fanout/bga/mod.rs`, `fanout/bga/array.rs` | `crates/freerouting-krt/src/fanout/mod.rs` (append) |
| X12.2 | `fanout/bga/channels.rs` | — |
| X12.3 | `fanout/bga/vias.rs` | — |
| X12.4 | `fanout/bga/emit.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X12.5 | `fixtures/krt/x12-bga-*.dsn`, `crates/freerouting-krt/tests/bga_escape.rs` | — |
| X12.6 | the X12 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **X12.1 — Array model.**
  Rust design: `fanout::bga::array::BallArray { rows, cols, pitch,
  populated: Vec<bool>, ball_of: BTreeMap<(u16, u16), PinId> }` in the
  package frame.
  - **Accept:** a full 10 × 10 array, a depopulated-centre array and a
    staggered pattern (rejected, reported) are recognised as such.

- [ ] **X12.2 — Channel planning and layer balancing.**
  - **Accept:** unit tests: capacity formula on 0.8, 0.65 and 0.5 mm pitch
    with typical rules; on a 10 × 10 array with 4 signal layers, each ring's
    assignment fits capacity and the per-layer counts differ by at most one
    where capacity allows.

- [ ] **X12.3 — Dog-bone vias.**
  - **Accept:** no via overlaps any pad (checked on every ball); at 0.5 mm
    pitch with a 0.45 mm smallest via the balls are reported as not
    escapable.

- [ ] **X12.4 — Emission through the board API.**
  Each ball's via and stub go in one B09 transaction; a rejected one is
  reported and skipped.
  - **Accept:** on `x12-bga-0p8.dsn` every signal ball has a via and a stub
    and every plane ball a via; no DRC item; `tools/determinism-check.sh`
    passes.

- [ ] **X12.5 — Rotated and back-side copies.**
  - **Accept:** the same BGA at 0°, 90° and on the back side gets the same
    escape counts per layer, and the escapes map onto each other through the
    part frame.

- [ ] **X12.6 — Measurement.**
  Preregister (F03.4). Arms: Freerouting fanout and `krt-bga-escape`, both
  followed by the full pipeline. Boards: at least three BGA boards from
  F03.3's corpus. Signal: escape completion and final unrouted. Guards:
  `drc_real`, vias in pads.
  - **Accept:** escape completion at least the Java pre-pass's on every
    board, and unrouted improving on N−1 and regressing on none, for it to be
    kept as a selectable stage.
