//! `--bench pan-zoom`: a scripted camera path whose frame times are
//! measured and printed as one JSON object.
//!
//! The script, one step per frame, repeated until the requested number of
//! frames is counted: fit; 10 zoom-in steps of x1.25 at the board centre;
//! pans of 20 % of the view width right, down, left and up; 10 zoom-out
//! steps. The first [`WARMUP`] frames are not counted.
//!
//! The report has both definitions of frame CPU (see [`crate::stats`]):
//! `p50_ms`, `p95_ms`, `max_ms` are the time inside `App::update` (FT0 as G01
//! recorded it), and `frame_p50_ms`, `frame_p95_ms`, `frame_max_ms` add what
//! eframe does until the board's callback has painted. Frames complete one
//! update later, so the frame figures cover one frame fewer.

use eframe::egui::Rect;

use crate::args::BenchScript;
use crate::scene::P;
use crate::stats::{FrameCpu, LoadTimings, PaintStats, load_average_1m, percentile};
use crate::view::Camera;

/// Frames run before counting starts.
pub const WARMUP: usize = 30;
const ZOOM_STEP: f64 = 1.25;
const ZOOM_STEPS: usize = 10;
const PAN_STEPS: usize = 4;
const PAN_FRACTION: f64 = 0.2;
/// Frames in one pass of the script: the fit, the zooms in, the pans, the
/// zooms out.
pub const CYCLE: usize = 1 + ZOOM_STEPS + PAN_STEPS + ZOOM_STEPS;

/// The camera of step `step` of the `pan-zoom` script, from the fitted
/// camera `fit` on a canvas of `canvas`.
pub fn pan_zoom_camera(step: usize, fit: Camera, canvas: Rect) -> Camera {
    let k = step % CYCLE;
    let zoomed = |n: usize| Camera {
        center: fit.center,
        zoom: fit.zoom * ZOOM_STEP.powi(n as i32),
        orientation: fit.orientation,
    };
    match k {
        0 => fit,
        1..=ZOOM_STEPS => zoomed(k),
        _ if k <= ZOOM_STEPS + PAN_STEPS => {
            let mut cam = zoomed(ZOOM_STEPS);
            let d = PAN_FRACTION * f64::from(canvas.width()) / cam.zoom;
            let moves = [(d, 0.0), (0.0, -d), (-d, 0.0), (0.0, d)];
            for &(dx, dy) in &moves[..k - ZOOM_STEPS] {
                cam.center = P::new(cam.center.x + dx, cam.center.y + dy);
            }
            cam
        }
        _ => zoomed(ZOOM_STEPS - (k - ZOOM_STEPS - PAN_STEPS)),
    }
}

/// The pointer path of the `drag` script: the offset in millimetres from the
/// press point at counted frame `step`, out to 0.3 mm along x and back
/// (`0.15 (1 - cos)`), a period of 120 frames, so the component moves at most
/// 0.016 mm per frame. 0.3 mm is the distance the bench's press point was
/// checked to accept.
pub fn drag_offset(step: usize) -> P {
    let phase = std::f64::consts::TAU * step as f64 / 120.0;
    P::new(0.15 * (1.0 - phase.cos()), 0.0)
}

/// A running `--bench`.
pub struct Bench {
    script: BenchScript,
    frames: usize,
    step: usize,
    samples: Vec<f64>,
    fit: Option<Camera>,
    last: PaintStats,
    triangles: usize,
    /// Frame CPU (second definition) of the counted frames.
    full: Vec<f64>,
    seen_full: usize,
    load: LoadTimings,
    first_frame_ms: Option<f64>,
    counted_from: Option<std::time::Instant>,
    /// Full rebuilds and patches at the first counted frame and now.
    baseline: Option<(u64, u64)>,
    latest: (u64, u64),
    /// GPU buffer bytes at the first counted frame and the largest since.
    gpu_first: Option<u64>,
    gpu_max: u64,
    route: RouteBench,
}

