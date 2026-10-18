# Preregistered A/B runs

A quality change (Q series), an alternative algorithm (X series) or the one
performance divergence (P04) is kept only if measurements say so
([routing-quality.md](../../docs/roadmap/routing-quality.md)). The rule is
enforced by `freerouting-dev gate` (F03.1), and this directory holds what makes
the measurement honest: the **registration** that is committed before the first
number exists, so that a result can't choose its own criteria afterwards. It is
KiCadRoutingTools' `*_prereg.json` practice: "arms, cells, signals, guards and
thresholds are not edited after a number exists".

## The flow

1. Copy `TEMPLATE.json` to `docs/roadmap/prereg/<id>.json` (or next to the
   checklist), fill it in and **commit it first**.
2. Run the arms with `freerouting-dev bench` (one `bench.json` per arm, same
   set, same bounds) and, where the board has a `.kicad_pcb`, the KiCad oracle
   (`tools/kicad-oracle/grade-set.sh`).
3. Build the records and check them against the registration:

```sh
freerouting-dev gate collect \
  --arm java=<out-java>/bench.json,oracle=<grades-java> \
  --arm krt=<out-krt>/bench.json,oracle=<grades-krt> \
  --arms-json <registration's "arms" copied to a file> --out records.json
freerouting-dev gate --records records.json --baseline baseline.json \
  --prereg docs/roadmap/prereg/<id>.json              # first time: add --write-baseline
```

`gate` prints the registration's sha256, refuses a run that differs from it,
prints each row's mark and the verdict, and exits 0 only if every check holds.
A rejected candidate leaves its numbers in the baseline file with
`"rejected": true` on the rows, but no code.

## `TEMPLATE.json`

| Field | Meaning |
|---|---|
| `format` | always `freerouting-ab-prereg-1` |
| `id`, `registered`, `question` | what is measured, when, and what a pass means |
| `arms` | at least two named arms, each an object with the **engine and commit**, the exact **command** and its **settings**. The run's own `arms` description (`--arms-json`) must be equal to it |
| `baseline_arm`, `candidate_arm` | which arm is the reference and which is on trial |
| `boards.set` | the board set file the run used |
| `boards.files` | **row name → sha256 of the input file** for every board. A board changed after registration is refused with its name |
| `metrics` | the judged metrics, in order of importance: `name`, `direction` (`lower`, the default, or `higher`), `kind` (`int`, `float`, `counts`), `role`, `tol`, `compared` |
| `rule`, `totals` | optional. `paired` (the default) or `totals`; see "The totals rule" below |
| `pass_rule` | `min_boards` (not below 3), `improve_all_but` (not above 1: N-1) and `max_regress` (0). A registration may tighten the global rule, never loosen it |
| `amendments` | `{date, reason, changes}` entries. A change after the first number goes here, with the reason, and the pinned hash in the test is updated in the same commit |

Metric roles:

- `hard`: a limit of the board's design rules (D8). Any value above zero in the
  candidate arm fails the gate (`HARD`) and marks the row `regress`.
  `via_in_pad` is required and `via_in_pad` / `below_minimum` can't be
  registered as another role: a via in a pad is a failure, not a quality note.
- `guard`: must not get worse than the baseline arm (`drc_real`,
  `clearance_violations`: "none new, ever").
- `score`: decides the row's mark; the first score that differs (in the order
  of `metrics`) says `improve` or `regress`.
- `report`: printed (and, if `compared`, held to the baseline), not judged.
  Time is a report metric: routing is deterministic (D2) but wall time isn't.

What the gate checks against the registration (class `REFUSED`, or `MISSING`):
arm names, engine, command and settings; every board's input hash (and boards
that were run but not registered, or registered but not run); the run's metric
list (a registered metric the run doesn't carry is `MISSING`, at the list and
in each row); that the baseline and candidate arm names match. Metrics the run
measured beyond the registered ones are not judged.

