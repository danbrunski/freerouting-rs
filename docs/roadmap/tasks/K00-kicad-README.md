# K00 — KiCad plugin and `.kicad_pcb` importer

Opened 2026-10-15. The K series makes the KiCad plugin route a real board end
to end (item 6 of "What 'complete' means" in
[port-completion.md](../port-completion.md)), packages it for KiCad's plugin
manager, and adds a direct `.kicad_pcb` reader. The reader fixes what
KiCad's DSN export loses: rule-area kinds, arcs, rotated and rounded pads.
The narrative is in [desktop-app.md](../desktop-app.md). Sources: the GUI,
plugin and packaging report of 2026-10-15 (§5, §6) and the
KiCadRoutingTools (KRT) report (§6, §8, "Proposed roadmap changes").

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [K01](archive/K01-plugin-before-routing.md) ✅ | Headless plugin tests, the rule-area DSN fix-up (the `canary-pwr` fan inlet), version handshake, UI-thread and shadow-install guards, the SWIG/IPC note, the owner's confirmation in KiCad | M / low | none |
| [K02](archive/K02-plugin-routing.md) ✅ | Route options persisted per board, live progress, Stop vs Abort, result summary, "Route in GUI", non-ASCII round trip, five demo boards routed and graded by KiCad | M / medium | A13, C05, C07, C08 (the CLI routes); F02.6; G08.4 for K02.5 |
| [K03](archive/K03-pcm-package.md) ✅ | One cross-platform PCM zip with a binary resolver, validated metadata, install from file on all three OSes | S / medium | K01, R01 |
| [K04](archive/K04-kicad-pcb-syntax.md) ✅ | `crates/freerouting-kicad`: s-expression lexer and tree, version gates, net-dialect detection, the test corpus | M / low | none |
| [K05](archive/K05-kicad-pcb-board-structure.md) ✅ | Layers, nets and net classes (`.kicad_pro`), board outline, footprints and pads, as a DSN syntax tree | L / medium | K04 |
| [K06](archive/K06-kicad-pcb-rule-areas-and-copper.md) ✅ | Rule areas to keepout kinds, zones, tracks and arcs, vias | M / medium | K05 |
| [K07](archive/K07-kicad-pcb-entry-points-and-parity.md) ✅ | `.kicad_pcb` opens in the CLI and the GUI; parity against KiCad's DSN export on the corpus; `.kicad_dru` subset; the coordinate-frame check | M / medium | K06; C07 (CLI `-de`); G01 (GUI `Session`) |
| [K08](archive/K08-kicad-pcb-writer.md) ✅ | *Optional.* A direct `.kicad_pcb` writer with deterministic UUIDs, for when KiCad drops SWIG (`-do routed.kicad_pcb`; KiCad's DRC grades it like `ImportSpecctraSES` on three boards; K08.4 `[~]`: the owner to accept an `f32` sum-order difference in the round-trip statistics) | M / medium | K07, C05; the owner's go-ahead (question 5, given 2026-10-17) |
| [K09](archive/K09-plugin-export-options.md) ✅ | Plugin options that reproduce canary's graded `dsn_export.py` flow: class widths, pours of listed classes kept clear, outer GND pours dropped, edge and pad keepouts, plane layers opened, add-only second run, wasted-pass stop, a canary preset | M / medium: measured on `canary-pwr` | K01, K02, K03; Q01, Q09; Q10 (K09.6); Q04 (K09.7) |
| [K10](archive/K10-plugin-direct-board-read.md) ✅ | An option for the plugin to snapshot the open board (`SaveBoard`) and route the `.kicad_pcb` with the reader instead of KiCad's DSN export; results still come back through `ImportSpecctraSES`; measured against the DSN path, the owner picks the default | M / medium: graded by KiCad on the K02 boards and `canary-pwr` | K02, K07; K09 (K10.4) |
| [K11](K11-kicad11-ipc-plugin.md) | KiCad 11 has no SWIG: an IPC-API plugin that keeps the DSN/SES flow (`RunBoardJobExportSpecctraDsn`, `ImportSpecctraSession`), a dialog without wx, both plugins in one package | M / high: the API is new and unwrapped by `kipy` | K02, K03; K10 (K11.6); a KiCad 11 release candidate or release date (K11.2 onward) |
| [K12](archive/K12-plugin-cli-preparation.md) ✅ | Shared plugin/script preparation and invocation, zone refills, current minimum width and input/argument records | S / medium: verified with real KiCad exports and small-board routing | K09, K10 |
| [K13](K13-kicad-pcb-input-by-default.md) | The plugin reads the board directly (`board_input = kicad_pcb`) by default once that grades the same or better on K10.5's boards: what each input carries, existing copper returned unchanged, a DSN fallback, load time, the measurement and the switch | M / medium: the reader must carry every rule the DSN path now gets as flags | K10, Q18, Q19 |

The report's PLG-1 is K01, PLG-2 is K02, PLG-1.5 (PCM) grew into K03, and
KIC-1 is split over K04–K07. KRT's proposed K01–K10 are folded in: its
syntax layer, board model, rule areas and design rules are K04–K07, its
writer is K08, its PCM, handshake, settings, UI-thread and IPC items are
K01–K03. Its numbering is not used.

## Order and critical path

- **The plugin run is the test (owner, 2026-10-17).** A plugin run with default options
  inside KiCad is the result that counts on `canary-pwr`; the CLI flow that grades 33 is
  only the target. So K09 (with K09.9) ranks with Q13 and Q14, ahead of the other K
  items. Every fix the CLI flow needed belongs in the plugin or the router: pours as fixed
  copper, the fan keepout as placement-only, edge and fiducial clearances, tap widths,
  no via in a pad, and no track on an inner layer.
- **K01 now.** Nothing in it needs the router. K01.2 delivers the
  rule-area fix for the owner's boards through the plugin, long before the reader.
- **K02 when the CLI routes**: the A series' batch pipeline (A13), the SES
  writer (C05), CLI parity (C07) and the plugin-facing CLI extras (C08).
  K02 is on the port's critical path: J01 → B → O → A → C → K02.
- **K03 after R01**, which builds the binaries the package carries.
- **K04–K07 have no router dependency** and touch only their own crate until
  K07. They can run in parallel with anything. port-completion.md puts the
  importer outside "complete", so schedule them after K02 unless the owner wants
  them sooner. K01.2 already covers the rule-area lesson for exported DSNs.
- **K08 only on the owner's go-ahead** (given 2026-10-17; done the same day).
- **K10 any time after K07.** It needs no router change. K10.4 waits for K09's options
  to exist, and K10.5's answer on the default is the owner's.
- **K11.1 now, the rest of K11 when KiCad 11 is near.** The probe runs on a nightly and
  settles the facts. The plugin is built once there is a release candidate or a release
  date (K11 question 2).

## Parallel lanes and file ownership

| Lane | Owns | Shared, append-only |
|---|---|---|
| Plugin (K01, K02, K03) | `kicad-plugin/**` | `docs/development/kicad-plugin.md` |
| Reader (K04–K06) | `crates/freerouting-kicad/**`, `fixtures/kicad/**` | — |
| Reader wiring (K06.1, K07) | — | `crates/freerouting-dsn/src/{ast.rs,lib.rs,parse/mod.rs,parse/dsn.rs,parse/common.rs}`, `crates/freerouting-dsn/tests/syntax.rs` and `crates/freerouting/src/io/specctra/parser/{structure,library}.rs` (one `KeepoutKind` variant, K06.1 only; coordinate with C06), `crates/freerouting/Cargo.toml`, `crates/freerouting/src/io/mod.rs`, `crates/freerouting-cli/src/` (extension dispatch), `crates/freerouting-gui/src/session.rs` (open `.kicad_pcb`) |
| Writer (K08) | `crates/freerouting-kicad/src/write/**` | — |
| Plugin input (K10) | `kicad-plugin/freerouting_rs/snapshot.py` | `kicad-plugin/freerouting_rs/{plugin,options,routing}.py` (after K09's edits land), `crates/freerouting-cli/src/` (K10.3, one switch) |
| IPC plugin (K11) | `kicad-plugin/ipc/**`, `tools/kicad-ipc/**` | `kicad-plugin/{install,package}.py`, `tools/install_plugin.py` |
| Input default (K13) | reader tests under `crates/freerouting-kicad/tests/`, `tools/bench/sets/k13-input.txt` | `crates/freerouting-kicad/src/**`, `kicad-plugin/freerouting_rs/{snapshot,export_flow,plugin,options}.py`, `tools/kicad-reader/plugin-input-ab.sh` |

The plugin lane and the reader lane are disjoint and can run in parallel.
Within each lane the checklists run in order.

**Crate layering.** `freerouting-kicad` depends only on `freerouting-dsn`
(its output is a `freerouting_dsn::ast::Pcb`) and `freerouting-geometry`.
The engine crate `freerouting` depends on it and exposes
`freerouting::io::kicad`, so the CLI and the GUI keep depending only on
`freerouting` (implementer agent, "Crate dependencies"). `build_board` stays
the only board builder.

## Rules for this series

These add to the shared rules in [README.md](README.md). The first five are
hard rules from `kicad-plugin.md` and the F02.6 notes.

- **Never close, restart or kill KiCad**, and never touch the Java
  Freerouting plugin (`3rdparty\plugins\app_freerouting_kicad-plugin`) or
  its Java processes. The owner runs it beside this one. Check with
  `pgrep -x kicad; pgrep -x pcbnew` (WSL) and `tasklist` (Windows) before
  and after any KiCad step. Not `pgrep -f kicad`: it also matches unrelated
  processes, such as KRT runs.
- **Never import the plugin package in a standalone KiCad Python.**
  `register()` outside the editor raises a wxWidgets "PgmOrNull() failed"
  dialog on the owner's desktop. Headless scripts use `pcbnew.LoadBoard` and
  `ExportSpecctraDSN` directly, on copies of the board.
- **KiCad launches the Windows exes in the Windows checkout's
  `target\release`** (`C:\Users\thebo\Documents\code\freerouting-rs`). A WSL
  build does not update them. Rebuild natively after any CLI or GUI change
  the plugin should see.
- **Native Windows builds need mingw on PATH** (the WinLibs path in
  `kicad-plugin.md`), because Rust's gnu `dlltool` calls an assembler it
  doesn't ship. Close the viewer first: Windows locks the running exe.
- **Two KiCads.** The owner's plugin runs in Windows KiCad 10.0.5
  (`/mnt/c/Program Files/KiCad/10.0/bin/`: `kicad-cli.exe`, `python.exe`;
  pass paths through `wslpath -w`). WSL also has KiCad 10.0.7 from the
  Ubuntu packages (`/usr/bin/kicad-cli`; system `python3` imports
  `pcbnew`; demos in `/usr/share/kicad/demos`). Headless steps may use
  either; record which, because 10.0.5 and 10.0.7 can differ. A Linux
  `kicad` GUI process was running on 2026-10-15 (started 19:27): it falls
  under the first rule like the Windows one.
- **The owner's display is at 125 %.** Screen captures on Windows call
  `SetProcessDPIAware()` first, or use the app's `--screenshot` (G01.5).
- **pcbnew is called only on KiCad's UI thread.** A worker-thread
  `SaveBoard` froze KiCad at 0 % CPU (KRT issue #688,
  `py_router/ui_thread.py`). Worker threads only run and read the
  subprocess.
- **Testable plugin code is pure Python.** Modules other than `plugin.py`
  import neither `pcbnew` nor `wx`, and the tests in `kicad-plugin/tests/`
  run with plain Python 3 (`python3 -m pytest kicad-plugin/tests`). `pytest`
  is a `NEW DEP` for the dev environment only.
- **The reader uses KiCad's DSN export frame** (question 2): `(unit um)`,
  `(resolution um 10)`, coordinates in µm as `f64` (KiCad nm / 1000), Y
  negated, no offset. On `canary-pwr` the fan inlet at (136.5, 53) mm is
  `136500 -53000` in the DSN (`canary-pwr/freerouting-rs/canary-pwr.dsn:64`).
  The reader keeps KiCad's full nm precision, which KiCad's export loses on
  shapes (`%.6g`), so K07 compares with the export within half a `%.6g`
  step and otherwise exactly.
- **Reader results are graded two ways:** against `build_board` of KiCad 10's
  DSN export of the same board (K07), and by KiCad's own DRC through F02.6.
- **KRT code may be ported** (MIT into GPL-3.0, decision D9), following
  [F04](archive/F04-provenance.md): the file keeps KRT's copyright line and an SPDX
  header, names its KRT source path and commit (`0486e799` at the time of
  writing), and gets a row in `docs/PROVENANCE.md`. An item that translates
  KRT code lists F04 as a prerequisite. Ideas without code are credited in
  the module docs. KRT's parser is Python; most of what transfers is its
  list of pitfalls, each linked to its KRT issue number.
- **Boards.** KiCad's bundled demos (18 boards from 72 KB to 89 MB, in
  `C:\Program Files\KiCad\10.0\share\kicad\demos` or
  `/usr/share/kicad/demos`) and the owner's boards stay out of the repo; board
  sets name them by path and skip them when missing. The Java repo's
  KiCad 6–10 fixtures (`git -C ../freerouting ls-tree -r --name-only cda540f`
  under `fixtures/` and `examples/`: 37 files at `(version 20211014)` or
  later; one more, `Issue069-TestSensel`, is KiCad 5) may be copied into
  `fixtures/kicad/` (K04.7). F03.3's out-of-repo corpus of open KiCad
  boards can extend K07's parity set; its licence rule applies to anything
  committed. The 1,157 PCBench boards in the Java repo are
  KiCad 5 files (`(version 20171130)` or older), which the reader refuses,
  so they are not part of the corpus.

## Open questions for the owner

1. **Tracks-only rule areas (`wire_keepout`).** KiCad exports a tracks-only
   rule area as `wire_keepout`. Java lexes that as `keepout`
   (`SpecctraFileDescription.flex:201`), and so does the Rust parser
   (`freerouting-dsn/src/parse/mod.rs`, `canonical_keyword`), so the area
   blocks vias too. `Issue754-avionics_hub.dsn:53` has one over the whole
   Ground layer. A trace-only area kind would change routing results
   against Java. *Recommendation:* keep Java's behaviour by default. K06.1
   keeps the distinction in the syntax tree (`KeepoutKind::WireKeepout`,
   still built as a full keepout) and reports each such area as a warning.
   Making it trace-only becomes a measured Q-series divergence after board
   parity, measured on Issue754 and the `canary-pwr` variants. It needs a
   new Q checklist: [Q09](archive/Q09-trace-only-keepouts.md).
2. **KiCad's coordinate resolution.** KiCad's DSN declares
   `(resolution um 10)` (tenths of µm) and writes coordinates in µm.
   Placements keep KiCad's nm precision (`133199.999` at
   `canary-pwr.dsn:107`); shapes, wires, vias and pins are printed with
   `%.6g`, which loses about 1 µm above 100 mm (K04, from KiCad 10.0.0's
   exporter). The exporter does not rescale for the resolution, and the SES
   import computes `nm = KiROUND(1000 · v / resolution)`. `build_board`
   rounds to 0.1 µm board units. K05 decides to keep full nm precision
   rather than reproduce the `%.6g` loss, and K07.4 checks the frame with a
   real KiCad round trip. Still open: whether a board resolution finer than
   0.1 µm (losing at most 50 nm per coordinate today) is worth it.
   *Recommendation:* no change before board parity; afterwards only as a
   measured change, using K07.4's numbers.