/// What the `route` script knows of the job.
#[derive(Default)]
struct RouteBench {
    started: Option<std::time::Instant>,
    cancelled: Option<std::time::Instant>,
    finished: Option<std::time::Instant>,
    report: Option<RouteReport>,
}

/// The job's own counters, for the `route` report.
#[derive(Clone, Debug, Default)]
pub struct RouteReport {
    pub passes: i32,
    pub status: String,
    pub progress_events: u64,
    pub delta_events: u64,
    pub patches: u64,
    pub replaced: u64,
    pub items_added: u64,
    pub reconcile_fixes: u64,
    pub max_apply_ms: f64,
}

impl Bench {
    pub fn new(script: BenchScript, frames: usize) -> Bench {
        Bench {
            script,
            frames,
            step: 0,
            samples: Vec::with_capacity(frames),
            fit: None,
            last: PaintStats::default(),
            triangles: 0,
            full: Vec::with_capacity(frames),
            seen_full: 0,
            load: LoadTimings::default(),
            first_frame_ms: None,
            counted_from: None,
            baseline: None,
            latest: (0, 0),
            gpu_first: None,
            gpu_max: 0,
            route: RouteBench::default(),
        }
    }

    /// The load phases and the time to the first frame, for the report.
    pub fn set_load(&mut self, load: LoadTimings, first_frame_ms: Option<f64>) {
        self.load = load;
        self.first_frame_ms = first_frame_ms;
    }

    /// Frames whose callbacks finished (frame CPU). The first [`WARMUP`] are
    /// not counted.
    pub fn full_done(&mut self, done: &[FrameCpu]) {
        for frame in done {
            if self.seen_full >= WARMUP && self.full.len() < self.frames {
                self.full.push(frame.frame_ms);
            }
            self.seen_full += 1;
        }
    }

    /// The camera for the coming frame. `fit` is the camera that fits the
    /// board in `canvas`.
    pub fn camera(&mut self, fit: Camera, canvas: Rect) -> Camera {
        let base = *self.fit.get_or_insert(fit);
        match self.script {
            BenchScript::PanZoom => pan_zoom_camera(self.step, base, canvas),
            // The drag and route benches look at the board as fitted.
            BenchScript::Drag | BenchScript::Route => base,
        }
    }

    /// The index of the coming frame when the script is `route`.
    pub fn route_step(&self) -> Option<usize> {
        (self.script == BenchScript::Route).then_some(self.step)
    }

    /// The route bench has started the job.
    pub fn route_started(&mut self) {
        self.route.started = Some(std::time::Instant::now());
    }

    /// The route bench pressed Cancel.
    pub fn route_cancelled(&mut self) {
        self.route.cancelled = Some(std::time::Instant::now());
    }

    /// When the job was started and cancelled, if it was.
    pub fn route_times(&self) -> (Option<std::time::Instant>, Option<std::time::Instant>) {
        (self.route.started, self.route.cancelled)
    }

    /// The job has ended: the next [`frame_done`](Self::frame_done) reports.
    pub fn route_finished(&mut self, run: RouteReport) {
        self.route.finished = Some(std::time::Instant::now());
        self.route.report = Some(run);
    }

    /// The index of the coming frame when the script is `drag`.
    pub fn drag_step(&self) -> Option<usize> {
        (self.script == BenchScript::Drag).then_some(self.step)
    }

    /// Frames the script runs in all, warm-up included.
    pub fn total_frames(&self) -> usize {
        if self.script == BenchScript::Route {
            return usize::MAX;
        }
        WARMUP + self.frames
    }

    /// The renderer's counters at the end of a frame: the first counted frame
    /// fixes the baseline, so the report says how many full rebuilds and
    /// patches the counted frames made.
    pub fn counters(&mut self, full_rebuilds: u64, patches: u64) {
        if self.step >= WARMUP {
            self.baseline.get_or_insert((full_rebuilds, patches));
        }
        self.latest = (full_rebuilds, patches);
    }

