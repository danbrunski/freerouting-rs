# N05 — Promote accepted improvements into `enhanced`

Opened 2026-10-17 (D10). Every Q, X and K candidate is built off by default, with
its measurement registered. When a measurement accepts a candidate, it joins
profile `enhanced` through this checklist: one entry in the profile table, one
DIVERGENCES line, and the measurement it rests on. The CLI must also be able to
reach the KRT stages at all: today it does not call `freerouting_krt::register`
(the C07 TODO).

Prerequisites: N01; each candidate's own accepted measurement.

## Checklist

- [ ] **N05.1 — The CLI, GUI and plugin register the KRT stages.** Call
  `freerouting_krt::register` in each binary.
  - **Accept:**
    - `--router.autorouter.connection_router=krt-astar` (and every other KRT id)
      is accepted by the CLI;
    - the version JSON lists the registered stage ids;
    - with no KRT key set, every session is byte-identical.
- [ ] **N05.2 — The promotion procedure.**
  - A short section in `docs/development/engine.md` gives the steps: the profile
    table entry, the DIVERGENCES line, a test that `enhanced` sets the key, and the
    measurement link.
  - **Accept:** the section exists, and the first promotion follows it.
- [ ] **N05.3 — Promotions.** This item stays open as a running list. Each accepted
  candidate gets one line here: the date, the key and value, the registration, and
  the result.
  - **Accept:** each line has its test and DIVERGENCES entry. The item is ticked
    when the Q, X, K and N series have no measurement left pending.
