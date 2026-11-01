use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::Color;
use crate::{Error, Result};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// GPU handles shared by every resource (devices are internally reference counted).
pub(crate) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub texture_layout: wgpu::BindGroupLayout,
    samplers: Vec<wgpu::Sampler>,
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("love2drs texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let mut samplers = Vec::new();
        for filter in [FilterMode::Linear, FilterMode::Nearest] {
            for wrap in [WrapMode::Clamp, WrapMode::Repeat, WrapMode::MirroredRepeat] {
                let f = match filter {
                    FilterMode::Linear => wgpu::FilterMode::Linear,
                    FilterMode::Nearest => wgpu::FilterMode::Nearest,
                };
                let w = match wrap {
                    WrapMode::Clamp => wgpu::AddressMode::ClampToEdge,
                    WrapMode::Repeat => wgpu::AddressMode::Repeat,
                    WrapMode::MirroredRepeat => wgpu::AddressMode::MirrorRepeat,
                };
                samplers.push(device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("love2drs sampler"),
                    address_mode_u: w,
                    address_mode_v: w,
                    address_mode_w: w,
                    mag_filter: f,
                    min_filter: f,
                    ..Default::default()
                }));
            }
        }
        Gpu {
            device,
            queue,
            texture_layout,
            samplers,
        }
    }

    fn sampler(&self, filter: FilterMode, wrap: WrapMode) -> &wgpu::Sampler {
        &self.samplers[filter as usize * 3 + wrap as usize]
    }

    fn bind_group(
        &self,
        view: &wgpu::TextureView,
        filter: FilterMode,
        wrap: WrapMode,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("love2drs texture bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(self.sampler(filter, wrap)),
                },
            ],
        })
    }
}

/// How a texture is sampled when scaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterMode {
    /// Smooth (bilinear) filtering.
    #[default]
    Linear = 0,
    /// Blocky filtering, ideal for pixel art.
    Nearest = 1,
}

/// How texture coordinates outside `0..1` are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WrapMode {
    #[default]
    Clamp = 0,
    Repeat = 1,
    MirroredRepeat = 2,
}

pub(crate) struct ImageInner {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub bind_group: Mutex<Arc<wgpu::BindGroup>>,
    sampling: Mutex<(FilterMode, WrapMode)>,
    gpu: Arc<Gpu>,
    pub is_canvas: bool,
}

/// A texture living on the GPU. Cheap to clone (reference counted).
///
/// Create one with [`Graphics::new_image`](super::Graphics::new_image) and
/// draw it with [`Graphics::draw`](super::Graphics::draw).
#[derive(Clone)]
pub struct Image {
    pub(crate) inner: Arc<ImageInner>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("id", &self.inner.id)
            .field("width", &self.inner.width)
            .field("height", &self.inner.height)
            .finish()
    }
}

