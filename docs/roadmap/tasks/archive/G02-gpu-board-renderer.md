# G02 — GPU board renderer

Opened 2026-10-15. Every frame, `view::paint` clones every batch's index
buffer, transforms every vertex on the CPU, and hands egui fresh
`Shape::mesh` and `Shape::line` shapes, which epaint re-tessellates and
uploads again. There is no culling. `Issue190-processor.Z80` is 350,056
triangles, and real boards are ten times larger. Live routing redraw (G07)
and dragging (G09) need geometry that stays on the GPU and changes in small
pieces. This checklist moves the board into GPU buffers uploaded once,
culls by viewport, patches by `ItemId`, and proves it with the frame
targets FT1–FT4 and FT6 in [G00](../G00-desktop-app-README.md). It is the
report's GUI-2 and the `performance.md` item "Large boards in the GUI".

**Prerequisites:** G01 (`lib.rs`, `src/stats.rs`, `--bench`, the kittest
harness and FT0). No engine dependency.

## What exists and what this adds

| Exists (Rust at `70389b7`) | State |
|---|---|
| `crates/freerouting-gui/src/view.rs` `Tessellation::new` | Triangulates the scene once per load (earcutr), as `f32` mm relative to the board centre, one `Batch` per (slot, kind); zero-width paths and outlines become `Line`s |
| `view.rs` `paint` | Draw order: copper layers from the bottom up skipping the active one, the active layer, then `Silk(Back)`, `Silk(Front)`, `Board`; within a slot, kinds in `Kind::ALL` order; per kind, meshes then lines. Inactive layers are dimmed toward the background (`BACKGROUND.lerp_to_gamma(color, 0.6)`), translucent kinds by `gamma_multiply(0.6)` |
| `view.rs` `draw_labels` | Reference designators as egui text, back side first; labels under 6 px skipped |
| `Camera` | Pan and zoom; `zoom` in px per mm |
| egui-wgpu 0.32.3 | `CallbackTrait { prepare, finish_prepare, paint }`, `Callback::new_paint_callback`, `Renderer::callback_resources` (a type map) |
| egui_kittest 0.32.3 | `wgpu.rs` sets `cc.wgpu_render_state` for `build_eframe`, so snapshots include a paint callback |
| `Cargo.toml` | wgpu 25 with `dx12, metal, vulkan, gles, wgsl` |
| Java | `gui/rendering/BoardRenderer.java` repaints the whole board with Java2D and culls by the clip box. Nothing to port; the design here is free (D1) |

What this adds: `src/canvas/` (GPU pipeline, WGSL shader, chunked buffers),
screen-space hairlines, viewport culling, patching by `ItemId`, a synthetic
16× stress board, a faster load, and a CPU fallback behind an environment
variable.

## Decisions made here

- **One paint callback draws the board.** `src/canvas/gpu.rs` holds a
  `BoardGpu` resource in `callback_resources`: one render pipeline for
  filled triangles, one for hairlines, the vertex and index buffers of every
  chunk, and a uniform buffer. Per frame, `prepare` writes one camera
  uniform (offset, scale, viewport size, pixels per point) and the draw list;
  `paint` binds and draws. Nothing else is uploaded unless a chunk changed.
- **Colours per draw, not per vertex.** Each draw (one chunk of one
  (slot, kind)) reads its colour, after dimming, from a dynamic-offset
  uniform buffer. Push constants are not available on every backend, so
  they are not used. Dimming keeps today's rule: blend toward the
  background for opaque kinds, multiply for translucent ones.
- **Draw order is today's order, exactly**, because it decides what is on
  top: the slot order and kind order listed above, chunks of a batch in
  chunk order. Within one (slot, kind) all draws have the same colour, and
  blending identical colours is order-independent, so the order of chunks
  inside a batch does not change the image.
- **Chunks hold items in ascending `ItemId` order.** A chunk is rebuilt from
  its item set, sorted, every time. A patched chunk is then bit-identical to
  one built fresh, which makes patching testable by equality.
