//! Window control (the `love.window` equivalent).

use std::sync::Arc;

use winit::window::{Fullscreen, Window as WinitWindow};

/// The game window. Reach it through `ctx.window`.
pub struct Window {
    pub(crate) inner: Arc<WinitWindow>,
    title: String,
}

impl Window {
    pub(crate) fn new(inner: Arc<WinitWindow>, title: String) -> Self {
        Window { inner, title }
    }

    /// The current window title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Changes the window title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
        self.inner.set_title(&self.title);
    }

    /// Inner size in logical pixels.
    pub fn size(&self) -> (f32, f32) {
        let s = self
            .inner
            .inner_size()
            .to_logical::<f32>(self.inner.scale_factor());
        (s.width, s.height)
    }

    /// Requests a new inner size in logical pixels.
    pub fn set_size(&mut self, width: u32, height: u32) {
        let _ = self
            .inner
            .request_inner_size(winit::dpi::LogicalSize::new(width, height));
    }

    /// Physical pixels per logical pixel (e.g. `2.0` on a Retina display).
    pub fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }

    /// Toggles borderless fullscreen on the current monitor.
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.inner
            .set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
    }

    /// Whether the window is currently fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.inner.fullscreen().is_some()
    }

    /// Allows or forbids the user resizing the window.
    pub fn set_resizable(&mut self, resizable: bool) {
        self.inner.set_resizable(resizable);
    }

    /// Whether the window has keyboard focus.
    pub fn has_focus(&self) -> bool {
        self.inner.has_focus()
    }

    /// Minimizes (iconifies) the window.
    pub fn minimize(&mut self) {
        self.inner.set_minimized(true);
    }

    /// Whether the window is minimized. While it is, the game keeps running
    /// but frames are limited to 30 per second.
    pub fn is_minimized(&self) -> bool {
        // Wayland can't report minimization; a zero-sized window counts too
        // (that is how Windows reports it).
        let size = self.inner.inner_size();
        self.inner.is_minimized().unwrap_or(false) || size.width == 0 || size.height == 0
    }

    /// Maximizes the window.
    pub fn maximize(&mut self) {
        self.inner.set_maximized(true);
    }

    /// Escape hatch to the underlying winit window.
    pub fn winit(&self) -> &WinitWindow {
        &self.inner
    }
}