The other problem classes (`INVERTED`, `DRIFT`, `ORPHAN`, `NEW ROW`,
`MALFORMED`, `UNGRADED`) are described in the `gate` module docs
(`crates/freerouting-dev/src/gate.rs`). In short: the pass rule needs **3
distinct boards**, improves on N-1 and regresses on none; rows of one board
count once (use `board` in the row, or `gate collect --group PREFIX=BOARD`, for
variants such as the canary-pwr family); neutral boards are printed; the luck
rate (1 run in 2^N) is printed with the result.

## The totals rule (N02)

`"rule": "paired"` (the default) is the rule above: `pass_rule`, improve on
N-1 boards, regress on none. A change to profile `enhanced` is judged by
`"rule": "totals"` instead (routing-quality.md, "Which rule judges"), with a
`totals` block next to `metrics`:

```json
"rule": "totals",
"totals": { "accept": "improve",
            "signal": ["kicad_unconnected", "unrouted"],
            "noise": { "canary-pwr": 10, "pic_programmer": 3 },
            "min_boards": 3,
            "speed": { "metric": "maze_nodes", "min_drop_percent": 20 } }
```

| Key | Meaning |
|---|---|
| `accept` | required. `improve`: the total improves by more than the set's noise (or the speed clause holds). `no-regress`: the total is not worse than the set's noise (correctness fixes, N06) |
| `signal` | optional, registered metrics, default `kicad_unconnected` then `unrouted`. Per row the first one that is a number in both arms is used; the rows that fall back are summed and printed separately |
| `noise` | optional. Band per distinct **board** (the row's `board`; the canary-pwr variants share one). A board without one gets `max(2, ceil(5 % of connections))` from the row's `connections`; neither is `MISSING`. A name no run row has is `REFUSED` |
| `min_boards` | optional, not below 3 |
| `speed` | optional, only with `accept: improve`. `metric` is a registered metric (lower is better); `min_drop_percent` defaults to and may not go below 20 |

The hard guards (`hard` and `guard` metrics) and D2 (the baseline
comparison) are the same as for the paired rule. `pass_rule` may stay in the
file (it is validated, and reported), but the verdict is the totals rule's.
`freerouting-dev gate` without `--prereg` takes `--rule totals [--accept ...]
[--noise BOARD=BAND]... [--speed METRIC[=PERCENT]]`; with `--prereg` the
registration states all of it, and an option that differs is `REFUSED`.
A registration that says `"rule": "totals"` without the block (Q14's, judged by
hand) parses, but a run against it is refused.
[`examples/n02-totals.prereg.json`](examples/n02-totals.prereg.json) is a
sample; `crates/freerouting-dev/tests/gate_totals.rs` pins its sha256 and
runs it.

## Worked example: the X01 head-to-head, re-registered

[`examples/x01-head-to-head.prereg.json`](examples/x01-head-to-head.prereg.json)
re-registers the head-to-head of X01 (Java Freerouting against
KiCadRoutingTools, graded by the same KiCad DRC):

- arms `java` (the baseline) and `krt`, with the commands X01 gives;
- five boards: `canary-pwr-9511d9a` (Dan's board as it stands, input
  `fixtures/canary-pwr/canary-pwr-9511d9a.dsn`) and the stripped DSNs of the
  KiCad demos `pic_programmer`, `interf_u`, `kit-dev-coldfire-xilinx_5213`
  and `video`, as `tools/corpus/prep.sh` exports them with the WSL KiCad
  10.0.7; the hashes are those of that export;
- metrics in X01's order: `via_in_pad` and `below_minimum` (hard), `drc_real`
  (guard), `kicad_unconnected`, `vias`, `length_mm` (scores), `wall_seconds`
  (report);
- the default pass rule: 5 boards, improve on at least 4, regress on none.

`crates/freerouting-dev/tests/gate.rs` pins the example's sha256, checks a
synthetic run against it, and shows that changing one board between
registration and run is refused with the board named. X01.1 owns the real
registration: it copies this file, adds the set file `tools/bench/sets/x01-h2h.txt`
and amends what differs (for example the jar's commit).
