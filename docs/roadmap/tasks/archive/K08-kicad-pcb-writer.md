# K08 — Direct `.kicad_pcb` writer (optional)

Opened 2026-10-15. **Optional: start only on the owner's go-ahead** (K00 question
5); go-ahead by the owner 2026-10-17. Today the routed result goes back to KiCad as a SES file through
`pcbnew.ImportSpecctraSES`, a SWIG call. If KiCad 11 drops SWIG, as KRT's
docstring says master did in 2026-03 (unverified), the plugin can no longer
call it. With [K07](K07-kicad-pcb-entry-points-and-parity.md) the router
already reads `.kicad_pcb` directly; this checklist writes the routed copper
back into a copy of the `.kicad_pcb`, so neither direction needs KiCad's
Specctra code.

The writer edits text; it never re-serializes the board. Every byte outside
the copper it adds or removes stays as it was, so footprints, zones, user
edits and settings survive. That is KRT's approach
(`kicad_writer.add_tracks_and_vias_to_pcb`, `docs/api-kicad-writer.md` at
`0486e799`), minus its side effects (it moves copper text to silkscreen,
strips zero-length Edge.Cuts lines and rewrites `.kicad_pro` DRC floors; KRT
report §6.4).

Prerequisites: K07 (reader, `read_design`), [C05](../C00-cli-settings-io-README.md)
(the SES writer, whose endpoint snapping and `(attach off)` handling this
reuses for the copper it emits), [F02.6](F02-parity-and-benchmark-harness.md)
(KiCad DRC grading), C07 for the CLI flag. Routed boards from the A series
are not needed to start: a session imported with `read_session` gives a
routed board to write.

## What exists and what this adds

