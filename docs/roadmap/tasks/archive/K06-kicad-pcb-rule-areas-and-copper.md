# K06 — Rule areas, zones, tracks and vias from `.kicad_pcb`

Opened 2026-10-15. This checklist adds the copper and the keepouts to the
syntax tree K05 builds. It is where the reader departs from KiCad's DSN
export on purpose. KiCad picks a rule area's DSN kind from its vias and
tracks flags only:

```
if vias && tracks      -> keepout
else if vias           -> via_keepout
else if tracks         -> wire_keepout
else                   -> keepout
```

(`SPECCTRA_DB::FromBOARD`, "zones flagged keepout areas become keepout", and
`makeIMAGE` for footprint-owned areas, `specctra_export.cpp` at `10.0.0`,
read 2026-10-15.) So an area that bans only footprints, pads or copper pour,
or a KiCad 9+ placement area that bans nothing, becomes a full keepout. On
`canary-pwr` that is the 26 × 26 mm "Fan inlet keep-out" on B.Cu (flags:
pads and footprints `not_allowed`, tracks, vias and copper pour allowed;
`canary-pwr.kicad_pcb` around line 48968). KiCad's export made it
`(keepout "" (polygon B.Cu 0 136500 -53000 ...))`
(`canary-pwr/freerouting-rs/canary-pwr.dsn:64`), which cost routing room on
B.Cu. That is the owner lesson from canary-09 this series exists for.

Track arcs are the other difference: KiCad exports each arc as one straight
chord (start to end, in the "create the wires from tracks" loop). The reader
follows the arc.

Prerequisites: [K05](K05-kicad-pcb-board-structure.md). K06.1 touches
`freerouting-dsn` and the engine's DSN reader; coordinate with
[C06](../C00-cli-settings-io-README.md) (the DSN writer) if it is in flight.

## What exists and what this adds

| Exists | State |
|---|---|
| `freerouting-dsn` `canonical_keyword` (`src/parse/mod.rs`) | Maps `wire_keepout` to `keepout` at the lexer, as Java does (`SpecctraFileDescription.flex:201` at `cda540f`). The tree can't tell the two apart |
| `ast::KeepoutKind::{Keepout, ViaKeepout, PlaceKeepout}`; `keyword()` drives the writer | `tests/syntax.rs` asserts a `wire_keepout` reads as `Keepout` |
| `io/specctra/parser/structure.rs` and `parser/library.rs` | Group keepouts by kind (index 0, 1, 2) in file order before inserting them; the order sets item ids |
| `BasicBoard::is_obstacle` | `AreaKind::Obstacle` blocks traces and vias, `ViaObstacle` vias, `ComponentObstacle` only placement. There is no kind that blocks traces but not vias |
| `Issue754-avionics_hub.dsn:53` | `(wire_keepout "" (polygon Ground 0 ...))` over the whole Ground layer; blocks every via there, today and in Java |
| KRT `extract_keepouts` (`py_router/kicad_parser.py:6917`) | Reads only the tracks, vias and copperpour flags. KiCad's reader (`pcb_io_kicad_sexpr_parser.cpp`, case `T_keepout`) also has `pads` and `footprints`, both defaulting to allowed when absent |
| KRT #478, #1214 | A footprint-owned zone stores its polygon in absolute board coordinates |
| Java fixtures | Rule areas only with tracks and vias banned (`Issue180-Test`: 5, `Issue558`: 1, `Issue230`: 8). Other flag mixes need a synthetic board and `canary-pwr` |

## Decisions made here

- **Rule-area mapping, per area and per copper layer:**

  | `not_allowed` flags | Reader emits | KiCad's export |
  |---|---|---|
  | tracks and vias (any others too) | `keepout` | `keepout` |
  | vias, not tracks | `via_keepout` | `via_keepout` |
  | tracks, not vias | `wire_keepout`, built as a full keepout, plus a warning | `wire_keepout` (read as `keepout`) |
  | footprints | also a `place_keepout` on the same layers | nothing extra |
  | pads only, copper pour only, nothing (placement areas) | nothing, one warning line | `keepout` |

  So `canary-pwr`'s fan inlet becomes a `place_keepout` only. The tracks-only
  case keeps Java's behaviour, so routes don't change on boards like Issue754
  (K00 question 1); making it trace-only is a later, measured Q-series
  change. Copper-pour-only areas could cut a plane; Freerouting has no fill
  model, so the warning says the plane on that layer is not cut.
