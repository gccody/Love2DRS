use std::cell::{Cell, RefCell};
use std::sync::Arc;

use super::image::{Gpu, Image, Quad};
use super::renderer::Vertex;
use super::{Color, DrawParams, quad_vertices};

pub(crate) struct BatchGpu {
    pub vertices: Arc<wgpu::Buffer>,
    pub indices: Arc<wgpu::Buffer>,
    capacity: usize,
    last_frame: u64,
}

/// Many sprites from one image, kept in a GPU buffer between frames
/// (`love.graphics.newSpriteBatch`).
///
/// Ideal for tile maps and other mostly-static geometry: the vertices are only
/// re-uploaded when the batch changes, and the whole batch is drawn with a
/// single draw call that can still be moved, rotated and scaled.
pub struct SpriteBatch {
    image: Image,
    vertices: Vec<Vertex>,
    color: Color,
    gpu: RefCell<Option<BatchGpu>>,
    dirty: Cell<bool>,
}

impl SpriteBatch {
    pub(crate) fn new(image: &Image, capacity: usize) -> Self {
        SpriteBatch {
            image: image.clone(),
            vertices: Vec::with_capacity(capacity * 4),
            color: Color::WHITE,
            gpu: RefCell::new(None),
            dirty: Cell::new(true),
        }
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    /// Number of sprites in the batch.
    pub fn len(&self) -> usize {
        self.vertices.len() / 4
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// Color applied to sprites added from now on (unless their params set one).
    pub fn set_color(&mut self, color: impl Into<Color>) {
        self.color = color.into();
    }

    fn sprite(&self, quad: Option<Quad>, params: &DrawParams) -> [Vertex; 4] {
        let (iw, ih) = (self.image.width() as f32, self.image.height() as f32);
        let q = quad.unwrap_or(Quad::new(0.0, 0.0, iw, ih));
        let t = params.to_transform(q.w, q.h);
        let uv = [q.x / iw, q.y / ih, (q.x + q.w) / iw, (q.y + q.h) / ih];
        let color = params.color.unwrap_or(self.color).to_rgba8();
        quad_vertices(&t, 0.0, 0.0, q.w, q.h, uv, color)
    }

    /// Adds the whole image as a sprite; returns its index.
    pub fn add(&mut self, params: impl Into<DrawParams>) -> usize {
        let v = self.sprite(None, &params.into());
        self.push(v)
    }

    /// Adds a sub-rectangle of the image as a sprite; returns its index.
    pub fn add_quad(&mut self, quad: Quad, params: impl Into<DrawParams>) -> usize {
        let v = self.sprite(Some(quad), &params.into());
        self.push(v)
    }

    fn push(&mut self, v: [Vertex; 4]) -> usize {
        self.vertices.extend_from_slice(&v);
        self.dirty.set(true);
        self.len() - 1
    }

    /// Replaces sprite `index` with the whole image at new params.
    pub fn set(&mut self, index: usize, params: impl Into<DrawParams>) {
        let v = self.sprite(None, &params.into());
        self.replace(index, v);
    }

    /// Replaces sprite `index` with a quad at new params.
    pub fn set_quad(&mut self, index: usize, quad: Quad, params: impl Into<DrawParams>) {
        let v = self.sprite(Some(quad), &params.into());
        self.replace(index, v);
    }

    fn replace(&mut self, index: usize, v: [Vertex; 4]) {
        self.vertices[index * 4..index * 4 + 4].copy_from_slice(&v);
        self.dirty.set(true);
    }

    /// Removes every sprite.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.dirty.set(true);
    }

    /// Makes sure GPU buffers reflect the CPU data. Returns `(vertices, indices, index_count)`.
    pub(crate) fn prepare(
        &self,
        gpu: &Gpu,
        frame: u64,
    ) -> Option<(Arc<wgpu::Buffer>, Arc<wgpu::Buffer>, u32)> {
        let n = self.len();
        if n == 0 {
            return None;
        }
        let mut slot = self.gpu.borrow_mut();
        let realloc = match &*slot {
            None => true,
            // Buffers already referenced by a draw this frame must not be
            // overwritten, so modifications mid-frame get fresh buffers.
            Some(b) => b.capacity < n || (self.dirty.get() && b.last_frame == frame),
        };
        if realloc {
            let capacity = n.next_power_of_two().max(16);
            let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("love2drs sprite batch vertices"),
                size: (capacity * 4 * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let indices: Vec<u32> = (0..capacity as u32)
                .flat_map(|i| {
                    let b = i * 4;
                    [b, b + 1, b + 2, b, b + 2, b + 3]
                })
                .collect();
            let index_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("love2drs sprite batch indices"),
                size: (indices.len() * 4) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.queue
                .write_buffer(&index_buffer, 0, bytemuck::cast_slice(&indices));
            *slot = Some(BatchGpu {
                vertices: Arc::new(vertices),
                indices: Arc::new(index_buffer),
                capacity,
                last_frame: u64::MAX,
            });
            self.dirty.set(true);
        }
        let b = slot.as_mut().unwrap();
        if self.dirty.replace(false) {
            gpu.queue
                .write_buffer(&b.vertices, 0, bytemuck::cast_slice(&self.vertices));
        }
        b.last_frame = frame;
        Some((b.vertices.clone(), b.indices.clone(), (n * 6) as u32))
    }
}
