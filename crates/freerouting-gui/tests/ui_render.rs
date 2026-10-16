//! The GPU board canvas (G02): uploads, hairlines, culling, patching, load
//! frames and the CPU fallback.
//!
//! Tests that read pixels run on Linux with lavapipe, like the snapshots;
//! the counters need no GPU.

mod common;

use common::{SIZE, app_harness, fixtures, rendering_enabled};
use eframe::egui::{self, Pos2, Rect};
use egui_kittest::Harness;
use freerouting::board::items::ItemId;
use freerouting::geometry::jmath::JavaRandom;
use freerouting_gui::App;
use freerouting_gui::commands::Command;
use freerouting_gui::look::{Look, OtherColor};
use freerouting_gui::scene::{Bounds, Geom, Item, Kind, Layer, P, Scene, Slot};
use freerouting_gui::session::{self, Loaded};
use freerouting_gui::view::{self, Camera, Labels, Mirror, Renderer, Tessellation, Visibility};

const ISSUE159: &str = "Issue159-setonix_2hp-pcb.dsn";
const ISSUE690: &str = "Issue690-kit-dev-coldfire-xilinx_5213.dsn";
const ISSUE190: &str = "Issue190-processor.Z80.dsn";

/// The egui background the canvas clears to.
const BACKGROUND: [u8; 3] = [12, 14, 18];

fn first_frame_uploaded(harness: &mut Harness<'static, App>) {
    harness.render().expect("render");
    harness.step();
}

#[test]
fn no_upload_after_first_frame() {
    if !rendering_enabled() {
        return;
    }
    let mut harness = app_harness(&[ISSUE159]);
    first_frame_uploaded(&mut harness);
    let board_bytes = harness.state().stats().uploaded_bytes;
    assert!(
        board_bytes > 10_000,
        "the board goes up once: {board_bytes}"
    );
    assert!(harness.state().stats().gpu_buffer_bytes >= board_bytes);

    for i in 0..10 {
        let view = harness.state_mut().view_mut();
        let cam = view.camera.as_mut().expect("camera");
        cam.center.x += 0.3 + f64::from(i) * 0.01;
        cam.zoom *= 1.01;
        view.auto_fit = None;
        harness.step();
        harness.render().expect("render");
        harness.step();
        let up = harness.state().stats().uploaded_bytes;
        // The camera uniform only: 12 floats (G04.6 added the orientation's
        // four to the first eight).
        assert_eq!(up, 48, "frame {i}");
        assert!(up <= 4096);
    }
    assert_eq!(harness.state().stats().full_rebuilds, 0);
}

/// A scene of one board outline segment, drawn in a window by itself.
struct LineApp {
    tess: Tessellation,
    camera: Camera,
    renderer: Renderer,
}

impl LineApp {
    fn new() -> LineApp {
        let scene = Scene {
            layers: vec![Layer {
                name: "F.Cu".into(),
                signal: true,
            }],
            items: vec![Item {
                id: ItemId(1),
                kind: Kind::Outline,
                slot: Slot::Board,
                geom: Geom::Path {
                    points: vec![P::new(0.0, 0.0), P::new(10.0, 0.0)],
                    width: 0.0,
                },
                net: None,
                owner: None,
            }],
            bounds: Bounds {
                min: P::new(0.0, 0.0),
                max: P::new(10.0, 0.0),
            },
            ..Scene::default()
        };
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(SIZE[0], SIZE[1]));
        let camera = Camera::fit(&scene, rect);
        LineApp {
            tess: Tessellation::new(&scene),
            camera,
            renderer: Renderer::default(),
        }
    }
}

impl eframe::App for LineApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.renderer
            .select(frame.wgpu_render_state().map(|s| s.target_format));
        let painter = ctx.layer_painter(egui::LayerId::background());
        let rect = ctx.screen_rect();
        let vis = Visibility {
            layers: &[true],
            kinds: &[true; 9],
            active: 0,
            labels: false,
            look: &Look::new(1),
        };
        view::paint(
            &painter,
            rect,
            &self.camera,
            &self.tess,
            1,
            &Labels::default(),
            &vis,
            &self.renderer,
        );
    }
}

fn lines_harness(ppp: f32) -> Harness<'static, LineApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(ppp)
        .wgpu()
        .build_eframe(|_| LineApp::new())
}