- **`wire_keepout` gets its own syntax-tree kind** (`KeepoutKind::WireKeepout`)
  and builds exactly like `keepout`, in the same group and file position.
  That keeps the information without changing a single routed result, and
  turns the later Q change into one match arm. The DSN parser stops
  aliasing the keyword; the board doesn't notice.
- **Zones: the outline, as KiCad exports them.** The plane is the zone
  outline with its holes, not the `filled_polygon` (KiCad's export uses
  `zone->IterateWithHoles()`, the outline). Fills can be stale or absent on
  routed boards (KRT §6.4).
- **Copper zones owned by footprints are dropped, with a warning**, as
  KiCad's export drops them (its plane loop walks `BOARD::Zones()` only; a
  footprint's zones reach `makeIMAGE` only when they are rule areas).
- **Arcs follow the curve.** A track arc becomes a polyline through points
  on the arc, at most 1 µm from it, with its two endpoints exact. KiCad's
  chord is shorter and can cut a corner; K07 lists the difference.
- **Orders that decide results** (item ids in `build_board`), reproduced
  from the exporter:
  - structure keepouts: the outline holes from K05.4 first, then rule areas
    in board zone order; per area, its copper layers in KiCad's layer-id
    order (KiCad 9+ ids: F.Cu 0, B.Cu 2, In1.Cu 4, In2.Cu 6 ..., so F.Cu,
    B.Cu, In1.Cu, In2.Cu ...: the `canary-pwr` planes come out as F.Cu,
    B.Cu, Gnd.Cu, Power.Cu);
  - image keepouts: copper-less holes in pad order (K05.6), then
    footprint rule areas, each in physical layer order (`CuStack`);
  - planes: zones in board order, layers in layer-id order;
  - wires: board track order, merged as the exporter merges them (K06.5);
  - vias: board track order.
  Whether `BOARD::Zones()` and `BOARD::Tracks()` keep file order after
  loading is unverified; K07.3's order comparison checks it.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K06.1 | — | `crates/freerouting-dsn/src/{ast.rs,lib.rs,parse/mod.rs,parse/dsn.rs,parse/common.rs}`, `crates/freerouting-dsn/tests/syntax.rs`, `crates/freerouting/src/io/specctra/parser/{structure.rs,library.rs}`, `docs/DIVERGENCES.md` |
| K06.2 | `crates/freerouting-kicad/src/rule_areas.rs`, `fixtures/kicad/synthetic/rule-areas.kicad_pcb` (+ `.kicad10.dsn`) | `src/read.rs` |
| K06.3 | — | `src/rule_areas.rs`, `src/footprints.rs` |
| K06.4 | `crates/freerouting-kicad/src/zones.rs` | `src/network.rs` (no-net nets), `src/read.rs` |
| K06.5 | `crates/freerouting-kicad/src/tracks.rs`, `fixtures/kicad/synthetic/arcs.kicad_pcb` (+ `.kicad10.dsn`) | `src/arc.rs`, `src/read.rs` |
| K06.6 | `crates/freerouting-kicad/src/vias.rs`, `fixtures/kicad/synthetic/vias.kicad_pcb` (+ `.kicad10.dsn`) | `src/classes.rs` (via library), `src/read.rs` |
| K06.7 | `crates/freerouting-kicad/src/report.rs`, `tests/copper.rs` | `docs/DIVERGENCES.md` (a "freerouting-kicad" section), `src/lib.rs` (docs) |

## Checklist