| Exists | State |
|---|---|
| K04 tree | Byte spans for every list, so a block can be cut out exactly |
| K06 | Each track and via read from the file, with its nm geometry, net, layer, uuid and byte span (`KicadRead.copper_index`, added by K08.1 if K06 didn't keep it) |
| `read_session` | Imports a SES onto a board; with K07's reader the board comes from the `.kicad_pcb` |
| KRT writer | Splices `(segment ...)` and `(via ...)` before the final `)`; removes by regex on coordinates gated by net (#369 A9); keeps the net dialect (`board_uses_name_nets`, #80); new vias without a tenting token (#489, #741, #962); fresh `uuid4()` per item |
| KRT finding on UUIDs | Overlapping zones of equal priority are tie-broken by KIID, so fresh random UUIDs changed KiCad's fill between runs on identical copper (one board graded "1 net unconnected" on 4 of 6 UUID rolls); KRT's fix was distinct zone priorities (`zone_overlap_priorities`) |
| KiCad reader, `10.0.0` | Accepts `tstamp` and `uuid` alike on tracks (`parsePCB_TRACK`); via types `blind`, `buried`, `micro` (`parsePCB_VIA`) |

## Decisions made here

- **Copper diff by geometry, not by ids.** The router normalizes and merges
  traces, so ids don't survive. An original segment or arc is kept when the
  final board's copper covers it: same layer, net and width, and each of its
  segments lies on a final trace segment (collinear and contained, exact
  integer test in board units with `i128` cross products). Otherwise its
  block is removed. Final copper not covered by kept originals is added. Vias
  match on position, net, span, diameter and drill.
- **Exact units.** Board units are 0.1 µm, so a board coordinate `b` is
  `100 × b` nm exactly. Coordinates are written from nm as decimal mm by
  integer formatting (up to six decimals, trailing zeros dropped), with Y
  negated back. No float formatting is involved.
- **Deterministic UUIDs.** Each new item's UUID is a 128-bit FNV-1a hash of
  the input file's own FNV-1a hash, the item kind, net, layer, nm
  coordinates and its index in emission order, formatted as a version-8 UUID.
  A collision with any UUID in the file appends a counter and hashes again.
  No randomness, no clock (D2). Implemented in a few lines on `u128`; no
  dependency.
- **The file's own dialect.** Net references follow K04.6's dialect (`(net
  "GND")` or `(net 11)`); the uuid token follows what the file's existing
  tracks use (`tstamp` in older files, else `uuid`); new vias get no
  tenting token, so they take the board's `(setup (tenting ...))`.
- **Never overwrite the input.** The writer refuses an output path equal to
  the input (after canonicalization). The plugin or the user replaces the
  board; the writer doesn't.
- **Zones are untouched.** Fills go stale after routing; grading uses
  `kicad-cli pcb drc --refill-zones`, as F02.6 does.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K08.1 | `crates/freerouting-kicad/src/write/{mod.rs,diff.rs}` | `src/tracks.rs`, `src/vias.rs` (keep spans and uuids) |
| K08.2 | `crates/freerouting-kicad/src/write/splice.rs`, `src/write/format.rs` | — |
| K08.3 | `crates/freerouting-kicad/src/write/uuid.rs` | — |
| K08.4 | `crates/freerouting/src/io/kicad.rs` (append `write_kicad_board`), `crates/freerouting/tests/kicad_write.rs` | — |
| K08.5 | `tools/kicad-reader/write-check.sh` | — |
| K08.6 | — | `crates/freerouting-cli/src/main.rs` (`-do *.kicad_pcb` case only), `docs/development/kicad-plugin.md` |

## Checklist

- [x] **K08.1 — Copper index and diff.**
  - The reader keeps, per track and via: kind, nm geometry (arcs as start,
    mid, end plus their flattened segments in board units), net, layer or
    span, width or size and drill, uuid, locked flag, and the byte span of
    its block.
  - `diff(index, final_board) -> CopperDiff { keep, remove, add }` as in the
    decisions. `add` lists final trace segments and vias in the board's item
    order (`BasicBoard::items()`), so the output is deterministic.
  - Locked (`(type fix)`) originals are always kept; the router can't move
    them.
  - **Accept:**
    - unit tests: an unchanged board gives `remove` and `add` empty; a
      segment split by normalization into two collinear parts is kept; a
      ripped and rerouted connection removes exactly its old blocks;
    - on `Issue180-Test` (102 segments, 627 vias) read, then written with no
      routing: the diff is empty.
  *Done 2026-10-17:* `crates/freerouting-kicad/src/write/{mod.rs,diff.rs}`. The reader keeps a
  `CopperIndex` (`KicadRead::copper_index`, built in `read.rs` from the parsed tree; `tracks.rs` and
  `vias.rs` unchanged): per `(segment)`, `(arc)` and `(via)` its kind (arc with its mid point and the
  reader's flattened points), nm start/end/centre, net code, layer or span, width or size, drill,
  uuid/tstamp, locked flag, whether the reader routed it, and the byte span of its block (from the
  head atom's offset, closed with the K04 lexer), plus layer names, net dialect and raw names, the
  uuid token, every uuid of the file and the DSN via padstacks with their nm sizes. `diff(index,
  final_items, frame) -> CopperDiff { keep, remove, add }`: pieces mapped to board units through the
  board's own `CoordinateTransform` (`TransformFrame` in `io/kicad.rs`), coverage by the union of
  collinear final segments of the same (layer, net, half width) with `i128` cross and dot products
  (`on_segment_is_exact_at_the_limits` at +-i32::MAX), vias by (position, net, span, diameter,
  drill), locked and unrouted originals always kept, `add` in `BasicBoard::items()` order with only
  the uncovered parts of each final segment. Tests `crates/freerouting-kicad/tests/write.rs` (8 pass):
  `an_unchanged_board_gives_an_empty_diff`, `a_segment_split_in_two_collinear_parts_is_kept`,
  `a_ripped_and_rerouted_connection_removes_exactly_its_old_blocks` (remove = [0, 1], two new
  segments), `a_locked_original_is_kept_even_when_the_board_lost_it`,
  `a_longer_route_adds_only_the_uncovered_part`, `the_index_keeps_spans_uuids_and_locks`.
  `crates/freerouting/tests/kicad_write.rs::issue180_written_without_routing_is_unchanged`: 102
  segments and 627 vias indexed, `tstamp` unquoted, diff kept 729 / removed 0 / added 0, and the
  written file equals the input byte for byte.

- [x] **K08.2 — Splicing and formatting.**
  - Remove blocks by byte span, together with their leading indentation and
    the newline before them.
  - Insert new blocks, in `add` order, before the board's final `)`, in
    KiCad 10's multi-line layout (tab indents):
    `(segment (start x y) (end x y) (width w) (layer "L") (net ...) (uuid "..."))`
    and `(via [blind|buried|micro] (at x y) (size d) (drill d) (layers "A"
    "B") (net ...) (uuid "..."))`. Layer names are the file's canonical names
    (`In1.Cu`), not the DSN user names (`Gnd.Cu`).
  - The result must parse with K04 and have balanced parentheses (KRT
    `assert_balanced_sexpr`).
  - **Accept:**
    - a text diff between input and output shows only removed and added
      copper blocks (test parses the diff hunks);
    - `format_nm_mm` tests: 0, 1, -1, 999, 1,000,000 and -123,456,789 nm give
      `0`, `0.000001`, `-0.000001`, `0.000999`, `1`, `-123.456789`.
  *Done 2026-10-17:* `write/splice.rs` (`removal_range` takes the block, its spaces and tabs and
  the line break before them, CRLF too; `splice`; `insertion_point` before the final `)`;
  quote-aware `is_balanced` in the manner of KRT's `assert_balanced_sexpr`) and `write/format.rs`
  (KiCad 10 multi-line blocks with tabs, `(via blind|buried|micro ...)`, the file's `(net N)` /
  `(net "name")` and `(uuid "...")` / `(tstamp ...)`, canonical layer names from the index).
  `render` parses its result with K04's `Tree::parse` and checks the balance before returning.
  `tests/write.rs::the_text_diff_shows_only_copper_blocks`: a Myers line diff of input and output has
  exactly the two removed blocks' lines on one side and on the other only lines that parse as
  `segment`, `segment`, `via`; the output reads back to an empty diff. `format_nm_mm_vectors`: 0, 1,
  -1, 999, 1,000,000, -123,456,789 nm give `0`, `0.000001`, `-0.000001`, `0.000999`, `1`,
  `-123.456789` (plus `0.0015` and the `i32` limits). `cargo test -p freerouting-kicad`: all pass.

- [x] **K08.3 — Deterministic UUIDs.**
  - FNV-1a 128 (offset `0x6c62272e07bb014262b821756295c58d`, prime
    `0x0000000001000000000000000000013b`) over the fields in the decisions;
    version nibble 8, variant `10`.
  - **Accept:**
    - writing the same input and the same routed board twice gives
      byte-identical files, also under load (`tools/determinism-check.sh`,
      F02.4);
    - a forced collision (test hook) gets a different UUID and is still
      deterministic.
  *Done 2026-10-17:* `write/uuid.rs`: FNV-1a 128 with the given offset and prime
  (`fnv1a128_known_vectors`: "" is the offset basis, "a" is `d228cb696f1a8caf78912b704e4a8964`,
  checked against an independent Python computation), over the file hash, kind, net, layer, nm
  coordinates and emission index; `format_v8` sets version 8 and variant `10`
  (`version_and_variant_bits`); a taken UUID (any uuid or tstamp of the file, any already issued)
  appends a counter and hashes again. Forced collision: `UuidGen::reserve` is the test hook;
  `forced_collision_is_resolved_deterministically` and
  `tests/write.rs::rendering_is_deterministic_and_survives_a_forced_collision` (two renders equal;
  with the would-be UUID reserved, another UUID, equal in two runs).
  `crates/freerouting/tests/kicad_write.rs::writing_twice_gives_identical_files`. Under load:
  `tools/determinism-check.sh --out <scratch>/det -- det-run.sh {OUT}`, where `det-run.sh` runs the
  release CLI `-de dev-board.kicad_pcb -do {OUT}/routed.kicad_pcb` (a full Rust route, 4.65 s, 0
  unrouted, 207,613 bytes): `identical` (8 burners; load 9.35 before the idle run, 13.34 after the
  loaded run; the machine was shared).

- [x] **K08.4 — Engine entry point.**
  - `freerouting::io::kicad::write_kicad_board(input_path, board, out_path)
    -> Result<WriteSummary, String>`: re-reads the input with K07's reader
    (it needs the copper index and must refuse a file changed since the
    board was built: compare the input's FNV hash with the one recorded at
    read time), diffs, splices, writes to a temporary file and renames it
    into place.
  - **Accept:** an integration test on `Issue558-dev-board`: route its
    KiCad export with Java at the pin (K07.4 step 1), `read_session` the SES
    onto the reader-built board, write; re-reading the output with K07's
    reader gives a board whose `BoardStatistics` equal the board that was
    written.
  *Partial 2026-10-17:* `freerouting::io::kicad::{write_kicad_board, write_kicad_board_checked,
  render_kicad_board, final_copper, same_file, read_kicad_bytes, KicadSource}`; `KicadInput` records
  `board_file`, `source_hash` (FNV-1a 128 of the bytes read) and `copper_index`. The writer refuses an
  output that is the input (`same_file`, canonicalized) and, with the recorded hash, a changed input
  (`the_writer_refuses_its_input_and_a_changed_file`), hashes and parses the same bytes, writes
  `.<name>.<pid>.tmp` and renames it. `kicad_write.rs::dev_board_round_trip_keeps_the_statistics`:
  Java's session (`dev-board.kicad10.java.ses`, the K07.4 step 1 run at `a773a70ec`, no new Java run)
  imported onto the reader's board (0 errors), written (459 segments, 35 vias added), read again:
  the traces are the same as undirected corner lists, and every field of `BoardStatistics` is
  equal (231 traces, 459 segments, 35 vias, 0 incomplete, 0 violations, bends, lengths, fanout)
  **except** `traces.total_{vertical,horizontal,angled}_length`, which agree to 1e-5 relative
  (4246.811 against 4246.8145 for the angled sum). The reader merges a track into the previous wire
  when it starts where that one ended (KiCad's export does the same), so 23 traces that start where
  the previous one ends are written backwards to read back as their own traces (without that the
  re-read board had 208 traces and 13 incomplete connections); the builder also orders the re-read
  items its own way; both change the order of the `f32` sums. Remaining: an operator decision
  whether that `f32` accumulation-order difference is acceptable (recommended) or the clause must be
  bit-exact (K08, operator).
  *Done 2026-10-17:* the orchestrator accepted the `f32` accumulation-order difference (option a,
  recommended; the owner's standing instruction to take recommended answers): every other field is exact
  and the three per-direction sums agree to 1e-5 relative, which the test asserts.

- [x] **K08.5 — KiCad grades both paths the same.**
  `tools/kicad-reader/write-check.sh <board-dir> <session.ses>`, on copies,
  with WSL KiCad 10.0.7 (or Windows 10.0.5, recorded):
  1. path A: load the original `.kicad_pcb`, `ImportSpecctraSES`, save;
  2. path B: K08's output from the same SES (K08.4);
  3. `kicad-cli pcb drc --refill-zones --format json` on both;
  4. compare violations by type and the unconnected count.
  The KiCad PID check of K04.8 applies; never touch the Java plugin.
  - **Accept:** on `Issue558-dev-board`, `Issue180-Test` and `canary-pwr`,
    both paths give the same DRC result; any difference is explained (for
    example arcs, which path A gets as chords) in the done note.
  *Done 2026-10-17:* `tools/kicad-reader/write-check.sh` (copies in a scratch folder; path A
  `LoadBoard` + `ImportSpecctraSES` + `SaveBoard` in the WSL python3, path B the release CLI
  `-de <copy>+<ses> -do <board>.kicad_pcb` with autorouter, fanout and optimizer off; `kicad-cli pcb
  drc --format json --severity-all --refill-zones` on each, one at a time; the KiCad process list
  and the originals' sha256 are checked before and after). WSL KiCad 10.0.7, `pgrep -x kicad; pgrep
  -x pcbnew` empty before and after, exit 0 `SAME` on all three: `Issue558-dev-board` with Java's
  session (copper_edge_clearance 22, hole_clearance 4, lib_footprint_issues 4,
  lib_footprint_mismatch 16, silk_edge_clearance 5, 0 unconnected, on both; 459 segments, 35 vias
  each); `Issue180-Test` with the Rust CLI's session of the reader's board (79 wires, 3 vias; the
  router added nothing), lib_footprint_issues 24, lib_footprint_mismatch 1, 0 unconnected on both:
  B is the input byte for byte, A rewrote the unlocked tracks as 110 pieces (every piece lies on a B
  segment and back; 20.3 mm more length, so they overlap); `canary-pwr` (owner's board, 15aa06a3...)
  with the Rust CLI's session (212 s route, `--router.min_trace_width_um=150`, 374 wires, 89 vias):
  copper_edge_clearance 2, lib_footprint_issues 4, lib_footprint_mismatch 6, starved_thermal 1,
  track_dangling 2, via_dangling 30, 70 unconnected on both; 813 segments and 130 vias, 1154.1893 mm
  each; one end differs by 1 nm (B writes an original track end's exact `130.699999`, A the
  session's `130.7`). No board has track arcs, so the arc case (A gets chords) did not arise. Not
  run: Windows KiCad 10.0.5.

- [x] **K08.6 — CLI output and documentation.**
  - `freerouting -de board.kicad_pcb -do routed.kicad_pcb` writes through
    `write_kicad_board`; `-do` to the input path is refused with exit 2
    (C07's code for bad arguments). Only this extension case is added here.
  - `kicad-plugin.md`: what the writer changes, what it never changes, and
    how the plugin could use it on KiCad 11 instead of `ImportSpecctraSES`.
  - **Accept:** CLI integration test for both cases; the doc section names
    K08.5's results.
  *Done 2026-10-17:* `OutputFormat::KicadPcb` (`job/outputs.rs`, through `render_kicad_board` with
  the `KicadSource` that `JobInput::set_kicad` records), and in `crates/freerouting-cli/src/cli.rs`
  (`main.rs` holds no behaviour; `settings_parity` checks that) `kicad_output_problem`: a
  `.kicad_pcb` output of a DSN input, or one that is the input after canonicalization, is a usage
  error, exit 2, before any work (the job deletes an old primary output, so this check must come
  first). `crates/freerouting-cli/tests/cli_kicad_write.rs` (3 pass):
  `kicad_output_gets_the_routed_copper` (459 segments, 35 vias, input unchanged, the output opens
  with `-drc`), `kicad_output_to_the_input_is_refused` (three spellings of the input path),
  `kicad_output_of_a_dsn_input_is_refused`. `help.txt`, the capabilities' `output_formats` and
  `docs/DIVERGENCES.md` name the new format. `docs/development/kicad-plugin.md` "Direct `.kicad_pcb`
  writer (K08)": what it changes, what it never changes, K08.5's table, and how the plugin could use
  it on KiCad 11.