#[test]
fn hairline_is_one_pixel() {
    if !rendering_enabled() {
        return;
    }
    for ppp in [1.0f32, 2.0] {
        let mut harness = lines_harness(ppp);
        let fit = harness.state().camera;
        // At the fitted zoom and at 20x, with the line at different
        // fractions of a pixel.
        for (zoom, dy) in [
            (fit.zoom, 0.0),
            (fit.zoom, 0.0123),
            (fit.zoom * 20.0, 0.0),
            (fit.zoom * 20.0, 0.00037),
            (fit.zoom * 20.0, 0.0012),
        ] {
            harness.state_mut().camera = Camera::new(P::new(5.0, dy), zoom);
            harness.step();
            let image = harness.render().expect("render");
            let x = (SIZE[0] / 2.0 * ppp) as u32;
            let rows = (0..image.height())
                .filter(|&y| image.get_pixel(x, y).0[..3] != BACKGROUND)
                .count();
            assert_eq!(
                rows,
                ppp.round() as usize,
                "ppp {ppp}, zoom {zoom}, offset {dy}"
            );
        }
    }
}

/// Counters of a board at 20x zoom centred on it.
fn at_20x(harness: &mut Harness<'static, App>) -> (u64, u64) {
    let centre = harness
        .state()
        .loaded()
        .expect("board")
        .scene
        .bounds
        .center();
    let view = harness.state_mut().view_mut();
    let fit = view.camera.expect("camera");
    view.camera = Some(Camera::new(centre, fit.zoom * 20.0));
    view.auto_fit = None;
    harness.step();
    let stats = harness.state().stats();
    (stats.chunks_drawn, stats.chunks_total)
}

#[test]
fn culls_at_20x() {
    let mut harness = app_harness(&[ISSUE190]);
    let fitted = harness.state().stats().chunks_drawn;
    let (drawn, total) = at_20x(&mut harness);
    eprintln!("Issue190: {fitted} chunks at the fitted zoom, {drawn} of {total} at 20x");
    assert!(total > 50, "{total} chunks");
    assert_eq!(fitted, total, "nothing is culled when the board is in view");
    assert!(
        (drawn as f64) < 0.2 * total as f64,
        "{drawn} of {total} chunks drawn"
    );
}

/// 200 distinct engine items with geometry, chosen with Java's `Random(7)`,
/// and their scene items.
fn delta(loaded: &Loaded) -> (Vec<ItemId>, Vec<Item>) {
    let board = &loaded.session.board;
    let flat = freerouting_gui::scene::Flattener::new(board);
    let mut ids: Vec<ItemId> = board
        .basic()
        .item_ids()
        .into_iter()
        .filter(|&id| !flat.item(id).is_empty())
        .collect();
    ids.sort_by_key(|id| id.0);
    let mut rng = JavaRandom::new(7);
    let mut picked = std::collections::BTreeSet::new();
    while picked.len() < 200 {
        picked.insert(ids[rng.next_int(ids.len() as i32) as usize].0);
    }
    let removed: Vec<ItemId> = picked.iter().map(|&i| ItemId(i)).collect();
    let added = removed.iter().flat_map(|&id| flat.item(id)).collect();
    (removed, added)
}

#[test]
fn delta_200_200() {
    if !rendering_enabled() {
        return;
    }
    let mut harness = app_harness(&[ISSUE190]);
    first_frame_uploaded(&mut harness);
    let gpu = harness.state().stats().gpu_buffer_bytes;
    eprintln!("Issue190: {} MiB of GPU buffers", gpu >> 20);
    // FT6, the static part.
    assert!(gpu > 0 && gpu <= 64 << 20, "{gpu} bytes of GPU buffers");

    let loaded = harness.state().loaded().expect("board");
    let (removed, added) = delta(loaded);
    assert_eq!((removed.len(), added.len() > 200), (200, true));
    let mut times = Vec::new();
    for _ in 0..20 {
        let started = std::time::Instant::now();
        let report = loaded.tess.patch(&removed, &added);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(report.dirty_bytes > 0);
    }
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    eprintln!("delta 200+200: median {median:.3} ms, all {times:.3?}");
    if cfg!(debug_assertions) {
        eprintln!("(debug build: the 2 ms bound is checked in release only)");
    } else {
        assert!(median <= 2.0, "median {median} ms");
    }

    // The upload of one more delta: only the dirty chunks, once.
    first_frame_uploaded(&mut harness);
    let loaded = harness.state().loaded().expect("board");
    let report = loaded.tess.patch(&removed, &added);
    harness.step();
    harness.render().expect("render");
    harness.step();
    let up = harness.state().stats().uploaded_bytes;
    eprintln!(
        "{} dirty chunks, {} bytes dirty, {up} bytes uploaded",
        report.dirty_chunks.len(),
        report.dirty_bytes
    );
    assert!(up <= 2 * report.dirty_bytes + 32, "{up} bytes");
    assert!(up >= report.dirty_bytes);
    let loaded = harness.state().loaded().expect("board");
    assert_eq!(loaded.tess.with(|s| s.stats.full_builds), 1);
    assert_eq!(harness.state().stats().full_rebuilds, 0);
}

