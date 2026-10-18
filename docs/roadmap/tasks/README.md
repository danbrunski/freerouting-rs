# Implementation task index

**Status 2026-10-16.** [port-completion.md](../port-completion.md)
is the plan for finishing the port. It sets out what "complete" means, the
decisions D1–D9, the verification ladder and the series below. Finished
checklists live in `archive/` and are marked ✅ in their series README; the
live state is `python3 tools/roadmap/checklists.py status`. A box is
evidence only with its done note; writing a plan is not evidence that
anything works.

**Cited research reports are not in the repository.** The series READMEs
and checklists cite "the board-gap report", "the autoroute report", "the
pipeline report", "the GUI, plugin and packaging report", "the KRT report"
and `series-map.md` (all 2026-10-15). Those were working documents of the
planning session and were never committed. Every decision they informed is
restated in the checklist that cites them, so treat a citation as provenance
only. When a checklist's own text is not enough, read the Java at the pin
through git, never guess at what the report said.

**The Java fork's research notes are in the repository, at `main`:**
`git -C ../freerouting show main:docs/research/<file>`. The ones the
checklists use: `large-board-autorouter-performance.md` (P03: which
parallel passes Java measured and removed), `escape_congestion_research.md`
(Q07, X10, X12), `kicad_ipc_api_research.md` (K00 question 5),
`star-ground-routing.md` (an unplanned feature request, issue 383). Dan's
analysis tools are in `docs/development/tools/` (Q06, X01).

## Series

| Series | README | Checklists | Items | Delivers |
|---|---|---:|---:|---|
| **F** Foundations | [F00](F00-foundations-README.md) | 4 | 14 | Fast test loop and CI; Java probes, `ses-diff`, `bench`, the determinism check, the KiCad oracle; the measurement gate and corpus; provenance for ported code |
| **J** Java reference | [J01](archive/J01-repin-deterministic-java.md) | 1 | 6 | Re-pin to the deterministic Java (`30baad7e2` or later) and regenerate exact references |
| **B** Board model | [B00](B00-board-gaps-README.md) | 9 | 58 | Ids, snapshots and hashes like Java; a search tree whose shape matches Java's (D5); room completion; board checks, item operations, scores; the engine API the GUI and stages need |
| **C** Settings, CLI, writers | [C00](C00-cli-settings-io-README.md) | 8 | 56 | Java reference dumps; the settings model and its board effects; rules files; SES and DSN writers; Java CLI parity and plugin-facing CLI extras |
| **O** Board operations and optimizer | [O00](O00-optimizer-README.md) | 6 | 43 | Pull-tight, shove checks and inserts, the via optimizer, a deterministic parallel batch optimizer |
| **A** Autorouter | [A00](A00-autorouter-README.md) | 13 | 85 | The search core (control, rooms, drills, maze search, path location, insertion, variants), the D9 stage traits, passes, fanout, the batch loop and the board-parity gate |
| **I** Interactive editing | [I00](I00-interactive-README.md) | 4 | 25 | `freerouting::interactive`: route, drag and make-space, editor commands, selection transforms |
| **G** Desktop app | [G00](G00-desktop-app-README.md) | 9 | 68 | Session model and test harness, GPU renderer, ratsnest/DRC/info lists, windows, rule dialogs, the autorouter run with live redraw, save, interactive tools |
| **K** KiCad | [K00](K00-kicad-README.md) | 11 | 80 | Plugin hardening and routing, a KiCad package, a direct `.kicad_pcb` reader (rule areas, copper text, board rules), an optional writer, export options for the graded canary flow, the plugin reading the board directly, a KiCad 11 IPC plugin |
| **R** Release | [R00](R00-release-README.md) | 2 | 12 | Release builds for Windows, macOS and Linux; signing |
| **P** Performance | [P00](P00-performance-README.md) | 5 | 19 | Profiling and baselines; optimizations that keep results identical; parallelism; a freed search tree; SIMD and an allocator trial |
| **Q** Routing quality | [Q00](Q00-routing-quality-README.md) | 17 | 80 | Measured improvements beyond Java: planes, neck-down, diagnostics, stop rule, ripup, ordering, budgets, trace-only keepouts |
| **X** Algorithms from KiCadRoutingTools | [X00](X00-krt-algorithms-README.md) | 13 | 67 | The head-to-head, then KRT algorithms as D9 stages: plane repair, a grid router, length matching, differential pairs, escapes |
| **N** Beyond Java | [N00](N00-beyond-java-README.md) | 6 | 32 | Profiles `legacy` and `enhanced`, with every stage and option still selectable; a totals-within-noise keep rule; the freed search tree judged again; contacts inside pads; promoting accepted stages into `enhanced`; fixing the Java bugs kept for parity |
| **S** Server and job management | [S00](S00-server-README.md) | 0 | 0 | Placeholder, outside "complete": the REST and MCP API, job scheduling and sessions (`api.*`, `management.*`), and the seams the port already leaves for them |

