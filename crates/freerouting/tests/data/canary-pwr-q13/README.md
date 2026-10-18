# canary-pwr inputs and sessions for Q13

Dan's CANary power board (`hardware/canary-pwr-rev1.0` in the private `danbrunski/canary`
repository), as in `fixtures/canary-pwr/` (committing canary-pwr files was approved on
2026-10-15). These files live here rather than under `fixtures/` because every DSN there needs
a Java load reference. Q13 uses them
(`docs/roadmap/tasks/archive/Q13-plane-nets-stay-in-their-island.md`): they are the sessions that put
`+5V_PI` on `Power.Cu` outside its island (x 107.3–112.2, y 45.2–55.0), plus a control.

| File | sha256 (prefix) | What it is |
|---|---|---|
| `plugin-e21ee05e-unrouted.dsn` | `5bc34813` | The KiCad plugin's DSN export at canary `e21ee05e` (`hardware/canary-pwr-rev1.0/freerouting-rs/canary-pwr.dsn`, sha256 `9ec9e39f…`), written from a board that already carried an earlier run's routing. That routing is taken out: `grep -v -E '^\s*\((wire\|via) .*\(type route\)\)\s*$'` drops its 691 `type route` wires and 92 `type route` vias and keeps the 71 fixed wires and 41 fixed vias. Q13.2's harness test; Q13.3's canary plugin row |
| `plugin-e21ee05e.ses` | `2584ed3b` | The session of that default-options plugin run (profile `enhanced`, `--router.min_trace_width_um=150`): `+5V_PI` on Power.Cu from (106.66, 56.44) to the island's border at (107.89, 54.99) |
| `exp-a-addonly.dsn` | `4cf8232a` | canary-7a's placement run a: its pass 1 by Java `7f56b062d`, fixed in by `dsn_export --fix-existing` for the add-only run. The fixed `+5V_PI` track (112.2, 54.0) → (114.0, 55.9) → (114.0, 60.2) on Power.Cu is in it |
| `exp-ab-addonly.dsn` | `498752bb` | canary-7a's placement run a+b, its add-only input in the same way |
| `exp-ab-addonly.7f56b062d.ses` | `d29021d4` | Java `7f56b062d`'s add-only session of run a+b: `+5V_PI` on Power.Cu along (108.2, 53.2) → (105.7, 53.9) |
| `9511d9a-q10-mp1.ses` | `502226b0` | The control: freerouting-rs pass 1 of `fixtures/canary-pwr/canary-pwr-9511d9a.dsn` (Q10.4's `mp1.ses`), which KiCad grades at 45 unconnected with no inner-layer track |
