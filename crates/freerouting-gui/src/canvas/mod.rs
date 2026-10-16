//! The board canvas: geometry in chunks, drawn by one wgpu paint callback.
//!
//! | Module | What it does |
//! |---|---|
//! | [`chunks`] | the board's triangles and hairline segments, cut into chunks by (slot, kind) and grid cell, patched by `ItemId` |
//! | `gpu` | the pipelines, the chunks' vertex, index and segment buffers, and the paint callback (`board.wgsl`, `hairline.wgsl`) |
//!
//! Each frame the canvas makes a *plan* from the chunks: which chunks are
//! visible and on screen, in draw order, with the index of their colour in a
//! small table. The GPU path hands the plan to a paint callback that writes
//! the camera uniform (and the colour table if it changed) and draws; the CPU
//! fallback ([`crate::view::paint_cpu`]) turns the same plan into egui
//! meshes. Chunks are uploaded when a board is loaded and again, one by one,
//! when a patch changes them; panning and zooming upload only the camera.
//!
//! Draw order is the order of the first canvas, because it decides what is
//! on top: the copper layers from the bottom up skipping the active one, the
//! active layer, then the back silk, the front silk and the board; inside a
//! slot the kinds in `Kind::ALL` order; per kind the triangles of all its
//! chunks, then their hairlines. All draws of one (slot, kind) share a
//! colour, so the order of the chunks inside it does not change the image.

pub mod chunks;
mod gpu;

use std::sync::atomic::{AtomicU64, Ordering};

use eframe::egui::{Color32, Painter, Rect};

use crate::look::Look;
use crate::scene::{Bounds, Kind};
use crate::view::{Camera, Renderer, Tessellation, Visibility};
use chunks::{KINDS, kind_index, slot_count, slot_of_index};

pub use gpu::CameraUniform;

/// One draw: the triangles or the hairlines of one chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Draw {
    pub chunk: u32,
    /// Draw the chunk's hairline segments instead of its triangles.
    pub lines: bool,
    /// Index into the colour table: `slot * KINDS + kind`.
    pub color: u32,
}

/// What a frame draws.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub draws: Vec<Draw>,
    /// Chunks drawn: visible, non-empty, and on screen.
    pub chunks_drawn: u64,
    /// Chunks that would be drawn without the culling.
    pub chunks_total: u64,
    /// Triangles in the chunks drawn.
    pub triangles: u64,
}

fn intersects(a: &Bounds, b: &Bounds) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}

/// The draws for the visible slots and kinds, in draw order. With `cull`,
/// chunks whose box misses `view` are left out.
pub fn plan(set: &chunks::ChunkSet, vis: &Visibility, view: &Bounds, cull: bool) -> Plan {
    let layers = set.layer_count();
    // Bottom layer first, the active layer last, everything off the copper
    // layers on top.
    let mut order: Vec<usize> = (0..layers).rev().filter(|&i| i != vis.active).collect();
    order.push(vis.active);
    order.extend(layers..slot_count(layers));

    let mut plan = Plan::default();
    let mut lines = Vec::new();
    for slot_i in order {
        if slot_i >= slot_count(layers) || !vis.slot(slot_of_index(slot_i, layers)) {
            continue;
        }
        for kind in Kind::ALL {
            if !vis.kind(kind) {
                continue;
            }
            let k = kind_index(kind);
            let color = (slot_i * KINDS + k) as u32;
            lines.clear();
            for &id in set.batch(slot_i, k) {
                let chunk = set.chunk(id);
                if chunk.entries.is_empty() {
                    continue;
                }
                plan.chunks_total += 1;
                if cull && !intersects(&chunk.bounds, view) {
                    continue;
                }
                plan.chunks_drawn += 1;
                plan.triangles += chunk.triangles() as u64;
                if !chunk.mesh.indices.is_empty() {
                    plan.draws.push(Draw {
                        chunk: id,
                        lines: false,
                        color,
                    });
                }
                if !chunk.mesh.lines.is_empty() {
                    lines.push(Draw {
                        chunk: id,
                        lines: true,
                        color,
                    });
                }
            }
            plan.draws.append(&mut lines);
        }
    }
    plan
}

