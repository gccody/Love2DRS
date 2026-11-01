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

    pub fn title(&self) -> &str {
        &self.title
    }

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

    pub fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }

    /// Toggles borderless fullscreen on the current monitor.
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.inner
            .set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
    }

    pub fn is_fullscreen(&self) -> bool {
        self.inner.fullscreen().is_some()
    }

    pub fn set_resizable(&mut self, resizable: bool) {
        self.inner.set_resizable(resizable);
    }

    pub fn has_focus(&self) -> bool {
        self.inner.has_focus()
    }

    pub fn minimize(&mut self) {
        self.inner.set_minimized(true);
    }

    pub fn maximize(&mut self) {
        self.inner.set_maximized(true);
    }

    /// Escape hatch to the underlying winit window.
    pub fn winit(&self) -> &WinitWindow {
        &self.inner
    }
}