- **Hairlines are screen-space quads** expanded in the vertex shader from
  segment endpoints and a width in pixels (1 px × pixels per point). Board
  outlines and footprint outlines stay 1 px at every zoom.
- **`f32` vertices relative to the board centre**, as today. At 500 mm from
  the centre an `f32` resolves about 3e-5 mm, far below a pixel at any
  useful zoom.
- **Labels stay egui text** on top of the callback; they are few after the
  6 px rule.
- **The CPU painter stays as a fallback** (`FREEROUTING_GUI_RENDERER=cpu`),
  and is used automatically when eframe gives no wgpu render state.

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G02.1 | `src/canvas/{mod,gpu}.rs`, `src/canvas/board.wgsl` | `src/view.rs` (keep `Tessellation` as the CPU builder), `src/app.rs` (call the canvas), `src/lib.rs` |
| G02.2 | `src/canvas/hairline.wgsl` | `src/canvas/gpu.rs`, `src/view.rs` |
| G02.3 | `src/canvas/chunks.rs`, `tests/ui_render.rs` | `src/view.rs`, `src/canvas/gpu.rs`, `src/stats.rs` (append counters) |
| G02.4 | — | `src/canvas/chunks.rs`, `src/scene.rs` (`ItemId` per scene item, per-item builder) |
| G02.5 | `benches/` or `src/bench_synthetic.rs` | `src/main.rs` (`--bench-synthetic`) |
| G02.6 | — | `src/view.rs`, `src/session.rs`, `Cargo.toml` (`rayon` only if measured) |
| G02.7 | — | `src/view.rs`, `src/canvas/mod.rs` |
| G02.8 | — | `docs/development/gui.md`, `src/stats.rs` |

`src/canvas/**` and `src/view.rs` are this checklist's alone. G03 draws its
overlays in `src/overlays/` with egui shapes and must not need changes here.

## Checklist

- [x] **G02.1 — Paint callback with GPU-resident buffers.**
  - `BoardGpu::upload(&Tessellation)` creates one vertex and one index
    buffer per batch (chunks come in G02.3), `u32` indices, `[f32; 2]`
    positions. `CanvasCallback` implements `egui_wgpu::CallbackTrait`;
    `prepare` writes the camera uniform and per-draw colours; `paint`
    issues the draws in the order in the decisions.
  - The board is uploaded when a load finishes, never per frame.
  - Counters in `src/stats.rs`: `draws`, `uploaded_bytes` (per frame and
    total), `gpu_buffer_bytes`.
  - **Accept:**
    - `tests/ui_viewer.rs::snapshot_issue159` (G01.4) passes on the GPU path
      without regenerating the PNG; if anti-aliasing differences need a new
      golden, the old and new PNGs are both opened by the implementer and
      described, and the change is ≤ 1 % of the pixels by `compare`.
    - `tests/ui_render.rs::no_upload_after_first_frame`: after the load,
      10 frames of panning report `uploaded_bytes` equal to the uniform
      writes only (≤ 4 KB per frame).
    - `--bench pan-zoom --frames 600` on Issue190 in release: numbers in the
      done note next to FT0. FT1 itself is due at G02.3.
  *Done 2026-10-15:* `src/canvas/{mod,gpu}.rs`, `board.wgsl`; `view::paint`
  makes a draw plan and adds one `CanvasCallback`; `BoardGpu` (in
  `callback_resources`) holds the pipelines, per-chunk buffers (chunks came
  with G02.3, so one buffer set per chunk, not per batch), a camera uniform
  and a dynamic-offset colour table indexed by (slot, kind). The board goes up
  in the first `prepare` after a load. Counters in `stats.rs`: `draws`,
  `uploaded_bytes` (last frame, one frame behind because `prepare` runs after
  the update), `uploaded_total`, `gpu_buffer_bytes`.
  `tests/ui_render.rs::no_upload_after_first_frame`: after the first upload
  (7 KB-scale for Issue159), 10 panning and zooming frames each report
  `uploaded_bytes == 32` (the camera uniform), asserted exactly, and `<= 4096`.
  Snapshot: the full-window `snapshot_issue159` golden cannot stay as it is,
  because G03's toolbar and Checks section changed the UI (G03 recorded
  `viewer_issue159_g03.png` with this renderer). The canvas itself is checked
  against the G01 golden by `ui_render.rs::gpu_canvas_matches_the_g01_golden`
  (a copy of the old `viewer_issue159.png` is `g01_canvas_issue159.png`): the
  board is drawn into the G01 canvas rectangle (203,22)-(1280,778) and 9,051
  pixels (0.884 % of the 1280x800 window; 10 % fuzz as in `smoke.sh`) differ.
  I opened both PNGs and a 400x300 crop of each with the difference: pads,
  vias, labels, the board outline and fills are identical; what differs is
  the hairlines (footprint and board outlines): the old canvas drew them as
  egui's feathered (anti-aliased, slightly wider and lighter) strokes, the
  shader draws crisp one-point lines. Run on the real app too:
  `freerouting-gui --screenshot out.png --exit Issue190-processor.Z80.dsn`
  under `xvfb-run` shows the whole board, layers, pads, outlines and labels
  correctly. `--bench pan-zoom --frames 600` on Issue190, release: see FT1 below
  and `gui.md` "Renderer numbers" (FT0 old definition: p95 3.07 to 3.57 ms;
  new renderer update p95 0.33 ms).