3. **Rule-area fix-up on by default.** K01.2 rewrites areas whose flags ban
   only footprints, pads or copper pour from `keepout` to `place_keepout`
   or drops them. That gives the router room Java never had on the same
   KiCad export. *Recommendation:* on by default, as the owner lesson asks,
   with a per-board option to turn it off and a log line per rewritten
   area.
4. **PCM distribution.** *Recommendation:* ship the zip as a GitHub release
   asset ("Install from File") first. Submit to the official KiCad PCM
   repository only after K02.7 has routed real boards. The identifier must
   differ from the Java plugin's `app.freerouting.kicad-plugin`; proposed
   `com.github.danbrunski.freerouting-rs`.
5. **KiCad 11.** KRT's docstring says KiCad master removed SWIG in 2026-03
   (commit `65a442b1`, unverified here). The Java fork's own research note
   (`git -C ../freerouting show main:docs/research/kicad_ipc_api_research.md`,
   729 lines) says the same from KiCad's side: the SWIG bindings are
   deprecated and removed in KiCad 11, and the Protocol Buffers IPC API is
   supported on KiCad 9 and 10. It also sketches an IPC-based plugin. The
   plugin needs `ExportSpecctraDSN` and `ImportSpecctraSES`, which are SWIG
   calls. *Recommendation:* K01.5 records the facts; decide on K08 (direct
   writer) or an IPC-API plugin once KiCad 11's release date is known.
   *Accepted by the owner 2026-10-15.* The owner gave the go-ahead for K08 on 2026-10-17.
