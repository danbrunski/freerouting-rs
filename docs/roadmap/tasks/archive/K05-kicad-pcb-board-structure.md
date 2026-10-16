# K05 — Board structure from `.kicad_pcb`

Opened 2026-10-15. This checklist turns K04's tree into the parts of a DSN
syntax tree that describe the board before any copper: copper layers, the
coordinate frame, nets, net classes, the board outline, and footprints as
images, padstacks and placements. It follows KiCad's own DSN exporter
(`pcbnew/specctra_import_export/specctra_export.cpp` at tag `10.0.0`) so that
`build_board` of the reader's tree and `build_board` of KiCad's export give the
same board, which K07 checks. Copper items (rule areas, zones, tracks, vias)
are [K06](K06-kicad-pcb-rule-areas-and-copper.md).

Prerequisites: [K04](K04-kicad-pcb-syntax.md) (tree, header, net table,
corpus, reference exports). [F04](F04-provenance.md) only for an item that
translates KRT code.

## What exists and what this adds

| Exists | State |
|---|---|
| K04 | `Tree`, `Header`, `NetTable`, `fixtures/kicad/` with KiCad 10 exports |
| `freerouting_dsn::ast` | `Pcb { parser, resolution, unit, structure, placement, library, network, wiring }`; `Structure { layers, boundaries, via, rules, keepouts, planes }`; `Image { name, side, pins, outlines, keepouts }`; `ImagePin { padstack, name, at, rotation }`; `Padstack { name, shapes, attach, absolute }`; `Place { name, location: { at, side, rotation }, part_number }`; `Network { nets, vias, via_rules, classes }`; `NetClass { name, nets, rules, circuit }` |
| `Pcb::to_dsn()` | Writes any tree as DSN text: used to debug the reader |
| KiCad exporter, 10.0.0 | Read in full for this checklist (fetched from GitLab on 2026-10-15). Function names below are from it |
| KiCad exports made in WSL KiCad 10.0.7 on 2026-10-15 (scratchpad, not committed) | Confirm: back-side `(at 202.4 96.3 90)` becomes `place ... back 270` and `(at 195 97.625)` becomes `back 180` (`Issue368`); no `.kicad_pro` gives `(rule (width 200) (clearance 200) (clearance 50 (type smd_smd)))` and `Via[0-1]_600:300_um` (`Issue508 bm1`); the KiCad 10 network lists nets in order of first appearance in the file (`Issue742`, 28 nets) |
| KRT `py_router/kicad_parser.py` | Pitfalls with issue numbers: pad angle in the file is absolute (#369 A4), plated through-hole pads are on every copper layer whatever the file lists (`canonical_pad_layers`, probed on pcbnew 10.0.0), duplicate references (`tests/measure_726_duplicate_census.py`: 5 of 22 boards) |

## Decisions made here

- **Reproduce KiCad's exporter, not a fresh design.** The goal is a tree
  that builds the same board as KiCad's DSN. Where the exporter makes a
  choice (padstack names, courtyard first, oval as a path, `(attach off)`),
  the reader makes the same one. Deliberate differences are only the ones
  K00 names: rule-area kinds (K06), track arcs (K06), and full precision.
- **Full precision.** KiCad formats shape and wire coordinates with C
  `%.6g` (`specctra.h`, `POINT::Format` and `PATH::Format`), which rounds
  any absolute coordinate above 100 mm to whole micrometres:
  `(at 116.9009 64.0861)` exports as `116901 -64086.1`. Placements use
  `FormatDouble2Str` and keep nanometres (`133199.999`). The reader keeps
  every value at KiCad's nanometre precision. K07 compares with a tolerance
  of half a `%.6g` step.
- **Exact conversion order (D1/D6).** `nm = (parse_f64(text) * 1e6).round()`
  as `i64` (KiCad `parseBoardUnits`: `KiROUND(parseDouble() * IU_PER_MM)`,
  `KiROUND` = `std::llround`, half away from zero, the same as Rust
  `f64::round`). Then `um = nm as f64 / 1000.0` and `y_um = -(nm_y as f64 /
  1000.0)` (`scale`, `mapY` in `specctra_export.cpp`). Derived positions
  (rotated pads, flipped footprints) are computed in integer nm with KiCad's
  `RotatePoint` and `MIRROR`, then converted. Never convert mm to µm
  directly.
- **Images are grouped the way KiCad groups them.** KiCad compares images
  by a hash of their printed contents (`IMAGE::Compare`, `makeHash`), so two
  footprints share an image only if their pins, outlines and keepouts print
  identically with `%.6g`. A second, different image with the same footprint
  id is named `<id>::1`, `<id>::2`, ... The reader computes the same key by
  printing each image with a `c_fmt_6g(f64) -> String` helper (C `%.6g`
  semantics, tested against `printf` output) and groups in footprint order.
  This decides component order, and so item ids in `build_board`.
- **Padstack order is KiCad's.** Pad padstacks go into a set ordered by
  their printed contents, then name (`PADSTACK::Compare`; the exporter's
  `m_padstackset` is a `boost::ptr_set`). The reader sorts the same way.
- **Net classes: `.kicad_pro` order.** KiCad writes the Default class first
  (as `kicad_default`) and then the classes in use in `std::unordered_map`
  order, which is not reproducible. The reader writes Default first, then
  the `.kicad_pro` order. K07 lists the difference; it changes no rule value.
- **JSON.** Reading `.kicad_pro` needs a JSON parser. Use `serde_json`
  (`Value` traversal, no derive), a `NEW DEP` unless C02 has added it to the
  workspace by then.
- **Custom pads need a polygon union** of the anchor and the primitives
  (`PAD::MergePrimitivesAsPolygon`; the exporter then writes outline 0).
  Take `NEW DEP: i_overlay` (pure Rust, integer coordinates, deterministic)
  on nm coordinates. If the orchestrator refuses it, the pad becomes the
  convex hull of its primitives with one warning per pad, and K07 lists it.
  The same union builds courtyards (K05.7).

## File ownership

| Item | New files | May edit |
|---|---|---|
| K05.1 | `crates/freerouting-kicad/src/{units.rs,layers.rs,fmt6g.rs}` | `src/lib.rs` |
| K05.2 | `crates/freerouting-kicad/src/network.rs` | — |
| K05.3 | `crates/freerouting-kicad/src/project.rs` (`.kicad_pro`), `src/classes.rs` | `Cargo.toml` (`serde_json`) |
| K05.4 | `crates/freerouting-kicad/src/outline.rs`, `src/arc.rs` | — |
| K05.5 | `crates/freerouting-kicad/src/footprints.rs` | — |
| K05.6 | `crates/freerouting-kicad/src/pads.rs` | `Cargo.toml` (`i_overlay`) |
| K05.7 | `crates/freerouting-kicad/src/image_outlines.rs` | `src/footprints.rs` |
| K05.8 | `crates/freerouting-kicad/src/read.rs`, `tests/structure.rs`, `fixtures/kicad/synthetic/{pads,outline,backside,classes}.kicad_pcb` (+ their `.kicad_pro` where needed and `.kicad10.dsn`), each generated once with pcbnew in WSL KiCad 10.0.7 | `src/lib.rs` |

## Checklist

- [x] **K05.1 — Frame, layers and number helpers.**
  - `Pcb.parser`: `host_cad "KiCad's Pcbnew"`, `host_version` = the file's
    `generator_version` plus ` (freerouting-kicad)`. `resolution um 10`,
    `unit um` (`FromBOARD`, "unit_descriptor"). `Pcb.name`: the board path
    with `.dsn` in place of `.kicad_pcb`.
  - Copper layers from `(layers ...)`: every `*.Cu` entry, in physical
    order F.Cu, In1.Cu ... InN.Cu, B.Cu (`buildLayerMaps`, `CuStack`). The
    DSN layer name is the user name when one is given (`(4 "In1.Cu" power
    "Gnd.Cu")` → `Gnd.Cu`). Type: `signal` → signal, `power` → power,
    `mixed` → signal, `jumper` → jumper. Property `(index n)`. KiCad 9+
    files number layers F.Cu 0, B.Cu 2, In1.Cu 4 ...; KiCad 6–8 files number
    them 0, 1 ... 31. Order comes from the names, never the numbers.
  - `units::nm(text) -> i64`, `units::um(nm) -> f64`, `units::map_pt(x_nm,
    y_nm) -> Point` with the operation order in the decisions.
  - `fmt6g::c_fmt_6g(f64) -> String` (C `%.6g`): the image and padstack
    keys of K05.5–K05.6 need it.
  - Data the DSN can't carry goes into `KicadRead.extras` for the X series
    (length and impedance work): the stackup from `(setup (stackup ...))`
    (per layer: type, thickness, material, `epsilon_r`, `loss_tangent`; KRT
    `extract_stackup` combines multi-sheet dielectrics) and each pad's
    `die_length` (KRT #1248). Nothing in the DSN tree depends on them.
  - **Accept:**
    - `units` tests: `66.1` → 66,100,000 nm (KRT notes `pcbnew.FromMM`
      truncates to 66,099,999; KiCad's reader rounds); `-0.0000005` →
      -1 nm (half away from zero).
    - `c_fmt_6g` matches `printf "%.6g"` on 200 values including
      `116900.9`, `-64086.1`, `0.000123456`, `1e21`, `-0.0`.
    - On every committed board, the layer list (names, types, order) equals
      the KiCad export's `structure` layers. On `canary-pwr` (out of repo):
      F.Cu, Gnd.Cu, Power.Cu, B.Cu with types signal, power, power, signal.
    - `canary-pwr`'s stackup in `extras`: four copper layers of 0.035 mm and
      dielectrics of 0.1, 1.24 and 0.1 mm with `epsilon_r` 4.5 (its
      `(setup (stackup ...))`).
  *Done 2026-10-16:* `src/{units,fmt6g,layers}.rs`, `KicadRead.extras`. `tests/numbers.rs`: `66.1` gives 66,100,000 nm and `-0.0000005` gives -1 nm (`nm_rounds_like_kicad_not_like_frommm`); `c_fmt_6g` equals C printf `%.6g` on 200 values including the named ones (`c_fmt_6g_matches_printf_on_200_values`, vectors from Python's C-library `%.6g`; `printf(1)` parses long double and differs on `99999.95`). `tests/structure.rs`: `layers_equal_the_export` (all 7 committed and 5 synthetic boards), `canary_pwr_structure` (F.Cu, Gnd.Cu, Power.Cu, B.Cu as signal, power, power, signal; four 0.035 mm coppers; dielectrics 100, 1240 and 100 um with epsilon_r 4.5; the corpus snapshot), `extras_hold_stackup_and_die_length` (multi-sheet dielectrics add up, epsilon_r thickness-weighted). Not representable: the layer `(property (index n))` has no field in `freerouting_dsn::ast::Layer`; nothing reads it.

- [x] **K05.2 — Nets and pins.**
  - The DSN network holds every net with at least one pin, in net-code order
    (`FromBOARD`, "build the images, components, and netlist"). Numeric
    dialect: the file's codes. Name dialect: first appearance in the file
    (verified on `Issue742`; check it on every committed name-dialect board).
  - Pins per net, in footprint order then pad order within the footprint.
  - Pin names: the pad number; a repeated number within one footprint gets
    `@1`, `@2`, ... in pad order (`makeIMAGE`, `pinmap`). Pads with an empty
    number repeat the same way.
  - Pads on no copper layer are skipped. Copper-less round through holes are
    keepouts, not pins (K05.6).
  - Nets of netless zones (`@:no_net_N`) are added by K06.4.
  - **Accept:** on every committed board, the net names, their order, and
    each net's pin list equal the KiCad export's `network`.
  *Done 2026-10-16:* `src/network.rs`. `nets_and_pins_equal_the_export`: net names, order and every pin list equal the KiCad 10.0.7 export on all 12 boards; `Issue742` (the only committed name-dialect board) confirms codes by first appearance. Net names containing `"` become `''` like 10.0.7. Pins skip pads without copper and copper-less round holes; a repeated or empty pad number becomes `@1`, `@2` (`synthetic_pads_cover_every_shape`).

- [x] **K05.3 — Net classes and default rules.**
  - Read `.kicad_pro` `net_settings`:
    - `classes[]`: `name`, `clearance`, `track_width`, `via_diameter`,
      `via_drill`, `priority` (mm values; convert with `units::nm` on the
      JSON number's text, not the parsed float, to keep KiCad's rounding);
    - KiCad 6 projects (`meta.version` 2, e.g. `Issue184`): a class's
      `nets` list assigns exact names;
    - `netclass_assignments` (net name → class name or list of names) and
      `netclass_patterns` (`{netclass, pattern}`; wildcard matching).
      Whether KiCad also accepts regex patterns is unverified: check
      `EDA_COMBINED_MATCHER` in KiCad's source and port what it does.
    - A net matching several classes gets an aggregate: each field from the
      member with the highest priority (lowest `priority` number), Default
      lowest (KRT `design_rules.py` docstring; KiCad 9+). How KiCad names an
      aggregate class in the DSN is unverified: export a synthetic board
      with a two-class net and copy what KiCad writes.
  - No `.kicad_pro`: KiCad's defaults. The 10.0.7 export of
    `Issue508 bm1` shows track 200 µm, clearance 200 µm, via 600/300 µm.
  - Output (`FromBOARD` "rules", "output vias used in netclasses",
    `exportNETCLASS`):
    - structure rules `(width W) (clearance C) (clearance C/4 (type
      smd_smd))` from the Default class, numbers as `%.6g` would print them;
    - via padstack per class `Via[0-N]_<d>:<drill>_um` (name via
      `snprintf("Via[%d-%d]_%.6g:%.6g_um")`), one circle per copper layer;
      the Default class's via first, then each other class's if new
      (`LookupVia`); the structure `(via ...)` list in library order;
    - one `NetClass` per class in use: Default as `kicad_default`, with
      `(width)`, `(clearance)`, `(circuit (use_via ...))`, and its nets in
      `GetNetInfo` order (sorted by name in the `Issue742` export; confirm).
  - **Accept:**
    - per class: width, clearance, `use_via` and net set equal the KiCad
      export on all committed boards with a `.kicad_pro` and on `Issue508
      bm1` (none);
    - a synthetic project with a pattern, an explicit assignment, a KiCad 6
      `nets` list and a two-class net gives the classes KiCad exports;
    - class order differs only as the decisions say, and K07 lists it.
  *Done 2026-10-16:* `src/{project,classes}.rs`, `serde_json` (`float_roundtrip`) and `regex`. Answers to the open points: (1) KiCad's pattern matcher for net classes is `EDA_COMBINED_MATCHER` with `CTX_NETCLASS`: an anchored regular expression **and** an anchored wildcard, either matching the whole name; the `regex` crate stands in for wx's ARE. (2) An aggregate class (a net matching several classes, or a class that lacks one of clearance, track width, via diameter and drill, microvia diameter and drill, differential pair width and gap, wire and bus width) is named by its members joined with `,`, in priority order, with `Default` appended when it filled a gap: `Power,Default`, `Fast,Wide` (seen in the dev-board and synthetic exports). Priority: lower number wins, then name; KiCad 6 projects (schema 2) get priorities 0, 1, ... in file order and a class's `nets` list becomes patterns. `net_classes_rules_and_vias_equal_the_export` (width, clearance, `use_via`, net set; all 12 boards, `Issue508 bm1` without a `.kicad_pro`) and `synthetic_projects_give_kicad_classes` (pattern, assignment, regex, two-class net, KiCad 6 `nets` list). Class order: Default, then the project order, then aggregates by name; KiCad's is `std::unordered_map` order (the export of `classes` lists them differently); K07 lists it. Not implemented: bus expansion in patterns (`IN[0..7]`), which gives a warning.

- [x] **K05.4 — Board outline.**
  - Collect Edge.Cuts shapes: `gr_line`, `gr_arc`, `gr_circle`, `gr_rect`,
    `gr_poly`, `gr_curve` (bezier), and the same shapes inside footprints on
    Edge.Cuts, transformed to board coordinates.
  - Chain them into closed contours the way `BOARD::GetBoardPolygonOutlines`
    does (`ConvertOutlineToPolygon`; read its chaining tolerance in
    `pcbnew/convert_shape_list_to_polygon.cpp` at 10.0.0). Arcs and circles
    become polylines with KiCad's segment count for the board's max error
    (`GetArcToSegmentCount`, `libs/kimath`); beziers likewise.
  - Output (`fillBOUNDARY`): one `(path pcb 0 ...)` boundary per outer
    contour, closed by repeating the first point; each hole becomes a
    structure keepout `(keepout (polygon signal 0 ...))` with no name.
  - An open or missing outline: the same warning KiCad's export gives
    (`aInferOutlineIfNecessary`; read what it infers) and the same result.
  - **Accept:**
    - on every committed board and `canary-pwr`, the boundary area equals
      the export's within 0.1 % and the vertex count is recorded beside the
      export's;
    - a synthetic concave, notched outline survives with its notch (the Java
      `KiCadJsonReader` polar sort scrambled such outlines; gui report §6.1);
    - a synthetic board with a slot (inner contour) gives one signal
      keepout, as the export does.
  *Done 2026-10-16:* `src/{outline,arc}.rs`. `outlines_equal_the_export` (area within 0.1 % and equal vertex counts on all 12 boards; 0.0004 % at most on the committed ones; `canary-pwr` 65 vs 65 vertices, areas 3,632,261,113 vs 3,632,262,427 um^2), `notched_outline_and_slot` (the concave outline keeps its four notch vertices, the slot gives one `signal` keepout, the export agrees). Chaining follows `doConvertOutlineToPolygon`; differences: the first shape of a chain is the first unused one in file order, a footprint's Edge.Cuts is a hole when a pad centre lies outside it, and `Simplify` is replaced by duplicate removal and Clipper's orientation (positive outer) with a fitted start vertex (`rotate_to_clipper_start`).

- [x] **K05.5 — Footprints and placement.**
  - Footprints in file order. Refuse an empty or repeated reference with
    KiCad's messages ("Footprint with value of '%s' has an empty reference
    designator.", "Multiple footprints have the reference designator
    '%s'."; `FromBOARD`), because a SES can't be imported back otherwise.
  - Front side: `place <ref> x y front <orientation>`, orientation in
    degrees from `(at x y a)`, `part_number` = the `Value` property.
  - Back side (`(layer "B.Cu")`): KiCad flips the footprint to the front
    before building its image (`FlipFOOTPRINTs`, `FOOTPRINT::Flip` with
    `TOP_BOTTOM` about the footprint position: Y of every child mirrored,
    orientation negated and normalized to (-180, 180]). The place rotation is
    `normalize_0_360(180 - flipped_orientation)`, side `back`. Verified on
    the 10.0.7 export: file orientation 90 → `back 270`, 0 → `back 180`.
  - Images: built per footprint (K05.6, K05.7, K06.3), keyed as in the
    decisions; a repeated key reuses the image; a new key with an existing
    id gets `::n`. Components grouped by image, images in first-use order,
    places in footprint order (`LookupIMAGE`, `LookupCOMPONENT`).
  - **Accept:**
    - every committed board: image names, the image of each reference, the
      component grouping and order, and each place's position, side and
      rotation equal the export (positions within 0.001 µm);
    - `Issue368` and `Issue184` cover back-side parts at 0° and 90°; a
      synthetic `backside.kicad_pcb` adds 30° and 225°.
  *Done 2026-10-16:* `src/footprints.rs`, `src/read.rs`. `placement_equals_the_export`: image names (with `::n`), component grouping and order, and every place's position (0.001 um), side, rotation and `PN` equal the export on all 12 boards; `back_side_rotations` (`Issue368` D16 `back 270`, S16 `back 180`; the synthetic `backside` at 0, 30, 90 and 225 degrees gives 0, 330, 270, 135 and the front parts 30 and -135, as exported). Image grouping depends on the start vertex of the courtyard polygon, so it is part of the rule above. **Decision to confirm:** the checklist asks to refuse an empty or repeated reference; KiCad 10.0.7 (which made the reference exports) no longer refuses (empty becomes `REF**`, repeats `R1_1`, case-insensitive, `specctra_export.cpp` at 10.0.7), and three corpus boards (`microwave`, `One-Air-Max`, `tinytapeout-demo`) have such references. The default follows 10.0.7 with a warning per footprint; `ReadOptions { strict_references: true }` gives 10.0.0's refusals with KiCad's messages (`footprint_reference_errors`).

- [x] **K05.6 — Pads, padstacks and image pins.**
  Port `SPECCTRA_DB::makePADSTACK` and the pad loop of `makeIMAGE`:
  - layer set: plated through-hole pads are on every copper layer whatever
    the file lists (KRT `canonical_pad_layers`, probed on 10.0.0); expand
    `*.Cu`, `F&B.Cu`, `*In.Cu`. The name's layer tag is `[A]` for all copper
    layers, else one character per layer: `T` top, `B` bottom, digit for
    inner.
  - shapes per layer: circle; rect (corners); oval as a `path` of width
    `2 × radius` along the long axis; trapezoid polygon from `(rect_delta)`;
    roundrect and chamfered rect as a polygon from
    `TransformRoundChamferedRectToPolygon` after enlarging the size and
    radius by `KiROUND(r × (1 − cos(π/36)))`; custom pads as the union of
    anchor and primitives, outline 0. A pad `(offset)` moves every shape and
    adds `[x,y]` to the name.
  - names exactly as the exporter builds them (`Round`, `Rect`, `Oval`,
    `Trapz`, `RoundRect`, `Cust` prefixes; `std::fixed`,
    `setprecision(6)`; `Cust` names end in KiCad's 128-bit shape hash, which
    the reader replaces by its own hash of the polygon, listed in K07).
  - every padstack `(attach off)` (the exporter's default; also owner rule
    D8, no via in a pad).
  - pins: rotation `normalize_0_360(pad_orientation − footprint_orientation)`
    with both taken after the back-side flip; the pad angle in the file is
    already absolute (KRT #369 A4), so it is not added to the footprint's;
    position = the pad's footprint-relative position after the flip, through
    `map_pt`.
  - copper-less round holes (`isRoundKeepout`: circle pad with drill ≥ size,
    or no copper layer) become one image keepout per copper layer, circle of
    `drill + 2 × hole_clearance` (the board's `m_HoleClearance`).
  - **Accept:**
    - every committed board: each image's pin names, padstack names,
      positions (within 1 board unit) and rotations equal the export, and
      every non-`Cust` padstack's shapes equal the export's within `%.6g`
      rounding;
    - synthetic `pads.kicad_pcb` (generated once with pcbnew and committed
      with its export): one pad of each shape, rotated 0/30/90°, with
      offsets, chamfers, a two-primitive custom pad, and an NPTH hole;
    - on `canary-pwr`, the `Cust` padstacks' polygons equal the export's
      within 1 µm.
  *Done 2026-10-16:* `src/pads.rs`. `pads_equal_the_export`: image pins (names, padstack names, positions within 0.1 um, rotations) and every non-custom padstack's shapes within `%.6g` rounding equal the export on all 12 boards; the synthetic `pads` board holds each shape at 0/30/90 degrees, an offset, a chamfered rect, a two-primitive custom pad, an NPTH hole and a repeated number; on `canary-pwr` the custom pads' hulls equal the export's within 1 um (`canary_pwr_structure`). **`i_overlay` is not needed:** KiCad's exporter has `#define EXPORT_CUSTOM_PADS_CONVEX_HULL`, so a custom pad is exported as the convex hull of anchor and primitives, and the hull of a union is the hull of its parts. Custom padstack names equal KiCad's up to the pad bounding box (up to about 2 um different) and the 128-bit hash, which is the reader's own hash of the hull. Trapezoid, oval, circle, rect and round-rect names and shapes equal. Not exact: `BEZIER_POLY` flattening of `gr_curve` primitives (a recursive subdivision).

- [x] **K05.7 — Image outlines.**
  - Courtyard first: the union of the F and B courtyard polygons built from
    the courtyard graphics (`FOOTPRINT::GetCourtyard`), one
    `(outline (polygon signal 0 ...))` per contour.
  - Then every other footprint graphic, in file order: segment → `path`
    with its stroke width; circle → `path` polyline at `ARC_HIGH_DEF`; rect →
    closed `path`; arc → `polygon` of the arc's stroke outline. The value of
    `ARC_HIGH_DEF` and what happens to `poly` and bezier graphics must be read
    from KiCad 10.0.0 (unverified here).
  - Coordinates relative to the footprint, after the back-side flip
    (`mapPt(pt, footprint)`: subtract the position, rotate by −orientation).
  - These outlines decide only the GUI drawing and the board's component
    outline items (`statistics.items.component_count`), not routing.
  - **Accept:** on every committed board, each image's outline count equals
    the export's, and each outline's points are within 1 µm or listed in
    K07 with the reason.
  *Done 2026-10-16:* `src/image_outlines.rs`. `image_outlines_equal_the_export`: outline counts equal and every outline's points are within 1 um on all 12 boards (no exceptions). `ARC_HIGH_DEF` is 0.005 mm (`base_units.h`); `fp_poly` and `fp_curve` graphics are skipped by KiCad's `default: continue`, as in the reader. Courtyards: front and back are chained and shrunk by the max error, identical or enclosed polygons are merged, and the start vertex follows a rule fitted to the exports (a rectangle starts at its bottom right corner), which decides the image grouping above. Not done: a real polygon union of overlapping courtyards.

- [x] **K05.8 — Assemble and build.**
  - `freerouting_kicad::read(text, project: Option<&str>) -> Result<KicadRead,
    KicadError>` with `KicadRead { pcb: Pcb, warnings: Vec<Warning> }`. K06
    adds keepouts, planes and wiring to the same `Pcb`.
  - **Accept:**
    - `build_board` succeeds on every committed board and every board in
      `$FR_KICAD_CORPUS` (ignored test `corpus_builds`, run in the done
      note);
    - `Pcb::parse(&pcb.to_dsn())` gives back an equal tree on every
      committed board;
    - read time for the largest corpus board in the done note.
  *Done 2026-10-16:* `src/read.rs`, `tests/structure.rs`, `tests/common/parity.rs`, `fixtures/kicad/synthetic/` (generated by `tools/kicad-reader/synthetic.py` with pcbnew 10.0.7; exports by `export.sh`). `build_board_succeeds` and `tree_round_trips_through_dsn_text` pass on all 12 boards (the tree prints and parses back equal). `corpus_builds` (ignored, `cargo test --release -p freerouting-kicad --test structure corpus_builds -- --ignored --nocapture`): all 57 readable corpus boards (the 58th, `TestSensel`, is KiCad 5 and refused by K04's version gate) read and `build_board` succeeds; the largest, `jetson-agx-thor-baseboard.kicad_pcb` (84,806,775 bytes), reads in 1,178 ms in release (1,500 ms in a standalone run; parse 322 ms, images 830 ms), `vme-wren` (69.6 MB) in 334 ms. A first version took over ten minutes on that board because it counted source lines per pad (linear in the file); line numbers are now computed only for warnings and errors. `corpus_parity` (ignored) compares all 56 boards that have an export: 13 still differ, none in layers, nets, classes, placement or boundary except `Natural_Tone_Preamp`/`tutorial_board` (see the report); the rest are via lists (K06), custom-pad hull sizes, rare outline details. Dependency note: the tests use `freerouting` as a dev-dependency for `build_board`, so they do not compile while the engine is mid-edit.