/// Texture format for all images and canvases. Not sRGB: colors are blended in
/// gamma space, exactly like LÖVE's default.
pub(crate) const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Image {
    pub(crate) fn create(
        gpu: &Arc<Gpu>,
        width: u32,
        height: u32,
        rgba: Option<&[u8]>,
        filter: FilterMode,
        is_canvas: bool,
    ) -> Image {
        let width = width.max(1);
        let height = height.max(1);
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC;
        if is_canvas {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(if is_canvas {
                "love2drs canvas"
            } else {
                "love2drs image"
            }),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = gpu.bind_group(&view, filter, WrapMode::Clamp);
        let img = Image {
            inner: Arc::new(ImageInner {
                id: next_id(),
                width,
                height,
                texture,
                view,
                bind_group: Mutex::new(Arc::new(bind_group)),
                sampling: Mutex::new((filter, WrapMode::Clamp)),
                gpu: gpu.clone(),
                is_canvas,
            }),
        };
        if let Some(data) = rgba {
            img.write_region(0, 0, width, height, data);
        }
        img
    }

    pub fn width(&self) -> u32 {
        self.inner.width
    }

    pub fn height(&self) -> u32 {
        self.inner.height
    }

    /// `(width, height)` in pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.inner.width, self.inner.height)
    }

    pub fn filter(&self) -> FilterMode {
        self.inner.sampling.lock().unwrap().0
    }

    pub fn wrap(&self) -> WrapMode {
        self.inner.sampling.lock().unwrap().1
    }

    /// Changes how the image is filtered when scaled (`image:setFilter`).
    pub fn set_filter(&self, filter: FilterMode) {
        let wrap = self.wrap();
        self.set_sampling(filter, wrap);
    }

    /// Changes how texture coordinates outside the image wrap (`image:setWrap`).
    pub fn set_wrap(&self, wrap: WrapMode) {
        let filter = self.filter();
        self.set_sampling(filter, wrap);
    }

    fn set_sampling(&self, filter: FilterMode, wrap: WrapMode) {
        let mut s = self.inner.sampling.lock().unwrap();
        if *s == (filter, wrap) {
            return;
        }
        *s = (filter, wrap);
        let bg = self.inner.gpu.bind_group(&self.inner.view, filter, wrap);
        *self.inner.bind_group.lock().unwrap() = Arc::new(bg);
    }

    /// Replaces the whole image with new RGBA8 pixels (`width * height * 4` bytes).
    pub fn replace_pixels(&self, rgba: &[u8]) {
        self.write_region(0, 0, self.inner.width, self.inner.height, rgba);
    }

    /// Replaces the pixels from [`ImageData`] of the same size.
    pub fn replace_with(&self, data: &ImageData) {
        self.replace_pixels(&data.pixels);
    }

    /// Replaces a sub-rectangle with RGBA8 pixels (`w * h * 4` bytes).
    pub fn write_region(&self, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) {
        assert!(
            x + w <= self.inner.width && y + h <= self.inner.height,
            "region outside image"
        );
        assert_eq!(
            rgba.len(),
            (w * h * 4) as usize,
            "pixel buffer has the wrong size"
        );
        self.inner.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.inner.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(crate) fn id(&self) -> u64 {
        self.inner.id
    }

    pub(crate) fn current_bind_group(&self) -> Arc<wgpu::BindGroup> {
        self.inner.bind_group.lock().unwrap().clone()
    }
}

/// A rectangular region of an image in pixels, for sprite sheets
/// (`love.graphics.newQuad`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Quad {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Quad { x, y, w, h }
    }

    /// Splits a sprite sheet into a row-major grid of `cols x rows` quads,
    /// each `cell_w x cell_h` pixels.
    pub fn grid(cell_w: f32, cell_h: f32, cols: u32, rows: u32) -> Vec<Quad> {
        let mut quads = Vec::with_capacity((cols * rows) as usize);
        for r in 0..rows {
            for c in 0..cols {
                quads.push(Quad::new(
                    c as f32 * cell_w,
                    r as f32 * cell_h,
                    cell_w,
                    cell_h,
                ));
            }
        }
        quads
    }
}

/// CPU-side RGBA8 pixel data (`love.image.newImageData`). Generate textures
/// procedurally, then upload with [`Graphics::new_image_from_data`](super::Graphics::new_image_from_data).
#[derive(Debug, Clone)]
pub struct ImageData {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl ImageData {
    /// A transparent image of the given size.
    pub fn new(width: u32, height: u32) -> Self {
        ImageData {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    /// Wraps existing RGBA8 bytes.
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        if pixels.len() != (width * height * 4) as usize {
            return Err(Error::Image(format!(
                "expected {} bytes for a {width}x{height} image, got {}",
                width * height * 4,
                pixels.len()
            )));
        }
        Ok(ImageData {
            width,
            height,
            pixels,
        })
    }

    /// Generates every pixel with a closure `f(x, y) -> Color`.
    pub fn from_fn(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> Color) -> Self {
        let mut data = ImageData::new(width, height);
        for y in 0..height {
            for x in 0..width {
                data.set_pixel(x, y, f(x, y));
            }
        }
        data
    }

    /// Decodes an image file (PNG/JPEG depending on enabled features).
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| Error::io(path.as_ref(), e))?;
        Self::decode(&bytes)
    }

    /// Decodes an in-memory image file, e.g. from `include_bytes!`.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let img = image::load_from_memory(bytes)?.to_rgba8();
        let (width, height) = img.dimensions();
        Ok(ImageData {
            width,
            height,
            pixels: img.into_raw(),
        })
    }

    /// Writes the pixels to a PNG file.
    #[cfg(feature = "png")]
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<()> {
        image::save_buffer(
            path.as_ref(),
            &self.pixels,
            self.width,
            self.height,
            image::ColorType::Rgba8,
        )?;
        Ok(())
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        let i = ((y * self.width + x) * 4) as usize;
        let p = &self.pixels[i..i + 4];
        Color::rgba8(p[0], p[1], p[2], p[3])
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            let i = ((y * self.width + x) * 4) as usize;
            self.pixels[i..i + 4].copy_from_slice(&color.to_rgba8());
        }
    }

    /// Fills an axis-aligned rectangle (clipped to the image).
    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color) {
        let c = color.to_rgba8();
        let x0 = x.max(0) as u32;
        let y0 = y.max(0) as u32;
        let x1 = ((x + w as i32).max(0) as u32).min(self.width);
        let y1 = ((y + h as i32).max(0) as u32).min(self.height);
        for yy in y0..y1 {
            for xx in x0..x1 {
                let i = ((yy * self.width + xx) * 4) as usize;
                self.pixels[i..i + 4].copy_from_slice(&c);
            }
        }
    }

    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
}