6. **What the reader adds beyond KiCad's export.** KiCad's exporter writes
   non-default net classes in `std::unordered_map` order (not
   reproducible), turns placement-only rule areas into full keepouts, and
   drops copper zones owned by footprints. *Recommendations:* sort classes
   by name and list the order difference in K07.3; map rule areas by flags
   (K06.2); keep footprint copper zones as planes and list them as a
   difference, unless the owner prefers KiCad's behaviour.
7. **`.kicad_dru` rules.** K07.5 reads them and reports every rule it can't
   express by name. *Recommendation:* applying the expressible subset stays
   off by default until K07.3 shows it doesn't change parity, then the owner
   decides.
8. **New dependencies and committed boards.** K05 needs `serde_json` (for
   `.kicad_pro`) and K05.6 `i_overlay` (polygon union for custom pads), each
   a `NEW DEP`. K04.7 commits seven boards from the Java repo's fixtures;
   F03's rule is that committed boards need an allow-listed licence.
   *Recommendation:* accept both crates; treat the Java repo's fixtures like
   the DSN fixtures already copied from it, and record their provenance in
   the K04.7 manifest.
9. **Data for the X series.** The DSN syntax tree has no place for the
   stackup, differential-pair rules or length rules that X07, X08 and X12
   need. K05.1 returns what it reads of them in `KicadRead.extras`; nothing
   in the K series uses it. X00 currently cites KRT's proposed numbering
   ("K02 (stackup)", "K02 (part frames)", "K04 (design rules)"); in this
   series those are K05.1, K05.5–K05.6 and K07.5.