- [x] **G02.2 — Hairlines in the shader.**
  - Lines (`Line` today) become instanced quads: per segment, two endpoints
    in board mm; the vertex shader offsets each corner by half a pixel
    width along the segment normal in screen space.
  - `paint` in the GPU path adds no `Shape::line` and no `Shape::mesh`.
  - **Accept:**
    - `tests/ui_render.rs::hairline_is_one_pixel`: a scene with one
      horizontal outline segment, rendered at the fitted zoom and at 20×,
      gives exactly `round(pixels_per_point)` lit rows in the column at the
      segment's midpoint (read back from the rendered image).
    - `grep -n "Shape::line\|Shape::mesh" src/canvas` finds nothing.
  *Done 2026-10-15:* `hairline.wgsl`: instanced quads (a, b, width in mm),
  expanded in screen space to at least one point wide, with square caps so
  polyline joints leave no gaps. `Line` is gone; `tessellate_item` puts
  hairline segments in `MeshBuf::lines`. `grep -n "Shape::line\|Shape::mesh"
  src/canvas` finds nothing (exit 1). `ui_render.rs::hairline_is_one_pixel`:
  at pixels_per_point 1 and 2, at the fitted zoom and at 20x, with the line at
  five different sub-pixel offsets, the column at the segment's midpoint has
  exactly `round(ppp)` non-background rows (read from the rendered image).

