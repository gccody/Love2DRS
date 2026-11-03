//! Window control (the `love.window` equivalent).

use std::sync::Arc;

use winit::window::{Fullscreen, Window as WinitWindow};

/// The size of the area a game draws to, in physical pixels. On iOS winit's
/// `inner_size` leaves out the safe area, but the surface covers the whole view.
pub(crate) fn drawable_size(window: &WinitWindow) -> winit::dpi::PhysicalSize<u32> {
    if cfg!(target_os = "ios") {
        window.outer_size()
    } else {
        window.inner_size()
    }
}

/// What takes a [`ReservedRegion`] out of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservedKind {
    /// A fold: content should sit on either side of it.
    Division,
    /// Hardware such as a camera covering the window.
    Occlusion,
}

/// A part of the window that a foldable's hardware has reserved.
/// See [`Window::reserved_regions`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReservedRegion {
    /// What reserved it.
    pub kind: ReservedKind,
    /// Left edge, in logical pixels from the window's left.
    pub x: f32,
    /// Top edge, in logical pixels from the window's top.
    pub y: f32,
    /// Width in logical pixels.
    pub width: f32,
    /// Height in logical pixels.
    pub height: f32,
}

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
        let s = drawable_size(&self.inner).to_logical::<f32>(self.inner.scale_factor());
        (s.width, s.height)
    }

    /// The part of the window that notches, rounded corners, system bars and the
    /// on-screen keyboard don't cover, as `(x, y, width, height)` in logical pixels
    /// (`love.window.getSafeArea`). On desktop, and on Android before version 11,
    /// this is the whole window. It changes when the device rotates.
    pub fn safe_area(&self) -> (f32, f32, f32, f32) {
        let scale = self.inner.scale_factor();
        #[cfg(target_os = "ios")]
        if let Ok(pos) = self.inner.inner_position() {
            let pos = pos.to_logical::<f32>(scale);
            let size = self.inner.inner_size().to_logical::<f32>(scale);
            let height = match self.ios_keyboard_top() {
                Some(top) => size.height.min(top - pos.y),
                None => size.height,
            };
            return (pos.x, pos.y, size.width, height);
        }
        #[cfg(target_os = "android")]
        if let Some((left, top, right, bottom)) = crate::platform::window_insets() {
            let size = self.inner.inner_size();
            let width = size.width as i32 - left - right;
            let height = size.height as i32 - top - bottom;
            let logical = |v: i32| (v as f64 / scale) as f32;
            return (logical(left), logical(top), logical(width), logical(height));
        }
        let _ = scale;
        let (w, h) = self.size();
        (0.0, 0.0, w, h)
    }

    /// What a foldable's fold and cameras take out of the window right now, in
    /// logical pixels. A fold shows up only while the device is partly open, and a
    /// camera only while it covers the window. The frames include the margins the OS
    /// asks interactive content to keep. Empty everywhere but iOS 27.1 and up.
    pub fn reserved_regions(&self) -> Vec<ReservedRegion> {
        #[cfg(target_os = "ios")]
        return self.ios_reserved_regions();
        #[cfg(not(target_os = "ios"))]
        Vec::new()
    }

    #[cfg(target_os = "ios")]
    fn ios_reserved_regions(&self) -> Vec<ReservedRegion> {
        use objc2::runtime::{AnyClass, AnyObject};
        use objc2::{msg_send, rc::autoreleasepool};
        use objc2_foundation::NSRect;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Ok(handle) = self.inner.window_handle() else {
            return Vec::new();
        };
        let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
            return Vec::new();
        };
        // The class is missing before iOS 27.1, which is how older systems opt out.
        let Some(kinds) = AnyClass::get(c"UIViewReservedRegionKind") else {
            return Vec::new();
        };
        let mut regions = Vec::new();
        // SAFETY: the handle's view lives as long as the window, and these messages
        // exist wherever the class above does.
        autoreleasepool(|_| unsafe {
            let view: &AnyObject = handle.ui_view.cast::<AnyObject>().as_ref();
            let division: *mut AnyObject = msg_send![kinds, divisionRegionKind];
            let occlusion: *mut AnyObject = msg_send![kinds, occlusionRegionKind];
            for (ios_kind, kind) in [(division, ReservedKind::Division), (occlusion, ReservedKind::Occlusion)] {
                let found: *mut AnyObject = msg_send![view, reservedRegionsOfKind: ios_kind];
                let count: usize = msg_send![found, count];
                for i in 0..count {
                    let region: *mut AnyObject = msg_send![found, objectAtIndex: i];
                    let frame: NSRect = msg_send![region, frame];
                    regions.push(ReservedRegion {
                        kind,
                        x: frame.origin.x as f32,
                        y: frame.origin.y as f32,
                        width: frame.size.width as f32,
                        height: frame.size.height as f32,
                    });
                }
            }
        });
        regions
    }

    /// The top of the on-screen keyboard's area, in logical pixels from the top of
    /// the window. Without a keyboard it's the bottom of the safe area.
    #[cfg(target_os = "ios")]
    fn ios_keyboard_top(&self) -> Option<f32> {
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, sel};
        use objc2_foundation::NSRect;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let handle = self.inner.window_handle().ok()?;
        let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
            return None;
        };
        // SAFETY: the handle's view lives as long as the window, and the guide is
        // only used where UIKit has it (iOS 15 and up).
        unsafe {
            let view: &AnyObject = handle.ui_view.cast::<AnyObject>().as_ref();
            let has_guide: bool = msg_send![view, respondsToSelector: sel!(keyboardLayoutGuide)];
            if !has_guide {
                return None;
            }
            let guide: *mut AnyObject = msg_send![view, keyboardLayoutGuide];
            let frame: NSRect = msg_send![guide, layoutFrame];
            Some(frame.origin.y as f32)
        }
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
