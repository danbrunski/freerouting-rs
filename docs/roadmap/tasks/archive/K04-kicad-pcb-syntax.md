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
([K00](../K00-kicad-README.md)). This checklist builds the bottom layer: a new
crate with an s-expression lexer and tree, the version gates, net-dialect
detection, and a test corpus paired with KiCad 10's own DSN exports, so
K05–K07 can compare the reader against KiCad bit for bit.

Prerequisites: none. KiCad 10 is needed only for K04.8. This machine has
two: Windows KiCad 10.0.5 (the owner's, the one the plugin runs in) and WSL KiCad
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

- [x] **K04.1 — Crate skeleton.**
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

  *Done 2026-10-16:* `crates/freerouting-kicad` (depends on `freerouting-dsn` and `freerouting-geometry` only; `license.workspace = true`). `lib.rs` has the layer overview and a Credits paragraph (KRT for the pitfall list and issue numbers, no code taken; KiCad 10.0.0 for the ported algorithms). `error.rs`: `KicadError { kind, line, column, message }`, `ErrorKind::{Syntax, UnsupportedVersion, NotABoard, Semantic}`, `Warning { line, message }`, `line_col` (lazy, character columns). Gate on this crate: `cargo clippy -p freerouting-kicad --all-targets -- -D warnings` clean, `RUSTDOCFLAGS="-D warnings" cargo doc -p freerouting-kicad --no-deps` clean, `rustfmt --edition 2024 --check` on its files clean. (`cargo fmt --all --check` is red at the time of writing on other sessions' uncommitted engine files, not on this crate.) `tools/check-provenance.sh`: `provenance ok` (no KRT code was ported).

- [x] **K04.2 — Lexer.**
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

  *Done 2026-10-16:* `sexpr/lexer.rs`. Unit tests (`cargo test -p freerouting-kicad --lib`): `issue_113_parens_inside_a_string_are_text`, `escaped_quotes_stay_in_the_string` (`"a \"quoted\" word"`), `string_with_embedded_newline`, `numbers_are_atoms_and_stop_at_delimiters` (`(xy 1.5 -2e-3)`), `error_unterminated_string_points_at_the_opening_quote` (2:6), `error_stray_close_paren` (2:2), `error_end_of_file_inside_a_list_points_at_the_innermost_open_paren` (3:3), `utf8_check_names_the_first_bad_byte` (`decode`), `unescape_follows_dsnlexer` (KiCad's `\a \b \f \n \r \t \v \xHH \NNN`, from `common/dsnlexer.cpp`). `tests/lexer_alloc.rs`: counting global allocator, a 1 MB synthetic stream (more than 200,000 tokens, 2 strings per line) lexes with exactly 0 allocations. Choice: the lexer tracks nesting depth, so a stray `)` and an unclosed list are lexer errors; the end-of-file error points at the innermost unclosed `(` (found by a rescan on the error path only).

- [x] **K04.3 — Arena tree.**
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

  *Done 2026-10-16:* `sexpr/tree.rs`: `Tree<'a> { src, extra, nodes, children }`, `Node::{List{first,len}, Atom(Span), Str(Span)}`, built in one pass with an explicit stack (plus a pending-children buffer so each list's children are contiguous). Node 0 is an implicit root list, so a board is `tree.root().child("kicad_pcb")`. `NodeRef` has `head`, `items`, `item`, `child`, `children` (file order), `atom`, `raw`, `str_value` (lazy `Cow`), `f64`, `line`/`position`, `error`. Synthesised text (legacy conversions) lives in a side buffer addressed by spans at or past `src.len()`. Tests (`tests/tree.rs`): `one_line_rewrite_of_corney_gives_the_same_tree` (all tokens joined by one space; `Tree` equality is iterative and structural, 1,715 nodes each) and the same for all 7 committed boards; `deeply_nested_input_does_not_overflow_the_stack` (100,000 levels, on a 256 KiB thread stack, including equality and drop); `accessors_follow_file_order`, `lines_of_nodes`. Timing (`cargo test -p freerouting-kicad --release --test parse_time -- --ignored --nocapture`, lexing plus tree, median of 3 in-process runs, file read not counted; the machine was heavily loaded, load average about 30, so these are pessimistic): `jetson-agx-thor-baseboard.kicad_pcb` 84.81 MB in 365 ms = 232 MB/s (11.56 M nodes), target <= 1 s met; `vme-wren.kicad_pcb` 69.64 MB in 268 ms = 260 MB/s; the whole corpus of 58 boards, 201.2 MB, in 0.760 s = 265 MB/s; boards under 1 MB run at 0.7 to 1.9 GB/s.

- [x] **K04.4 — Header and version gate.**
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

  *Done 2026-10-16:* `header.rs`: `Header { version: u32, generator: String, generator_version: Option<String> }`, `read_header`, `board`, `check`, `FIRST_SUPPORTED_VERSION = 20201115`, `LATEST_KNOWN_VERSION = 20260206` (`Header::is_newer_than_known`; newer files are read, not refused). A `kicad_pcb` without `(version)` reads as `20201115`, as KiCad's `parseHeader` does. Tests (`tests/header.rs`, 12): `issue069_testsensel_is_refused` (read through `git show cda540f:`; message contains `20171130`, one line, "open and save the board in KiCad 6 or later"); `pcbench_file_with_version_3_is_refused` (real PCBench file from the Java repo) and `pcbench_version_3_is_refused_with_one_line`; `not_a_board` (`NotABoard` for `footprint`, `kicad_sch`, a bare atom, a nested list, an empty file; `Syntax` for data after the board); `committed_boards_pass_the_gate` (versions 20211014, 20221018, 20240108 x2, 20241229 x2, 20260206); `corpus_boards_pass_the_gate_except_kicad_5`: all 38 Java boards (exactly one refused, `TestSensel`, so 37 pass) and all 19 `.kicad_pcb` files of the WSL KiCad 10.0.7 demos pass (the checklist says 18; there are 19 boards because `royalblue54L_feather` holds a second one under `nfc_antenna/`). Windows 10.0.5 demos not checked.

- [x] **K04.5 — Legacy conversions.**
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

  *Done 2026-10-16:* `legacy.rs` (ported from KiCad 10.0.0: `parsePCB_SHAPE`, `EDA_SHAPE::SetArcAngleAndEnd/GetArcMid/CalcArcAngles`, `RotatePoint`, `EDA_ANGLE`, `ConvertToNewOverbarNotation`) and `units.rs` (`parseBoardUnits`, `KiROUND`). `normalise(&mut Tree, version) -> Conversions { arcs, overbars, warnings }`: `gr_arc`/`fp_arc` with `version <= 20210925` become `(start) (mid) (end)` (a broken old arc is left alone with a warning); `~TEXT~` becomes `~{TEXT}` in `(net N "name")` and `(add_net "name")` for `version < 20210606` (a zone's `net_name` is not converted, as in KiCad); `item_uuid` reads `tstamp` where `uuid` is absent (an accessor, not a rewrite). Tests: unit tests in `legacy.rs` with the expected values computed by hand from the cited KiCad code (`arc_90_degrees_counter_example_by_hand` etc.). Beyond the checklist, `tests/legacy_kicad.rs` pins six arcs and three overbar names against what KiCad 10.0.7 itself holds after `pcbnew.LoadBoard` of the old syntax (`tools/kicad-reader/old_arcs.py`): all six arcs agree to the nanometre, and the three net names agree. None of the committed boards hits the arc or overbar case (the oldest is `20211014`), nor does any corpus board that passes the gate.

- [x] **K04.6 — Net dialect and net table.**
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

  *Done 2026-10-16:* `nets.rs`: `Dialect::{Ids, Names}`, `NetTable { dialect, by_code, codes_by_name: BTreeMap, warnings }`, `detect_dialect`, `NetTable::build/build_ids/build_names`, `name`, `code_of_raw`, `net_of`. Lookups use the raw escaped text (`lookups_use_the_raw_escaped_text_369`). Tests (`tests/nets.rs`, 9): `cm5_minima_has_a_numeric_table_despite_its_stamp` (`20250513` -> ids; net 1 `GND`, net 2 `Net-(J1-VCC)`, 221 nets); `name_dialect_boards` (`tastexx-pcb` `20260206` -> names, 29 table entries (net 0 included), all 43 pads with a net resolve; `pic_programmer` -> names, 1 `VCC`, 2 `GND`); `numeric_table_wins_over_a_new_stamp_issue_1028` (synthetic `20260206` over a numeric table -> ids, segment, via and pad nets all resolve); `a_board_with_no_nets_is_decided_by_the_stamp`; `names_get_codes_in_order_of_first_appearance`; `table_problems_are_warnings`; `every_corpus_board_resolves_all_its_tracks_and_vias` (Java boards plus demos, minus TestSensel and the two 70-85 MB demos: 51 ids boards, 3 names boards, every track and via with a `(net)` child resolves). Deviation from the text above: the stamp fallback for a board with no nets uses `20251028` (KiCad 10.0.0's `SEXPR_BOARD_FILE_VERSION` comment "Stop writing netcodes") instead of KRT's `20250000`; it only matters for a board without any net, and `NAME_NETS_FROM_VERSION` is one constant. Name-dialect codes (first appearance, 0 = unconnected) are still unverified against KiCad's export; K05.2 owns that.

- [x] **K04.7 — Corpus.**
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

  *Done 2026-10-16:* `fixtures/kicad/` holds the seven boards and their `.kicad_pro` files (six of seven; `bm1.unrouted` has none), `MANIFEST.tsv` (path, source `freerouting@cda540f:fixtures/<path>`, sha256, bytes, file version, `.kicad_pro`, licence `LicenseRef-freerouting-fixture`, plus the K04.8 export columns) and `README.md`; `tools/kicad-reader/fetch-corpus.sh <dir>` (`--committed` re-extracts the set), `manifest.py write|check`, `tools/bench/sets/kicad-reader.txt` (70 lines: the 7 committed boards, 38 Java boards, 19 demos, canary-pwr). Evidence: `python3 -I tools/kicad-reader/manifest.py check` prints `manifest ok` (every committed board's sha256 equals `git show cda540f:` output); `tests/fixtures.rs::committed_boards_match_the_java_repo` does the byte comparison in Rust. Fetching three times into two fresh folders gave `diff -r` identical trees (106 files; canary-pwr included because nobody edited it in between; its `SNAPSHOT.sha256` pins `canary-pwr.kicad_pcb` at `15aa06a3...`). The corpus has 58 boards: 38 Java, 19 demos (each with its folder's licence file), 1 canary. F03.3's licence rule accepts the committed set: `committed_violations` (`crates/freerouting-dev/tests/corpus.rs::no_committed_board_in_this_repository_has_a_licence_that_is_not_allow_listed`) matches files by sha256 against the corpus manifest and these boards are not in it; it passed with `fixtures/kicad/` present. (`LicenseRef-freerouting-fixture` is not on `allowed-licenses.txt`, so a rule that matched by directory would move the set out; see `fixtures/kicad/README.md`.)

- [x] **K04.8 — KiCad 10 reference exports.**
  - `tools/kicad-reader/export_dsn.py` runs under a KiCad interpreter:
    `pcbnew.LoadBoard`, then `pcbnew.ExportSpecctraDSN(board, out)`, then
    prints `pcbnew.GetBuildVersion()`. Default: WSL KiCad 10.0.7, run as
    `python3 -I export_dsn.py` (no path translation). Optional: Windows
    KiCad 10.0.5 (`/mnt/c/Program Files/KiCad/10.0/bin/python.exe`, paths
    through `wslpath -w`), the version the owner's plugin uses. The script never
    imports the plugin package, in either interpreter (`register()` outside
    the editor raises a dialog on the owner's desktop).
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

  *Done 2026-10-16:* `tools/kicad-reader/export_dsn.py` (`LoadBoard`, `ExportSpecctraDSN`, prints `KICAD_VERSION ...`, never imports the plugin), `export.sh [--windows]` (copies the board, `.kicad_pro` and `.kicad_dru` into a scratch folder, runs `python3 -I` there or Windows KiCad's `python.exe` with `wslpath -w`, compares `pgrep -x kicad`/`pcbnew` or `tasklist.exe` before and after and fails on a difference, refuses a scratch folder inside `fixtures/` or the canary repo, replaces the export's first line with `(pcb "<stem>.dsn"` and strips CR) and `export-corpus.sh`. Evidence: (a) seven `fixtures/kicad/**/<stem>.kicad10.dsn` made with WSL KiCad 10.0.7 (`10.0.7-10.0.7~ubuntu24.04.1`, recorded in `MANIFEST.tsv` as `kicad10_host_version`; `tests/fixtures.rs` checks it and that all seven parse with `freerouting_dsn::Pcb::parse`). (b) all seven also exported with Windows KiCad 10.0.5: after CRLF -> LF the files are identical except the single `(host_version "10.0.5")` line (2 differing lines per board in `diff`), on every board. (c) exporting each board twice (`KEEP_FIRST_LINE=1`, separate processes) gave identical files apart from the first line on all seven; those have at most 2 `class` blocks, so this cannot show ordering instability. The corpus board `CNH_Functional_Tester_1` (9 classes) was exported three times with WSL KiCad 10.0.7: all three identical, so the `unordered_map` order was stable here, but that is not a guarantee (noted in `tools/kicad-reader/README.md`). On that board WSL 10.0.7 and Windows 10.0.5 differ in 52 diff lines beyond `host_version`: inner layer `(type signal)` in 10.0.7 vs `(type power)` in 10.0.5, and the board outline path is written differently (different segmentation of the same arcs); the seven committed boards show no such difference. (d) `pgrep -x kicad` and `pgrep -x pcbnew` printed nothing before, during (checked inside every `export.sh` run, WSL and Windows) and after; the originals' sha256 sums are unchanged (`export.sh` checks, and `manifest.py check` re-verified). The 58 corpus boards were exported too (`kicad10-export.log`: 58 exported, 0 failed, jetson-agx-thor-baseboard included).
