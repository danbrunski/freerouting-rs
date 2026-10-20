# Q00 — Routing quality beyond Java

Opened 2026-10-15. These checklists make the port route better than Java on
the boards the owner cares about, once it routes like Java. The plan and the
measurement rules are in [routing-quality.md](../routing-quality.md). The
sources are the `canary-pwr` layout lessons, the Java fork's HANDOVER and
shelved neck-down experiment, and KiCadRoutingTools (KRT) at `0486e799`.

Every item is either a report that changes no routes, or an alternative stage
implementation behind the A10 traits that a setting selects. The Freerouting
implementation is never edited, so default results stay byte-identical.

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [Q01](archive/Q01-planes-block-pass-through-nets.md) ✅ | Pours of listed net classes block only nets with no pin inside them; pour-cut report | M / medium: obstacle hook in the search | rung 3; B09 obstacle predicate hook; C02, C03 |
| [Q02](archive/Q02-neck-down-through-pin-fields.md) ✅ | Delivered-width report, no width ratchet, neck bounded by distance, widen-back with taper | L / high: width policy inside the maze search | rung 3; A04, A06 width hooks; B05; O01; Q03 |
| [Q03](archive/Q03-infeasible-connection-diagnostic.md) ✅ | "Cannot route `<net>` to `<pin>` at width W: widest corridor C" from a static widest-path check | M / low: report only | **none: starts now**; F02.2 for the subcommand; B05 for one item |
| [Q04](Q04-restore-stop-rule.md) | Work-based stop when restores stop helping; two-axis output guard | S / low | rung 3; A10, A13, A08; B01 |
| [Q05](archive/Q05-congestion-aware-ripup.md) ✅ | Rip instrumentation; history and corridor costs; victim-cost ripup, each measured | L / high: weak prior evidence | rung 3; A04 cost hook; A11 rip events; Q06 |
| [Q06](archive/Q06-failure-attribution-and-verdicts.md) ✅ | Blocking nets per failed connection; one verdict per incomplete connection | M / low: report only | rung 3; A04 failure record; Q03; C08 |
| [Q07](archive/Q07-first-pass-ordering.md) ✅ | Pass-1 conflict-round and inside-out orderings (measured 2026-10-17: both rejected, code kept outside the repository for Q12) | S / medium | rung 3; A10 |
| [Q08](Q08-earned-budget-extension.md) | One more maze budget only when the search is still closing in | S / medium: changes where limits cut | rung 3; A04 budget hook; B02 WorkMeter |
| [Q09](archive/Q09-trace-only-keepouts.md) ✅ | Specctra `wire_keepout` as trace-only (vias allowed) behind a setting; a trace-only mode for Q01's plane policy | M / medium: changes routing on Issue754 when on | rung 3; B09.4; Q01; C02; K06; F03 |
| [Q10](archive/Q10-add-only-completion-sweep.md) ✅ | Add-only completion sweep after the best-board restore (Java's two-run "freeze and add" flow, in-engine), also after the optimizer | S / low: only adds copper | rung 3; A10, A13; B01; Q06 |
| [Q11](Q11-transactional-ripup.md) | Transactional rip-up: re-route ripped victims at once, roll back if they don't re-land; a rip-cost sweep first | S–M / medium | rung 3; A10, A11; B01; Q05.1 |
| [Q12](Q12-pass-1-portfolio.md) | Pass-1 portfolio: K deterministic orders on board clones in parallel, keep the best; squeaky-wheel restarts; per-board order spread | S / medium: memory × K | rung 3; A10; B01; O06; Q07.1–Q07.3 |
| [Q13](archive/Q13-plane-nets-stay-in-their-island.md) ✅ | A plane net's traces on a power layer stay inside its own plane, not just on its layer (canary-7a, 2026-10-17) | S–M / medium: room clipping in the maze search | rung 3; Q01.7; N01 |
| [Q14](archive/Q14-via-clearance-follows-net-class.md) ✅ | A via gets its net class's clearance, so a Power_High via keeps 0.25 mm from other nets, not the default 0.2 (canary-7a, 2026-10-17) | S / medium: changes via spacing on boards with class clearances | rung 3; C02; N01 |
| [Q15](archive/Q15-route-tidiness-score.md) ✅ | A measured tidiness score (detour, bends, acute corners, jogs, vias per connection) in the bench and the grader | S / low: report only | F03, F02.6, C05 |
| [Q16](Q16-no-dangling-stubs.md) | A final cleanup removes dangling vias and trace stubs left by unfinished connections (32 on the owner's canary plugin run) | S / low: removal of copper that connects nothing | Q10, N02, F02.6 |
| [Q17](Q17-neck-only-in-the-pin-field.md) | Neck down only within reach of the pins (Q02.3's width policy in the search, the `bounded-neck` stage) and widen back with tapers (Q02.5), measured against `enhanced`'s retry at 300 µm | M / high: width policy inside the maze search | Q02.1–Q02.2, N02, N05 |

Sizes: S under about 1k lines of Rust, M 1–3k, L more.

## Order and critical path

1. **Now:** Q03. It needs only the loaded board, the rules and F02's dev
   crate. It is the one Q item that may land before parity, because a static
   diagnostic changes no routes.
2. **After board parity (rung 3) and after P04** (owner, 2026-10-16: P04
   frees the search tree and changes routes, so every route-changing Q item
   is measured against the freed tree, once; the report-only items Q01.4,
   Q02.1 and Q06 need only rung 3), in this order of expected value on
   the owner's boards:
   - Q04 (time saved on every `canary-pwr` run, mostly byte-identical);
   - Q01 and Q02 (the two failure causes canary-09 named);
   - Q06 (needs the A04 failure record), then Q05.1 instrumentation;
   - Q05.2–Q05.4 only if Q05.1 and Q06 show congestion on at least three
     boards;
   - Q07 and Q08, as experiments.
   - Q10, Q11 and Q12 (added 2026-10-17 from freerouting-rs-f1's research, at
     the owner's request): on `canary-pwr` Q06.5 found 83 of 109 incomplete
     connections routable on the final board, 73 of them routed and then ripped
     in pass 1. These three stop that loss. Q10 first, then Q11 (its no-code
     rip-cost sweep first), then Q12 after Q07's orders exist. Their code may
     land before P04; they are measured after P04 as one preregistration
     family, because they compose (portfolio of transactional pass 1, then
     sweep, optimizer, sweep).
3. Q01.6 (narrowness cost) waits for Q05.2's lattice and the A04 cost hook.

The critical path into Q is the port's: J01 → B → O → A (rung 3) → the F03
gate → P01, P02, P04. F03.1 (comparator) and F03.4 (preregistration) must
land before any Q item's first measurement, and the first measurement of a
route-changing item waits for P04's default-tree decision.

## Parallel lanes and file ownership

All Q code lives in a new engine module, `crates/freerouting/src/quality/`.
Q03 creates `quality/mod.rs` and appends `pub mod quality;` to
`crates/freerouting/src/lib.rs`. Each later checklist appends its own
`pub mod` line to `quality/mod.rs` and its own registration call to
`quality/register.rs` (created by the first checklist that registers a stage).
Those two files and `docs/DIVERGENCES.md` are append-only shared files.

| Checklist | Owns (creates) | Appends to |
|---|---|---|
| Q01 | `quality/pour_guard.rs`, `quality/pour_audit.rs`, `fixtures/quality/q01-*.dsn`, `tools/bench/sets/q01-pours.txt` | `quality/mod.rs`, the C02 settings struct (one field), the C03 apply step (one call) |
| Q02 | `quality/neckdown/*.rs`, `fixtures/quality/q02-*.dsn`, `tools/quality/ses_widths.py`, `tools/bench/sets/q02-wide-nets.txt` | `quality/mod.rs`, `quality/register.rs`, the C02 settings struct |
| Q03 | `quality/mod.rs`, `quality/feasibility/*.rs`, `crates/freerouting-dev/src/feasibility.rs`, `fixtures/quality/q03-*.dsn` | `crates/freerouting/src/lib.rs`, `crates/freerouting-dev/src/main.rs` (one subcommand) |
| Q04 | `quality/stop_rule.rs`, `quality/output_guard.rs` | `quality/mod.rs`, `quality/register.rs` |
| Q05 | `quality/congestion/*.rs`, `tools/bench/sets/q05-congested.txt` | `quality/mod.rs`, `quality/register.rs` |
| Q06 | `quality/attribution.rs`, `quality/verdict.rs`, `docs/development/verdicts.md` | `quality/mod.rs`, `quality/register.rs` |
| Q07 | `quality/ordering/*.rs` | `quality/mod.rs`, `quality/register.rs` |
| Q08 | `quality/budget.rs` | `quality/mod.rs`, `quality/register.rs` |
| Q10 | `quality/sweep.rs` | `quality/mod.rs`, `quality/register.rs`, the C02 settings struct |
| Q11 | `quality/transaction.rs` | `quality/mod.rs`, `quality/register.rs`; one `ripup_factor` hook on `RipupPolicy` (Q11.5, default 1, not consulted in the parity path) |
| Q12 | `quality/portfolio/{mod,arms}.rs` | `quality/mod.rs`, `quality/register.rs`, the C02 settings struct, the pipeline's stage list (one call) |

Q01, Q02, Q04, Q07 and Q08 touch disjoint files and can run in parallel after
rung 3. Q05 and Q06 share nothing but Q06's output type, which Q05 reads.

Each checklist also owns its preregistration and baseline files, at the paths
F03.4 and F03.1 set (proposed: `docs/roadmap/prereg/QNN.json` and
`tools/bench/baselines/QNN.json`).

## Rules for this series

These add to the shared rules in [README.md](README.md).

1. **Off means identical.** Every Q setting defaults to off (the Freerouting
   implementation). Each checklist proves, with `freerouting-dev ses-diff`, that
   the SES with the setting off equals the pre-change engine's on
   `connection-parity` and `benchmark`.
2. **Reports change nothing.** A report item (Q01.4, Q02.1, Q03, Q05.1, Q06)
   proves that the SES is byte-identical with the report on and off.
3. **Preregister first** (F03.4). Before the first measured number: arms,
   boards and variants, eligibility decided from the inputs, the signal, the
   guards, the keep rule, the stop rule, and the decision table. Edits after a
   number exists go in `amendments` with the reason.
4. **The measurement rules** in routing-quality.md apply to every Accept
   clause that says "measured":
   - at least three distinct boards plus the `canary-pwr` variants, paired;
   - improves on at least N−1 boards and regresses on none; neutral boards
     printed; the 1-in-2^N luck rate printed;
   - order of importance: no new clearance violations, no via in a pad,
     then unrouted, then vias and length, then time;
   - numbers in the baseline file, compared by `freerouting-dev gate`
     (F03.1) with its problem classes: INVERTED, DRIFT, ORPHAN, MISSING,
     NEW ROW, MALFORMED;
   - graded by KiCad (F02.6) on boards with a `.kicad_pcb`: copper
     severities forced to error, input-board DRC subtracted, KiCad version
     recorded, and "not graded" never a pass. The tooling is F03's and
     F02.6's; Q checklists only use it.
5. **A rejected candidate leaves no code.** Its numbers go in the item's done
   note and the baseline file keeps its rows marked `rejected`.
6. **Determinism.** Every item that routes proves D2 with
   `tools/determinism-check.sh`. Q state lives in ordered collections; ties are
   broken by net number, then item id.
7. **No width ratchet** (Q02.2), for every series: nothing the engine writes
   lowers a net class width or the board's minimum width below the input's.
8. **Owner boards stay out of the repo.** `canary-pwr` and its variants are
   read through `tools/bench/sets/owner.txt` (F02.5) and skipped when missing.
   Committed fixtures for Q items are synthetic DSNs written for the test.
9. **KRT code.** Reading KRT source is allowed. A file whose code follows a KRT
   function's structure gets the F04 header (KRT path, symbol, commit
   `0486e799`) and a `docs/PROVENANCE.md` row. A file written from the idea
   alone gets a one-line credit in its module docs.

## Open questions for the owner

1. **Commit the `canary-pwr` Q03 board?** canary-09 is sending the DSN, 3–5
   variants and a post-processor. *Recommendation:* keep them out of the repo
   in `owner.txt`, as F02 does, and commit synthetic fixtures that reproduce
   the U1 and U7 geometry. Q03.6's canary check then runs locally and is
   recorded in its done note.
2. **May report-only items run on imported Java sessions before parity?**
   Q01.4 (pour-cut report) and Q02.1 (delivered-width report) read a routed
   board, so they could grade Java's own `canary-pwr` sessions today.
   *Recommendation:* yes, as with Q03. They change no routes and would show
   how much Java narrows and cuts pours now.
3. **Which net classes block other nets by default (Q01)?** *Recommendation:*
   none by default. Your `freerouting.json` lists `Power_High` and
   `Switch_Node` (and GND if wanted). A default change would need the
   measurement rules on boards that aren't yours.
4. **Neck distance D for Q02.** The candidate is "courtyard plus one pitch";
   KRT uses 2.5 mm of path length with no measurement. *Recommendation:*
   preregister both as arms and let the measurement choose.
5. **Is Q05 worth building?** KRT's history cost helped only congested boards
   and failed its own confirmation. *Recommendation:* build Q05.1
   instrumentation and Q06 first; build history costs only if more than half
   of the unrouted connections on at least three boards are `blocked_by`
   routed nets.

## Gated candidates (research, 2026-10-17)

freerouting-rs-f1's survey (`docs/roadmap/research/2026-10-17-routing-algorithms.md`,
sections 4–5, with sources) also sketched eight candidates. None is a checklist
yet; each opens as Q13+ only when its gate holds:

| Candidate | Gate |
|---|---|
| Shove before rip (Mighty's weak modification) | Q11 measured; code after P04.1 |
| Acute-angle and acid-trap cleanup (optimizer stage, geometry only) | a report-only count shows acute corners on the owner's boards |
| Spread and centre DFM pass | The owner wants DFM work |
| Exact assignment (min-cost flow, DP) in the X10/X12 escape planners | X10/X12 measured after P04 |
| SAT window repair of the last unrouted connections (new dependency) | Q10–Q12 and shove-before-rip leave ≥ 5 `blocked_by`/`found` per board on 3 boards |
| Specctra-style conflict-tolerant passes (negotiated congestion) | Q10–Q12 and shove-before-rip recover under half of the `found` failures |
| Soft layer pre-assignment from a crossing graph (max-cut) | experiment only; mixed evidence |
| CDT topological feasibility report, then corridors | the report shows ≥ 30 % of `unrouted` feasible-but-failed on 3 boards |

Not recommended, with reasons in the survey: a full topological router,
coarse or LP global routing, RL/ML routers, GPU lattice routers, teardrops and
stitching (KiCad does them). PCBWorld-Bench D3-C (arXiv 2607.05915) is a
candidate for F03.3's corpus once its board licences are checked.
