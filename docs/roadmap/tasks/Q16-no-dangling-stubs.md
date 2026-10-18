# Q16 — No dangling vias or trace stubs in the result

Opened 2026-10-18, from canary-7a's grade of Dan's `enhanced` plugin run on `canary-pwr`
(`target/canary-handoff/2026-10-17-plugin45/`, 45 unconnected). KiCad's DRC counted **32
dangling vias**, mostly pairs on supervisor signals (CAN_*, LED_*, I2C_READY, MCU_SCL). They
are stubs of connections the router started and never finished. KiCad only warns about them,
but they are copper a designer has to delete by hand, and they block later manual routing.

What exists:
- the autorouter's rip-up already removes unconnected vias
  (`AutorouteControl::remove_unconnected_vias`, Java's `removeUnconnectedVias`, on by
  default) when it rips a connection;
- the DRC reports dangling stubs (`drc/checker.rs`, `UnconnectedKind::ViaDangling` and
  `TrackDangling`) and advises an optimizer pass.

Nothing removes stubs that are still there at the end of the run: a connection that failed in
the last pass, the sweep's partial inserts (Q10), or the optimizer's leftovers.

Prerequisites: Q10 (the sweep), N02 (the totals rule), F02.6 (the KiCad oracle).

## Decisions made here

- **A final cleanup stage under `enhanced`, off in `legacy`:** after the last routing or
  optimizer step, remove every routed via and trace that is a stub:
  - a stub connects fewer than two of the net's items (pins, planes, other copper), counted
    the way KiCad counts them. That includes contact with a plane of the net, so a via that
    stitches a trace to its plane is kept;
  - fixed or locked copper (`type fix`, the user's tracks) is never removed;
  - the removal repeats until nothing changes, so a chain of stubs goes as a whole.
- **It removes only copper that connects nothing.** It cannot change which connections are
  complete, so the unconnected count must not rise. That is a hard guard of the measurement.

## Checklist

- [ ] **Q16.1 — Where the stubs come from.** On `canary-pwr` (the plugin45 handoff DSN and
  the default plugin flags) and two corpus boards, attribute each dangling via and track that
  KiCad reports to the stage that inserted it (last pass, sweep, optimizer) using the
  routing ledger or the verdicts.
  - **Accept:** a table in this file, with a count per stage.
- [ ] **Q16.2 — The cleanup stage.** `router.cleanup.remove_stubs = true | false`, unset by
  default and set in no profile until Q16.3 measures it.
  - **Accept:**
    - fixtures: a via pair on a failed connection is removed; a via onto its own plane is
      kept; a fixed via is kept; a three-stub chain goes in one call;
    - with the setting unset, A14 is unchanged.
- [ ] **Q16.3 — Measure under N02 (no-regress).** Register first: `canary-pwr` via the plugin
  path plus the K09 boards.
  - **Accept:**
    - KiCad's `via_dangling` and `track_dangling` are 0 on every board;
    - unconnected does not rise on any board;
    - if accepted, N05 adds the setting to `enhanced`.