- [x] **G02.3 — Chunked and culled batches (meets FT1).**
  - `src/canvas/chunks.rs`: each (slot, kind) batch is split into chunks of
    at most 4,096 items, grouped by a uniform grid over the board bounds
    (cell size chosen so the median cell holds about 1,000 items); chunk
    order is row-major by cell. Each chunk keeps its bounding box in board
    mm and its vertex and index ranges.
  - Chunks whose box misses the viewport are not drawn. Counter
    `chunks_drawn` and `chunks_total`.
  - **Accept:**
    - `chunks::tests::every_item_in_one_chunk`: on Issue159 and Issue690,
      every scene item appears in exactly one chunk, and every chunk's box
      contains its items' boxes.
    - `tests/ui_render.rs::culls_at_20x`: on Issue190 at 20× zoom centred on
      the board, `chunks_drawn < 0.2 × chunks_total` (deterministic
      counters).
    - FT1: `--bench pan-zoom --frames 600` on Issue190, release, quiet
      machine, three runs: frame CPU p95 ≤ 2 ms and ≥ 5× better than FT0.
      Numbers and load averages in the done note.
  *Done 2026-10-15:* `src/canvas/chunks.rs`: grid chosen so the median
  occupied cell holds about 1,000 items, chunks of at most 4,096 items cut at
  id boundaries, row-major by cell, each with its box and ranges. Issue190: 96
  chunks. `canvas::plan` skips chunks whose box misses the view (2-point
  margin); `chunks_drawn`/`chunks_total` in `PaintStats` and `FrameStats`.
  `chunks::tests::every_item_in_one_chunk` (Issue159 and Issue690-coldfire):
  every scene item (by `(id, seq)`) in exactly one chunk of its slot and kind,
  whose box contains the item's box. `ui_render.rs::culls_at_20x`: Issue190 at
  20x centred on the board: 15 of 96 chunks drawn (15.6 %, bound 20 %); at the
  fitted zoom all 96. FT1, release, Xvfb/lavapipe, 3 runs (the machine was
  busy: load average 11.2, 13.2, 15.2): frame CPU (new definition: update plus
  egui end-of-pass, tessellation and upload plus the callback's prepare and
  paint) p95 0.461, 0.444, 0.468 ms (target 2 ms); p50 0.333, 0.260, 0.299.
  FT0 re-baselined with the old renderer under the same definition (HEAD plus a
  marker callback, interleaved runs, load 12.8 to 13.1): p95 26.4, 19.5, 9.8 ms,
  so at least 21x better; against G01's recorded update-only FT0 (p95 3.07 to
  3.57) at least 6.5x. Both definitions are in `gui.md` "Renderer numbers".
  The measurement now includes egui's tessellation and upload (orchestrator
  decision); `--bench` prints both (`p50_ms`.. update CPU, `frame_*` frame CPU).