    /// Records a finished frame. Returns the JSON report after the last
    /// counted frame.
    pub fn frame_done(&mut self, ms: f64, paint: PaintStats, triangles: usize) -> Option<String> {
        if self.step >= WARMUP {
            self.counted_from
                .get_or_insert_with(std::time::Instant::now);
            self.samples.push(ms);
            self.gpu_first.get_or_insert(paint.gpu_buffer_bytes);
            self.gpu_max = self.gpu_max.max(paint.gpu_buffer_bytes);
        }
        self.step += 1;
        self.last = paint;
        self.triangles = triangles;
        if self.script == BenchScript::Route {
            return self.route.finished.is_some().then(|| self.report());
        }
        (self.samples.len() >= self.frames).then(|| self.report())
    }

    /// One JSON object. `p50_ms`, `p95_ms`, `max_ms` are update CPU;
    /// `frame_*` are frame CPU (`frame_frames` samples). `triangles` is the
    /// tessellation's count; `draws`, `uploaded_bytes`, `chunks_drawn`,
    /// `chunks_total` and `gpu_buffer_bytes` are those of the last frame.
    /// `wall_ms` is the mean time per counted frame, presentation included.
    /// `first_frame_ms` is the time from the start of the load to the end of
    /// the first board paint; `load_ms` has the load phases.
    pub fn report(&self) -> String {
        let max = |v: &[f64]| v.iter().copied().fold(0.0, f64::max);
        let wall = self.counted_from.map_or(0.0, |t| {
            t.elapsed().as_secs_f64() * 1000.0 / self.samples.len().max(1) as f64
        });
        let base = self.baseline.unwrap_or(self.latest);
        let first = self
            .first_frame_ms
            .map_or_else(|| "null".to_string(), |ms| format!("{ms:.1}"));
        let mut json = format!(
            "{{\"script\":\"{}\",\"frames\":{},\"p50_ms\":{:.3},\"p95_ms\":{:.3},\"max_ms\":{:.3},\"frame_frames\":{},\"frame_p50_ms\":{:.3},\"frame_p95_ms\":{:.3},\"frame_max_ms\":{:.3},\"wall_ms\":{:.3},\"triangles\":{},\"draws\":{},\"uploaded_bytes\":{},\"chunks_drawn\":{},\"chunks_total\":{},\"gpu_buffer_bytes\":{},\"first_frame_ms\":{},\"load_ms\":{},\"full_rebuilds\":{},\"patches\":{},\"load_avg_1m\":{:.2}}}",
            self.script.name(),
            self.samples.len(),
            percentile(self.samples.clone(), 50.0),
            percentile(self.samples.clone(), 95.0),
            max(&self.samples),
            self.full.len(),
            percentile(self.full.clone(), 50.0),
            percentile(self.full.clone(), 95.0),
            max(&self.full),
            wall,
            self.triangles,
            self.last.draws,
            self.last.uploaded_bytes,
            self.last.chunks_drawn,
            self.last.chunks_total,
            self.last.gpu_buffer_bytes,
            first,
            self.load.json(),
            self.latest.0 - base.0,
            self.latest.1 - base.1,
            load_average_1m()
        );
        if let (Some(run), Some(started), Some(finished)) =
            (&self.route.report, self.route.started, self.route.finished)
        {
            // The cancel latency is the time from the press to `Finished`.
            let cancel = self.route.cancelled.map_or_else(
                || "null".to_string(),
                |c| {
                    format!(
                        "{:.1}",
                        finished.saturating_duration_since(c).as_secs_f64() * 1000.0
                    )
                },
            );
            json.pop();
            json.push_str(&format!(
                ",\"route_ms\":{:.1},\"cancel_to_finished_ms\":{cancel},\"job_status\":\"{}\",\"passes\":{},\"progress_events\":{},\"delta_events\":{},\"replaced\":{},\"items_added\":{},\"reconcile_fixes\":{},\"max_apply_ms\":{:.3},\"gpu_buffer_first\":{},\"gpu_buffer_max\":{}}}",
                finished.saturating_duration_since(started).as_secs_f64() * 1000.0,
                run.status,
                run.passes,
                run.progress_events,
                run.delta_events,
                run.replaced,
                run.items_added,
                run.reconcile_fixes,
                run.max_apply_ms,
                self.gpu_first.unwrap_or(0),
                self.gpu_max,
            ));
        }
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, vec2};

