//! Drawing: shapes, images, text, canvases, shaders and transforms
//! (the `love.graphics` equivalent).
//!
//! Every draw call is batched automatically. Consecutive draws that share a
//! texture, blend mode, shader and scissor become a single GPU draw call, and
//! shapes share a texture with the default font's glyph atlas, so interleaving
//! rectangles and text does not break batches either.

mod color;
mod font;
mod image;
mod params;
mod renderer;
mod shader;
mod spritebatch;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use ab_glyph::{Font as _, ScaleFont};
use winit::window::Window as WinitWindow;

pub use color::Color;
pub use font::{Align, Font};
pub use image::{FilterMode, Image, ImageData, Quad, WrapMode};
pub use params::DrawParams;
pub use renderer::{BlendMode, FrameStats, Vertex};
pub use shader::{IntoUniform, Shader, UNIFORM_SLOTS};
pub use spritebatch::SpriteBatch;

use self::font::GlyphAtlas;
use self::image::Gpu;
use self::renderer::{
    DrawCmd, FrameData, GeomKind, Geometry, Globals, Pass, QUAD_KIND, QuadInstance, Renderer,
    ScreenTarget,
};
use crate::math::Transform;
use crate::{Conf, Error, Result};

/// Whether shapes are filled or outlined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawMode {
    /// Solid shape.
    Fill,
    /// Outline drawn with the current [line width](Graphics::set_line_width).
    Line,
}

/// An off-screen render target (`love.graphics.newCanvas`). Draw into it with
/// [`Graphics::set_canvas`] or [`Graphics::with_canvas`], then draw it like an image.
#[derive(Clone, Debug)]
pub struct Canvas {
    image: Image,
}

impl Canvas {
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.image.width()
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.image.height()
    }

    /// `(width, height)` in pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        self.image.dimensions()
    }

    /// The canvas contents as an image (shares the same texture).
    pub fn image(&self) -> &Image {
        &self.image
    }

    /// Changes how the canvas is filtered when drawn scaled.
    pub fn set_filter(&self, filter: FilterMode) {
        self.image.set_filter(filter);
    }

    /// Changes how texture coordinates outside the canvas wrap.
    pub fn set_wrap(&self, wrap: WrapMode) {
        self.image.set_wrap(wrap);
    }
}

/// Anything that can be passed to [`Graphics::draw`].
pub trait Drawable {
    /// Records this object into `g` with the given placement.
    fn draw(&self, g: &mut Graphics, params: &DrawParams);
}

impl Drawable for Image {
    #[inline]
    fn draw(&self, g: &mut Graphics, params: &DrawParams) {
        g.draw_image(self, None, params);
    }
}

impl Drawable for Canvas {
    #[inline]
    fn draw(&self, g: &mut Graphics, params: &DrawParams) {
        g.draw_image(&self.image, None, params);
    }
}

impl Drawable for SpriteBatch {
    #[inline]
    fn draw(&self, g: &mut Graphics, params: &DrawParams) {
        g.draw_sprite_batch(self, params);
    }
}

#[derive(Clone)]
struct State {
    color: Color,
    color8: [u8; 4],
    line_width: f32,
    point_size: f32,
    font: Font,
    blend: BlendMode,
    shader: Option<Shader>,
    scissor: Option<[f32; 4]>,
    transform: Transform,
    canvas: Option<Canvas>,
}

enum StackEntry {
    Transform(Transform),
    All(Box<State>),
}

/// The drawing API. Reach it through `ctx.graphics`.
pub struct Graphics {
    gpu: Arc<Gpu>,
    instance: wgpu::Instance,
    window: Arc<WinitWindow>,
    /// `None` while the app is suspended: Android takes the native window away
    /// then, and a surface can't outlive it.
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    can_screenshot: bool,
    msaa: u32,
    msaa_view: Option<wgpu::TextureView>,
    scale_factor: f32,
    minimized: bool,
    renderer: Renderer,
    atlas: GlyphAtlas,
    frame: FrameData,
    white: Image,
    white_uv: [f32; 2],
    background: Color,
    default_filter: FilterMode,
    state: State,
    stack: Vec<StackEntry>,
    /// The kind and texture id of the last command, while it can still be
    /// extended with frame geometry.
    batch: Option<(GeomKind, u64)>,
    /// Texture id for which quads can be appended without any other check
    /// (pass open, quad command with this texture open, no custom shader).
    /// 0 means "take the slow path".
    fast_tex: u64,
    pass_open: bool,
    pass_proj: Transform,
    pass_scale: f32,
    pass_globals: u32,
    user_uniforms_offset: u32,
    shader_version: u64,
    frame_index: u64,
    stats: FrameStats,
    screenshot: Option<PathBuf>,
    scratch: Vec<[f32; 2]>,
    scratch_pts: Vec<[f32; 2]>,
}

const DEFAULT_FONT_SIZE: f32 = 14.0;

