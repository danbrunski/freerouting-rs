//! `--bench pan-zoom`: a scripted camera path whose frame times are
//! measured and printed as one JSON object.
//!
//! The script, one step per frame, repeated until the requested number of
//! frames is counted: fit; 10 zoom-in steps of x1.25 at the board centre;
//! pans of 20 % of the view width right, down, left and up; 10 zoom-out
//! steps. The first [`WARMUP`] frames are not counted.

use eframe::egui::Rect;

use crate::args::BenchScript;
use crate::scene::P;
use crate::stats::{PaintStats, load_average_1m, percentile};
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
            self.samples.push(ms);
        }
        self.step += 1;
        self.last = paint;
        self.triangles = triangles;
        (self.samples.len() >= self.frames).then(|| self.report())
    }

    /// `{"script", "frames", "p50_ms", "p95_ms", "max_ms", "triangles",
    /// "draws", "uploaded_bytes", "load_avg_1m"}`. `triangles` is the
    /// tessellation's count; `draws` and `uploaded_bytes` are those of the
    /// last frame.
    pub fn report(&self) -> String {
        let max = self.samples.iter().copied().fold(0.0, f64::max);
        format!(
            "{{\"script\":\"{}\",\"frames\":{},\"p50_ms\":{:.3},\"p95_ms\":{:.3},\"max_ms\":{:.3},\"triangles\":{},\"draws\":{},\"uploaded_bytes\":{},\"load_avg_1m\":{:.2}}}",
            self.script.name(),
            self.samples.len(),
            percentile(self.samples.clone(), 50.0),
            percentile(self.samples.clone(), 95.0),
            max,
            self.triangles,
            self.last.draws,
            self.last.uploaded_bytes,
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
        Camera {
            center: P::new(50.0, 20.0),
            zoom: 8.0,
        }
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
    }
}
