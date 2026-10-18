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
    (Q13.3's set plus `tests/data/canary-pwr-n05/plugin-b1994423.dsn`, the DSN of the owner's plugin run
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

  - **2026-10-18, rejected (stays unset): `router.cleanup.remove_stubs = true`** (Q16.3).
    Registration `docs/roadmap/prereg/Q16.json` (`83d71ce1…` before any number; amended once
    before any Q16.3 number to `d3d20814…`, because `enhanced` gained `power_layer_traces = none`),
    pinned by `crates/freerouting-dev/tests/prereg_q16.rs`; N02 totals, `accept: no-regress`,
    plugin45 options against plugin45 plus the setting, canary-pwr (plugin path and the plugin45
    handoff DSN), interf_u, CM5_MINIMA_3, pic_programmer. `freerouting-dev gate`: **FAIL** on two
    hard guards: canary-pwr (plugin path) KiCad unconnected 37 → 38, CM5_MINIMA_3 `track_dangling`
    1 → 1. via_dangling 15 → 0, 16 → 0 (handoff), 9 → 0 (interf_u); drc_real 1 → 0 on both canary
    rows; totals 121 → 122 against noise 18.08; determinism byte-identical. Causes and numbers in
    Q16.3: the stage's contact test (Java's normal contacts) misses a via crossing a fixed pour
    strip mid-segment, and a trace ending in a plane's outline outside KiCad's fill.

  - **2026-10-18, rejected again (stays unset): `router.cleanup.remove_stubs = true`** (Q16.4, the
    stub test on KiCad's contacts). Registration `docs/roadmap/prereg/Q16-4.json` (`45a26792…`
    before any number; amended once before any route to `c0674fb3…`: engine main b93bcd0, whose
    `enhanced` adds `via_clearance = net_class`), pinned by
    `crates/freerouting-dev/tests/prereg_q16_4.rs`; N02 totals, `accept: no-regress`, plugin45
    options against plugin45 plus the setting, canary-pwr (plugin path) and four stripped KiCad
    demos. `freerouting-dev gate`: **FAIL** on four hard guards: `below_minimum` 199 (kit-dev) and
    58 (multichannel_mixer) in both arms (plugin45's 0.15 mm minimum under Board Setup minimums of
    0.1981 and 0.2 mm), kit-dev `via_dangling_dsn` 11 → 1 (a via with three contacts on one
    layer, two of which meet only through the third, kept by the every-pair guard), StickHub
    `drc_real` 13 → 14 (one `solder_mask_bridge`; the byte-identical rerun grades 13). Totals
    207 → 205 against noise 29.50; canary-pwr 41 → 41 unconnected, dangling vias 17 → 0;
    determinism byte-identical. Numbers and causes in Q16.4; sessions for canary's official grade:
    `target/canary-handoff/2026-10-18-q164/`.
  - **2026-10-18, rejected (stays unset in the engine's profiles): `router.autorouter.sweep =
    free`** (Q10.4, archived). Registration `docs/roadmap/prereg/Q10.json` (paired rule;
    `c280ca8e…` at registration, amended three times before the affected numbers to `cefea2fa…`:
    the opt arm's preflight guards off, engine main f6e75ab with its `enhanced`, the eligible list
    and a safety bound), pinned by `crates/freerouting-dev/tests/prereg_q10.rs`. `freerouting-dev
    gate --prereg`: **FAIL** (primary pair `mp1` → `mp1+sweep-free`: improved 7 of 25 boards,
    regressed 9, neutral 9); every pair fails on the 21 eligible boards, the `mpdef` pair not run.
    The HARD lines (`via_in_pad` 42 and 6 on smoothieboard and kit-dev, `below_minimum` on six
    corpus boards from the commands' 150 um floor) hold in both arms. KiCad unconnected summed over
    the 8 graded rows 234 → 186; canary-pwr-9511d9a 49 → **32**, clean. The plugin's `enhanced`
    keeps K09.9's `sweep = free`. Sessions: `target/canary-handoff/2026-10-18-q104/`.

  Sessions for canary's official grade: `target/canary-handoff/2026-10-18-n05/` (8 SESs, the
  plugin DSN, `SHA256SUMS`, `README.txt` with flags and the self-grade).

  - **2026-10-18, accepted: `router.neck_width_um = 300`** (Java's necked retry,
    `retryConnectionNecked`: a connection that fails at its class width is tried once more with every
    trace clamped to 300 µm, never below `min_trace_width_um`; owner, 2026-10-17: "we should turn that
    on"). The only neck-down built: Q02.3's distance-bounded neck and Q02.5's widen-back are not, so
    a failed connection ships narrow end to end, not only through the pin field. Registration
    `docs/roadmap/prereg/N05-neck-down.json` (`bd0c0784…`, before any number, no amendment), pinned
    by `crates/freerouting-dev/tests/prereg_n05_neck.rs` (4 tests); set
    `tools/bench/sets/n05-neck-down.txt`; new input
    `crates/freerouting/tests/data/canary-pwr-n05/canary-pwr-9511d9a-board-setup-widths.dsn` (the
    fixture with its four narrowed classes back at canary 9511d9a's Board Setup widths). Arms: **A**
    enhanced at f010b1e with the class narrowing the plugin flow applies (canary: the narrowed DSN;
    demos: the plugin's `class_widths auto`), **plain** (reported) Board Setup widths without
    neck-down (today's plugin default: `PROFILE_DEFAULTS["enhanced"]` is `{}` since K09.9),
    **B** Board Setup widths plus `--router.neck_width_um=300`. One Linux release build
    (`1b0e4c01…`); arm A reproduces N05's sessions byte for byte (plugin45 `844e2c0e…`, 9511d9a
    `333c79f7…` = Q10's 33). Canary rows graded with canary's `grade.sh` steps (WSL KiCad 10.0.7,
    plugin45 against `b1994423`, 9511d9a against `e21ee05e`), demos through
    `kicad-plugin/headless_run.py` (export, route, import, `kicad-cli pcb drc`, `via_in_pad.py`,
    `inner_tracks.py`; K09's stripped boards). N02 totals rule, `accept: no-regress`;
    `freerouting-dev gate --records records.json --baseline baseline.json --prereg ...`: **PASS**
    (0 refusals; total 103 → 106, +3 against set noise 17.14; no board beyond twice its band).
    Registered width clause: copper under the Board Setup width, summed over the rows where A's
    narrowing acted (9511d9a, pic_programmer), 848.9 → 310.4 mm: holds.

    | row | KiCad unconnected A / plain / B | under Board Setup width, wide nets, mm (A / plain / B) | copper errors A / B | starved A / B | wall s A / plain / B |
    |---|---|---|---|---|---|
    | canary-plugin45 | 37 / = A / 36 | 30.9 / = A / 32.6 | 1 / 1 (the same +12V_PROT via, 0.245 mm) | 5 / 5 | 110.5 / – / 104.2 |
    | canary-9511d9a-mp1sweep | 33 / 49 / 35 | 696.7 / 96.7 / 309.9 | 0 / 0 | 9 / 9 | 66.7 / 86.6 / 154.0 |
    | interf_u | 10 / 10 (identical) / 12 | 1.8 / 1.8 / 1.8 | 0 / 0 | 5 / 4 | 11.1 / 13.3 / 9.9 |
    | pic_programmer | 2 / 2 / 2 | 152.2 / 0.6 / 0.6 | 2 / 2 (input text crossings) | 0 / 0 | 0.3 / 0.3 / 0.3 |
    | CM5_MINIMA_3 | 21 / = A / 21 (B identical to A) | 92.2 / = A / 92.2 | 10 / 10 | 0 / 0 | 85.5 / – / 86.1 |

    Hard metrics (via in pad, track_width below minimum, power-layer traces) are 0 in every run;
    clearance-type errors and `drc_real` equal A's on every row, inner-layer tracks 0 on canary (425
    = 425 on CM5's signal inner layers). pic_programmer's two errors in every arm are the
    `/DATA-RB7` track over the copper text `1=>>`, which the DSN does not carry; in B KiCad files
    one of them as `shorting_items` (text without a net) instead of `clearance`. CM5_MINIMA_3 has no
    class wider than 300 µm, so B is byte-identical to A. Per-net delivered widths on canary's wide
    classes (`freerouting-dev widths` against the Board Setup DSN; length / under the Board Setup
    width, mm), 9511d9a row: Power_High 180.1/180.1 (A) → 123.5/44.1 (plain) → 206.0/140.9 (B);
    Switch_Node 7.2/7.2 → 4.5/2.3 → 7.9/5.8; Power 330.6/330.6 → 305.3/28.0 → 427.3/144.3; GND
    178.1/178.1 → 175.5/15.6 → 168.2/7.8. So B ships 65 mm of Power_High at 1.5 mm, 283 mm of Power
    and 160 mm of GND at 0.5 mm where A ships none, with 2 more unconnected. Largest B nets under
    width: `+5V_BUCK` 54.8 of 56.7 mm (min 0.18), `+3V3_AON` 52.1 of 126.3, `/Holdup/LTC_CAPRTN` 33.4
    of 34.9, `/Holdup/LTC_INTVCC` 31.2 of 31.2, `+12V_PROT` 29.2 of 58.7, `+5V_PI` 24.0 of 30.5 (min
    0.18). plugin45 row (canary b1994423's Board Setup is already the taps' widths, 0.3/0.4 mm):
    only GND necks, 177.6/5.0 → 179.3/6.7 (min 0.24 → 0.18). Over all nets (`ses_widths.py`, not
    registered) B also ships less copper under its class width than A on every row but plugin45
    (+2.2 mm): interf_u 130.2 → 99.4 mm and pic_programmer 207.0 → 118.7 mm, where the retry also
    necks default-class connections (0.4 and 0.5 mm classes) to 0.3 mm. Wall time: the retry costs
    time where many connections fail at 1.5 mm (9511d9a 66.7 → 154.0 s, 2.3×); elsewhere B is not
    slower (plugin45 −6 %, interf_u −11 %, CM5_MINIMA_3 +1 %).
    `ENHANCED` entry in `settings/profile.rs`, DIVERGENCES "Profile `enhanced`" line,
    `tests/settings_profile.rs::enhanced_sets_neck_width_um_300`, help text in
    `settings/fields.rs`; `tests/quality_attribution.rs` now pins its verdict runs to `legacy`
    (Q03's 1.5 mm QFN pin is infeasible for Java's router) and
    `qfn_power_pin_routes_necked_under_enhanced` pins the new behaviour (PWR routes, at most
    0.3 mm); `settings_board.rs`, `settings_path.rs` and `stage_catalog.rs` check 0 under
    `legacy` and 300 under `enhanced`. The plugin's `PROFILE_DEFAULTS["enhanced"]` keeps the Board Setup widths
    (no `class_widths`) and passes no neck flag
    (`kicad-plugin/tests/test_options.py::test_enhanced_keeps_the_board_setup_widths_and_leaves_the_neck_down_to_the_engine`).
    End to end with the rebuilt CLI on plugin45 with the plugin's flags and no neck flag: B's session
    byte for byte (`3d2cedef…`).
    Sessions for canary's official grade: `target/canary-handoff/2026-10-18-neckdown/` (5 SESs, the
    three DSNs, `SHA256SUMS`, `README.txt` with flags, self-grade and widths).
    *Official grade (canary-7a, 2026-10-18, `grade.sh`, KiCad 10.0.5, canary b1994423):* all five
    match the self-grade: 9511d9a mp1sweep A 33, plain 49, B 35 (starved 9 / 4 / 9, 0 copper
    errors); plugin45 A 37, B 36 (starved 5, the same Q14 `+12V_PROT` via clearance error, 0.245 mm).
    canary-7a: no objection; on canary its value is the safety net for un-narrowed classes
    (49 → 35), and the full-width Power_High it ships is not a defect.

  - **2026-10-18, accepted: `router.via_clearance = net_class`** (Q14; judged a third time on the
    operator's decision after the rejection above, under a registration whose clearance metric
    counts only KiCad's net-class-rule items). Registration
    `docs/roadmap/prereg/N05-via-clearance-netclass.json` (`08ede5c0…`, before any number, no
    amendment), pinned by `crates/freerouting-dev/tests/prereg_n05_netclass.rs` (3 tests); set
    `tools/bench/sets/n05-via-clearance-netclass.txt`: canary-pwr's plugin path
    (`plugin-b1994423.dsn`, the plugin45 flags) plus five boards N05-via-clearance.json did not route,
    each with a net class whose clearance is above the default's and the board minimum
    (pic_programmer POWER 0.28/0.25, tinytapeout-demo pwr 0.23 and pwrhi 0.25/0.2, RoyalBlue54L-Feather
    USB_DIFF 0.2032/0.15, corpus slice-dcmt-bread-slice HV Relay 1.25/0.2, CM5_MINIMA_3 100ohm and
    90ohm 0.2/0.13). Arms: `enhanced` at origin/main 4555d4b (`power_layer_traces = none`,
    `neck_width_um = 300`) against `enhanced` plus `--router.via_clearance=net_class`; one Linux
    release build (`46f39e11…`). New metric `netclass_via_clearance_errors`
    (`tools/kicad-oracle/oracle_lib.py` `netclass_clearance`, the `netclass-clearance` command and
    the summary's `netclass_clearance`; `tests/test_oracle_lib.py` `NetclassClearanceTest`, 4 tests
    on captured KiCad 10.0.7 items): clearance and hole-clearance items whose description names a
    net-class rule (`netclass '<class>' clearance`), with a via and two nets; board-minimum,
    board-setup, custom-rule and no-net items do not count. Role guard (must not rise on any row),
    with via in a pad, copper below the minimum and power-layer traces hard. Canary row graded with
    canary's `grade.sh` steps (WSL KiCad 10.0.7, Q13's path shim, canary b1994423), the others with
    `tools/kicad-oracle/grade.sh` plus `via_in_pad.py`/`inner_tracks.py` and `freerouting-dev
    power-traces`. Calibration: arm `enhanced` on canary-plugin45 writes the neck-down entry's arm B
    session (`3d2cedef…`) and grades as it did (36, the `+12V_PROT` via). N02 totals rule, `accept:
    no-regress`; `freerouting-dev gate --records records.json --baseline baseline.json --prereg ...`:
    **PASS** (0 refusals; total 263 → 261, −2 against set noise 42.70; no board beyond twice its band).

    | row | KiCad unconnected (enhanced → +netclass) | net-class via clearance errors | net-class clearance items (all) | drc_real | vias | wall s |
    |---|---|---|---|---|---|---|
    | canary-plugin45 | 36 → 41 | 1 → **0** | 1 → 0 | 1 → 0 (starved 5 → 8) | 128 → 125 | 106.2 → 104.6 |
    | pic_programmer | 2 → 2 (identical sessions) | 0 → 0 | 0 → 0 | 2 → 2 | 0 → 0 | 0.3 → 0.3 |
    | tinytapeout-demo | 103 → 104 | 0 → 0 | 4 → 4 (USB-C pad pairs of the input) | 37 → 37 | 126 → 129 | 1038 → 1114 |
    | RoyalBlue54L-Feather | 91 → 87 | 0 → 0 | 0 → 0 | 81 → 81 | 71 → 62 | 92.6 → 140.1 |
    | slice-dcmt-bread-slice | 9 → 9 (identical sessions) | 0 → 0 | 0 → 0 | 2 → 2 | 8 → 8 | 0.5 → 0.5 |
    | CM5_MINIMA_3 | 22 → 18 | 1 → 1 | 1 → 1 | 38 → 38 | 187 → 167 | 83.7 → 106.2 |

    Hard metrics (via in pad, below minimum, power-layer traces) are 0 in every run. CM5_MINIMA_3's
    one net-class via item differs between the arms: `enhanced` has a `/CM5/USB2P.D_P` via (class
    90ohm, 0.2 mm) 0.158 mm from a GND pad of D701, which `net_class` removes; `net_class` has a
    `+3V3_PI` via (default class) 0.111 mm from a `/CM5/PCIE_PI.PWR_EN` track (class 90ohm), a pair
    the via's own class does not decide. pic_programmer's identical session graded twice gave
    drc_real 2 both times but filed one item as `shorting_items` the second time (grading noise seen
    before); the first grade counts. On canary `net_class` removes the plugin path's last copper error
    and costs 5 unconnected (band 10), so the plugin result goes from not counting at 36 to 41 with
    no copper error (self-grade; the official grade is canary's). `ENHANCED` entry in
    `settings/profile.rs`, DIVERGENCES "Profile `enhanced`" line (and the Autorouter `via_clearance`
    line updated), `tests/settings_profile.rs::enhanced_sets_via_clearance_net_class`, help text in
    `settings/fields.rs`; `tests/via_clearance_net_class.rs` now pins `legacy` (Java's `via_info`
    is the unset value there) and `enhanced_routes_net_hv_with_the_net_class_rule` pins the new
    default. End to end with the rebuilt CLI on plugin45 with the plugin's flags and no via flag: the
    `net_class` session byte for byte (`4c8cb7e1…`). Sessions for canary's official grade:
    `target/canary-handoff/2026-10-18-netclass/` (2 SESs, the DSN, `SHA256SUMS`, `README.txt` with
    flags and the self-grade).
    *Official grade (canary-7a, 2026-10-18, `grade.sh`, KiCad 10.0.5, canary b1994423):* both match
    the self-grade: plugin45 `enhanced` 36 unconnected, 5 starved, 1 clearance error (the
    `+12V_PROT` via, 0.245 mm), 16 dangling vias; `enhanced` + `net_class` 41, 8 starved, 0 copper
    errors, 17 dangling vias. canary-7a: the promotion stays; 41 is the first plugin-path result
    on canary-pwr that counts, and the gap to the CLI sweep's 33 is the next to close.