- [x] **K06.1 — `KeepoutKind::WireKeepout`.**
  - `ast.rs`: add `WireKeepout` with `keyword()` `"wire_keepout"` and a doc
    line saying the board treats it as `keepout`, as Java does.
  - `parse/mod.rs`: remove `"wire_keepout" => "keepout"` from
    `canonical_keyword`. Accept the keyword everywhere `keepout` is
    accepted: `parse/dsn.rs` structure and image scopes and
    `keepout_scope`, and `parse/common.rs` place-level
    `(keepout name (clearance_class c))` (`place.keepouts`). Update the
    keyword list in `lib.rs`'s docs.
  - Engine: `parser/structure.rs` and `parser/library.rs` map
    `WireKeepout` to index 0, the `keepout` group, in its file position.
    Each such keepout logs one warning: "wire_keepout on layer L is treated
    as a keepout and also blocks vias".
  - `tests/syntax.rs`: the fourth keepout is now `WireKeepout`; add a
    round-trip test that `to_dsn` writes `wire_keepout`.
  - `DIVERGENCES.md`, freerouting-dsn: "The syntax tree keeps `wire_keepout`
    as its own kind; the board builds it as `keepout`, as Java does."
  - **Accept:**
    - `tests/reference_load.rs` and `tests/reference_drc.rs` pass with no new
      expected mismatch, on every fixture;
    - on `Issue754-avionics_hub`, the Ground area is still an
      `AreaKind::Obstacle`, and `is_obstacle` says it blocks a via inside it
      (unit test with a via placed in the area);
    - the item ids of a `build_board` of Issue754 are the same before and
      after (compare the id sequence of all items by kind).
  *Done 2026-10-16:* `KeepoutKind::WireKeepout` in `freerouting-dsn` (`ast.rs`, `parse/{mod,dsn,common}.rs`, docs in `lib.rs`); the alias is gone from `canonical_keyword`. In a `(place ...)` scope, which holds names only, `wire_keepout` is read as `keepout` (noted in the crate docs). Engine: one match arm each in `parser/structure.rs` and `parser/library.rs` mapping it to index 0, with a **log line** per area (`board.log_messages()`), not a caller warning: `reference_load` compares the warning list with Java's and a warning there would be a new mismatch. `tests/syntax.rs`: `full_design` (fourth keepout is `WireKeepout`) and `wire_keepout_keeps_its_kind` (structure, image, place scopes; round trip writes `wire_keepout`). `crates/freerouting-kicad/tests/wire_keepout.rs`: `issue754_item_ids_do_not_change` (2,384 items, FNV hash of `id type layers nets` of every item, taken before the change: `0xa2275f6b2b485322`, unchanged) and `issue754_ground_area_still_blocks_vias` (the area is `AreaKind::Obstacle`, `is_obstacle(via, area)` true). `cargo test -p freerouting --test reference_load --test reference_drc` pass with no new expected mismatch. DIVERGENCES line added.