- [x] **G02.4 — Patching by `ItemId` (meets the CPU part of FT4).**
  - Scene items carry their `ItemId`. `Scene::item(board, id) ->
    Option<SceneItem>` flattens one item, so a delta needs no full scene
    rebuild (B08's `ItemGeometry` replaces this flattening later, in G07).
  - `ChunkSet::patch(removed: &[ItemId], added: &[SceneItem])` finds the
    owning chunks through an `ItemId → chunk` index (a `Vec` indexed by id
    or a `BTreeMap`; no hash map), rebuilds and re-uploads only those
    chunks, and puts new items in the chunk of their grid cell. A chunk that
    grows past 8,192 items splits.
  - Counter `full_rebuilds` counts calls of the full build after the first.
  - **Accept:**
    - `chunks::tests::patch_equals_fresh_build`: on Issue690, remove 200
      items and add them back, chosen with
      `freerouting::geometry::jmath::JavaRandom::new(7)`; every chunk's
      vertex and index buffers are bit-identical to a fresh build.
    - `tests/ui_render.rs::delta_200_200`: a synthetic delta of 200 removed
      plus 200 added items on Issue190 costs ≤ 2 ms CPU in release
      (median of 20), uploads ≤ 2 × the dirty chunks' buffer size, and
      leaves `full_rebuilds == 0`.
    - FT6 static part: `gpu_buffer_bytes` for Issue190 ≤ 64 MB.
  *Done 2026-10-15:* `scene::Item::id` (the engine `ItemId`),
  `scene::Flattener` (one engine item to its scene items, used by the full load
  too) and `Scene::item(board, id) -> Vec<Item>` (a `Vec`, not `Option`: a pin
  or via has one shape per layer). `ChunkSet::patch(removed, added)` through a
  `BTreeMap<i32, Vec<chunk>>` index (no hash map): rebuilds only the owning
  chunks and the chunks that receive an added item (the chunk of its group
  whose id range holds it), splits chunks over 8,192 items (repeatedly, tested
  by `big_chunks_split_and_stay_consistent`), marks them dirty so the next
  `prepare` re-uploads just those. `Tessellation::patch` is the entry point.
  Counters: `ChunkStats::full_builds`, `FrameStats::full_rebuilds` (a new
  tessellation after the first, including `Loaded::rebuild_scene`).
  `chunks::tests::patch_equals_fresh_build` (Issue690-coldfire, 200 ids from
  `JavaRandom::new(7)`): removing them equals a build without them; adding them
  back, and a combined remove-and-add, give chunks bit-identical to a fresh
  build. `ui_render.rs::delta_200_200` (Issue190): release, median of 20 =
  0.444 ms (bound 2 ms; debug 1.4 ms is not asserted); the upload after a patch
  is 7,551,156 bytes for 7,551,124 dirty bytes (+32 camera), bound 2x; 64 of 96
  chunks are dirty because 200 random items hit most of the big trace chunks;
  `full_builds == 1`, `full_rebuilds == 0`. FT6 static: Issue190
  `gpu_buffer_bytes` 7,788,460 (7.4 MiB) <= 64 MB. Also added a grid to
  `Scene::pick` (the hover test scanned all items every frame, 2.7 ms on the 4x4
  board); `the_grid_picks_what_a_linear_scan_picks` checks it against the linear
  scan on 4,000 random points.

- [~] **G02.5 — Synthetic 16× stress board (meets the CPU part of FT2).**
  - `--bench-synthetic 4x4 <fixture>` tiles the fixture's scene 4×4 with a
    gap of 10 % of the board size, offsetting `ItemId`s per tile, and then
    runs `--bench pan-zoom`. Test-only data; the engine is not involved.
  - **Accept:**
    - `bench_synthetic::tests::tiling_is_deterministic`: two builds of the
      4×4 Issue190 board give the same item count (174,160), triangle count
      and an equal checksum of the vertex buffers.
    - FT2 CPU part: frame CPU p95 ≤ 4 ms on this machine, release, three
      runs, numbers in the done note.
    - FT2 wall-clock part (≥ 60 fps on Dan's Windows GPU) is manual: the
      item stays partial until Dan runs the native build and the numbers are
      in `gui.md`.
  *Done 2026-10-15 (CPU part):* `src/bench_synthetic.rs` and
  `--bench-synthetic 4x4 board.dsn` (implies `--bench pan-zoom`; tiling on the
  load thread; `args::tests::parses_the_synthetic_tiling`).
  `bench_synthetic::tests::tiling_is_deterministic` (Issue190): two builds give
  equal items, equal triangle counts (16 x one tile) and an equal FNV checksum
  of all chunk buffers. The spec's 174,160 is the engine item count (10,885 x
  16, asserted); in scene items it is 12,473 per tile, 199,568 in all (pads
  and vias are one per layer); 5,600,896 triangles. FT2 CPU part, release, 3
  runs (load 15.3, 13.2, 8.8): frame CPU p95 1.796, 1.550, 1.582 ms (target 4);
  1,514 chunks, 386 drawn in the last frame; `gpu_buffer_bytes` 124,442,080.
  *Partial 2026-10-15:* the wall-clock part (60 fps on Dan's Windows GPU) is manual and
  still open: needs Windows run (Dan, by hand, together with G02.8; G07.8 records
  the matching FT5 line for his GPU).

- [x] **G02.6 — Faster load (meets FT3).**
  - Measure the load phases first: read, parse, `build_board`, scene,
    tessellation, chunking, upload. Optimise only the phases that miss the
    target. Per-chunk tessellation may run on rayon (`NEW DEP`) if the
    measurement says so, with results merged in chunk order.
  - The load stays on the background thread; only the upload runs on the
    GUI thread.
  - **Accept:**
    - FT3 on Issue190 in release: file to first frame ≤ 1.5 s, tessellation
      plus chunking ≤ 200 ms. The phase table (before and after) is in the
      done note and in `gui.md`.
    - `tests/ui_render.rs::frames_flow_during_load`: while Issue190 loads,
      the stats' maximum frame CPU stays ≤ 50 ms (release-only test,
      `#[ignore]` in debug).
  *Done 2026-10-15:* phases measured first (`LoadTimings` in
  `Loaded::timings`, printed by `--bench` as `load_ms`, plus `first_frame_ms`
  = start of the load to the end of the first board paint, CPU, presentation
  excluded). Issue190, release, load 12 to 20: read 1.7, parse 15.6,
  `build_board` 169, scene 2.2, tessellation 4.6, chunking 0.8, total 194 ms.
  Before (G01 renderer, instrumented HEAD): read plus parse 19, `build_board`
  422, scene 4.1, tessellation 6.8, total 452 ms. First frame 264, 411, 736 ms in
  three runs (bound 1.5 s); tessellation plus chunking 5.4 ms (bound 200 ms).
  Nothing needed optimising, so no `rayon` (no new dependency). The load stays
  on its thread; only the upload runs on the GUI thread (7.8 MB for Issue190).
  `ui_render.rs::frames_flow_during_load` (`#[ignore]` in debug): 208 frames
  while Issue190 loads, max update CPU 0.31 ms (bound 50 ms), release.

- [x] **G02.7 — CPU fallback.**
  - `FREEROUTING_GUI_RENDERER=cpu` selects today's painter, kept as
    `view::paint_cpu`, with the same chunk culling. The CPU path is also
    used when `frame.wgpu_render_state()` is `None`.
  - **Accept:** `tests/ui_render.rs::cpu_and_gpu_match`: snapshots of
    Issue159 and Issue690 at the fitted zoom, one per path, differ in ≤ 1 %
    of the pixels (`dify` comparison through kittest's snapshot options).
  *Done 2026-10-15:* `view::paint_cpu` (egui meshes, hairlines as quads) draws
  the same plan with the same chunk culling; `Renderer::select` uses it when
  `FREEROUTING_GUI_RENDERER=cpu` or `frame.wgpu_render_state()` is `None`
  (`Renderer::force_cpu` for tests). `ui_render.rs::cpu_and_gpu_match`: the canvas
  of Issue159 and Issue690-coldfire at the fitted zoom, one image per path:
  0.0000 % of the canvas pixels differ on both (10 % fuzz per channel; the
  shaders map points to clip space the way egui does, over the whole screen).
  Deviation: the comparison is a direct pixel count over the canvas, not
  kittest's dify (kittest compares an image with a stored file; the image crate
  is not a direct dev-dependency).

- [~] **G02.8 — Renderer notes and the Windows check.**
  - `gui.md`: a "Renderer" section (pipeline, chunking, patching, fallback,
    counters) and the FT0–FT4 and FT6 numbers for this machine (lavapipe).
  - Dan runs `freerouting-gui.exe --bench pan-zoom --frames 600` on
    Issue190 and the 4×4 board on the native Windows build (DX12 and, with
    `WGPU_BACKEND=gl`, GL), as described in `kicad-plugin.md` for the
    native build.
  - **Accept:** the numbers for both machines are in `gui.md`. Until Dan's
    run, the item is partial with "needs Windows run".
  *Done 2026-10-15 (this machine):* `docs/development/gui.md` has a "Renderer"
  section (pipeline, chunking, patching, fallback, counters, the two frame CPU
  definitions) and the numbers for FT0 (both definitions), FT1 to FT4 and FT6
  on lavapipe, with load averages (the machine was busy, load 9 to 15).
  *Partial 2026-10-15:* the Windows numbers (`freerouting-gui.exe --bench pan-zoom
  --frames 600` on Issue190 and `--bench-synthetic 4x4`, DX12 and
  `WGPU_BACKEND=gl`) are Dan's; needs Windows run (the same session as G07.8's
  line for his GPU).