/// The colour of every (slot, kind), indexed `slot * KINDS + kind`: the
/// palette, the levels and the dimming of the layers other than `active`
/// ([`Look::draw_color`]).
pub fn color_table(look: &Look, layer_count: usize, active: usize) -> Vec<Color32> {
    let mut table = Vec::with_capacity(slot_count(layer_count) * KINDS);
    for slot_i in 0..slot_count(layer_count) {
        let slot = slot_of_index(slot_i, layer_count);
        for kind in Kind::ALL {
            table.push(look.draw_color(kind, slot, active));
        }
    }
    table
}

/// Counters the GPU side of a board keeps. The callback runs after the frame
/// that made it, so the app reads what the previous callbacks did.
#[derive(Debug, Default)]
pub struct GpuCounters {
    pending_upload: AtomicU64,
    uploaded_total: AtomicU64,
    buffer_bytes: AtomicU64,
    first_upload_us: AtomicU64,
}

impl GpuCounters {
    fn add_upload(&self, bytes: u64) {
        self.pending_upload.fetch_add(bytes, Ordering::Relaxed);
        self.uploaded_total.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Bytes uploaded since the last call, and the bytes of GPU buffers now.
    pub fn take_frame(&self) -> (u64, u64) {
        (
            self.pending_upload.swap(0, Ordering::Relaxed),
            self.buffer_bytes.load(Ordering::Relaxed),
        )
    }

    /// Bytes uploaded since the board was loaded.
    pub fn uploaded_total(&self) -> u64 {
        self.uploaded_total.load(Ordering::Relaxed)
    }

    /// Bytes of vertex, index, segment and uniform buffers on the GPU.
    pub fn buffer_bytes(&self) -> u64 {
        self.buffer_bytes.load(Ordering::Relaxed)
    }

    /// Microseconds the first prepare took, which uploads the whole board;
    /// 0 before it ran.
    pub fn first_upload_us(&self) -> u64 {
        self.first_upload_us.load(Ordering::Relaxed)
    }
}

/// Adds the paint callback that draws the plan.
#[allow(clippy::too_many_arguments)]
pub fn add_board_callback(
    painter: &Painter,
    rect: Rect,
    camera: &Camera,
    tess: &Tessellation,
    origin: crate::scene::P,
    plan: Plan,
    colors: &[Color32],
    renderer: &Renderer,
) {
    let Some(format) = renderer.target_format else {
        return;
    };
    let callback = gpu::CanvasCallback::new(
        tess,
        format,
        gpu::CameraUniform::new(camera, rect, origin),
        plan,
        colors,
        renderer.clock.clone(),
    );
    painter.add(eframe::egui_wgpu::Callback::new_paint_callback(
        rect, callback,
    ));
}

/// Adds an empty paint callback after the meshes of the CPU path, so the
/// frame clock sees when egui has tessellated and uploaded them.
pub fn add_clock_marker(painter: &Painter, rect: Rect, renderer: &Renderer) {
    if renderer.target_format.is_some() {
        painter.add(eframe::egui_wgpu::Callback::new_paint_callback(
            rect,
            gpu::ClockMarker(renderer.clock.clone()),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_index_follows_kind_all() {
        for (i, kind) in Kind::ALL.iter().enumerate() {
            assert_eq!(kind_index(*kind), i);
        }
    }

    #[test]
    fn color_table_dims_inactive_layers() {
        let look = Look::new(2);
        let table = color_table(&look, 2, 0);
        assert_eq!(table.len(), slot_count(2) * KINDS);
        let trace = kind_index(Kind::Trace);
        let active = table[trace];
        let dimmed = table[KINDS + trace];
        assert_eq!(
            active,
            look.draw_color(Kind::Trace, crate::scene::Slot::Layer(0), 0)
        );
        assert_ne!(
            dimmed,
            look.base_color(Kind::Trace, crate::scene::Slot::Layer(1))
        );
    }
}