/// Frames of an app that loads Issue190 flow while the worker reads it.
#[test]
#[cfg_attr(debug_assertions, ignore = "frame times are a release measurement")]
fn frames_flow_during_load() {
    let open = vec![fixtures().join(ISSUE190)];
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .wgpu()
        .build_eframe(|cc| App::new(cc, open));
    let mut frames = 0;
    while harness.state().is_loading() || frames == 0 {
        harness.step();
        frames += 1;
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(frames < 100_000, "the board did not load");
    }
    harness.run();
    let max = harness.state().stats().max();
    eprintln!("{frames} frames while loading, max update CPU {max:.2} ms");
    assert!(frames > 20, "{frames} frames");
    assert!(max <= 50.0, "max frame CPU {max} ms");
}

fn render_with(path_cpu: bool, board: &str) -> (Vec<u8>, (u32, u32), Rect) {
    render_oriented(path_cpu, board, 0, Mirror::None)
}

/// `render_with` with the board turned by `turns` quarter turns and mirrored.
fn render_oriented(
    path_cpu: bool,
    board: &str,
    turns: u8,
    mirror: Mirror,
) -> (Vec<u8>, (u32, u32), Rect) {
    let mut harness = app_harness(&[board]);
    harness.state_mut().view_mut().renderer.force_cpu(path_cpu);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .execute(&ctx, Command::SetMirror(mirror));
    harness
        .state_mut()
        .execute(&ctx, Command::SetRotation(turns));
    harness.step();
    harness.step();
    let image = harness.render().expect("render");
    if let Ok(dir) = std::env::var("G02_DUMP") {
        image
            .save(format!(
                "{dir}/{board}-{}.png",
                if path_cpu { "cpu" } else { "gpu" }
            ))
            .unwrap();
    }
    let rect = harness.state().view().canvas;
    (image.as_raw().clone(), image.dimensions(), rect)
}

/// Share of the pixels in `rect` (points, at pixel ratio 1) that differ by
/// more than 10 % in some channel.
fn fraction_differing(a: &[u8], b: &[u8], width: u32, rect: Rect) -> f64 {
    differing_pixels(a, b, width, rect) as f64 / (rect.width() * rect.height()) as f64
}

