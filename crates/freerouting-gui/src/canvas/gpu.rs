//! The wgpu side of the canvas: two pipelines (filled triangles and
//! hairlines), the chunks' buffers, and the paint callback that draws a plan.
//!
//! [`BoardGpu`] lives in egui's `callback_resources`. The first `prepare`
//! after a board is loaded uploads every chunk; later ones upload only the
//! chunks a patch marked dirty, the camera uniform, and the colour table when
//! it changed (a different active layer).

use std::num::NonZeroU64;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use eframe::egui::{PaintCallbackInfo, Rect};
use eframe::egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu::{self, util::DeviceExt};

use super::chunks::{ChunkSet, LINE_BYTES};
use super::{Draw, GpuCounters, Plan};
use crate::scene::P;
use crate::stats::FrameClock;
use crate::view::{Camera, Tessellation};

/// Bytes of the camera uniform.
pub const CAMERA_BYTES: usize = 32;

/// The camera as the shaders read it: where the view is centred relative to
/// the board origin (millimetres), points per millimetre, and the centre of
/// the canvas in screen points. The shaders map points to clip space the way
/// egui's own shader does, over the whole screen (the callback widens the
/// viewport to it), so the board lands on the same pixels as the CPU painter's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraUniform {
    pub offset: [f32; 2],
    pub zoom: f32,
    pub center: [f32; 2],
}

impl CameraUniform {
    pub fn new(camera: &Camera, rect: Rect, origin: P) -> CameraUniform {
        CameraUniform {
            offset: [
                (camera.center.x - origin.x) as f32,
                (camera.center.y - origin.y) as f32,
            ],
            zoom: camera.zoom as f32,
            center: [rect.center().x, rect.center().y],
        }
    }

    /// The uniform's bytes for a screen of `screen` points.
    fn bytes(&self, screen: [f32; 2]) -> [u8; CAMERA_BYTES] {
        let words = [
            self.offset[0],
            self.offset[1],
            self.zoom,
            0.0,
            self.center[0],
            self.center[1],
            screen[0] / 2.0,
            screen[1] / 2.0,
        ];
        let mut out = [0u8; CAMERA_BYTES];
        for (chunk, w) in out.chunks_exact_mut(4).zip(words) {
            chunk.copy_from_slice(&w.to_ne_bytes());
        }
        out
    }
}

struct Buf {
    buffer: wgpu::Buffer,
    size: u64,
}

#[derive(Default)]
struct ChunkBuffers {
    vertices: Option<Buf>,
    indices: Option<Buf>,
    lines: Option<Buf>,
    index_count: u32,
    line_count: u32,
}

impl ChunkBuffers {
    fn bytes(&self) -> u64 {
        [&self.vertices, &self.indices, &self.lines]
            .into_iter()
            .flatten()
            .map(|b| b.size)
            .sum()
    }
}

struct ColorTable {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
    values: Vec<[f32; 4]>,
}

/// The pipelines and buffers of the board on one device.
pub struct BoardGpu {
    triangles: wgpu::RenderPipeline,
    hairlines: wgpu::RenderPipeline,
    camera: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    color_layout: wgpu::BindGroupLayout,
    color_stride: u64,
    color: Option<ColorTable>,
    /// The chunk set the buffers hold.
    set_id: u64,
    chunks: Vec<ChunkBuffers>,
    scratch: Vec<u8>,
}