    fn canvas() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0))
    }

    fn fit() -> Camera {
        Camera::new(P::new(50.0, 20.0), 8.0)
    }

    #[test]
    fn script_zooms_in_pans_and_zooms_out() {
        let at = |k| pan_zoom_camera(k, fit(), canvas());
        assert_eq!(at(0).zoom, 8.0);
        assert!((at(10).zoom - 8.0 * 1.25f64.powi(10)).abs() < 1e-9);
        assert_eq!(at(10).center, fit().center);
        // 20 % of the view width: 200 px at that zoom, to the right.
        let d = 200.0 / at(10).zoom;
        assert!((at(11).center.x - (50.0 + d)).abs() < 1e-9);
        assert!((at(12).center.y - (20.0 - d)).abs() < 1e-9);
        // The four pans cancel out.
        assert!((at(14).center.x - 50.0).abs() < 1e-9 && (at(14).center.y - 20.0).abs() < 1e-9);
        // Zoom-out steps walk back to the fit zoom, and the cycle repeats.
        assert!((at(15).zoom - 8.0 * 1.25f64.powi(9)).abs() < 1e-9);
        assert!((at(23).zoom - 8.0 * 1.25).abs() < 1e-9);
        assert_eq!(at(24).zoom, 8.0);
        assert_eq!(at(CYCLE).zoom, 8.0);
    }

    #[test]
    fn drag_path_is_a_slow_sine() {
        assert_eq!(drag_offset(0), P::new(0.0, 0.0));
        assert!((drag_offset(60).x - 0.3).abs() < 1e-12);
        let fastest = (0..120)
            .map(|k| (drag_offset(k + 1).x - drag_offset(k).x).abs())
            .fold(0.0, f64::max);
        assert!(fastest < 0.02, "{fastest} mm per frame");
        assert!((0..240).all(|k| (0.0..=0.3 + 1e-12).contains(&drag_offset(k).x)));
    }

    #[test]
    fn route_script_reports_when_the_job_ends() {
        let mut bench = Bench::new(BenchScript::Route, 1);
        let paint = PaintStats {
            gpu_buffer_bytes: 10,
            ..PaintStats::default()
        };
        for _ in 0..WARMUP + 5 {
            assert!(
                bench.frame_done(1.0, paint, 5).is_none(),
                "runs until the job ends"
            );
        }
        assert_eq!(bench.total_frames(), usize::MAX);
        bench.route_started();
        bench.route_finished(RouteReport {
            passes: 3,
            status: "Completed".into(),
            ..RouteReport::default()
        });
        let json = bench.frame_done(2.0, paint, 5).expect("done");
        assert!(json.starts_with("{\"script\":\"route\","), "{json}");
        assert!(json.contains("\"cancel_to_finished_ms\":null,"), "{json}");
        assert!(
            json.contains("\"job_status\":\"Completed\",\"passes\":3,"),
            "{json}"
        );
        assert!(
            json.ends_with("\"gpu_buffer_first\":10,\"gpu_buffer_max\":10}"),
            "{json}"
        );
    }

    #[test]
    fn reports_after_the_counted_frames() {
        let mut bench = Bench::new(BenchScript::PanZoom, 3);
        let paint = PaintStats {
            draws: 7,
            uploaded_bytes: 99,
            ..PaintStats::default()
        };
        for i in 0..WARMUP + 2 {
            assert!(bench.frame_done(1.0 + i as f64, paint, 5).is_none());
        }
        let json = bench.frame_done(100.0, paint, 5).expect("done");
        assert!(
            json.starts_with("{\"script\":\"pan-zoom\",\"frames\":3,"),
            "{json}"
        );
        assert!(json.contains("\"max_ms\":100.000"), "{json}");
        assert!(
            json.contains("\"triangles\":5,\"draws\":7,\"uploaded_bytes\":99,"),
            "{json}"
        );
        assert!(json.contains("\"frame_frames\":0,"), "{json}");
    }
}
