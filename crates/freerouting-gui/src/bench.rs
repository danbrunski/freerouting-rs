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
        }
    }

    /// Records a finished frame. Returns the JSON report after the last
    /// counted frame.
    pub fn frame_done(&mut self, ms: f64, paint: PaintStats, triangles: usize) -> Option<String> {
        if self.step >= WARMUP {
            self.counted_from
                .get_or_insert_with(std::time::Instant::now);
            self.samples.push(ms);
        }
        self.step += 1;
        self.last = paint;
        self.triangles = triangles;
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
        let first = self
            .first_frame_ms
            .map_or_else(|| "null".to_string(), |ms| format!("{ms:.1}"));
        format!(
            "{{\"script\":\"{}\",\"frames\":{},\"p50_ms\":{:.3},\"p95_ms\":{:.3},\"max_ms\":{:.3},\"frame_frames\":{},\"frame_p50_ms\":{:.3},\"frame_p95_ms\":{:.3},\"frame_max_ms\":{:.3},\"wall_ms\":{:.3},\"triangles\":{},\"draws\":{},\"uploaded_bytes\":{},\"chunks_drawn\":{},\"chunks_total\":{},\"gpu_buffer_bytes\":{},\"first_frame_ms\":{},\"load_ms\":{},\"load_avg_1m\":{:.2}}}",
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
            load_average_1m()
        )
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
