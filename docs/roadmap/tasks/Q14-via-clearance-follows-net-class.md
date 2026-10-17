# Q14 — A via's clearance follows its net's class

Opened 2026-10-17, from canary-7a's `canary-pwr` grading. The DSN carries the class
clearance: in `fixtures/canary-pwr/canary-pwr-1a7df77-addonly.dsn`, class
`Power_High` has `(rule (width 300) (clearance 250))` and `(use_via
"Via[0-3]_800:400_um")`; `Switch_Node` is also 250, and every other class is 200.
Yet every clearance error canary-pwr has produced involves a via on a 0.25 mm-class
net, at 0.20–0.245 mm from another net:

| Run | Item pair | Gap (mm) |
|---|---|---|
| placement a, add-only | `+12V_BUS` via vs `LTC_INTVCC` track (class Power) | 0.2018 |
| b and a+b | `+12V_BUS` via vs `LTC_INTVCC` via (×4) | 0.2347 |
| b and a+b | `VCAP` via vs U7 pad 14 (`CAN_STB_N`) | 0.2101 |
| b and a+b | `+12V_PROT` via vs D5 pad 1 (`AON_IN`) | 0.2453 |
| `routed36` (Java `46bf9e2ef`) | `PWR_FAIL` track (default) vs a `+12V_BUS` via | 0.2186 |

None of the errors involves a Power_High track. So traces get their class's
clearance, and vias don't. Java does the same, so it is in profile `legacy` too.

**Likely cause (to confirm in Q14.1):** a via's clearance class comes from its
`ViaInfo`, not from its net's class. The DSN has no `(via ...)` in its network, so
`insert_via_infos` calls `create_default_via_infos` once, for the default net class
(`io/specctra/parser/network.rs`). Every via padstack's `ViaInfo` then gets the
default class's via clearance class, and `AutorouteControl::rebuild_via_info`
(`autoroute/control.rs`) takes `via_clearance_class` from the first via of the net's
via rule. A `Power_High` via is therefore cleared at 0.2 mm.

Prerequisites: C02 (the settings model); N01 (the fix goes into `enhanced`, and
`legacy` keeps Java's result).

## Decisions made here

- **The fix is in the router, not the export.** canary's `dsn_export.py` stays as
  it is. The fixture should expose the bug, not have it worked around with
  `class_class` rules.
- **Under `enhanced`, a via inserted for net N uses N's class's via clearance
  class** (KiCad: a via's clearance is its net class's). One way to get there is
  to create, for each class whose clearance differs from the default, the
  class-named `ViaInfo`s that `create_default_via_infos` already knows how to
  build (`<padstack>#<class>`), and point the class's via rule at them. Q14.1
  picks whichever keeps the rules file and SES output sound.
- **Setting:** `router.via_clearance = via_info | net_class`. `via_info` is Java's
  rule, under `legacy`; `net_class` becomes `enhanced`'s default once Q14.3
  accepts it (N05). Like Q13 it is a correctness fix (KiCad's DRC fails the other
  result), so acceptance is expected.

## Checklist

- [ ] **Q14.1 — Confirm the cause with a test.** A two-layer fixture: class `HV`
  (clearance 250, its own `use_via`) and the default class (clearance 200). Route
  an `HV` connection that must via next to a default-class track.
  - **Accept:**
    - with `via_info`, the test shows the gap Java gives; the done note records
      it and names the code path;
    - a Java probe on the same fixture agrees.
- [ ] **Q14.2 — The fix.** With `net_class`, the gap is at least 0.25 mm between
  the `HV` via and every other net's item. The DRC (`check_clearances`) reports a
  hand-placed `HV` via at 0.2 mm as a violation under both settings.
  - **Accept:**
    - the Q14.1 fixture passes;
    - with `via_info`, the A14 exact set is unchanged;
    - the rules file round-trips the class-named via infos.
- [ ] **Q14.3 — Measure.** Register before the first number (N02's totals rule), on
  `canary-pwr` (the add-only fixture and placement variants a and b) and two corpus
  boards with class-specific clearances.
  - **Accept:**
    - KiCad reports no clearance error between a 0.25 mm-class via and another
      net;
    - the unconnected totals are within the rule;
    - if accepted, N05 adds it to `enhanced`.