fn blend() -> wgpu::BlendState {
    // The blend egui uses: premultiplied alpha.
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

impl BoardGpu {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> BoardGpu {
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("board camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(CAMERA_BYTES as u64),
                },
                count: None,
            }],
        });
        let color_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("board color layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(16),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("board pipeline layout"),
            bind_group_layouts: &[&camera_layout, &color_layout],
            push_constant_ranges: &[],
        });
        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("board camera"),
            size: CAMERA_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("board camera group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let fragment = if format.is_srgb() {
            "fs_linear"
        } else {
            "fs_gamma"
        };
        let pipeline = |label: &str,
                        source: &str,
                        topology: wgpu::PrimitiveTopology,
                        buffers: &[wgpu::VertexBufferLayout<'_>]| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: None,
                    ..wgpu::PrimitiveState::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend()),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let vertex_attrs = wgpu::vertex_attr_array![0 => Float32x2];
        let triangles = pipeline(
            "board triangles",
            include_str!("board.wgsl"),
            wgpu::PrimitiveTopology::TriangleList,
            &[wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vertex_attrs,
            }],
        );
        let segment_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32];
        let hairlines = pipeline(
            "board hairlines",
            include_str!("hairline.wgsl"),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[wgpu::VertexBufferLayout {
                array_stride: LINE_BYTES as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &segment_attrs,
            }],
        );
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        BoardGpu {
            triangles,
            hairlines,
            camera,
            camera_group,
            color_layout,
            color_stride: alignment.max(16),
            color: None,
            set_id: 0,
            chunks: Vec::new(),
            scratch: Vec::new(),
        }
    }

    /// Bytes of all buffers.
    pub fn buffer_bytes(&self) -> u64 {
        let colors = self
            .color
            .as_ref()
            .map_or(0, |c| c.values.len() as u64 * self.color_stride);
        CAMERA_BYTES as u64 + colors + self.chunks.iter().map(ChunkBuffers::bytes).sum::<u64>()
    }

    /// Writes `bytes` to `slot`, creating the buffer if it does not exist or
    /// has another size. Returns the bytes uploaded.
    fn put(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut Option<Buf>,
        bytes: &[u8],
        usage: wgpu::BufferUsages,
        label: &'static str,
    ) -> u64 {
        if bytes.is_empty() {
            *slot = None;
            return 0;
        }
        let size = bytes.len() as u64;
        match slot {
            Some(b) if b.size == size => queue.write_buffer(&b.buffer, 0, bytes),
            _ => {
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: usage | wgpu::BufferUsages::COPY_DST,
                });
                *slot = Some(Buf { buffer, size });
            }
        }
        size
    }

    /// Uploads one chunk's buffers. Returns the bytes uploaded.
    fn upload_chunk(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        set: &ChunkSet,
        id: usize,
    ) -> u64 {
        if self.chunks.len() <= id {
            self.chunks.resize_with(id + 1, ChunkBuffers::default);
        }
        let mesh = &set.chunk(id as u32).mesh;
        let mut bytes = std::mem::take(&mut self.scratch);
        let out = &mut self.chunks[id];

        bytes.clear();
        for v in &mesh.vertices {
            bytes.extend_from_slice(&v[0].to_ne_bytes());
            bytes.extend_from_slice(&v[1].to_ne_bytes());
        }
        let mut sent = Self::put(
            device,
            queue,
            &mut out.vertices,
            &bytes,
            wgpu::BufferUsages::VERTEX,
            "board vertices",
        );
        bytes.clear();
        for i in &mesh.indices {
            bytes.extend_from_slice(&i.to_ne_bytes());
        }
        sent += Self::put(
            device,
            queue,
            &mut out.indices,
            &bytes,
            wgpu::BufferUsages::INDEX,
            "board indices",
        );
        out.index_count = mesh.indices.len() as u32;
        bytes.clear();
        for l in &mesh.lines {
            for w in l {
                bytes.extend_from_slice(&w.to_ne_bytes());
            }
        }
        sent += Self::put(
            device,
            queue,
            &mut out.lines,
            &bytes,
            wgpu::BufferUsages::VERTEX,
            "board segments",
        );
        out.line_count = mesh.lines.len() as u32;
        self.scratch = bytes;
        sent
    }

    /// Writes the colour table if it differs from the one on the GPU.
    fn write_colors(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        values: &[[f32; 4]],
    ) -> u64 {
        if self.color.as_ref().is_some_and(|c| c.values == values) {
            return 0;
        }
        let stride = self.color_stride as usize;
        let mut bytes = vec![0u8; values.len() * stride];
        for (i, v) in values.iter().enumerate() {
            for (k, w) in v.iter().enumerate() {
                bytes[i * stride + k * 4..][..4].copy_from_slice(&w.to_ne_bytes());
            }
        }
        let size = bytes.len() as u64;
        match &mut self.color {
            Some(c) if c.values.len() == values.len() => {
                queue.write_buffer(&c.buffer, 0, &bytes);
                c.values = values.to_vec();
            }
            _ => {
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("board colors"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("board color group"),
                    layout: &self.color_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffer,
                            offset: 0,
                            size: NonZeroU64::new(16),
                        }),
                    }],
                });
                self.color = Some(ColorTable {
                    buffer,
                    group,
                    values: values.to_vec(),
                });
            }
        }
        size
    }
}

/// Draws one frame's plan.
pub struct CanvasCallback {
    chunks: Arc<Mutex<ChunkSet>>,
    counters: Arc<GpuCounters>,
    clock: Arc<FrameClock>,
    format: wgpu::TextureFormat,
    camera: CameraUniform,
    draws: Vec<Draw>,
    colors: Vec<[f32; 4]>,
}

