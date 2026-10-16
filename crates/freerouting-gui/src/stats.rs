//! Frame statistics: the CPU time of the last frames and the counters that
//! tests and `--bench` read.
//!
//! There are two definitions of "frame CPU":
//!
//! - **update CPU**, the first one (FT0 as G01 recorded it): the time on the
//!   GUI thread inside `App::update`, which includes building and painting
//!   the board's shapes. It excludes egui's tessellation and upload.
//! - **frame CPU**, the one the frame targets FT1 to FT7 use from G02 on:
//!   the update, plus what eframe does on the GUI thread between the update
//!   and the board's paint callback (finishing egui's pass, tessellating its
//!   shapes, uploading its meshes), plus the callback's `prepare` and
//!   `paint`. It still excludes queue submission and presentation, which
//!   under Xvfb is llvmpipe rasterising and not a usable metric. The
//!   [`FrameClock`] measures it.
//!
//! Later checklists add their counters here.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

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
    /// Chunks that were drawn rather than skipped.
    pub chunks_drawn: u64,
    /// Chunks that would be drawn without viewport culling.
    pub chunks_total: u64,
    /// Bytes of vertex, index, segment and uniform buffers on the GPU (the
    /// GPU path only).
    pub gpu_buffer_bytes: u64,
}

/// What the overlays drew in the last frame (G03).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayCounts {
    /// Ratsnest lines.
    pub airlines: usize,
    /// Clearance violation markers.
    pub markers: usize,
    /// Shapes of the selection highlight.
    pub highlighted: usize,
}

/// One frame's CPU time on the GUI thread, by the two definitions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameCpu {
    /// Inside `App::update`.
    pub update_ms: f64,
    /// The update, plus egui's finishing and tessellation up to the board's
    /// callback, plus the callback's `prepare` and `paint`.
    pub frame_ms: f64,
}

#[derive(Debug, Default)]
struct ClockState {
    update_start: Option<Instant>,
    update_end: Option<Instant>,
    update_ms: f64,
    /// Time from the end of the update to the start of the first `prepare`.
    gap_ms: Option<f64>,
    callback_ms: f64,
    done: Vec<FrameCpu>,
    first_paint_end: Option<Instant>,
}

/// Measures frame CPU (see the module docs). The app calls
/// [`begin_update`](Self::begin_update) and [`end_update`](Self::end_update);
/// the paint callback calls the other four. A frame is complete when its
/// callback has painted, which is after the update that made it returned, so
/// [`take_done`](Self::take_done) hands out frames one behind.
#[derive(Debug, Default)]
pub struct FrameClock {
    state: Mutex<ClockState>,
}

impl FrameClock {
    fn lock(&self) -> std::sync::MutexGuard<'_, ClockState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn begin_update(&self) {
        let mut s = self.lock();
        s.update_start = Some(Instant::now());
        s.update_end = None;
        s.gap_ms = None;
        s.callback_ms = 0.0;
    }

    /// Ends the update; returns its duration in milliseconds.
    pub fn end_update(&self) -> f64 {
        let mut s = self.lock();
        let now = Instant::now();
        s.update_end = Some(now);
        s.update_ms = s
            .update_start
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64() * 1000.0);
        s.update_ms
    }

    /// The callback's `prepare` starts. The first call of a frame fixes the
    /// time spent between the update and here.
    pub fn prepare_begin(&self) -> Instant {
        let now = Instant::now();
        let mut s = self.lock();
        if s.gap_ms.is_none()
            && let Some(end) = s.update_end
        {
            s.gap_ms = Some(now.duration_since(end).as_secs_f64() * 1000.0);
        }
        now
    }

    pub fn prepare_end(&self, started: Instant) {
        self.lock().callback_ms += started.elapsed().as_secs_f64() * 1000.0;
    }

    /// The callback's `paint` ends; the frame is complete.
    pub fn paint_end(&self, started: Instant) {
        let mut s = self.lock();
        s.callback_ms += started.elapsed().as_secs_f64() * 1000.0;
        if let Some(gap) = s.gap_ms.take() {
            let frame = FrameCpu {
                update_ms: s.update_ms,
                frame_ms: s.update_ms + gap + s.callback_ms,
            };
            s.done.push(frame);
            s.callback_ms = 0.0;
            s.first_paint_end.get_or_insert_with(Instant::now);
        }
    }

    /// Forgets the time of the first paint, when a load starts.
    pub fn reset_first_paint(&self) {
        self.lock().first_paint_end = None;
    }

    /// When the first frame with a board finished painting since the last
    /// [`reset_first_paint`](Self::reset_first_paint).
    pub fn first_paint_end(&self) -> Option<Instant> {
        self.lock().first_paint_end
    }

    /// The frames completed since the last call, oldest first.
    pub fn take_done(&self) -> Vec<FrameCpu> {
        std::mem::take(&mut self.lock().done)
    }
}