fn differing_pixels(a: &[u8], b: &[u8], width: u32, rect: Rect) -> usize {
    let mut count = 0usize;
    for y in rect.min.y as u32..rect.max.y as u32 {
        for x in rect.min.x as u32..rect.max.x as u32 {
            let i = (y * width + x) as usize * 4;
            if (0..3).any(|c| a[i + c].abs_diff(b[i + c]) > 26) {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn cpu_and_gpu_match() {
    if !rendering_enabled() {
        return;
    }
    for board in [ISSUE159, ISSUE690] {
        let (cpu, size, rect) = render_with(true, board);
        let (gpu, size_gpu, _) = render_with(false, board);
        assert_eq!(size, size_gpu);
        let share = fraction_differing(&cpu, &gpu, size.0, rect);
        eprintln!("{board}: {:.4} % of the canvas differs", share * 100.0);
        assert!(share <= 0.01, "{board}: {:.3} % differ", share * 100.0);
    }
}

/// The shaders' camera has the orientation's basis: the GPU draws a rotated and
/// mirrored board on the pixels the CPU painter does (G04.6).
#[test]
fn cpu_and_gpu_match_when_rotated() {
    if !rendering_enabled() {
        return;
    }
    for (turns, mirror) in [
        (1, Mirror::None),
        (2, Mirror::None),
        (3, Mirror::None),
        (0, Mirror::LeftRight),
        (1, Mirror::TopBottom),
    ] {
        let (cpu, size, rect) = render_oriented(true, ISSUE159, turns, mirror);
        let (gpu, size_gpu, _) = render_oriented(false, ISSUE159, turns, mirror);
        assert_eq!(size, size_gpu);
        let share = fraction_differing(&cpu, &gpu, size.0, rect);
        eprintln!(
            "{turns} {mirror:?}: {:.4} % of the canvas differs",
            share * 100.0
        );
        assert!(
            share <= 0.01,
            "{turns} {mirror:?}: {:.3} % differ",
            share * 100.0
        );
        // The board is really turned: a rotated render differs from the
        // upright one by much more than the painters differ from each other.
        let (upright, _, _) = render_oriented(false, ISSUE159, 0, Mirror::None);
        // (A mirror alone moves little on this narrow, near-symmetric board.)
        let least = if turns == 0 { 0.001 } else { 0.02 };
        assert!(
            fraction_differing(&gpu, &upright, size.0, rect) > least,
            "{turns} {mirror:?} looks like no turn"
        );
    }
}

/// The board drawn into the canvas rectangle of the G01 viewer (the window
/// of `viewer_issue159` has the board in (203, 22)-(1280, 778)), with
/// nothing else on screen.
struct CanvasApp {
    loaded: Loaded,
    renderer: Renderer,
}

const G01_CANVAS: Rect = Rect {
    min: Pos2::new(203.0, 22.0),
    max: Pos2::new(1280.0, 778.0),
};

impl eframe::App for CanvasApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.renderer
            .select(frame.wgpu_render_state().map(|s| s.target_format));
        let painter = ctx.layer_painter(egui::LayerId::background());
        let camera = Camera::fit(&self.loaded.scene, G01_CANVAS);
        let layers = self.loaded.scene.layers.len();
        // The golden is the first canvas, which drew every component outline
        // in the silkscreen colours and no part numbers; G04.5 gives the
        // courtyard and fabrication outlines their own colours.
        let mut look = Look::new(layers);
        for (silk, others) in [
            (
                OtherColor::SilkscreenFront,
                [OtherColor::CourtyardFront, OtherColor::FabFront],
            ),
            (
                OtherColor::SilkscreenBack,
                [OtherColor::CourtyardBack, OtherColor::FabBack],
            ),
        ] {
            for other in others {
                look.set_other_color(other, look.other_color(silk));
            }
        }
        let vis = Visibility {
            layers: &vec![true; layers],
            kinds: &[true; 9],
            active: 0,
            labels: true,
            look: &look,
        };
        view::paint(
            &painter,
            G01_CANVAS,
            &camera,
            &self.loaded.tess,
            layers,
            &Labels::designators(&self.loaded.scene.labels),
            &vis,
            &self.renderer,
        );
    }
}

/// The GPU painter draws the board of the G01 snapshot (kept here as
/// `g01_canvas_issue159.png`, a copy of the first `viewer_issue159.png`), in the
/// canvas area, with at most 1 % of the window's pixels different (fuzz
/// 10 %, as `compare -fuzz 10%` in `tools/gui/smoke.sh`). The rest of that
/// snapshot (menus, panels) is not compared: later checklists change them.
/// What differs is the hairlines: egui's feathered strokes of the first
/// canvas against the shader's crisp one-point lines.
#[test]
fn gpu_canvas_matches_the_g01_golden() {
    if !rendering_enabled() {
        return;
    }
    let loaded = session::load(&fixtures().join(ISSUE159), None).expect("load");
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .wgpu()
        .build_eframe(|_| CanvasApp {
            loaded,
            renderer: Renderer::default(),
        });
    harness.step();
    let image = harness.render().expect("render");
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/g01_canvas_issue159.png");
    let mut decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(&golden_path).expect("golden"),
    ));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("png");
    let mut golden = vec![0u8; reader.output_buffer_size().expect("size")];
    let info = reader.next_frame(&mut golden).expect("frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!((info.width, info.height), image.dimensions());
    if let Ok(dir) = std::env::var("G02_DUMP") {
        image.save(format!("{dir}/canvas-app.png")).unwrap();
    }
    let pixels = differing_pixels(&golden, image.as_raw(), info.width, G01_CANVAS);
    let share = pixels as f64 / f64::from(info.width * info.height);
    eprintln!(
        "{pixels} pixels ({:.3} % of the window) differ from the G01 golden",
        share * 100.0
    );
    assert!(share <= 0.01, "{:.3} % differ", share * 100.0);
}
