//! Frame statistics: the CPU time of the last frames and the counters that
//! tests and `--bench` read.
//!
//! "Frame CPU" is the time on the GUI thread inside `App::update`, which
//! includes building and painting the board's shapes. It excludes egui's
//! tessellation, queue submission and presentation, which under Xvfb is
//! llvmpipe rasterising and not a usable metric. Later checklists add their
//! counters here.

use std::collections::VecDeque;

/// How many frame times the ring keeps.
pub const RING: usize = 600;

/// What one frame drew, as reported by the painter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintStats {
    /// Triangles in the meshes painted.
    pub triangles: u64,
    /// Meshes and line shapes added to the painter.
    pub draws: u64,
    /// Bytes handed to the renderer for the meshes this frame (egui
    /// vertices are 20 bytes, indices 4).
    pub uploaded_bytes: u64,
    /// Batches that were drawn rather than skipped.
    pub chunks_drawn: u64,
}

/// A ring of the last [`RING`] frame CPU times plus counters.
#[derive(Clone, Debug, Default)]
pub struct FrameStats {
    ms: VecDeque<f64>,
    frames: u64,
    pub triangles: u64,
    pub draws: u64,
    pub chunks_drawn: u64,
    pub uploaded_bytes: u64,
    pub full_rebuilds: u64,
}

impl FrameStats {
    /// Records one frame: its CPU time in milliseconds and what it drew.
    pub fn record(&mut self, ms: f64, paint: PaintStats) {
        if self.ms.len() == RING {
            self.ms.pop_front();
        }
        self.ms.push_back(ms);
        self.frames += 1;
        self.triangles = paint.triangles;
        self.draws = paint.draws;
        self.chunks_drawn = paint.chunks_drawn;
        self.uploaded_bytes = paint.uploaded_bytes;
    }

    /// Frames recorded since the start.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Frame times in the ring, oldest first.
    pub fn samples(&self) -> impl Iterator<Item = f64> + '_ {
        self.ms.iter().copied()
    }

    pub fn p50(&self) -> f64 {
        percentile(self.ms.iter().copied().collect(), 50.0)
    }

    pub fn p95(&self) -> f64 {
        percentile(self.ms.iter().copied().collect(), 95.0)
    }

    pub fn max(&self) -> f64 {
        self.ms.iter().copied().fold(0.0, f64::max)
    }

    /// One line for the overlay and the log.
    pub fn summary(&self) -> String {
        format!(
            "frame CPU p50 {:.2} ms  p95 {:.2} ms  max {:.2} ms | {} tris, {} draws, {} KiB up, {} rebuilds",
            self.p50(),
            self.p95(),
            self.max(),
            self.triangles,
            self.draws,
            self.uploaded_bytes / 1024,
            self.full_rebuilds
        )
    }
}

/// The `p` percentile (nearest rank) of `values`, 0 for none.
pub fn percentile(mut values: Vec<f64>, p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * values.len() as f64).ceil() as usize;
    values[rank.clamp(1, values.len()) - 1]
}

/// Shows the statistics in a corner of the window and logs them every 120
/// frames (`FREEROUTING_GUI_STATS=1`).
pub fn overlay(ctx: &eframe::egui::Context, stats: &FrameStats) {
    use eframe::egui;
    egui::Area::new(egui::Id::new("frame-stats"))
        .anchor(egui::Align2::RIGHT_TOP, [-8.0, 36.0])
        .interactable(false)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(stats.summary()).monospace().strong());
        });
    if stats.frames() % 120 == 0 {
        eprintln!("{}", stats.summary());
    }
}

/// One-minute load average, or 0 where the platform has none.
pub fn load_average_1m() -> f64 {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_the_last_600_frames() {
        let mut s = FrameStats::default();
        for i in 0..700 {
            s.record(f64::from(i), PaintStats::default());
        }
        assert_eq!(s.frames(), 700);
        assert_eq!(s.samples().count(), RING);
        assert_eq!(s.samples().next(), Some(100.0));
        assert_eq!(s.max(), 699.0);
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(v.clone(), 50.0), 50.0);
        assert_eq!(percentile(v.clone(), 95.0), 95.0);
        assert_eq!(percentile(v, 100.0), 100.0);
        assert_eq!(percentile(Vec::new(), 50.0), 0.0);
    }
}
