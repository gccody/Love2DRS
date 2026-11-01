//! Frame recording structures and the wgpu backend that executes them.
//!
//! Draw calls made through [`Graphics`](super::Graphics) never touch the GPU
//! directly. Geometry is appended to one big CPU vertex/index buffer and
//! consecutive draws sharing a texture + state are merged into a single
//! [`DrawCmd`]. At the end of the frame everything is uploaded with one
//! `write_buffer` per buffer and replayed in as few render passes as possible.

use std::collections::HashMap;
use std::sync::Arc;

use super::Color;
use super::image::{Gpu, IMAGE_FORMAT, Image};
use super::shader::{DEFAULT_EFFECT, PRELUDE, ShaderInner};
use crate::util::FxBuild;

/// A vertex as consumed by the built-in shaders. Positions are in the current
/// target's coordinate space (logical pixels for the screen).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

impl Vertex {
    pub const fn new(x: f32, y: f32, u: f32, v: f32, color: [u8; 4]) -> Self {
        Vertex {
            pos: [x, y],
            uv: [u, v],
            color,
        }
    }
}

/// How drawn pixels are combined with what is already on the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    /// Standard alpha blending.
    #[default]
    Alpha,
    /// Alpha blending for sources whose color is already multiplied by alpha
    /// (e.g. canvases that were drawn into with `Alpha`).
    PremultipliedAlpha,
    /// Additive: great for glows, lasers and particles.
    Add,
    /// Subtracts the source from the destination.
    Subtract,
    /// Multiplies source and destination.
    Multiply,
    /// Overwrites the destination, including alpha.
    Replace,
    /// Inverse multiply; brightens.
    Screen,
    /// Keeps the brighter of source and destination.
    Lighten,
    /// Keeps the darker of source and destination.
    Darken,
}

impl BlendMode {
    fn to_wgpu(self) -> Option<wgpu::BlendState> {
        use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
        let c = |src, dst, op| C {
            src_factor: src,
            dst_factor: dst,
            operation: op,
        };
        let state = |color, alpha| Some(wgpu::BlendState { color, alpha });
        match self {
            BlendMode::Alpha => state(
                c(F::SrcAlpha, F::OneMinusSrcAlpha, O::Add),
                c(F::One, F::OneMinusSrcAlpha, O::Add),
            ),
            BlendMode::PremultipliedAlpha => state(
                c(F::One, F::OneMinusSrcAlpha, O::Add),
                c(F::One, F::OneMinusSrcAlpha, O::Add),
            ),
            BlendMode::Add => state(c(F::SrcAlpha, F::One, O::Add), c(F::Zero, F::One, O::Add)),
            BlendMode::Subtract => state(
                c(F::SrcAlpha, F::One, O::ReverseSubtract),
                c(F::Zero, F::One, O::ReverseSubtract),
            ),
            BlendMode::Multiply => {
                state(c(F::Dst, F::Zero, O::Add), c(F::DstAlpha, F::Zero, O::Add))
            }
            BlendMode::Replace => None,
            BlendMode::Screen => state(
                c(F::One, F::OneMinusSrc, O::Add),
                c(F::One, F::OneMinusSrcAlpha, O::Add),
            ),
            BlendMode::Lighten => state(c(F::One, F::One, O::Max), c(F::One, F::One, O::Max)),
            BlendMode::Darken => state(c(F::One, F::One, O::Min), c(F::One, F::One, O::Min)),
        }
    }
}

pub(crate) enum Geometry {
    /// Indices into this frame's shared vertex/index buffers.
    Frame,
    /// A retained buffer pair owned by a `SpriteBatch`.
    Retained {
        vertices: Arc<wgpu::Buffer>,
        indices: Arc<wgpu::Buffer>,
    },
}

pub(crate) struct DrawCmd {
    pub shader: Option<Arc<ShaderInner>>,
    pub blend: BlendMode,
    pub texture: Arc<wgpu::BindGroup>,
    pub geometry: Geometry,
    pub index_start: u32,
    pub index_end: u32,
    /// Physical pixel scissor rectangle (unclamped).
    pub scissor: Option<[i32; 4]>,
    pub globals_offset: u32,
    pub uniforms_offset: u32,
}

pub(crate) struct Pass {
    /// `None` renders to the window.
    pub target: Option<Image>,
    pub clear: Option<Color>,
    pub cmd_start: usize,
}

#[derive(Default)]
pub(crate) struct FrameData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub uniforms: Vec<u8>,
    pub passes: Vec<Pass>,
    pub cmds: Vec<DrawCmd>,
}

impl FrameData {
    pub fn reset(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.uniforms.clear();
        self.passes.clear();
        self.cmds.clear();
    }
}

/// Uniform block shared by every draw (`Globals` in WGSL).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Globals {
    pub transform: [[f32; 4]; 4],
    pub tint: [f32; 4],
}

