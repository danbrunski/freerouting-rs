# K04 — `.kicad_pcb` syntax layer and test corpus

Opened 2026-10-15. KiCad's DSN export loses information Freerouting needs:
rule-area kinds, track arcs, and some precision. Shapes, paths, wires, vias
and image pins are printed with C `%.6g` (`specctra.h` at `10.0.0`:
`POINT::Format`, `PATH::Format`, `CIRCLE::Format`, `PIN::Format`), so
absolute coordinates above 100 mm lose everything below 1 µm: the via at
`(at 116.9009 64.0861)` in `canary-pwr.kicad_pcb` is written
`116901 -64086.1` (`canary-pwr.dsn:3299`). Placements are printed with
`FormatDouble2Str` (`PLACE::Format`, `specctra.cpp`) and keep nanometres
(`place C5 133199.999 ...`, `canary-pwr.dsn:107`). The K
series replaces that export with a direct reader of `.kicad_pcb` files that
produces the same DSN syntax tree `build_board` already consumes
([K00](K00-kicad-README.md)). This checklist builds the bottom layer: a new
crate with an s-expression lexer and tree, the version gates, net-dialect
detection, and a test corpus paired with KiCad 10's own DSN exports, so
K05–K07 can compare the reader against KiCad bit for bit.

Prerequisites: none. KiCad 10 is needed only for K04.8. This machine has
two: Windows KiCad 10.0.5 (Dan's, the one the plugin runs in) and WSL KiCad
10.0.7 from the Ubuntu packages (`/usr/bin/kicad-cli`; system `python3`
imports `pcbnew`), see K00 "Two KiCads". [F04](F04-provenance.md) is a prerequisite
only if an item translates KiCadRoutingTools (KRT) code; the plan here takes
KRT's pitfalls and test cases, not its code.

## What exists and what this adds

| Exists | State |
|---|---|
| `crates/freerouting-dsn` | DSN/SES/rules lexer, typed syntax tree (`ast::Pcb`), writer (`Pcb::to_dsn`). Its lexer has DSN-only modes (`string_quote`, `space_in_quoted_tokens`) and must not be bent to read KiCad files |
| `freerouting::io::specctra::build_board(&Pcb)` | The only board builder. Stays so (K00) |
| KRT `py_router/kicad_parser.py` at `0486e799` (10,451 lines, Python) | Regex and paren scanning, no tree. Its version constants and pitfall list are the useful part: `FIRST_SUPPORTED_BOARD_VERSION = 20201115`, `LEGACY_ARC_FORMATTING = 20210925`, `NEW_OVERBAR_NOTATION = 20210606`, `KICAD_10_MIN_VERSION = 20250000`, `board_uses_name_nets`, `find_matching_paren` (#113), `reflow_one_line_board` |
| KiCad's own reader, `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` at tag `10.0.0` (GitLab `kicad/code/kicad`) | The authority for every format question. Fetch with `curl -sS https://gitlab.com/kicad/code/kicad/-/raw/10.0.0/<path>` into the scratchpad. KiCad is GPL-2.0-or-later, so its algorithms may be ported here |
| Java repo boards at `cda540f` | 38 `.kicad_pcb` files under `fixtures/` and `examples/`; 37 are `(version 20211014)` or later, 14 MB total, largest `Issue230-CNH_Functional_Tester_1.kicad_pcb` (2.4 MB). `Issue069-TestSensel` is KiCad 5 (`20171130`) |
| KiCad 10 demos | 18 boards, in `/usr/share/kicad/demos` (WSL, 10.0.7) and `/mnt/c/Program Files/KiCad/10.0/share/kicad/demos` (Windows, 10.0.5); the two copies differ (`jetson-agx-thor-baseboard.kicad_pcb` is 85 MB in WSL, 89 MB on Windows). 72 KB up to that size. Licences per demo folder (`cm5_minima/LICENSE.txt` is CERN-OHL-S-2.0). Out of repo |
| Java repo PCBench (`scripts/benchmark/fixtures/PCBench`, 1,157 boards) | All sampled files are `(version 20171130)` or `(version 3/4)`: KiCad 5 and older. Excluded |

What this adds:
- `crates/freerouting-kicad` with `sexpr` (lexer and tree), `header`
  (version gates), `nets` (dialect detection);
- `fixtures/kicad/`: seven Java-repo boards plus their KiCad 10 DSN exports
  and a manifest;
- `tools/kicad-reader/`: the corpus fetch script and the headless KiCad DSN
  export used to make reference exports.

## Decisions made here

- **A tree, not regex scanning.** KRT scans the text with regexes per block
  kind and had to add a reflow step for one-line boards (KRT
  `reflow_one_line_board`: on a one-line board it measured zones 1 → 0 and
  copper layers 2 → 0). A real tokenizer is layout-independent, so one-line
  boards need no special path.
- **Arena layout for speed.** The largest demo board is 89 MB. The tree is
  one `Vec<Node>` in pre-order plus a `Vec<u32>` of child indices, with atoms
  and strings as byte spans into the source (`&str` borrowed, no copies).
  Escaped strings are unescaped lazily on access. No recursion in the parser
  (an explicit stack), so deep nesting cannot overflow the thread stack.
- **Numbers stay text until used.** The tree stores the atom span. Distance
  conversion happens in K05 with KiCad's exact operation order:
  `nm = llround(parse_f64(text) * 1e6)` (`parseBoardUnits`,
  `pcb_io_kicad_sexpr_parser.cpp`; `KiROUND` is `std::llround`,
  `libs/kimath/include/math/util.h`). Parsing early would lose nothing but
  would cost time on numbers no one reads.
- **Refuse what KiCad 6 can't read.** Files before `(version 20201115)` use
  `(module ...)` blocks and bare net names. They are refused with one line
  naming the version and saying "open and save the board in KiCad 6 or
  later", never half-read (KRT `UnsupportedBoardFormat`).
- **The net dialect comes from the content, not the stamp.** KiCad 10
  writes `(net "GND")` and no net table; KiCad 9 writes a table
  `(net 1 "GND")` and numeric references. The stamp alone is wrong for real
  files: the KiCad 10 demo `CM5_MINIMA_3.kicad_pcb` is `(version 20250513)`,
  above KRT's `KICAD_10_MIN_VERSION`, and still has a numeric table.
- **Committed corpus: small, with provenance.** Seven Java-repo boards whose
  DSN versions this repo already carries as fixtures go into
  `fixtures/kicad/`, one per KiCad file generation (6, 7, 8, 9, 10) plus the
  two rule-area boards, about 1.4 MB of `.kicad_pcb`. Their manifest names
  the Java commit and path and marks them `LicenseRef-freerouting-fixture`
  (same status as the existing `fixtures/*.dsn`). Everything else (the
  other 30 Java boards, the KiCad demos, `canary-pwr`, F03.3's corpus) stays
  out of the repo, in `$FR_KICAD_CORPUS`. If F03.3's licence allow-list
  rejects `LicenseRef-freerouting-fixture`, these seven move out too (see the
  done note of K04.7).
- **Reference DSNs are KiCad 10 exports, committed.** Tests compare against
  `fixtures/kicad/<board>.kicad10.dsn`, made once by K04.8 with KiCad's
  `pcbnew.ExportSpecctraDSN`, so `cargo test` runs offline and without KiCad.
  The older DSNs in `fixtures/*.dsn` came from various KiCad versions and are
  not used as references.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K04.1 | `crates/freerouting-kicad/{Cargo.toml,src/lib.rs,src/error.rs}` | — (workspace members are `crates/*`) |
| K04.2 | `crates/freerouting-kicad/src/sexpr/lexer.rs` | `src/lib.rs` (module list) |
| K04.3 | `crates/freerouting-kicad/src/sexpr/{mod.rs,tree.rs}`, `benches/` or `tests/parse_time.rs` | — |
| K04.4 | `crates/freerouting-kicad/src/header.rs`, `tests/header.rs` | — |
| K04.5 | `crates/freerouting-kicad/src/legacy.rs` | `src/header.rs` |
| K04.6 | `crates/freerouting-kicad/src/nets.rs`, `tests/nets.rs` | — |
| K04.7 | `fixtures/kicad/**` (boards, `MANIFEST.tsv`, `README.md`), `tools/kicad-reader/fetch-corpus.sh`, `tools/bench/sets/kicad-reader.txt` | — |
| K04.8 | `tools/kicad-reader/{export.sh,export_dsn.py,README.md}`, `fixtures/kicad/**/*.kicad10.dsn` | — |

## Checklist

- [ ] **K04.1 — Crate skeleton.**
  - `crates/freerouting-kicad`, `license.workspace = true`, depending only on
    `freerouting-dsn` and `freerouting-geometry` (K00 layering). No other
    dependency in this checklist.
  - `lib.rs` module docs: what the crate reads, that its output is a
    `freerouting_dsn::ast::Pcb` in KiCad's DSN frame, and a "Credits"
    paragraph naming KRT (`drandyhaas/KiCadRoutingTools`, MIT) for the
    pitfall list and issue numbers it cites, and KiCad (GPL-2.0-or-later) for
    the algorithms ported from its source.
  - `error.rs`: `KicadError { kind: ErrorKind, line: u32, column: u32,
    message: String }` with `ErrorKind::{Syntax, UnsupportedVersion,
    NotABoard, Semantic}`, and `Warning { line: u32, message: String }`.
  - **Accept:** the gate is green with the empty crate; `cargo doc -p
    freerouting-kicad` builds without warnings.

- [ ] **K04.2 — Lexer.**
  - Tokens: `(`, `)`, bare atoms (symbols and numbers, terminated by
    whitespace, `(`, `)` or `"`), and quoted strings. Inside a quoted string,
    `\"` and `\\` are escapes and parentheses are text. This is KRT #113: a
    part number `"TCR2EF115,LM(CT"` made a naive paren counter swallow the
    following footprints.
  - Each token carries its byte span and line and column, computed lazily
    from the span (count newlines on error only), so the hot loop does no
    line bookkeeping.
  - Errors: an unterminated string at its opening quote; a stray `)` at its
    position; end of file inside a list at the list's opening `(`.
  - The input is `&str`. A file that isn't UTF-8 is an error naming the
    first bad byte offset (KiCad writes UTF-8).
  - **Accept:** unit tests for: the #113 string; `"a \"quoted\" word"`; a
    string with an embedded newline; `(xy 1.5 -2e-3)`; each error kind with
    its line and column; a 1 MB synthetic token stream lexes with no
    allocation per token (check with a counting allocator in the test, or
    assert the token vector is the only growth).

- [ ] **K04.3 — Arena tree.**
  - `Tree<'a> { src: &'a str, nodes: Vec<Node>, children: Vec<u32> }`,
    `Node::{List { first: u32, len: u32 }, Atom(Span), Str(Span)}`. Built in
    one pass with an explicit stack.
  - Accessors on `NodeRef<'a>`: `head() -> Option<&'a str>` (the first
    atom), `items()`, `child(name)` (first child list with that head),
    `children(name)` (iterator, file order), `atom(i)`, `str_value(i)`
    (unescaped `Cow<str>`), `f64(i)`, `line()`.
  - **Order that decides results:** children are kept in file order, and
    every accessor iterates in file order. K05–K06 rely on it to reproduce
    KiCad's item order.
  - Parse time: lexing plus tree for every board in the corpus, release
    build, quiet machine, three runs, median.
  - **Accept:**
    - A one-line rewrite of `fixtures/kicad/Issue368-CorneyIslandWireless`
      (all whitespace between tokens collapsed to one space) gives a tree
      equal to the original's, node for node.
    - Deeply nested input (100,000 levels) parses without stack overflow.
    - The done note gives MB/s and the time for
      `jetson-agx-thor-baseboard.kicad_pcb` (85 MB in WSL). Target ≤ 1 s;
      record the number either way.

- [ ] **K04.4 — Header and version gate.**
  - Read `(kicad_pcb (version N) (generator ...) (generator_version ...))`.
    KiCad 6+ files start this way; KiCad 5 has `(host pcbnew ...)` instead
    of `generator`.
  - A top-level list that isn't `kicad_pcb` is `NotABoard`. `N < 20201115`
    is `UnsupportedVersion` with the message from the decisions above.
  - Expose `Header { version: u32, generator: String, generator_version:
    Option<String> }`.
  - **Accept:**
    - `Issue069-TestSensel.kicad_pcb` (`20171130`, read through `git show`)
      fails with one line containing `20171130`.
    - A PCBench file with `(version 3)` fails the same way.
    - All 37 newer Java-repo boards and all 18 demos pass the gate.

- [ ] **K04.5 — Legacy conversions.**
  KiCad converts some older spellings on load; the reader does the same, on
  the tree, so later items see one format. Port each from KiCad's reader,
  not from KRT:
  - arcs written as `(start CENTER) (end ARC_START) (angle SWEEP)` at
    `version <= 20210925` become start/mid/end (KiCad's
    `LEGACY_ARC_FORMATTING` in `pcb_io_kicad_sexpr.h`);
  - net and pad names with `~TEXT~` overbars below `20210606` become
    `~{TEXT}` (KiCad's `ConvertToNewOverbarNotation`). Net names reach the DSN
    and the SES, so this decides whether KiCad's SES import finds the net.
  - `(tstamp ...)` is read as the item's uuid where `(uuid ...)` is absent.
  - **Accept:** unit tests on hand-written snippets for each conversion, with
    expected values computed by hand from KiCad's code and cited in the test.
    None of the committed boards hits the arc or overbar case (the oldest is
    `20211014`); the done note says so.

- [ ] **K04.6 — Net dialect and net table.**
  - Detect in this order (KRT `board_uses_name_nets`, KRT #1028, #163):
    1. any `(net "name")` reference → names;
    2. a top-level `(net N "name")` table and no name references → ids;
    3. neither → the stamp decides (`>= 20250000` → names).
  - `NetTable { dialect, by_code: Vec<Option<String>>, codes_by_name:
    BTreeMap<String, u32> }`. Name dialect: codes are assigned in order of
    first appearance in the file. Whether that matches the codes KiCad 10
    assigns on load is unverified; K05.2 checks it against the export's net
    order.
  - Lookups use the raw escaped token text, not the unescaped string (KRT
    #369 A12: unescaping first put a zone on the wrong net).
  - No `HashMap` reaches a result (shared rule D2): `BTreeMap` or vectors.
  - **Accept:**
    - `CM5_MINIMA_3.kicad_pcb` (`20250513`, numeric table) → ids, 2 nets
      checked by name.
    - `pic_programmer.kicad_pcb` and `Issue742-tastexx-pcb` (`20260206`) →
      names.
    - A synthetic KiCad 10 stamp over a numeric table (#1028) → ids, every
      net resolved.
    - A board with no nets → decided by the stamp.

- [ ] **K04.7 — Corpus.**
  - Copy with `git -C ../freerouting show cda540f:<path>` into
    `fixtures/kicad/<Issue dir>/`, with the `.kicad_pro` next to it where the
    Java repo has one:

    | Board | Version (KiCad) | Size | Why |
    |---|---|---|---|
    | `Issue368-CorneyIslandWireless/corney_island_wireless.kicad_pcb` | 20240108 (8) | 12 KB | back-side footprints, NPTH holes |
    | `Issue269-min_fr_test/min_fr_test.kicad_pcb` | 20240108 (8) | 18 KB | 4 copper layers, a zone |
    | `Issue508-DAC2020/DAC2020_bm01/bm1.unrouted.kicad_pcb` | 20241229 (9) | 145 KB | no `.kicad_pro` (KiCad defaults); has a Java route reference |
    | `Issue558-dev-board-autoroute-demo/dev-board.kicad_pcb` | 20241229 (9) | 133 KB | a rule area, back side, oval and roundrect pads |
    | `Issue742-tastexx-pcb/tastexx-pcb.kicad_pcb` | 20260206 (10) | 172 KB | name-net dialect |
    | `Issue184-motorizedopener/motorizedopener.kicad_pcb` | 20211014 (6) | 225 KB | KiCad 6 project format, 10 back-side parts |
    | `Issue180-Test/Test.kicad_pcb` | 20221018 (7) | 726 KB | 4 layers, 102 segments, 627 vias (locked, KiCad 7 `(via locked ...)` form), 1 copper zone plus 5 rule areas |

  - `fixtures/kicad/MANIFEST.tsv`: path, source (`freerouting@cda540f:<path>`),
    sha256, bytes, file version, `.kicad_pro` present, licence
    (`LicenseRef-freerouting-fixture`). `fixtures/kicad/README.md` explains
    the set, the out-of-repo corpus, and how to regenerate exports (K04.8).
  - `tools/kicad-reader/fetch-corpus.sh <dir>` fills `$FR_KICAD_CORPUS` with:
    the other 30 Java boards (`git show`), the 18 demos (copied from
    `/usr/share/kicad/demos`, each with its folder's licence file), and a
    snapshot of `canary-pwr` (its `.kicad_pcb`,
    `.kicad_pro` and `.kicad_dru` copied together, sha256 recorded, because
    the board is being edited: the `freerouting-rs/canary-pwr.dsn` of 17:46
    no longer matches the 19:48 `.kicad_pro`). Missing sources are skipped
    with a note. `tools/bench/sets/kicad-reader.txt` lists the corpus.
  - **Accept:**
    - every committed board's sha256 equals `git show` output at `cda540f`;
    - fetching twice gives byte-identical files (canary excepted, by sha);
    - the done note states whether F03.3's licence rule accepts the
      committed set, or that the set moved out of the repo.

- [ ] **K04.8 — KiCad 10 reference exports.**
  - `tools/kicad-reader/export_dsn.py` runs under a KiCad interpreter:
    `pcbnew.LoadBoard`, then `pcbnew.ExportSpecctraDSN(board, out)`, then
    prints `pcbnew.GetBuildVersion()`. Default: WSL KiCad 10.0.7, run as
    `python3 -I export_dsn.py` (no path translation). Optional: Windows
    KiCad 10.0.5 (`/mnt/c/Program Files/KiCad/10.0/bin/python.exe`, paths
    through `wslpath -w`), the version Dan's plugin uses. The script never
    imports the plugin package, in either interpreter (`register()` outside
    the editor raises a dialog on Dan's desktop).
  - `tools/kicad-reader/export.sh [--windows] <board.kicad_pcb> <out.dsn>`:
    copies the board and its `.kicad_pro`/`.kicad_dru` into a scratch folder,
    runs the script there, and copies the DSN back. It records the KiCad
    PIDs before and after with `pgrep -x kicad; pgrep -x pcbnew` (Linux) and
    `tasklist.exe /FI "IMAGENAME eq kicad.exe"` (Windows), and fails if they
    differ. Don't use `pgrep -f -i kicad`: it also matches unrelated
    processes whose paths contain "KiCad", such as KRT runs. A Linux
    `kicad` GUI process was running on 2026-10-15; never stop it. The script
    refuses to run on a path inside `fixtures/` or the canary repo (copies
    only). If F02.6 has landed, it may call F02.6's export step instead.
  - Export each committed board to `fixtures/kicad/<dir>/<stem>.kicad10.dsn`.
    The DSN's first line holds the export path; the parity tests ignore the
    `pcb` name.
  - Export the out-of-repo corpus into `$FR_KICAD_CORPUS` the same way.
  - **Accept:**
    - seven committed exports made with WSL KiCad 10.0.7, the version and
      the `(host_version ...)` line recorded in the manifest;
    - one of them also exported with Windows KiCad 10.0.5, and any
      difference between the two exports named in the done note;
    - exporting the same board twice gives identical files apart from the
      first line (KiCad orders non-default net classes through a
      `std::unordered_map` in `SPECCTRA_DB::FromBOARD`, so check this rather
      than assume it);
    - `pgrep` shows the same KiCad processes before and after; the originals'
      checksums are unchanged.