impl CanvasCallback {
    pub fn new(
        tess: &Tessellation,
        format: wgpu::TextureFormat,
        camera: CameraUniform,
        plan: Plan,
        colors: &[eframe::egui::Color32],
        clock: Arc<FrameClock>,
    ) -> CanvasCallback {
        CanvasCallback {
            chunks: tess.shared().clone(),
            counters: tess.counters().clone(),
            clock,
            format,
            camera,
            draws: plan.draws,
            colors: colors
                .iter()
                .map(|c| c.to_array().map(|b| f32::from(b) / 255.0))
                .collect(),
        }
    }
}

impl CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let started = self.clock.prepare_begin();
        let first = resources.get::<BoardGpu>().is_none();
        if first {
            resources.insert(BoardGpu::new(device, self.format));
        }
        let Some(gpu) = resources.get_mut::<BoardGpu>() else {
            return Vec::new();
        };
        let mut sent = 0;
        {
            let mut set = self.chunks.lock().unwrap_or_else(PoisonError::into_inner);
            let dirty = set.take_dirty();
            if gpu.set_id != set.id() {
                // Another board (or the first): everything goes up.
                gpu.chunks.clear();
                gpu.set_id = set.id();
                for id in 0..set.chunks().len() {
                    sent += gpu.upload_chunk(device, queue, &set, id);
                }
                if self.counters.first_upload_us() == 0 {
                    let us = started.elapsed().as_micros() as u64;
                    self.counters
                        .first_upload_us
                        .store(us.max(1), std::sync::atomic::Ordering::Relaxed);
                }
            } else {
                for id in dirty {
                    sent += gpu.upload_chunk(device, queue, &set, id as usize);
                }
            }
        }
        let points = [
            screen.size_in_pixels[0] as f32 / screen.pixels_per_point,
            screen.size_in_pixels[1] as f32 / screen.pixels_per_point,
        ];
        queue.write_buffer(&gpu.camera, 0, &self.camera.bytes(points));
        sent += CAMERA_BYTES as u64;
        sent += gpu.write_colors(device, queue, &self.colors);
        self.counters.add_upload(sent);
        self.counters
            .buffer_bytes
            .store(gpu.buffer_bytes(), std::sync::atomic::Ordering::Relaxed);
        self.clock.prepare_end(started);
        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let started = Instant::now();
        // The shaders work in screen points. egui's scissor rectangle still
        // limits the drawing to the canvas.
        pass.set_viewport(
            0.0,
            0.0,
            info.screen_size_px[0] as f32,
            info.screen_size_px[1] as f32,
            0.0,
            1.0,
        );
        let Some(gpu) = resources.get::<BoardGpu>() else {
            return;
        };
        let Some(colors) = &gpu.color else {
            return;
        };
        let mut current: Option<bool> = None;
        let mut color: Option<u32> = None;
        for draw in &self.draws {
            let Some(buffers) = gpu.chunks.get(draw.chunk as usize) else {
                continue;
            };
            if current != Some(draw.lines) {
                pass.set_pipeline(if draw.lines {
                    &gpu.hairlines
                } else {
                    &gpu.triangles
                });
                pass.set_bind_group(0, &gpu.camera_group, &[]);
                current = Some(draw.lines);
                color = None;
            }
            // Consecutive chunks of a (slot, kind) share their colour.
            let mut set_color = |pass: &mut wgpu::RenderPass<'static>| {
                if color != Some(draw.color) {
                    let offset = (u64::from(draw.color) * gpu.color_stride) as u32;
                    pass.set_bind_group(1, &colors.group, &[offset]);
                    color = Some(draw.color);
                }
            };
            if draw.lines {
                let Some(lines) = &buffers.lines else {
                    continue;
                };
                set_color(pass);
                pass.set_vertex_buffer(0, lines.buffer.slice(..));
                pass.draw(0..4, 0..buffers.line_count);
            } else {
                let (Some(vertices), Some(indices)) = (&buffers.vertices, &buffers.indices) else {
                    continue;
                };
                set_color(pass);
                pass.set_vertex_buffer(0, vertices.buffer.slice(..));
                pass.set_index_buffer(indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..buffers.index_count, 0, 0..1);
            }
        }
        self.clock.paint_end(started);
    }
}

/// An empty callback that only reports to the frame clock.
pub struct ClockMarker(pub Arc<FrameClock>);

impl CallbackTrait for ClockMarker {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        _resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let started = self.0.prepare_begin();
        self.0.prepare_end(started);
        Vec::new()
    }

    fn paint(
        &self,
        _info: PaintCallbackInfo,
        _pass: &mut wgpu::RenderPass<'static>,
        _resources: &CallbackResources,
    ) {
        self.0.paint_end(Instant::now());
    }
}
