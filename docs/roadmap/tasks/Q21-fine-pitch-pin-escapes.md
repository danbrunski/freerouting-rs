# Q21 — Fine-pitch pin escapes within the board minimums (an option)

Opened 2026-10-20 at the owner's request. Q18 stopped every neck-down at the board's minimum
track width. That cost connections: KiCad unconnected 130 → 151 on Q18.6's four rows, 22 of them
on mydevboard, whose RP2040 QFN pins were reached only with 0.15 mm traces. Many such pins can be
reached at the minimum by leaving the pad straight along its long axis and turning only once
past the pin field.

X10 built that kind of stub for QFN, QFP and SOIC parts: the stage `krt-qfn-escape`, selected by
`router.fanout.algorithm` ([archive/X10](archive/X10-qfn-escape.md)). Its stubs start narrow:
`min(class half-width, the pin's neck-down half-width)`, floored at the `min_half_width` its caller
passes (`crates/freerouting-krt/src/fanout/qfn.rs`, `plan_pin`). Its measurement (X10.4) never
finished. This checklist makes sure that floor is Q18's board minimum, keeps a stub that is no
wider than the pad straight along the pad's axis, makes the stage a preference a user turns on
or off, and measures what it wins back.

Prerequisites: X10 (the stage, its part frame and its rotated/mirrored tests), Q18 (the board
minimums).

## Decisions made here

- **An optional preference** (decided 2026-10-20). There is one user-facing switch:
  - the setting is `router.fanout.pin_escape = off | straight`;
  - it is off by default in both profiles, and off gives byte-identical output to today's;
  - the plugin dialog shows it as a checkbox and saves it per board;
  - the desktop app shows it in the autoroute settings, and the CLI takes it as
    `--router.fanout.pin_escape`.

  Turning it on by default in `enhanced` would be a separate decision, after Q21.4.
- **The stub width is never below the board minimum.** The narrow width is `max(board minimum,
  min(class width, neck-down width))`. The board minimum is `control::min_trace_half_width`,
  which includes `router.board_minimums.track_width_um` since b98074e. A pin where that stub
  doesn't fit with clearance to its neighbours gets no stub.
- **Only parts X10 qualifies**: edge rows in the package frame at pitch ≤ 0.65 mm. BGAs stay
  with X12's planner.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q21.1 | — | — (analysis of `target/orchestrator/q18/out/`) |
| Q21.2 | tests under `crates/freerouting-krt/tests/` | `crates/freerouting-krt/src/fanout/qfn.rs` and `fanout/mod.rs` (X10's stage) |
| Q21.3 | — | the settings table, `crates/freerouting-stages`, the GUI's autoroute settings dialog, `kicad-plugin/freerouting_rs/options.py` and the dialog, `docs/DIVERGENCES.md` |
| Q21.4 | `docs/roadmap/prereg/Q21.json`, `tools/bench/sets/q21-pin-escape.txt` | — |

## Checklist

- [ ] **Q21.1 — Which connections the floor cost.** From Q18.6's pair-A sessions
  (`target/orchestrator/q18/out/`), list every connection that was routed before the floor and is
  open after it. For each, give:
  - the part, pin, pitch and pad size;
  - the width it was routed at before;
  - whether a straight stub at the board minimum fits with clearance to the neighbouring pads.
  - **Accept:** the table is in the done note, with the count that fits and the count that
    doesn't.

- [ ] **Q21.2 — Stubs at the board minimum.** Make the `min_half_width` that every caller of
  `krt-qfn-escape` passes include Q18's board minimum, keep the narrow stub straight along the
  pad's axis through the pin field, and skip pins where the stub doesn't fit.
  - **Accept:**
    - fixture tests, written from scratch, for a 0.4 mm-pitch QFN with 0.2 mm pads and a
      0.2 mm minimum: the stubs are 0.2 mm, run straight out past the outline, and leave the
      neighbours their clearance;
    - a pitch where nothing fits gets no stubs;
    - X10.3's rotated and mirrored copies give the same stubs in the part frame;
    - no stub is ever below the board minimum (`tests/board_minimums.rs` style check).

- [ ] **Q21.3 — The option.** Add `router.fanout.pin_escape`, routed through the stage registry
  (`freerouting-stages`, so the CLI and GUI get it without depending on `freerouting-krt`), and
  show it in the plugin dialog and the desktop app.
  - **Accept:**
    - tests for the setting's default and parse;
    - plugin tests for the checkbox, its saved value and the options schema bump;
    - GUI test that the dialog sets the key;
    - with the option off, connection-parity stays 7 of 7 exact under `legacy`, and the Q18
      set's sessions are byte-identical under `enhanced`;
    - a DIVERGENCES line (Java has no such escape).

- [ ] **Q21.4 — Measure what it wins back.** Register first (`docs/roadmap/prereg/Q21.json`,
  `tools/bench/sets/q21-pin-escape.txt`):
  - **Arms:** off and `straight`.
  - **Boards:** the KiCad 6+ rows that lost connections in Q18.6 pair A, plus canary-pwr (U1,
    38 pins at 0.5 mm), each with its Board Setup minimums as the plugin passes them.
  - **Time:** a few minutes in all, one KiCad grade per distinct session.
  - **Signal:** KiCad unconnected.
  - **Guards:** zero routing-caused manufacturing breaches and zero vias in pads, in both arms.
  - **Accept:**
    - the per-board table, with how many of Q21.1's connections came back;
    - the option is kept unless it routes worse on every board it acts on, in which case the
      owner decides whether it stays.