- [x] **K06.2 — Board rule areas.**
  - A rule area is a `(zone ...)` with `(keepout ...)` or `(placement ...)`
    (both set `SetIsRuleArea` in KiCad's reader). Read the five flags;
    absent `pads` and `footprints` mean allowed (KiCad's reader). The flag
    defaults for an area with `(placement ...)` and no `(keepout ...)`: read
    `ZONE`'s constructor in `pcbnew/zone.cpp` at `10.0.0` (unverified here).
  - Layers: `(layer x)` or `(layers ...)`, expanded (`*.Cu`, `F&B.Cu`,
    `*In.Cu`); keep only copper layers. An unresolved name is a warning,
    never a silently disabled keepout (KRT #369 A5).
  - Polygon: the first `(polygon (pts ...))` is the outline; each further
    `(polygon ...)` is a hole, emitted as a `(window ...)` (KiCad's reader:
    "The first polygon is the main outline. Others are holes"). `(arc ...)`
    points inside `pts` are tessellated the way KiCad's
    `parseOutlinePoints` and `SHAPE_ARC::ConvertToPolyline` do (KRT ports
    the latter with 1,000 nm for KiCad 10 and 5,000 nm for KiCad 9;
    unverified, read `libs/kimath/src/geometry/shape_arc.cpp`).
  - Emit per the mapping table, name `""`, in the order of the decisions.
  - Synthetic `rule-areas.kicad_pcb` (made with pcbnew in WSL KiCad 10.0.7,
    committed with its export): a 4-layer board with one area per row of the
    table, plus tracks-and-footprints, pads-and-footprints (the canary
    case), a placement-only area, one area on `*.Cu`, one on `F&B.Cu`, and
    one with a hole.
  - **Accept:**
    - on the synthetic board, the built board's areas are exactly: one
      `Obstacle` per layer for each tracks-and-vias area; `ViaObstacle` for
      vias-only; `Obstacle` plus a warning for tracks-only; an added
      `ComponentObstacle` for each area banning footprints; nothing for
      pads-only, copper-pour-only and placement-only;
    - a probe trace and a probe via inside the pads-and-footprints area are
      not blocked by it (`BasicBoard::is_obstacle(trace, area)` and
      `is_obstacle(via, area)` are false), while the area is an obstacle to
      another component's `ComponentObstacle` area, the only thing
      `ComponentObstacle` blocks;
    - `canary-pwr` (out of repo): the fan inlet is one `ComponentObstacle`
      on B.Cu and blocks neither traces nor vias;
    - its KiCad export, read through the DSN path, still shows the full
      keepout (the expected difference K07 lists).
  *Done 2026-10-16:* `src/{zones,rule_areas}.rs`, `fixtures/kicad/synthetic/rule-areas.kicad_pcb` (+ `.kicad10.dsn`, pcbnew 10.0.7, generated by `tools/kicad-reader/synthetic.py rule-areas`). Defaults read from KiCad 10.0.0 `zone_settings.cpp` (tracks, vias, pads not allowed; a `(keepout)` block resets pads and footprints to allowed; a `(placement)` area without one keeps the defaults: `rule_area_flag_defaults_follow_kicad`). `tests/copper.rs`: `rule_areas_map_by_flags` (the board has exactly 6 `Obstacle` on F.Cu, 2 on B.Cu, one each on In1/In2; `ViaObstacle` on F, B, In1, In2; `ComponentObstacle` 2 on F.Cu and 1 on B.Cu; nothing for pads, pour and placement; 4 tracks-only warnings and 3 no-keepout warnings), `pads_and_footprints_area_blocks_only_placement` (trace and via inside: `is_obstacle` false; it blocks another component's placement area), `canary_pwr_fan_inlet` (out of repo: one `ComponentObstacle` on B.Cu, report lists flags and `place_keepout`, probes not blocked, KiCad's export still has the full `keepout` at 136500 -53000), `unusable_copper_gives_warnings` (unresolved layer name warns). Holes become windows; arcs in `pts` are tessellated at 1 um (`zone_outline_arcs_are_polylines`, `ShapeArc::to_polyline`; 1,000 nm = `DefaultAccuracyForPCB()/5` in 10.0.0). `copper_equals_the_export` and `corpus_copper` (ignored; 56 boards incl. the two over 20 MB, `FR_ALL=1`) compare every area with the export's polygon (0 differences).

- [x] **K06.3 — Rule areas owned by footprints.**
  - A `(zone ...)` inside a `(footprint ...)` stores absolute board
    coordinates (KRT #478, #1214). Convert as `makeIMAGE` does: after the
    back-side flip (K05.5), rotate the polygon about the footprint position
    by −orientation, subtract the position, then `map_pt`.
  - Same flag mapping as K06.2. Layers in physical order (`CuStack`).
    Emitted as image keepouts after the copper-less hole keepouts; they are
    part of the image key, so they affect image grouping (K05.5).
  - **Accept:**
    - footprints with a rule area, added to `rule-areas.kicad_pcb` and
      placed at 0°, 30° and 135° on both sides: after `build_board`, each area's absolute polygon equals the
      file's absolute coordinates within 1 board unit;
    - on boards whose footprint areas ban tracks or vias, the image keepouts
      equal the export's (kind, layer, points within `%.6g` rounding).
  *Done 2026-10-16:* `src/footprints.rs` (`Footprint.zones`, mirrored and layer-flipped by `flip_to_front`, and `zones_as_filed`), `src/rule_areas.rs`. Converted as `makeIMAGE`: rotate about the position by the normalised minus orientation, subtract the position, `map_pt`; layers in `CuStack` order; image key includes kind and windows. A KiCad quirk is reproduced: a footprint area's holes are written on the zone's first layer whichever layer the keepout is on. `footprint_areas_keep_the_files_coordinates`: after `build_board` each footprint area's corners are within 1 board unit of the file's coordinates (footprints at 0, 30 and 135 degrees, front and back, areas on 1 to 2 layers, one with a hole). `copper_equals_the_export` / `corpus_copper`: image keepouts equal the export's (kind, layer, points within `%.6g`) on all boards (`Issue180-Test` has 4 footprint areas, `Issue558` one).

- [x] **K06.4 — Copper zones become planes.**
  - Each non-rule-area zone on a copper layer, in board zone order; one
    `Plane` per copper layer in layer-id order; net name from the zone's
    `(net ...)` (dialect from K04.6); the outline and holes as in K06.2.
  - A netless zone gets a made-up net `@:no_net_<n>` (n counting from 0 in
    zone order). The exporter pushes these nets into the network while it
    writes the planes, before any real net, so they come first in the network
    list (`FromBOARD`, "zones (not keepout areas) become planes").
  - Footprint-owned copper zones: dropped with a warning naming the
    footprint.
  - Note for later: KiCad master exports pours as `(wire (polygon ...))`
    instead of planes (gui report §6.2, unverified). The reader doesn't
    depend on the export, so it is unaffected; K07 records it.
  - **Accept:**
    - on every committed board with zones (`Issue269-min_fr_test`,
      `Issue558`, `Issue180-Test`) and on `canary-pwr`: plane count, order,
      nets and layers equal the export, and each plane's area equals the
      export's within 0.1 %;
    - a synthetic netless zone gives `@:no_net_0` at the head of the
      network, as the export does.
  *Done 2026-10-16:* `src/zones.rs` (`make_plane`), `read.rs`. `copper_equals_the_export` / `corpus_copper`: plane count, order, nets, layers equal the export's and each plane's vertices agree within a `%.6g` step (area within 0.1 % otherwise) on all 12 committed boards (`Issue180-Test`: 2 planes, `Issue269-min_fr_test`: 1) and the 56-board corpus including `canary-pwr` (20 planes). `planes_and_netless_zones`: netless zones give `@:no_net_0..2` (one per plane, In1 then In2) at the head of the network as the export does. **Deviation, K00 question 6 (Dan accepted the recommendation):** copper zones owned by footprints are kept as planes after the board's own, with a warning each, instead of being dropped as this item said; `ReadOptions::drop_footprint_zones` gives KiCad's behaviour (the parity tests use it). DIVERGENCES line added.

- [x] **K06.5 — Tracks.**
  - `(segment ...)` and `(arc ...)` in file order; skip net 0 (no net, or
    `(net "")`, or code 0).
  - Merge into wires exactly as the exporter does: start a new `Wire` when
    the net, the width or the layer changes, or when the item's start isn't
    the current path's last point; otherwise append the end point. Compare
    points in nm.
  - Arcs: points from `(start) (mid) (end)` in nm; centre and radius in
    `f64` from those three points; interior points at a step whose sagitta
    is at most 1 µm, each rounded to nm with `llround`; start and end exact.
    Fixed operation order and no FMA (D6), so runs are identical.
  - Wire type: `(type fix)` when locked (`locked` atom in older files,
    `(locked yes)` in newer ones; KRT #1158 found a stray `(locked yes)`
    dropping tracks), else `(type route)`. Width as the path aperture.
  - **Accept:**
    - on `Issue180-Test` (102 segments, no arcs), the wiring equals the
      export: wire count, nets, layers, widths, types, and every point within
      half a `%.6g` step;
    - synthetic `arcs.kicad_pcb`: arcs of 10°, 90° and 270° at radii 0.1 mm
      to 20 mm, joined to segments: each wire's length equals the analytic
      length within 0.1 %, and the wire stays connected (endpoints equal);
    - `CM5_MINIMA_3.kicad_pcb` (out of repo, 200 track arcs, numeric nets):
      total trace length after `build_board` within 0.1 % of KiCad's own
      length (`pcbnew` `GetLength()` summed over tracks, recorded by the
      export script);
    - two reads give identical trees.
  *Done 2026-10-16:* `src/tracks.rs`, `fixtures/kicad/synthetic/arcs.kicad_pcb` (+ export). Merge, net-0 skipping and `locked`/`(locked yes)` as specified (`consecutive_tracks_merge_into_wires`). Arcs: circle from the three points in `f64` relative to the start, chords of at most 0.1 rad **and** 1 um from the curve (the 0.1 rad limit is added: 1 um alone leaves 0.3 % length error at r = 0.1 mm), ends exact. `issue180_wires_equal_the_export`: 86 wires (15 `fix`, 71 `route`) equal the export. `arcs_follow_the_curve`: 16 arcs (10, 90, 270, -90 degrees; r = 0.1, 1, 5, 20 mm): ends exact, length within 0.1 % of an independent analytic length, and the export's chord points lie on our path. `cm5_trace_length_matches_kicad` (out of repo, 200 arcs): `build_board` total 2785.768 mm vs pcbnew `GetLength()` sum 2785.802 mm (`tools/kicad-reader/track_length.py`, KiCad 10.0.7), 0.001 %. `reads_are_deterministic`: two reads give identical trees, warnings and reports.

- [x] **K06.6 — Vias.**
  - `(via [blind|buried|micro] (at) (size) (drill) (layers a b) (net ...)
    ...)` in file order; skip net 0. The span is the copper layers between
    `a` and `b` in physical order.
  - Padstack per `SPECCTRA_DB::makeVia`: one circle of the via diameter per
    spanned layer, name `Via[top-bottom]_<d>:<drill>_um` via
    `snprintf("%.6g")`, registered in the via library after the class vias if
    new (`LookupVia`).
  - Type `fix` when locked, else `route`.
  - **Accept:**
    - via count, positions, padstack names and the structure `(via ...)`
      list equal the export on `Issue180-Test` (627 locked vias, so `(type fix)`) and `canary-pwr`;
    - synthetic `vias.kicad_pcb` (6 layers, one through, one blind, one
      buried, one micro via) gives the export's padstacks and spans.
  *Done 2026-10-16:* `src/vias.rs`, `fixtures/kicad/synthetic/vias.kicad_pcb` (+ export). `issue180_vias_are_fixed`: 627 vias (the file has 624 locked, not 627 as this item said; 624 are `(type fix)`) with the export's positions, padstack names and via list; `copper_equals_the_export` / `corpus_copper` cover `canary-pwr` (41 vias) and the corpus. `via_padstacks_and_spans`: the 6-layer board gives `Via[0-5]_800:400_um`, `Via[0-2]_700:300_um`, `Via[1-3]_600:300_um`, `Via[0-1]_300:100_um`, `Via[3-5]_700:300_um` with 6/3/3/2/3 circles, equal to the export. The class vias are listed in project order and KiCad's in `std::unordered_map` order, so the lists compare as sets after the Default via (`copper::via_lists`; DIVERGENCES line).

- [x] **K06.7 — Read report, warnings and divergence lines.**
  - `ReadReport` beside the warnings: per rule area its name, layers, flags
    and the kinds emitted; counts of arcs flattened, footprint copper zones
    dropped, custom-pad hulls used. `KicadRead` carries it; the CLI and GUI
    print it in K07.
  - `DIVERGENCES.md`, a new "freerouting-kicad" section (differences from
    KiCad's DSN export, which is what Java reads):
    - rule areas banning only footprints, pads or copper pour, or nothing,
      are not keepouts; footprint bans add a placement keepout;
    - track arcs are followed, not chorded;
    - coordinates keep KiCad's nanometres instead of `%.6g`.
  - Each line names its test.
  - **Accept:** the report printed for `canary-pwr` lists the fan inlet with
    its flags and `place_keepout`; every warning kind above has a test that
    triggers it.
  *Done 2026-10-16:* `src/report.rs` (`ReadReport` in `KicadRead::report`, `Display`), DIVERGENCES "freerouting-kicad" section (each line names its test). `canary_pwr_fan_inlet`: the printed report lists the fan inlet with flags (`pads not_allowed ... footprints not_allowed`) and `emitted: place_keepout`. Every warning kind has a test: tracks-only (`rule_areas_map_by_flags`), pads/pour/nothing (same), unresolved layer, fewer than three corners, non-copper track layer, via with bad layers, arc without mid (`unusable_copper_gives_warnings`), footprint copper zone kept or dropped (`planes_and_netless_zones`); counts: track arcs (`arcs_follow_the_curve`), zone arcs (`zone_outline_arcs_are_polylines`), custom pad hulls (`report_counts_hulls_and_prints`: 3 on `pads`).
