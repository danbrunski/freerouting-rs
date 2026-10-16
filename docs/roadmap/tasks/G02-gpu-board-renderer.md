# G02 — GPU board renderer

Opened 2026-10-15. Every frame, `view::paint` clones every batch's index
buffer, transforms every vertex on the CPU, and hands egui fresh
`Shape::mesh` and `Shape::line` shapes, which epaint re-tessellates and
uploads again. There is no culling. `Issue190-processor.Z80` is 350,056
triangles, and real boards are ten times larger. Live routing redraw (G07)
and dragging (G09) need geometry that stays on the GPU and changes in small
pieces. This checklist moves the board into GPU buffers uploaded once,
culls by viewport, patches by `ItemId`, and proves it with the frame
targets FT1–FT4 and FT6 in [G00](G00-desktop-app-README.md). It is the
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

- [ ] **G02.1 — Paint callback with GPU-resident buffers.**
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

- [ ] **G02.2 — Hairlines in the shader.**
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

- [ ] **G02.3 — Chunked and culled batches (meets FT1).**
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

- [ ] **G02.4 — Patching by `ItemId` (meets the CPU part of FT4).**
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

- [ ] **G02.5 — Synthetic 16× stress board (meets the CPU part of FT2).**
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

- [ ] **G02.6 — Faster load (meets FT3).**
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

- [ ] **G02.7 — CPU fallback.**
  - `FREEROUTING_GUI_RENDERER=cpu` selects today's painter, kept as
    `view::paint_cpu`, with the same chunk culling. The CPU path is also
    used when `frame.wgpu_render_state()` is `None`.
  - **Accept:** `tests/ui_render.rs::cpu_and_gpu_match`: snapshots of
    Issue159 and Issue690 at the fitted zoom, one per path, differ in ≤ 1 %
    of the pixels (`dify` comparison through kittest's snapshot options).

- [ ] **G02.8 — Renderer notes and the Windows check.**
  - `gui.md`: a "Renderer" section (pipeline, chunking, patching, fallback,
    counters) and the FT0–FT4 and FT6 numbers for this machine (lavapipe).
  - Dan runs `freerouting-gui.exe --bench pan-zoom --frames 600` on
    Issue190 and the 4×4 board on the native Windows build (DX12 and, with
    `WGPU_BACKEND=gl`, GL), as described in `kicad-plugin.md` for the
    native build.
  - **Accept:** the numbers for both machines are in `gui.md`. Until Dan's
    run, the item is partial with "needs Windows run".
