# K08 — Direct `.kicad_pcb` writer (optional)

Opened 2026-10-15. **Optional: start only on Dan's go-ahead** (K00 question
5). Today the routed result goes back to KiCad as a SES file through
`pcbnew.ImportSpecctraSES`, a SWIG call. If KiCad 11 drops SWIG, as KRT's
docstring says master did in 2026-03 (unverified), the plugin can no longer
call it. With [K07](archive/K07-kicad-pcb-entry-points-and-parity.md) the router
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

Prerequisites: K07 (reader, `read_design`), [C05](C00-cli-settings-io-README.md)
(the SES writer, whose endpoint snapping and `(attach off)` handling this
reuses for the copper it emits), [F02.6](archive/F02-parity-and-benchmark-harness.md)
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

- [ ] **K08.1 — Copper index and diff.**
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

- [ ] **K08.2 — Splicing and formatting.**
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

- [ ] **K08.3 — Deterministic UUIDs.**
  - FNV-1a 128 (offset `0x6c62272e07bb014262b821756295c58d`, prime
    `0x0000000001000000000000000000013b`) over the fields in the decisions;
    version nibble 8, variant `10`.
  - **Accept:**
    - writing the same input and the same routed board twice gives
      byte-identical files, also under load (`tools/determinism-check.sh`,
      F02.4);
    - a forced collision (test hook) gets a different UUID and is still
      deterministic.

- [ ] **K08.4 — Engine entry point.**
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

- [ ] **K08.5 — KiCad grades both paths the same.**
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

- [ ] **K08.6 — CLI output and documentation.**
  - `freerouting -de board.kicad_pcb -do routed.kicad_pcb` writes through
    `write_kicad_board`; `-do` to the input path is refused with exit 2
    (C07's code for bad arguments). Only this extension case is added here.
  - `kicad-plugin.md`: what the writer changes, what it never changes, and
    how the plugin could use it on KiCad 11 instead of `ImportSpecctraSES`.
  - **Accept:** CLI integration test for both cases; the doc section names
    K08.5's results.
