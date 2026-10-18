# N05 — Promote accepted improvements into `enhanced`

Opened 2026-10-17 (D10). Every Q, X and K candidate is built off by default, with
its measurement registered. When a measurement accepts a candidate, it joins
profile `enhanced` through this checklist: one entry in the profile table, one
DIVERGENCES line, and the measurement it rests on. The CLI must also be able to
reach the KRT stages at all: since N05.1 it does, through `freerouting-stages`.

Prerequisites: N01; each candidate's own accepted measurement.

## Checklist

- [x] **N05.1 — The CLI, GUI and plugin register the KRT stages.** Call
  `freerouting_krt::register` in each binary.
  - **Accept:**
    - `--router.autorouter.connection_router=krt-astar` (and every other KRT id)
      is accepted by the CLI;
    - the version JSON lists the registered stage ids;
    - with no KRT key set, every session is byte-identical.

  *Done 2026-10-18:* design (c'), a variant of the checklist's (b): the engine cannot register
  the KRT stages (`freerouting-krt` depends on `freerouting`; a feature-gated dependency back is a
  cycle), and `cli`/`gui` must not depend on `freerouting-krt`. New crate `crates/freerouting-stages`
  (`registry()` = `StageRegistry::builtin()` + `freerouting_krt::register`; `shared_registry()`)
  sits above both; `cli` and `gui` depend on `freerouting` and `freerouting-stages` only, so a new
  stage family changes that crate and no binary. The engine gained `JobContext::registry`
  (`None` = built-in, as before); the CLI sets it (`cli.rs`), and `--capabilities`/`--list-stages`
  and the GUI (`routing.rs`, the Algorithms dialog) use `freerouting_stages::registry()`. The rule is
  amended in `docs/PORT_PLAN.md` and `docs/development/engine.md`. The plugin needs no change: it
  gets the stage ids from the CLI's `--list-stages=json`. Note `krt-astar` no longer exists (X06.3
  removed it); the ids are `krt-grid`, `krt-meander`, `krt-diffpair`, `krt-qfn-escape`,
  `krt-bga-escape`. Tests: `freerouting-cli/tests/krt_stages.rs` (4: capabilities lists every
  registered id with `krt-grid`/`krt-qfn-escape`/`krt-meander`; `--list-stages` too;
  `--router.autorouter.connection_router=krt-grid` routes J2 with exit 0 and `krt-nonesuch` is
  refused; the CLI's default SES equals the in-process built-in-registry SES),
  `freerouting-stages/tests/registry.rs` (3: registry = builtin + KRT; default SES byte-identical
  with and without the registry; `krt-grid` unknown without it),
  `freerouting-gui/tests/ui_routing.rs::the_algorithms_section_offers_the_krt_stages`.
  Heavy run pending: A14 full gate, `cargo test -p freerouting --release --test
  autoroute_board_parity -- --ignored` (the engine path is unchanged: `registry: None` is the same
  call as before).
- [x] **N05.2 — The promotion procedure.**
  - A short section in `docs/development/engine.md` gives the steps: the profile
    table entry, the DIVERGENCES line, a test that `enhanced` sets the key, and the
    measurement link.
  - **Accept:** the section exists, and the first promotion follows it.

  *Done 2026-10-18:* section "Promoting an improvement into `enhanced` (N05.2)" in
  `docs/development/engine.md` (under "Parity mode"): register then measure (totals block,
  pinned prereg test, KiCad grading, `freerouting-dev gate --prereg ... --records ...`), the
  `ENHANCED` entry, the DIVERGENCES "Profile `enhanced`" bullet starting with the key, a test that
  `enhanced` sets the key and `legacy` does not, and the record (N05.3 line, the candidate's item,
  the field's help text). The first promotion, `power_layer_traces = none` (N05.3), followed it:
  registration `docs/roadmap/prereg/N05-power-layer-traces.json` pinned by
  `crates/freerouting-dev/tests/prereg_n05.rs`, gate PASS, `ENHANCED` entry in
  `settings/profile.rs`, DIVERGENCES line, `tests/settings_profile.rs::enhanced_sets_power_layer_traces_none`,
  help text in `settings/fields.rs`.
- [~] **N05.3 — Promotions.** This item stays open as a running list. Each accepted
  candidate gets one line here: the date, the key and value, the registration, and
  the result.
  - **Accept:** each line has its test and DIVERGENCES entry. The item is ticked
    when the Q, X, K and N series have no measurement left pending.

  *Partial 2026-10-18:* the running list. The item stays open while Q, X, K and N measurements
  are pending (N03, N04, N06's batches, Q04, Q11, Q12, X13 and others own them).

  - **2026-10-18, accepted: `router.power_layer_traces = none`** (new value, Q13's setting).
    Registration `docs/roadmap/prereg/N05-power-layer-traces.json` (sha256 `a5ca6049…` before any
    number; amended once before any graded number to `2013aaac…`, below), pinned by
    `crates/freerouting-dev/tests/prereg_n05.rs`; set `tools/bench/sets/n05-power-layer-traces.txt`
    (Q13.3's set plus `tests/data/canary-pwr-n05/plugin-b1994423.dsn`, the DSN of Dan's plugin run
    of 2026-10-17). N02 totals rule, `accept: no-regress`, `none` against `own_plane`;
    `freerouting-dev gate --records records-reg1.json --baseline baseline-reg1.json --prereg ...`:
    **PASS** (0 refusals; total 256 → 256, set noise 25.87; every row neutral). One Linux release
    build (`42269eb6…`); canary rows graded with canary's `grade.sh` steps (WSL KiCad 10.0.7, Q13's
    path shim; plugin45 against canary `b1994423`, the rest against `e21ee05e`), corpus rows with
    `tools/kicad-oracle/grade.sh` plus `via_in_pad.py`/`inner_tracks.py`; power-layer traces by
    `freerouting-dev power-traces` (its new `all` count).

    | row | layer | own_plane | none | power-layer traces (layer → none) | drc_real (layer / own / none) |
    |---|---|---|---|---|---|
    | canary-plugin45 | 38 | 37 | 37 | 1 → 0 (inner tracks 2 → 0) | 4 / 1 / 1 |
    | canary-plugin68 | 68 | 68 | 68 | 1 → 0 (inner 2 → 0) | 2 / 2 / 2 |
    | canary-9511d9a-mp1 | 45 | 45 | 45 | 0 | 0 |
    | canary-9511d9a-mp1sweep | 33 | 33 | 33 | 0 | 0 |
    | canary-in2-open-mp1 | 28 | 28 | 28 | 0 (Power.Cu is a signal layer) | 3 |
    | canary-in2-open-wirekeepout-mp1 | 40 | 40 | 40 | 0 | 1 |
    | m-fc-g474 | 5 | 5 | 5 | 4 → 0 (inner 7 → 0) | 23 / 21 / 21 |
    | microzed | 0 | 0 | 0 | 0 | 100 / 101 / 101 |

    KiCad unconnected; `none`'s session is byte-identical to `own_plane`'s on every row, and on the
    four 9511d9a rows all three arms are identical (9511d9a mp1 `502226b0…` = Q10's `mp1.ses`).
    Totals against `layer` (reported): 257 → 256. Hard clauses for `none` (via in pad,
    below minimum, outside own plane, foreign, power-layer traces) are 0 on every row. Wall time
    within 4 % per row. Amendment: the registered calibration (arm `layer` on canary-plugin45
    byte-identical to the plugin's own session) failed: the plugin ran the Windows build, whose
    session (`5ad40948…`, 476 wires) differs from this build's (`4d67703c…`, 559 wires) on the same
    DSN and flags; on this build the file name, `--progress json`, `--result-json` and
    `--threads 1` change nothing. Grading noise seen: byte-identical sessions graded twice gave
    drc_real 1 and 2 (in2-open-wirekeepout) and 101 and 98 (microzed); per the registration each
    session's first grade counts. `ENHANCED` entry, DIVERGENCES "Profile `enhanced`" line,
    `tests/settings_profile.rs::enhanced_sets_power_layer_traces_none` (11 tests green). End to end
    with the rebuilt CLI on plugin45 with the plugin's flags: no flag gives `none`'s session and
    `--profile legacy` gives `layer`'s (bodies byte-identical).
  - **2026-10-18, rejected (stays off): `router.via_clearance = net_class`** with
    `power_layer_traces = none` (Q14.3 judged again). Registration
    `docs/roadmap/prereg/N05-via-clearance.json` (`75794c38…`, no amendment), set
    `tools/bench/sets/n05-via-clearance.txt`; gate **FAIL** on a hard clause:
    `overlord.plt+netclass.class_via_clearance_errors = 15`. Totals within the rule (119 → 130,
    +11 against noise 13.60; canary +11 against band 10, so no board beyond twice its band).

    | row | KiCad unconnected (plt → +netclass) | class-via clearance errors | drc_real | starved |
    |---|---|---|---|---|
    | canary-plugin45 | 37 → 44 | 1 → **0** | 1 → 0 | 5 → 10 |
    | canary-9511d9a-mp1 | 45 → 49 | 0 → 0 | 0 → 0 | 4 → 3 |
    | canary-9511d9a-mp1sweep | 33 → 33 | 0 → 0 | 0 → 0 | 9 → 8 |
    | Issue180-Test | 0 → 0 (identical sessions) | 0 → 0 | 0 → 0 | – |
    | overlord | 4 → 4 | 21 → 15 | 249 → 209 | – |

    All of overlord's via clearance errors in both arms are KiCad's *board minimum* clearance
    (0.175 mm, 134 → 110 counting every via pair), which the KiCad DSN export does not carry; none is
    a netclass clearance, as in Q14.3. The registered metric counts them, so the hard clause fails.
    On canary `net_class` removes the plugin run's last clearance error but costs 7 and 4
    unconnected on two rows. Operator decision (see the N05 report): a new registration whose
    metric counts only netclass-rule violations, on boards it has not seen.
  - **2026-10-18, official grade (canary-7a, `grade.sh`, KiCad 10.0.5, canary b1994423)** of both
    entries' canary sessions (`target/canary-handoff/2026-10-18-n05/`): every count agrees with the
    self-grade except plugin45 `layer`, 2 clearance errors (self-grade 4). Dangling vias 16-31 on
    every session (Q16). plugin45 `none` (37) does not count yet: its one error is a `+12V_PROT`
    via 0.245 mm from an `LTC_INTVCC` track (Q14). Both `mp1sweep` rows (with and without
    `net_class`) count at 33, the number to beat, and `sweep + net_class` is clean, which is the
    case for `net_class` once its cost on the plugin path (37 → 44) is closed.

  Sessions for canary's official grade: `target/canary-handoff/2026-10-18-n05/` (8 SESs, the
  plugin DSN, `SHA256SUMS`, `README.txt` with flags and the self-grade).