impl Graphics {
    pub(crate) fn new(
        window: Arc<WinitWindow>,
        display: winit::event_loop::OwnedDisplayHandle,
        conf: &Conf,
    ) -> Result<Graphics> {
        let gfx_err = |e: &dyn std::fmt::Display| Error::Graphics(e.to_string());
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display)),
        );
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| gfx_err(&e))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| gfx_err(&e))?;
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("love2drs device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits())
                    .using_alignment(adapter.limits()),
                ..Default::default()
            }),
        )
        .map_err(|e| gfx_err(&e))?;
        let info = adapter.get_info();
        log::info!("love2drs using {} ({:?})", info.name, info.backend);

        let size = crate::window::drawable_size(&window);
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| {
                Error::Graphics("the window surface is not supported by the GPU".into())
            })?;
        // Prefer a non-sRGB format so colors blend in gamma space, like LÖVE.
        if let Some(f) = caps.formats.iter().copied().find(|f| !f.is_srgb()) {
            config.format = f;
        }
        let view_format = config.format.remove_srgb_suffix();
        if view_format != config.format {
            config.view_formats = vec![view_format];
        }
        let can_screenshot = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        if can_screenshot {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        config.present_mode = present_mode(conf.vsync);
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        let msaa = if conf.msaa > 1
            && adapter
                .get_texture_format_features(view_format)
                .flags
                .sample_count_supported(4)
        {
            4
        } else {
            1
        };

        let gpu = Arc::new(Gpu::new(device, queue));
        let renderer = Renderer::new(gpu.clone());
        let atlas = GlyphAtlas::new(gpu.clone());
        let (white, white_uv) = atlas.white();
        let font = Font::default_with_size(DEFAULT_FONT_SIZE);
        let mut g = Graphics {
            gpu,
            instance,
            window: window.clone(),
            surface: Some(surface),
            config,
            view_format,
            can_screenshot,
            msaa,
            msaa_view: None,
            scale_factor: window.scale_factor() as f32,
            minimized: size.width == 0 || size.height == 0,
            renderer,
            atlas,
            frame: FrameData::default(),
            white,
            white_uv,
            background: Color::BLACK,
            default_filter: FilterMode::Linear,
            state: State {
                color: Color::WHITE,
                color8: [255; 4],
                line_width: 1.0,
                point_size: 1.0,
                font,
                blend: BlendMode::Alpha,
                shader: None,
                scissor: None,
                transform: Transform::IDENTITY,
                canvas: None,
            },
            stack: Vec::new(),
            batch: None,
            fast_tex: 0,
            pass_open: false,
            pass_proj: Transform::IDENTITY,
            pass_scale: 1.0,
            pass_globals: 0,
            user_uniforms_offset: 0,
            shader_version: u64::MAX,
            frame_index: 0,
            stats: FrameStats::default(),
            screenshot: None,
            scratch: Vec::new(),
            scratch_pts: Vec::new(),
        };
        g.recreate_msaa();
        g.reset_frame();
        Ok(g)
    }

    // ------------------------------------------------------------------
    // Frame lifecycle (driven by the app loop)
    // ------------------------------------------------------------------

    fn reset_frame(&mut self) {
        self.frame.reset();
        // Offset 0 always holds zeroed custom-shader uniforms.
        self.push_uniform_block(&[0u8; renderer::USER_UNIFORMS_SIZE as usize]);
        self.user_uniforms_offset = 0;
        self.shader_version = u64::MAX;
        self.pass_open = false;
        self.break_batch();
    }

    /// Makes the next draw start a new command.
    #[inline]
    fn break_batch(&mut self) {
        self.batch = None;
        self.fast_tex = 0;
    }

    pub(crate) fn begin_frame(&mut self) {
        if self.atlas.trim() {
            let (white, uv) = self.atlas.white();
            self.white = white;
            self.white_uv = uv;
        }
        if self.state.canvas.is_some() {
            log::warn!(
                "a canvas was still active at the start of the frame; resetting to the screen"
            );
        }
        self.state.canvas = None;
        self.state.transform = Transform::IDENTITY;
        self.stack.clear();
        self.open_pass(Some(self.background));
    }

    pub(crate) fn end_frame(&mut self) {
        if self.state.canvas.is_some() {
            log::warn!("set_canvas(None) was not called before the end of draw()");
            self.set_canvas(None);
        }
        self.atlas.flush();
        self.frame.close_cmd();

        let mut surface_texture = None;
        let mut reconfigure = false;
        if let Some(surface) = self.surface.as_ref().filter(|_| !self.minimized) {
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) => surface_texture = Some(t),
                wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                    surface_texture = Some(t);
                    reconfigure = true;
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    reconfigure = true
                }
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Validation => {}
            }
        }

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("love2drs frame"),
            });
        let mut readback = None;
        // Canvas passes are rendered even when the window is hidden, so
        // pre-rendered canvases (e.g. drawn in `load`) are never lost.
        if let Some(st) = &surface_texture {
            let view = st.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.view_format),
                ..Default::default()
            });
            let target = ScreenTarget {
                view: self.msaa_view.as_ref().unwrap_or(&view),
                resolve: self.msaa_view.as_ref().map(|_| &view),
                format: self.view_format,
                samples: self.msaa,
                size: (self.config.width, self.config.height),
            };
            self.stats = self
                .renderer
                .render(&self.frame, Some(&target), &mut encoder);
            if let Some(path) = self.screenshot.take() {
                readback = self
                    .encode_screenshot(&st.texture, &mut encoder)
                    .map(|b| (b, path));
            }
        } else {
            self.stats = self.renderer.render(&self.frame, None, &mut encoder);
        }
        self.gpu.queue.submit([encoder.finish()]);
        if let Some(((buffer, padded_row), path)) = readback
            && let Err(e) = self.save_screenshot(&buffer, padded_row, &path)
        {
            log::error!("screenshot failed: {e}");
        }
        if let Some(st) = surface_texture {
            self.gpu.queue.present(st);
        }
        if reconfigure {
            self.configure_surface();
        }
        self.frame_index += 1;
        self.reset_frame();
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32, scale_factor: f64) {
        self.scale_factor = scale_factor as f32;
        self.minimized = width == 0 || height == 0;
        if self.minimized {
            return;
        }
        if self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.configure_surface();
            self.recreate_msaa();
        }
    }

    fn configure_surface(&self) {
        if let Some(surface) = &self.surface {
            surface.configure(&self.gpu.device, &self.config);
        }
    }

    /// Lets go of the window's surface while the app is in the background.
    pub(crate) fn suspend(&mut self) {
        self.surface = None;
    }

    /// Reconnects to the window when the app comes back. Images, canvases and
    /// shaders survive, since only the surface was lost.
    pub(crate) fn resume(&mut self) -> Result<()> {
        let surface = self
            .instance
            .create_surface(self.window.clone())
            .map_err(|e| Error::Graphics(e.to_string()))?;
        self.surface = Some(surface);
        let size = crate::window::drawable_size(&self.window);
        self.resize(size.width, size.height, self.window.scale_factor());
        self.configure_surface();
        Ok(())
    }

    fn recreate_msaa(&mut self) {
        self.msaa_view = (self.msaa > 1).then(|| {
            self.gpu
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("love2drs msaa"),
                    size: wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: self.msaa,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.view_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        });
    }

    // ------------------------------------------------------------------
    // Window-level settings
    // ------------------------------------------------------------------

    /// Screen width in logical pixels.
    pub fn width(&self) -> f32 {
        self.config.width as f32 / self.scale_factor
    }

    /// Screen height in logical pixels.
    pub fn height(&self) -> f32 {
        self.config.height as f32 / self.scale_factor
    }

    /// `(width, height)` of the screen in logical pixels.
    pub fn dimensions(&self) -> (f32, f32) {
        (self.width(), self.height())
    }

    /// Physical pixels per logical pixel (`love.graphics.getDPIScale`).
    pub fn dpi_scale(&self) -> f32 {
        self.scale_factor
    }

    /// Enables or disables vertical sync.
    pub fn set_vsync(&mut self, vsync: bool) {
        self.config.present_mode = present_mode(vsync);
        self.configure_surface();
    }

    /// Rendering statistics for the previous frame.
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// Saves the current frame as a PNG once it has finished rendering
    /// (`love.graphics.captureScreenshot`).
    pub fn capture_screenshot(&mut self, path: impl Into<PathBuf>) {
        if !self.can_screenshot {
            log::warn!("this surface does not support screenshots");
            return;
        }
        self.screenshot = Some(path.into());
    }

    fn encode_screenshot(
        &self,
        texture: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Option<(wgpu::Buffer, u32)> {
        let (w, h) = (self.config.width, self.config.height);
        let padded = (w * 4).div_ceil(256) * 256;
        let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("love2drs screenshot"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: Default::default(),
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        Some((buffer, padded))
    }

    fn save_screenshot(&self, buffer: &wgpu::Buffer, padded_row: u32, path: &Path) -> Result<()> {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| Error::Graphics(e.to_string()))?;
        let (w, h) = (self.config.width, self.config.height);
        let mapped = slice
            .get_mapped_range()
            .map_err(|e| Error::Graphics(e.to_string()))?;
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        let bgra = matches!(self.view_format, wgpu::TextureFormat::Bgra8Unorm);
        for row in 0..h {
            let start = (row * padded_row) as usize;
            for px in mapped[start..start + (w * 4) as usize].as_chunks::<4>().0 {
                if bgra {
                    pixels.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    pixels.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        drop(mapped);
        buffer.unmap();
        let data = ImageData::from_rgba(w, h, pixels)?;
        #[cfg(feature = "png")]
        {
            data.save_png(path)
        }
        #[cfg(not(feature = "png"))]
        {
            let _ = (data, path);
            Err(Error::Image("screenshots require the `png` feature".into()))
        }
    }

    // ------------------------------------------------------------------
    // Resource creation
    // ------------------------------------------------------------------

    /// Loads an image file (`love.graphics.newImage`).
    pub fn new_image(&self, path: impl AsRef<Path>) -> Result<Image> {
        Ok(self.new_image_from_data(&ImageData::load(path)?))
    }

    /// Decodes an image from memory, e.g. `include_bytes!("player.png")`.
    pub fn new_image_from_bytes(&self, bytes: &[u8]) -> Result<Image> {
        Ok(self.new_image_from_data(&ImageData::decode(bytes)?))
    }

    /// Uploads CPU pixel data as a new image.
    pub fn new_image_from_data(&self, data: &ImageData) -> Image {
        Image::create(
            &self.gpu,
            data.width(),
            data.height(),
            Some(data.pixels()),
            self.default_filter,
            false,
        )
    }

    /// Creates an image from raw RGBA8 bytes (`width * height * 4`).
    pub fn new_image_from_rgba(&self, width: u32, height: u32, rgba: &[u8]) -> Result<Image> {
        let data = ImageData::from_rgba(width, height, rgba.to_vec())?;
        Ok(self.new_image_from_data(&data))
    }

    /// Creates an off-screen render target of `width x height` pixels.
    pub fn new_canvas(&self, width: u32, height: u32) -> Canvas {
        Canvas {
            image: Image::create(&self.gpu, width, height, None, self.default_filter, true),
        }
    }

    /// Creates a retained batch of sprites using `image`.
    pub fn new_sprite_batch(&self, image: &Image, capacity: usize) -> SpriteBatch {
        SpriteBatch::new(image, capacity)
    }

    /// The bundled default font at another size.
    pub fn new_font(&self, size: f32) -> Font {
        Font::default_with_size(size)
    }

    /// Loads a TTF/OTF font file at the given size.
    pub fn new_font_from_file(&self, path: impl AsRef<Path>, size: f32) -> Result<Font> {
        Font::load(path, size)
    }

    /// Parses a TTF/OTF font from memory at the given size.
    pub fn new_font_from_bytes(&self, bytes: &[u8], size: f32) -> Result<Font> {
        Font::from_bytes(bytes.to_vec(), size)
    }

    /// Compiles a custom pixel shader. See [`Shader`] for the `effect` contract.
    pub fn new_shader(&mut self, effect_wgsl: &str) -> Result<Shader> {
        let device = &self.gpu.device;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("love2drs custom shader"),
            source: wgpu::ShaderSource::Wgsl(Renderer::shader_source(effect_wgsl).into()),
        });
        let shader = Shader::from_module(module);
        // Building a pipeline validates the shader against the bindings too.
        self.renderer
            .validate_shader(&shader.inner, self.view_format, self.msaa);
        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(Error::Graphics(format!("shader failed to compile:\n{err}")));
        }
        Ok(shader)
    }

    /// Filter mode for images and canvases created from now on.
    pub fn set_default_filter(&mut self, filter: FilterMode) {
        self.default_filter = filter;
    }

    /// The filter mode new images and canvases get.
    pub fn default_filter(&self) -> FilterMode {
        self.default_filter
    }

    // ------------------------------------------------------------------
    // State
    // ------------------------------------------------------------------

    /// Color used to clear the screen at the start of each frame.
    pub fn set_background_color(&mut self, color: impl Into<Color>) {
        self.background = color.into();
    }

    /// The clear color set with [`set_background_color`](Self::set_background_color).
    pub fn background_color(&self) -> Color {
        self.background
    }

    /// Color that tints everything drawn afterwards.
    #[inline]
    pub fn set_color(&mut self, color: impl Into<Color>) {
        let c = color.into();
        self.state.color = c;
        self.state.color8 = c.to_rgba8();
    }

    /// The current draw color.
    pub fn color(&self) -> Color {
        self.state.color
    }

    /// Width, in pixels, of lines and `Line`-mode shapes (default `1.0`).
    pub fn set_line_width(&mut self, width: f32) {
        self.state.line_width = width;
    }

    /// The current line width.
    pub fn line_width(&self) -> f32 {
        self.state.line_width
    }

    /// Size of points drawn with [`Graphics::points`], in pixels.
    pub fn set_point_size(&mut self, size: f32) {
        self.state.point_size = size;
    }

    /// The current point size.
    pub fn point_size(&self) -> f32 {
        self.state.point_size
    }

    /// Font used by [`print`](Self::print) and friends from now on.
    pub fn set_font(&mut self, font: &Font) {
        self.state.font = font.clone();
    }

    /// The current font.
    pub fn font(&self) -> &Font {
        &self.state.font
    }

    /// How subsequent draws combine with the target (default [`BlendMode::Alpha`]).
    pub fn set_blend_mode(&mut self, mode: BlendMode) {
        if self.state.blend != mode {
            self.state.blend = mode;
            self.break_batch();
        }
    }

    /// The current blend mode.
    pub fn blend_mode(&self) -> BlendMode {
        self.state.blend
    }

    /// Uses a custom shader for subsequent draws (`None` restores the default).
    pub fn set_shader(&mut self, shader: Option<&Shader>) {
        let same = match (&self.state.shader, shader) {
            (Some(a), Some(b)) => a.id() == b.id(),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.state.shader = shader.cloned();
        self.break_batch();
        self.shader_version = u64::MAX;
        if shader.is_none() {
            self.user_uniforms_offset = 0;
        }
    }

    /// The active custom shader, if any.
    pub fn shader(&self) -> Option<&Shader> {
        self.state.shader.as_ref()
    }

    /// Restricts drawing to a screen-space rectangle (ignores transforms).
    pub fn set_scissor(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.state.scissor = Some([x, y, w.max(0.0), h.max(0.0)]);
        self.break_batch();
    }

    /// Removes the scissor rectangle.
    pub fn reset_scissor(&mut self) {
        if self.state.scissor.take().is_some() {
            self.break_batch();
        }
    }

    /// The active scissor rectangle `(x, y, w, h)`, if any.
    pub fn scissor(&self) -> Option<(f32, f32, f32, f32)> {
        self.state.scissor.map(|[x, y, w, h]| (x, y, w, h))
    }

    /// Redirects drawing to a canvas, or back to the screen with `None`.
    pub fn set_canvas(&mut self, canvas: Option<&Canvas>) {
        let same = match (&self.state.canvas, canvas) {
            (Some(a), Some(b)) => a.image.id() == b.image.id(),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.state.canvas = canvas.cloned();
        self.pass_open = false;
        self.break_batch();
    }

    /// Runs `f` with drawing redirected to `canvas`, then restores the previous target
    /// (`canvas:renderTo`).
    pub fn with_canvas(&mut self, canvas: &Canvas, f: impl FnOnce(&mut Graphics)) {
        let previous = self.state.canvas.clone();
        self.set_canvas(Some(canvas));
        f(self);
        self.set_canvas(previous.as_ref());
    }

    /// The canvas being drawn into, or `None` for the screen.
    pub fn canvas(&self) -> Option<&Canvas> {
        self.state.canvas.as_ref()
    }

    /// Clears the current target (screen or canvas) to `color`.
    pub fn clear(&mut self, color: impl Into<Color>) {
        let color = color.into();
        if self.pass_open
            && self
                .frame
                .passes
                .last()
                .is_some_and(|p| p.cmd_start == self.frame.cmds.len())
        {
            self.frame.passes.last_mut().unwrap().clear = Some(color);
        } else {
            self.open_pass(Some(color));
        }
    }

    // ------------------------------------------------------------------
    // Transform stack
    // ------------------------------------------------------------------

    /// Saves the current transform (`love.graphics.push()`).
    pub fn push(&mut self) {
        self.stack.push(StackEntry::Transform(self.state.transform));
    }

    /// Saves the whole graphics state: transform, color, font, blend mode,
    /// shader, scissor, canvas, line width (`love.graphics.push("all")`).
    pub fn push_all(&mut self) {
        self.stack
            .push(StackEntry::All(Box::new(self.state.clone())));
    }

    /// Restores what the matching [`push`](Self::push) / [`push_all`](Self::push_all) saved.
    pub fn pop(&mut self) {
        match self.stack.pop() {
            Some(StackEntry::Transform(t)) => self.state.transform = t,
            Some(StackEntry::All(s)) => {
                let s = *s;
                self.set_canvas(s.canvas.as_ref());
                self.set_blend_mode(s.blend);
                self.set_shader(s.shader.as_ref());
                if s.scissor != self.state.scissor {
                    self.break_batch();
                }
                self.state = s;
            }
            None => log::warn!("Graphics::pop called without a matching push"),
        }
    }

    /// Resets the transform to identity.
    pub fn origin(&mut self) {
        self.state.transform = Transform::IDENTITY;
    }

    /// Moves the coordinate system by `(x, y)`.
    pub fn translate(&mut self, x: f32, y: f32) {
        self.state.transform.translate(x, y);
    }

    /// Rotates the coordinate system by `angle` radians (clockwise on screen).
    pub fn rotate(&mut self, angle: f32) {
        self.state.transform.rotate(angle);
    }

    /// Scales the coordinate system.
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.state.transform.scale(sx, sy);
    }

    /// Shears the coordinate system.
    pub fn shear(&mut self, kx: f32, ky: f32) {
        self.state.transform.shear(kx, ky);
    }

    /// Multiplies the current transform by `t`.
    pub fn apply_transform(&mut self, t: &Transform) {
        self.state.transform = self.state.transform.mul(t);
    }

    /// Replaces the current transform with `t`.
    pub fn replace_transform(&mut self, t: Transform) {
        self.state.transform = t;
    }

    /// The current transform.
    pub fn transform(&self) -> Transform {
        self.state.transform
    }

    /// Converts local coordinates to screen coordinates.
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        self.state.transform.apply(x, y)
    }

    /// Converts screen coordinates (e.g. the mouse) to local coordinates.
    pub fn inverse_transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        self.state
            .transform
            .inverse()
            .map_or((x, y), |t| t.apply(x, y))
    }

    // ------------------------------------------------------------------
    // Batching internals
    // ------------------------------------------------------------------

    fn push_uniform_block(&mut self, bytes: &[u8]) -> u32 {
        let align = self.renderer.uniform_align as usize;
        let offset = self.frame.uniforms.len();
        let padded = bytes.len().div_ceil(align).max(1) * align;
        self.frame.uniforms.extend_from_slice(bytes);
        self.frame.uniforms.resize(offset + padded, 0);
        offset as u32
    }

    fn push_globals(&mut self, transform: &Transform, tint: Color) -> u32 {
        let globals = Globals {
            transform: transform.to_mat4(),
            tint: tint.into(),
        };
        self.push_uniform_block(bytemuck::bytes_of(&globals))
    }

    fn open_pass(&mut self, clear: Option<Color>) {
        self.break_batch();
        let target = self.state.canvas.as_ref().map(|c| c.image.clone());
        let (w, h, scale) = match &target {
            Some(img) => (img.width() as f32, img.height() as f32, 1.0),
            None => (self.width(), self.height(), self.scale_factor),
        };
        self.pass_proj = Transform {
            a: 2.0 / w,
            b: 0.0,
            c: 0.0,
            d: -2.0 / h,
            tx: -1.0,
            ty: 1.0,
        };
        self.pass_scale = scale;
        self.pass_globals = self.push_globals(&self.pass_proj.clone(), Color::WHITE);
        self.frame.passes.push(Pass {
            target,
            clear,
            cmd_start: self.frame.cmds.len(),
        });
        self.pass_open = true;
    }

    /// Re-snapshots custom shader uniforms if they changed since the last draw.
    #[inline]
    fn sync_shader_uniforms(&mut self) {
        let Some(shader) = &self.state.shader else {
            return;
        };
        let version = shader.inner.version.load(Ordering::Relaxed);
        if version == self.shader_version {
            return;
        }
        let data = *shader.inner.uniforms.lock().unwrap();
        self.shader_version = version;
        self.user_uniforms_offset = self.push_uniform_block(bytemuck::cast_slice(&data));
        self.break_batch();
    }

    fn scissor_px(&self) -> Option<[i32; 4]> {
        let s = self.pass_scale;
        self.state.scissor.map(|[x, y, w, h]| {
            [
                (x * s).round() as i32,
                (y * s).round() as i32,
                (w * s).round() as i32,
                (h * s).round() as i32,
            ]
        })
    }

    fn new_cmd(&mut self, texture: Arc<wgpu::BindGroup>, geometry: Geometry, globals_offset: u32) {
        self.frame.close_cmd();
        self.fast_tex = 0;
        let start = match geometry {
            Geometry::Quads => self.frame.quads.len() as u32,
            Geometry::Mesh => self.frame.indices.len() as u32,
            Geometry::Retained { .. } => 0,
        };
        self.frame.cmds.push(DrawCmd {
            shader: self.state.shader.as_ref().map(|s| s.inner.clone()),
            blend: self.state.blend,
            texture,
            geometry,
            start,
            end: start,
            scissor: self.scissor_px(),
            globals_offset,
            uniforms_offset: self.user_uniforms_offset,
        });
    }

    /// Opens the pass, syncs shader uniforms and opens a command of `kind`
    /// using `tex`, unless the last command can simply be extended.
    fn begin_geometry(&mut self, kind: GeomKind, tex: &Image) {
        if !self.pass_open {
            self.open_pass(None);
        }
        if self.state.shader.is_some() {
            self.sync_shader_uniforms();
        }
        let id = tex.id();
        if self.batch != Some((kind, id)) {
            let geometry = match kind {
                GeomKind::Quads => Geometry::Quads,
                GeomKind::Mesh => Geometry::Mesh,
            };
            self.new_cmd(tex.current_bind_group(), geometry, self.pass_globals);
            self.batch = Some((kind, id));
        }
    }

    /// Slow path of [`ensure_quads`](Self::ensure_quads).
    #[inline(never)]
    fn begin_quads(&mut self, tex: &Image) {
        self.begin_geometry(GeomKind::Quads, tex);
        // With a custom shader every draw re-checks the uniforms, so only
        // shaderless drawing gets the fast path.
        if self.state.shader.is_none() {
            self.fast_tex = tex.id();
        }
    }

    /// Makes sure quad instances pushed next are drawn with `tex` and the
    /// current state. A single comparison when nothing changed.
    #[inline(always)]
    fn ensure_quads(&mut self, tex: &Image) {
        if self.fast_tex != tex.id() {
            self.begin_quads(tex);
        }
    }

    /// [`ensure_quads`](Self::ensure_quads) for the shape texture.
    #[inline(always)]
    fn ensure_white_quads(&mut self) {
        if self.fast_tex != self.white.id() {
            let white = self.white.clone();
            self.begin_quads(&white);
        }
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn push_quad(
        &mut self,
        tex: &Image,
        t: &Transform,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        uv: [f32; 4],
        color: [u8; 4],
    ) {
        self.ensure_quads(tex);
        self.frame
            .quads
            .push(quad_instance(t, x0, y0, x1, y1, uv, color));
    }

    /// Fills a convex (or star-shaped around `pts[0]`) polygon as a triangle fan.
    fn fill_fan(&mut self, pts: &[[f32; 2]]) {
        if pts.len() < 3 {
            return;
        }
        self.ensure_white_quads();
        let t = self.state.transform;
        let c = self.state.color8;
        let uv = self.white_uv;
        let apply = |p: &[f32; 2]| {
            let (x, y) = t.apply(p[0], p[1]);
            [x, y]
        };
        let first = apply(&pts[0]);
        let mut prev = apply(&pts[1]);
        self.frame.quads.reserve(pts.len() - 2);
        for p in &pts[2..] {
            let cur = apply(p);
            self.frame
                .quads
                .push(QuadInstance::triangle(first, prev, cur, uv, c));
            prev = cur;
        }
        self.frame.triangle_instances += pts.len() as u32 - 2;
    }

    /// Strokes a polyline with the current line width using miter joins.
    fn stroke(&mut self, pts: &[[f32; 2]], closed: bool) {
        let mut n = pts.len();
        if closed && n > 2 && pts[0] == pts[n - 1] {
            n -= 1;
        }
        if n < 2 {
            return;
        }
        let pts = &pts[..n];
        let hw = self.state.line_width * 0.5;
        let normal = |a: [f32; 2], b: [f32; 2]| {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            [-dy / len, dx / len]
        };
        let mut offsets = std::mem::take(&mut self.scratch);
        offsets.clear();
        for i in 0..n {
            let prev = if i > 0 {
                Some(pts[i - 1])
            } else if closed {
                Some(pts[n - 1])
            } else {
                None
            };
            let next = if i + 1 < n {
                Some(pts[i + 1])
            } else if closed {
                Some(pts[0])
            } else {
                None
            };
            let p = pts[i];
            let off = match (prev.map(|q| normal(q, p)), next.map(|q| normal(p, q))) {
                (Some(a), Some(b)) => {
                    let (mx, my) = (a[0] + b[0], a[1] + b[1]);
                    let ml = (mx * mx + my * my).sqrt();
                    if ml < 1e-4 {
                        [a[0] * hw, a[1] * hw]
                    } else {
                        let (mx, my) = (mx / ml, my / ml);
                        // Miter length, limited so very sharp corners don't spike.
                        let d = (mx * a[0] + my * a[1]).max(0.25);
                        [mx * hw / d, my * hw / d]
                    }
                }
                (Some(a), None) | (None, Some(a)) => [a[0] * hw, a[1] * hw],
                (None, None) => [0.0, 0.0],
            };
            offsets.push([p[0] + off[0], p[1] + off[1]]);
            offsets.push([p[0] - off[0], p[1] - off[1]]);
        }
        let t = self.state.transform;
        for p in offsets.iter_mut() {
            let (x, y) = t.apply(p[0], p[1]);
            *p = [x, y];
        }
        self.ensure_white_quads();
        let c = self.state.color8;
        let [u, v] = self.white_uv;
        if !closed && n == 2 {
            // A single segment is a parallelogram: one quad.
            let o = &offsets;
            self.frame.quads.push(QuadInstance {
                p0: o[0],
                p1: o[2],
                p2: o[1],
                uv: [u, v, u, v],
                color: c,
                kind: QUAD_KIND,
            });
        } else {
            let segments = if closed { n } else { n - 1 };
            self.frame.quads.reserve(segments * 2);
            for i in 0..segments {
                let j = (i + 1) % n;
                let (a0, a1, b0, b1) = (
                    offsets[i * 2],
                    offsets[i * 2 + 1],
                    offsets[j * 2],
                    offsets[j * 2 + 1],
                );
                let quads = &mut self.frame.quads;
                quads.push(QuadInstance::triangle(a0, b0, b1, [u, v], c));
                quads.push(QuadInstance::triangle(a0, b1, a1, [u, v], c));
            }
            self.frame.triangle_instances += segments as u32 * 2;
        }
        self.scratch = offsets;
    }

    fn segments_for(&self, radius: f32) -> usize {
        let r_px = radius.abs() * self.state.transform.approx_scale() * self.pass_scale.max(1.0);
        if r_px <= 0.5 {
            return 8;
        }
        let step = (1.0 - 0.25 / r_px).clamp(-1.0, 1.0).acos();
        ((std::f32::consts::TAU / step).ceil() as usize).clamp(8, 512)
    }

    #[inline]
    fn ensure_pass(&mut self) {
        if !self.pass_open {
            self.open_pass(None);
        }
    }

    /// Builds a temporary point list (reusing an allocation) and draws it.
    fn with_points(
        &mut self,
        build: impl FnOnce(&mut Vec<[f32; 2]>),
        draw: impl FnOnce(&mut Self, &[[f32; 2]]),
    ) {
        let mut pts = std::mem::take(&mut self.scratch_pts);
        pts.clear();
        build(&mut pts);
        draw(self, &pts);
        self.scratch_pts = pts;
    }

    fn shape(&mut self, mode: DrawMode, pts: &[[f32; 2]]) {
        match mode {
            DrawMode::Fill => self.fill_fan(pts),
            DrawMode::Line => self.stroke(pts, true),
        }
    }

    // ------------------------------------------------------------------
    // Shapes
    // ------------------------------------------------------------------

    /// Draws a rectangle with its top-left corner at `(x, y)`.
    #[inline]
    pub fn rectangle(&mut self, mode: DrawMode, x: f32, y: f32, w: f32, h: f32) {
        match mode {
            DrawMode::Fill => {
                self.ensure_white_quads();
                let [u, v] = self.white_uv;
                let q = quad_instance(
                    &self.state.transform,
                    x,
                    y,
                    x + w,
                    y + h,
                    [u, v, u, v],
                    self.state.color8,
                );
                self.frame.quads.push(q);
            }
            DrawMode::Line => self.stroke(&[[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true),
        }
    }

    /// Draws a rectangle with rounded corners of radius `r`.
    pub fn rounded_rectangle(&mut self, mode: DrawMode, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.ensure_pass();
        let r = r.min(w.abs() * 0.5).min(h.abs() * 0.5).max(0.0);
        if r <= 0.0 {
            return self.rectangle(mode, x, y, w, h);
        }
        let per_corner = (self.segments_for(r) / 4).max(2);
        self.with_points(
            |pts| {
                let corners = [
                    (x + w - r, y + r, 1.5),
                    (x + w - r, y + h - r, 0.0),
                    (x + r, y + h - r, 0.5),
                    (x + r, y + r, 1.0),
                ];
                for (cx, cy, start) in corners {
                    for i in 0..=per_corner {
                        let a = (start + 0.5 * i as f32 / per_corner as f32) * std::f32::consts::PI;
                        pts.push([cx + r * a.cos(), cy + r * a.sin()]);
                    }
                }
            },
            |g, pts| g.shape(mode, pts),
        );
    }

    /// Draws a circle centered at `(x, y)`.
    pub fn circle(&mut self, mode: DrawMode, x: f32, y: f32, radius: f32) {
        self.ellipse(mode, x, y, radius, radius);
    }

    /// Draws an ellipse centered at `(x, y)`.
    pub fn ellipse(&mut self, mode: DrawMode, x: f32, y: f32, rx: f32, ry: f32) {
        self.ensure_pass();
        let n = self.segments_for(rx.max(ry));
        self.with_points(
            |pts| {
                let step = std::f32::consts::TAU / n as f32;
                pts.extend((0..n).map(|i| {
                    let (s, c) = (i as f32 * step).sin_cos();
                    [x + rx * c, y + ry * s]
                }));
            },
            |g, pts| g.shape(mode, pts),
        );
    }

    /// Draws an arc from angle `a1` to `a2` (radians). `Fill` draws a pie slice,
    /// `Line` the open arc.
    pub fn arc(&mut self, mode: DrawMode, x: f32, y: f32, radius: f32, a1: f32, a2: f32) {
        self.ensure_pass();
        let span = a2 - a1;
        let n = ((self.segments_for(radius) as f32 * span.abs() / std::f32::consts::TAU).ceil()
            as usize)
            .max(2);
        self.with_points(
            |pts| {
                if mode == DrawMode::Fill {
                    pts.push([x, y]);
                }
                pts.extend((0..=n).map(|i| {
                    let a = a1 + span * i as f32 / n as f32;
                    [x + radius * a.cos(), y + radius * a.sin()]
                }));
            },
            |g, pts| match mode {
                DrawMode::Fill => g.fill_fan(pts),
                DrawMode::Line => g.stroke(pts, false),
            },
        );
    }

    /// Draws a polygon from a flat `[x1, y1, x2, y2, ...]` list. Filled
    /// polygons must be convex (as in LÖVE).
    pub fn polygon(&mut self, mode: DrawMode, points: &[f32]) {
        self.with_points(
            |pts| pts.extend(points.as_chunks::<2>().0.iter().map(|p| [p[0], p[1]])),
            |g, pts| g.shape(mode, pts),
        );
    }

    /// Draws a triangle through three points.
    #[allow(clippy::too_many_arguments)]
    pub fn triangle(
        &mut self,
        mode: DrawMode,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    ) {
        self.shape(mode, &[[x1, y1], [x2, y2], [x3, y3]]);
    }

    /// Draws a line segment.
    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.stroke(&[[x1, y1], [x2, y2]], false);
    }

    /// Draws connected line segments through a flat `[x1, y1, x2, y2, ...]` list.
    pub fn polyline(&mut self, points: &[f32]) {
        self.with_points(
            |pts| {
                for p in points.as_chunks::<2>().0 {
                    if pts.last() != Some(&[p[0], p[1]]) {
                        pts.push([p[0], p[1]]);
                    }
                }
            },
            |g, pts| g.stroke(pts, false),
        );
    }

    /// Draws a single point of the current point size.
    pub fn point(&mut self, x: f32, y: f32) {
        self.points(&[x, y]);
    }

    /// Draws square points at a flat `[x1, y1, x2, y2, ...]` list.
    pub fn points(&mut self, points: &[f32]) {
        let points = points.as_chunks::<2>().0;
        if points.is_empty() {
            return;
        }
        self.ensure_white_quads();
        let [u, v] = self.white_uv;
        let half = self.state.point_size * 0.5;
        let c = self.state.color8;
        let t = self.state.transform;
        self.frame.quads.extend(points.iter().map(|p| {
            let (x, y) = t.apply(p[0], p[1]);
            QuadInstance {
                p0: [x - half, y - half],
                p1: [x + half, y - half],
                p2: [x - half, y + half],
                uv: [u, v, u, v],
                color: c,
                kind: QUAD_KIND,
            }
        }));
    }

    /// Draws arbitrary triangles. Vertex positions are transformed by the
    /// current transform; colors are multiplied by nothing (use them directly).
    /// Without `indices`, every three vertices form a triangle. Without an
    /// `image`, texture coordinates are ignored and the mesh is solid.
    pub fn mesh(&mut self, vertices: &[Vertex], indices: Option<&[u32]>, image: Option<&Image>) {
        let tex = image.cloned().unwrap_or_else(|| self.white.clone());
        self.begin_geometry(GeomKind::Mesh, &tex);
        let base = self.frame.vertices.len() as u32;
        let t = self.state.transform;
        let white_uv = image.is_none().then_some(self.white_uv);
        self.frame.vertices.extend(vertices.iter().map(|v| {
            let (x, y) = t.apply(v.pos[0], v.pos[1]);
            Vertex {
                pos: [x, y],
                uv: white_uv.unwrap_or(v.uv),
                color: v.color,
            }
        }));
        match indices {
            Some(idx) => self.frame.indices.extend(idx.iter().map(|i| base + i)),
            None => self
                .frame
                .indices
                .extend(base..base + (vertices.len() / 3 * 3) as u32),
        }
    }

    // ------------------------------------------------------------------
    // Images
    // ------------------------------------------------------------------

    /// Draws an image, canvas or sprite batch with its top-left at `(x, y)`.
    #[inline]
    pub fn draw(&mut self, drawable: &impl Drawable, x: f32, y: f32) {
        drawable.draw(self, &DrawParams::at(x, y));
    }

    /// Draws with full placement control (rotation, scale, origin, color).
    #[inline]
    pub fn draw_ex(&mut self, drawable: &impl Drawable, params: impl Into<DrawParams>) {
        drawable.draw(self, &params.into());
    }

    /// Draws a sub-rectangle of an image (sprite sheets).
    #[inline]
    pub fn draw_quad(&mut self, image: &Image, quad: Quad, x: f32, y: f32) {
        self.draw_image(image, Some(quad), &DrawParams::at(x, y));
    }

    /// Draws a sub-rectangle of an image with full placement control.
    #[inline]
    pub fn draw_quad_ex(&mut self, image: &Image, quad: Quad, params: impl Into<DrawParams>) {
        self.draw_image(image, Some(quad), &params.into());
    }

    #[inline(always)]
    fn draw_image(&mut self, image: &Image, quad: Option<Quad>, params: &DrawParams) {
        let inner = &*image.inner;
        if inner.is_canvas && self.is_current_canvas(image) {
            return;
        }
        let (iw, ih) = (inner.width as f32, inner.height as f32);
        let (w, h, uv) = match quad {
            None => (iw, ih, [0.0, 0.0, 1.0, 1.0]),
            Some(q) => (
                q.w,
                q.h,
                [q.x / iw, q.y / ih, (q.x + q.w) / iw, (q.y + q.h) / ih],
            ),
        };
        let local = params.to_transform(w, h);
        let t = if self.state.transform == Transform::IDENTITY {
            local
        } else {
            self.state.transform.mul(&local)
        };
        let color = params.color.map_or(self.state.color8, Color::to_rgba8);
        self.ensure_quads(image);
        self.frame.quads.push(QuadInstance {
            p0: [t.tx, t.ty],
            p1: [t.a * w + t.tx, t.b * w + t.ty],
            p2: [t.c * h + t.tx, t.d * h + t.ty],
            uv,
            color,
            kind: QUAD_KIND,
        });
    }

    /// True (with a warning) if `image` is the canvas being drawn into.
    #[cold]
    #[inline(never)]
    fn is_current_canvas(&self, image: &Image) -> bool {
        let same = self
            .state
            .canvas
            .as_ref()
            .is_some_and(|c| c.image.id() == image.id());
        if same {
            log::warn!("a canvas cannot be drawn into itself; skipping");
        }
        same
    }

    fn draw_sprite_batch(&mut self, batch: &SpriteBatch, params: &DrawParams) {
        if !self.pass_open {
            self.open_pass(None);
        }
        self.sync_shader_uniforms();
        let Some((instances, count)) = batch.prepare(&self.gpu, self.frame_index) else {
            return;
        };
        let (w, h) = batch.image().dimensions();
        let model = self
            .state
            .transform
            .mul(&params.to_transform(w as f32, h as f32));
        let mvp = self.pass_proj.mul(&model);
        let tint = params.color.unwrap_or(self.state.color);
        let globals = self.push_globals(&mvp, tint);
        self.new_cmd(
            batch.image().current_bind_group(),
            Geometry::Retained { instances },
            globals,
        );
        let cmd = self.frame.cmds.last_mut().unwrap();
        cmd.start = 0;
        cmd.end = count;
        self.break_batch();
    }

    // ------------------------------------------------------------------
    // Text
    // ------------------------------------------------------------------

    /// Draws text with the current font; `(x, y)` is the top-left of the first line.
    #[inline]
    pub fn print(&mut self, text: &str, x: f32, y: f32) {
        self.print_impl(text, &DrawParams::at(x, y), None);
    }

    /// Draws text with full placement control (anchor is relative to the text block).
    pub fn print_ex(&mut self, text: &str, params: impl Into<DrawParams>) {
        self.print_impl(text, &params.into(), None);
    }

    /// Draws word-wrapped text in a box `limit` wide, aligned within it (`love.graphics.printf`).
    pub fn printf(&mut self, text: &str, x: f32, y: f32, limit: f32, align: Align) {
        self.print_impl(text, &DrawParams::at(x, y), Some((limit, align)));
    }

    fn print_impl(&mut self, text: &str, params: &DrawParams, wrap: Option<(f32, Align)>) {
        if text.is_empty() {
            return;
        }
        if !self.pass_open {
            self.open_pass(None);
        }
        let font = self.state.font.clone();
        let face = font.face.clone();
        let line_advance = font.line_advance();

        let wrapped;
        let lines: Vec<&str> = match wrap {
            Some((limit, _)) => {
                wrapped = font.wrap(text, limit);
                wrapped.iter().map(String::as_str).collect()
            }
            None => text.split('\n').collect(),
        };
        let block = if params.anchor.is_some() {
            (
                wrap.map_or_else(|| font.width(text), |w| w.0),
                lines.len() as f32 * line_advance,
            )
        } else {
            (0.0, 0.0)
        };
        let mut t = self
            .state
            .transform
            .mul(&params.to_transform(block.0, block.1));

        // Rasterize at the size the text will actually cover on screen, so
        // scaled text stays crisp. Sizes are quantized to whole pixels.
        let px = (font.size() * self.pass_scale * t.approx_scale())
            .round()
            .clamp(1.0, 512.0);
        let dpi = px / font.size();
        let snap = t.is_axis_aligned_unit();
        if snap {
            t.tx = (t.tx * dpi).round() / dpi;
            t.ty = (t.ty * dpi).round() / dpi;
        }
        let scale = face.scale_for(px);
        let scaled = face.font.as_scaled(scale);
        let ascent = scaled.ascent().round();
        let color = params.color.map_or(self.state.color8, Color::to_rgba8);

        let mut page_img: Option<(usize, Image)> = None;
        for (li, line) in lines.iter().enumerate() {
            let x_off = match wrap {
                Some((limit, Align::Center)) => (limit - font.width(line)) * 0.5,
                Some((limit, Align::Right)) => limit - font.width(line),
                _ => 0.0,
            };
            let x_off = (x_off * dpi).round();
            let baseline = (li as f32 * line_advance * dpi).round() + ascent;
            let mut pen = 0.0f32;
            let mut prev = None;
            for ch in line.chars() {
                let id = scaled.glyph_id(ch);
                if let Some(p) = prev {
                    pen += scaled.kern(p, id);
                }
                prev = Some(id);
                if let Some(e) = self.atlas.glyph(&face, id, scale) {
                    if page_img.as_ref().is_none_or(|(p, _)| *p != e.page) {
                        page_img = Some((e.page, self.atlas.page_image(e.page).clone()));
                    }
                    let img = &page_img.as_ref().unwrap().1;
                    let gx = (x_off + pen.round() + e.offset[0]) / dpi;
                    let gy = (baseline + e.offset[1]) / dpi;
                    self.push_quad(
                        img,
                        &t,
                        gx,
                        gy,
                        gx + e.size[0] / dpi,
                        gy + e.size[1] / dpi,
                        e.uv,
                        color,
                    );
                }
                pen += scaled.h_advance(id);
            }
        }
    }
}

fn present_mode(vsync: bool) -> wgpu::PresentMode {
    if vsync {
        wgpu::PresentMode::AutoVsync
    } else {
        wgpu::PresentMode::AutoNoVsync
    }
}

/// The local rectangle `(x0, y0)-(x1, y1)` under `t` as a quad instance.
#[inline(always)]
pub(crate) fn quad_instance(
    t: &Transform,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    uv: [f32; 4],
    color: [u8; 4],
) -> QuadInstance {
    let (x0a, x0b) = (t.a * x0, t.b * x0);
    let (x1a, x1b) = (t.a * x1, t.b * x1);
    let (y0c, y0d) = (t.c * y0 + t.tx, t.d * y0 + t.ty);
    let (y1c, y1d) = (t.c * y1 + t.tx, t.d * y1 + t.ty);
    QuadInstance {
        p0: [x0a + y0c, x0b + y0d],
        p1: [x1a + y0c, x1b + y0d],
        p2: [x0a + y1c, x0b + y1d],
        uv,
        color,
        kind: QUAD_KIND,
    }
}
