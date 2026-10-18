# canary-pwr input for N05

Dan's CANary power board (`hardware/canary-pwr-rev1.0` in the private `danbrunski/canary`
repository; committing canary-pwr files was approved on 2026-10-15). It lives here rather than
under `fixtures/` because every DSN there needs a Java load reference. N05.3 uses it
(`docs/roadmap/prereg/N05-power-layer-traces.json`, `N05-via-clearance.json`).

| File | sha256 (prefix) | What it is |
|---|---|---|
| `plugin-b1994423.dsn` | `0c49b990` | The DSN the KiCad plugin handed the router in Dan's run of 2026-10-17 at canary `b1994423` (profile `enhanced`, canary's graded-flow preset: one pass, minimum width 150 um, rip-up cost 1500, free sweep x2, fixed pours of Power_High and Switch_Node, GND outer pours dropped). Unrouted: no `type route` wire or via. Its session graded 45 unconnected but was invalid: two `+12V_BUS` (Power_High, 0.25 mm) vias at 0.2353 and 0.2054 mm from other nets, and a `+12V_BUS` track on Power.Cu. Handoff `target/canary-handoff/2026-10-17-plugin45/` |