pub(crate) const GLOBALS_SIZE: u64 = std::mem::size_of::<Globals>() as u64;
pub(crate) const USER_UNIFORMS_SIZE: u64 = 256;

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct PipelineKey {
    shader: u64,
    blend: BlendMode,
    format: wgpu::TextureFormat,
    samples: u32,
}

/// Where the window's pixels go this frame.
pub(crate) struct ScreenTarget<'a> {
    pub view: &'a wgpu::TextureView,
    pub resolve: Option<&'a wgpu::TextureView>,
    pub format: wgpu::TextureFormat,
    pub samples: u32,
    pub size: (u32, u32),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStats {
    /// GPU draw calls issued last frame.
    pub draw_calls: u32,
    /// Render passes (screen + canvas switches) last frame.
    pub passes: u32,
    /// Vertices generated on the CPU last frame (excludes sprite batches).
    pub vertices: u32,
    /// Triangles submitted last frame (including sprite batches).
    pub triangles: u32,
}

struct GrowBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl GrowBuffer {
    fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        capacity: u64,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: capacity,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        GrowBuffer {
            buffer,
            capacity,
            usage,
            label,
        }
    }

    /// Uploads `data`, growing the buffer if needed. Returns true if reallocated.
    fn upload(&mut self, gpu: &Gpu, data: &[u8]) -> bool {
        let mut grew = false;
        if data.len() as u64 > self.capacity {
            let cap = (data.len() as u64).next_power_of_two();
            *self = GrowBuffer::new(&gpu.device, self.label, self.usage, cap);
            grew = true;
        }
        if !data.is_empty() {
            gpu.queue.write_buffer(&self.buffer, 0, data);
        }
        grew
    }
}

pub(crate) struct Renderer {
    gpu: Arc<Gpu>,
    uniform_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    default_module: wgpu::ShaderModule,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline, FxBuild>,
    vertex_buffer: GrowBuffer,
    index_buffer: GrowBuffer,
    uniform_buffer: GrowBuffer,
    uniform_bind_group: wgpu::BindGroup,
    pub uniform_align: u32,
}

