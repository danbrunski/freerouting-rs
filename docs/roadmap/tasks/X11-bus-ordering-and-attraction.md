# X11 — Bus ordering and attraction

Opened 2026-10-15. Groups of nets that run side by side between the same two
parts (data buses, address lines) route more cleanly when routed together,
middle first, each member following its neighbour. KRT does this
(`docs/bus-routing.md`; `py_router/bus_detection.py` `detect_bus_groups`,
`get_bus_routing_order`; `bus_corridor.py`; an attraction bonus toward the
routed neighbour in `rust_router/src/router.rs`, `--bus-attraction-bonus
5000`; at `0486e799`). This checklist ports the detection and the ordering,
and turns the attraction into a gridless cost term. It changes routes, so it
is measured.

Prerequisites: board parity (rung 3); A10 (`TaskOrdering`); A04 (the cost
hook in maze expansion); Q07 (shares the pass-1-only ordering wrapper
pattern); F04; F03.1, F03.4.

## What exists and what this adds

| Exists | State |
|---|---|
| A10 `FreeroutingOrdering` | pass-1 item order, later passes shuffled |
| KRT detection: cliques of nets whose source or target endpoints lie pairwise within 5 mm, sub-clustered by their other endpoints so a group hangs together at both ends | the design |
| KRT order: the middle member first, alternating outward | the design |
| KRT hazard: `remaining = set(net_endpoints.keys())` iterates a Python set; closest-first sorting has distance ties | not copied |

## Decisions made here

- **Detection in integers:** two connections are bus neighbours if their
  start pins are within 5 mm (squared distance in `i64`) and so are their end
  pins. Groups are maximal cliques found greedily in net-number order
  (`BTreeSet` throughout); groups of fewer than 3 nets are ignored.
- **Order id `krt-bus`**, selected by `router.autorouter.ordering`: within
  pass 1, each group is routed as a block, middle member first (by position
  along the line through the start pins), then alternating outward; groups
  ordered by their first net number; non-bus tasks keep Freerouting's order.
  Plane-net tasks stay first.
- **Attraction as a cost:** A04's cost hook adds, for a door of a bus member,
  a cost that rises with the door's distance from the routed neighbour's
  trace beyond one pitch (`2·hw + c`), capped. No negative costs, so the
  search's estimates stay admissible.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X11.1 | `crates/freerouting-krt/src/bus/mod.rs`, `bus/detect.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X11.2 | `bus/order.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X11.3 | `bus/attraction.rs` | — |
| X11.4 | `fixtures/krt/x11-bus8.dsn`, `crates/freerouting-krt/tests/bus.rs`, the X11 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **X11.1 — Detection.**
  - **Accept:** on `x11-bus8.dsn` (8 nets between two connectors plus 2
    unrelated nets) one group of 8 is found; moving one end connector 10 mm
    away splits nothing off wrongly (the moved nets leave the group).

- [ ] **X11.2 — Middle-out ordering.**
  - **Accept:** the pass-1 order on `x11-bus8.dsn` is the hand-derived
    middle-out order; with the default ordering, `ses-diff` is identical on
    `benchmark`.

- [ ] **X11.3 — Attraction cost.**
  - **Accept:** on `x11-bus8.dsn` with attraction on, the mean distance
    between neighbouring members' traces over their common length is within
    10 % of one pitch; with it off, the value is reported for comparison;
    `tools/determinism-check.sh` passes.

- [ ] **X11.4 — Measurement.**
  Preregister (F03.4). Boards: at least three with buses (connectors or
  memory parts with parallel nets). Signal: vias and length on bus nets.
  Guards: unrouted (no regression on any board), `drc_real`, vias in pads.
  - **Accept:** measured: vias or length on bus nets improve on N−1 boards and
    completion regresses on none; otherwise the code is removed and the
    numbers stay here.