/// How long the phases of loading a board took, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadTimings {
    /// Reading and decoding the file.
    pub read_ms: f64,
    /// Parsing the design into the syntax tree.
    pub parse_ms: f64,
    /// `build_board` (and the session import, if any).
    pub board_ms: f64,
    /// Flattening the board to scene items.
    pub scene_ms: f64,
    /// Triangulating the items.
    pub tessellation_ms: f64,
    /// Bounds, grid and grouping into chunks.
    pub chunking_ms: f64,
    /// All of it, from the start of the read.
    pub total_ms: f64,
}

impl LoadTimings {
    /// `{"read":..,"parse":..,..}` for the bench report.
    pub fn json(&self) -> String {
        format!(
            "{{\"read\":{:.1},\"parse\":{:.1},\"build_board\":{:.1},\"scene\":{:.1},\"tessellation\":{:.1},\"chunking\":{:.1},\"total\":{:.1}}}",
            self.read_ms,
            self.parse_ms,
            self.board_ms,
            self.scene_ms,
            self.tessellation_ms,
            self.chunking_ms,
            self.total_ms
        )
    }
}

/// A ring of the last [`RING`] frame CPU times plus counters.
#[derive(Clone, Debug, Default)]
pub struct FrameStats {
    ms: VecDeque<f64>,
    /// Frame CPU (the second definition) of the last frames.
    full: VecDeque<f64>,
    frames: u64,
    pub triangles: u64,
    pub draws: u64,
    pub chunks_drawn: u64,
    pub chunks_total: u64,
    /// Bytes handed to the renderer in the last frame.
    pub uploaded_bytes: u64,
    /// Bytes handed to the renderer since the start.
    pub uploaded_total: u64,
    pub gpu_buffer_bytes: u64,
    /// Boards built (tessellated and chunked in full) since the start.
    pub builds: u64,
    /// Full builds after the first: a patch never counts here.
    pub full_rebuilds: u64,
    /// What the overlays drew in the last frame.
    pub overlay: OverlayCounts,
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
        self.chunks_total = paint.chunks_total;
        self.uploaded_bytes = paint.uploaded_bytes;
        self.uploaded_total += paint.uploaded_bytes;
        self.gpu_buffer_bytes = paint.gpu_buffer_bytes;
    }

    /// Records the frame CPU (second definition) of a finished frame.
    pub fn record_full(&mut self, ms: f64) {
        if self.full.len() == RING {
            self.full.pop_front();
        }
        self.full.push_back(ms);
    }

    /// Counts a board that was tessellated and chunked in full.
    pub fn record_build(&mut self) {
        self.builds += 1;
        self.full_rebuilds = self.builds - 1;
    }

    pub fn full_p50(&self) -> f64 {
        percentile(self.full.iter().copied().collect(), 50.0)
    }

    pub fn full_p95(&self) -> f64 {
        percentile(self.full.iter().copied().collect(), 95.0)
    }

    pub fn full_max(&self) -> f64 {
        self.full.iter().copied().fold(0.0, f64::max)
    }

    /// Frame CPU samples (second definition), oldest first.
    pub fn full_samples(&self) -> impl Iterator<Item = f64> + '_ {
        self.full.iter().copied()
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
            "update CPU p50 {:.2} p95 {:.2} max {:.2} ms | frame CPU p50 {:.2} p95 {:.2} max {:.2} ms | {} tris, {} draws, {}/{} chunks, {} KiB up, {} MiB gpu, {} rebuilds",
            self.p50(),
            self.p95(),
            self.max(),
            self.full_p50(),
            self.full_p95(),
            self.full_max(),
            self.triangles,
            self.draws,
            self.chunks_drawn,
            self.chunks_total,
            self.uploaded_bytes / 1024,
            self.gpu_buffer_bytes >> 20,
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