impl Renderer {
    pub fn new(gpu: Arc<Gpu>) -> Self {
        let device = &gpu.device;
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("love2drs uniforms layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(USER_UNIFORMS_SIZE),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("love2drs pipeline layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&gpu.texture_layout)],
            immediate_size: 0,
        });
        let default_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("love2drs default shader"),
            source: wgpu::ShaderSource::Wgsl(format!("{PRELUDE}\n{DEFAULT_EFFECT}").into()),
        });
        let uniform_align = device.limits().min_uniform_buffer_offset_alignment.max(256);
        let vertex_buffer = GrowBuffer::new(
            device,
            "love2drs vertices",
            wgpu::BufferUsages::VERTEX,
            1 << 20,
        );
        let index_buffer = GrowBuffer::new(
            device,
            "love2drs indices",
            wgpu::BufferUsages::INDEX,
            1 << 19,
        );
        let uniform_buffer = GrowBuffer::new(
            device,
            "love2drs uniforms",
            wgpu::BufferUsages::UNIFORM,
            1 << 16,
        );
        let uniform_bind_group =
            Self::make_uniform_bind_group(device, &uniform_layout, &uniform_buffer.buffer);
        Renderer {
            gpu,
            uniform_layout,
            pipeline_layout,
            default_module,
            pipelines: HashMap::default(),
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            uniform_bind_group,
            uniform_align,
        }
    }

    fn make_uniform_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("love2drs uniforms"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(USER_UNIFORMS_SIZE),
                    }),
                },
            ],
        })
    }

    /// Builds the WGSL module for a custom shader `effect`.
    /// The user's code comes first so compiler error line numbers match it.
    pub fn shader_source(effect: &str) -> String {
        format!("{effect}\n{PRELUDE}")
    }

    fn ensure_pipeline(&mut self, key: PipelineKey, shader: Option<&ShaderInner>) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let module = shader.map(|s| &s.module).unwrap_or(&self.default_module);
        let pipeline = self.gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("love2drs pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: key.samples, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: key.format,
                    blend: key.blend.to_wgpu(),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pipeline);
    }

    /// Eagerly builds a pipeline for a freshly compiled custom shader so that
    /// binding errors surface at creation time (inside an error scope).
    pub fn validate_shader(
        &mut self,
        shader: &ShaderInner,
        format: wgpu::TextureFormat,
        samples: u32,
    ) {
        let key = PipelineKey {
            shader: shader.id,
            blend: BlendMode::Alpha,
            format,
            samples,
        };
        self.ensure_pipeline(key, Some(shader));
    }

    /// Uploads the frame and records every pass into `encoder`.
    pub fn render(
        &mut self,
        frame: &FrameData,
        screen: Option<&ScreenTarget>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> FrameStats {
        let mut stats = FrameStats {
            vertices: frame.vertices.len() as u32,
            ..Default::default()
        };

        self.vertex_buffer
            .upload(&self.gpu, bytemuck::cast_slice(&frame.vertices));
        self.index_buffer
            .upload(&self.gpu, bytemuck::cast_slice(&frame.indices));
        if self.uniform_buffer.upload(&self.gpu, &frame.uniforms) {
            self.uniform_bind_group = Self::make_uniform_bind_group(
                &self.gpu.device,
                &self.uniform_layout,
                &self.uniform_buffer.buffer,
            );
        }

        let pass_range = |i: usize| {
            let start = frame.passes[i].cmd_start;
            let end = frame
                .passes
                .get(i + 1)
                .map_or(frame.cmds.len(), |p| p.cmd_start);
            start..end
        };
        let target_info = |pass: &Pass| -> Option<(wgpu::TextureFormat, u32)> {
            match &pass.target {
                Some(_) => Some((IMAGE_FORMAT, 1)),
                None => screen.map(|s| (s.format, s.samples)),
            }
        };

        // Create any pipelines this frame needs before borrowing them immutably.
        for (i, pass) in frame.passes.iter().enumerate() {
            let Some((format, samples)) = target_info(pass) else {
                continue;
            };
            for cmd in &frame.cmds[pass_range(i)] {
                let key = PipelineKey {
                    shader: cmd.shader.as_ref().map_or(0, |s| s.id),
                    blend: cmd.blend,
                    format,
                    samples,
                };
                self.ensure_pipeline(key, cmd.shader.as_deref());
            }
        }

        for (i, pass) in frame.passes.iter().enumerate() {
            let Some((format, samples)) = target_info(pass) else {
                continue;
            };
            let range = pass_range(i);
            if range.is_empty() && pass.clear.is_none() {
                continue;
            }
            let (view, resolve, size) = match (&pass.target, screen) {
                (Some(img), _) => (&img.inner.view, None, (img.width(), img.height())),
                (None, Some(s)) => (s.view, s.resolve, s.size),
                (None, None) => continue,
            };
            let load = match pass.clear {
                Some(c) => wgpu::LoadOp::Clear(c.to_wgpu()),
                None => wgpu::LoadOp::Load,
            };
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("love2drs pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            stats.passes += 1;

            let mut last_pipeline: Option<PipelineKey> = None;
            let mut last_texture: *const wgpu::BindGroup = std::ptr::null();
            let mut last_uniforms: Option<(u32, u32)> = None;
            let mut last_scissor: Option<[u32; 4]> = None;
            let mut last_geometry: *const wgpu::Buffer = std::ptr::null();

            for cmd in &frame.cmds[range] {
                if cmd.index_end <= cmd.index_start {
                    continue;
                }
                let scissor = clamp_scissor(cmd.scissor, size);
                if scissor[2] == 0 || scissor[3] == 0 {
                    continue;
                }
                let key = PipelineKey {
                    shader: cmd.shader.as_ref().map_or(0, |s| s.id),
                    blend: cmd.blend,
                    format,
                    samples,
                };
                if last_pipeline != Some(key) {
                    rpass.set_pipeline(&self.pipelines[&key]);
                    last_pipeline = Some(key);
                }
                if last_uniforms != Some((cmd.globals_offset, cmd.uniforms_offset)) {
                    rpass.set_bind_group(
                        0,
                        &self.uniform_bind_group,
                        &[cmd.globals_offset, cmd.uniforms_offset],
                    );
                    last_uniforms = Some((cmd.globals_offset, cmd.uniforms_offset));
                }
                if last_texture != Arc::as_ptr(&cmd.texture) {
                    rpass.set_bind_group(1, &*cmd.texture, &[]);
                    last_texture = Arc::as_ptr(&cmd.texture);
                }
                if last_scissor != Some(scissor) {
                    rpass.set_scissor_rect(scissor[0], scissor[1], scissor[2], scissor[3]);
                    last_scissor = Some(scissor);
                }
                let (vb, ib) = match &cmd.geometry {
                    Geometry::Frame => (&self.vertex_buffer.buffer, &self.index_buffer.buffer),
                    Geometry::Retained { vertices, indices } => (&**vertices, &**indices),
                };
                if !std::ptr::eq(last_geometry, vb) {
                    rpass.set_vertex_buffer(0, vb.slice(..));
                    rpass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    last_geometry = vb as *const _;
                }
                rpass.draw_indexed(cmd.index_start..cmd.index_end, 0, 0..1);
                stats.draw_calls += 1;
                stats.triangles += (cmd.index_end - cmd.index_start) / 3;
            }
        }
        stats
    }
}

fn clamp_scissor(scissor: Option<[i32; 4]>, (w, h): (u32, u32)) -> [u32; 4] {
    match scissor {
        None => [0, 0, w, h],
        Some([x, y, sw, sh]) => {
            let x0 = x.clamp(0, w as i32);
            let y0 = y.clamp(0, h as i32);
            let x1 = (x + sw).clamp(0, w as i32);
            let y1 = (y + sh).clamp(0, h as i32);
            [
                x0 as u32,
                y0 as u32,
                (x1 - x0).max(0) as u32,
                (y1 - y0).max(0) as u32,
            ]
        }
    }
}