## Where to start

These have no unmet prerequisites, so they can start now and in parallel:
- F01, F02, F04;
- J01, once F02.4 exists;
- C01, C02;
- B01;
- B07;
- G01–G03;
- K01;
- Q03, a static diagnostic that changes no routes.

The critical path to a routing CLI is:
1. F02, then J01, then B01–B03 with C02–C03;
2. O01–O05;
3. A01–A08, then A10–A13;
4. the A14 board-parity gate;
5. C07–C08, then K02.
Each series README gives its own order, its lanes, and the files each checklist
owns.

## How the checklists work

- **One checklist file is one unit of work.** The `implementer` agent
  (`.claude/agents/implementer.md`) takes one file, or a named subset of its
  items, per dispatch, and stays within 250 turns. The orchestrator reviews,
  runs the gate, and commits one commit per checklist file.
- **Item ids** are `<series><NN>.<n>`, for example `A03.2`. Refer to items by
  id in commits (`A03.2: ...`) and in DIVERGENCES entries.
- **Boxes:**
  - `[ ]` not done.
  - `[x]` done, with a `*Done <date>:*` note under the item giving the test
    names, commands and measured numbers that prove each **Accept:** clause.
  - `[~]` partial, with a `*Partial <date>:*` note naming the remainder and
    the checklist that owns it (an id such as `A13` or `F01.2`): an
    operator decision, a push, a Windows or KiCad check, or work in another
    series that hasn't landed.
  A box is never ticked on a claim nobody ran. Mark each item as soon as it
  is done, in the same change as the work, never later from memory.
- **Each checklist file has these sections:**
  1. A header: when it was opened and why.
  2. **What exists and what this adds**: the Rust and Java starting point,
     so nobody re-ports what exists.
  3. **Decisions made here**: implement them; don't reopen them.
  4. **File ownership**: the files each item creates or may edit, so
     implementers can run in parallel on disjoint files.
  5. **Checklist**: each item gives the Java source (paths at the pin, read
     through git), the Rust design, the orders that decide results (D1), and
     **Accept:** criteria.
- **Anchors drift.** Java line numbers refer to the pinned commit; Rust line
  numbers refer to the commit the checklist was written at. Find code by its
  symbol name.
- **Finished checklists move to `archive/`.** A checklist is finished when no
  box is `[ ]`, every `[x]` has its `*Done*` note, and every `[~]` names the
  checklist that owns its remainder. Archive it with
  `python3 tools/roadmap/checklists.py archive <file>`. The tool refuses an
  unfinished checklist, moves it (`git mv`), rewrites every link to and from
  it, marks it ✅ in its series README, and fails if any roadmap link breaks.
  The implementer archives its checklist when its dispatch finishes the last
  item; the orchestrator commits the move with the work.
- **Check the state after every dispatch:**
  `python3 tools/roadmap/checklists.py status --strict`. It lists every
  checklist's `[x]`/`[~]`/`[ ]` counts, flags any ticked box without its
  evidence note, and names finished checklists that haven't been archived.

## Shared rules

These hold for every checklist. A checklist may add to them, never relax
them.

- **Results match Java; structure is free (D1).** Port behaviour, not class
  layout. Where a checklist names an iteration order, a tie-break or a
  floating-point operation order that decides results, reproduce it exactly.
- **Deterministic (D2).**
  - Seeded `JavaRandom` from `freerouting_geometry::jmath`.
  - No hash-order iteration reaching a result.
  - Parallel work merged in submission order.
  - Budgets count work, never time.
  - Any checklist that routes, optimizes or edits proves determinism with
    `tools/determinism-check.sh` (F02.4).
- **Java at the pin, through git.** Read `../freerouting` only with
  `git -C ../freerouting show <pin>:<path>`. Expected values in differential
  tests come from running Java (`tools/java-reference/probe.sh`, F02.1),
  never from reasoning about what Java would do.
- **Every deliberate difference from Java** gets three things: a test that
  pins it, a note in the module docs, and a line in
  [DIVERGENCES.md](../../DIVERGENCES.md). An expected mismatch lives in the
  test that would otherwise fail, with its reason.
- **Performance work measures first** ([performance.md](../performance.md)):
  release build, quiet machine, before and after. After the parity rung, an
  optimization must leave routed results byte-identical (`ses-diff`).
- **The gate**: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace`. These are the same commands as CI.
- **No new dependency** without a `NEW DEP: <crate> — <why>` line in the
  report. The orchestrator decides.
- **The board's design rules are hard limits (D8).** Never narrower, smaller or
  closer than the board allows, and never rewrite the board's rules. Grade every
  result under the board's original rules.
- **Owner rules (D8) are behaviour.** No via in a pad. Honour
  `(attach off)`. Plane-net pads get their own via to the plane.
