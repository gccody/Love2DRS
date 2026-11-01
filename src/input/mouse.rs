//! Mouse state (the `love.mouse` equivalent).

use std::sync::Arc;

use winit::window::{CursorGrabMode, Window};

use crate::math::{Vec2, vec2};

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Primary (usually left) button.
    Left,
    /// Secondary (usually right) button.
    Right,
    /// Middle button / wheel click.
    Middle,
    /// "Back" side button.
    Back,
    /// "Forward" side button.
    Forward,
    /// Any other button, by platform index.
    Other(u16),
}

impl MouseButton {
    pub(crate) fn from_winit(b: winit::event::MouseButton) -> Self {
        use winit::event::MouseButton as W;
        match b {
            W::Left => MouseButton::Left,
            W::Right => MouseButton::Right,
            W::Middle => MouseButton::Middle,
            W::Back => MouseButton::Back,
            W::Forward => MouseButton::Forward,
            W::Other(n) => MouseButton::Other(n),
        }
    }

    fn index(self) -> usize {
        match self {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            MouseButton::Back => 3,
            MouseButton::Forward => 4,
            MouseButton::Other(n) => 5 + (n as usize).min(10),
        }
    }
}

const BUTTONS: usize = 16;

/// Mouse state, updated before each `update`. Reach it through `ctx.mouse`.
pub struct Mouse {
    window: Arc<Window>,
    pub(crate) position: Vec2,
    delta: Vec2,
    wheel: Vec2,
    down: [bool; BUTTONS],
    pressed: [bool; BUTTONS],
    released: [bool; BUTTONS],
    visible: bool,
    grabbed: bool,
    pub(crate) inside: bool,
}

impl Mouse {
    pub(crate) fn new(window: Arc<Window>) -> Self {
        Mouse {
            window,
            position: Vec2::ZERO,
            delta: Vec2::ZERO,
            wheel: Vec2::ZERO,
            down: [false; BUTTONS],
            pressed: [false; BUTTONS],
            released: [false; BUTTONS],
            visible: true,
            grabbed: false,
            inside: false,
        }
    }

    /// Cursor x in logical pixels.
    pub fn x(&self) -> f32 {
        self.position.x
    }

    /// Cursor y in logical pixels.
    pub fn y(&self) -> f32 {
        self.position.y
    }

    /// Cursor position in logical pixels.
    pub fn position(&self) -> Vec2 {
        self.position
    }

    /// How far the cursor moved since the previous frame.
    pub fn delta(&self) -> Vec2 {
        self.delta
    }

    /// Wheel movement since the previous frame (`y > 0` is away from the user).
    pub fn wheel(&self) -> Vec2 {
        self.wheel
    }

    /// Whether `button` is currently held down.
    pub fn is_down(&self, button: MouseButton) -> bool {
        self.down[button.index()]
    }

    /// Whether `button` went down since the previous frame.
    pub fn was_pressed(&self, button: MouseButton) -> bool {
        self.pressed[button.index()]
    }

    /// Whether `button` was let go since the previous frame.
    pub fn was_released(&self, button: MouseButton) -> bool {
        self.released[button.index()]
    }

    /// Whether the cursor is over the window.
    pub fn is_inside(&self) -> bool {
        self.inside
    }

    /// Shows or hides the cursor while it is over the window.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.window.set_cursor_visible(visible);
    }

    /// Whether the cursor is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Confines the cursor to the window.
    pub fn set_grabbed(&mut self, grabbed: bool) {
        self.grabbed = grabbed;
        let mode = if grabbed {
            CursorGrabMode::Confined
        } else {
            CursorGrabMode::None
        };
        let result = self.window.set_cursor_grab(mode);
        if grabbed && result.is_err() {
            let _ = self.window.set_cursor_grab(CursorGrabMode::Locked);
        }
    }

    /// Whether the cursor is confined to the window.
    pub fn is_grabbed(&self) -> bool {
        self.grabbed
    }

    /// Moves the cursor (logical pixels).
    pub fn set_position(&mut self, x: f32, y: f32) {
        let _ = self
            .window
            .set_cursor_position(winit::dpi::LogicalPosition::new(x, y));
        self.position = vec2(x, y);
    }

    pub(crate) fn moved(&mut self, pos: Vec2) -> Vec2 {
        let d = pos - self.position;
        self.delta += d;
        self.position = pos;
        d
    }

    pub(crate) fn scrolled(&mut self, d: Vec2) {
        self.wheel += d;
    }

    pub(crate) fn press(&mut self, b: MouseButton) {
        let i = b.index();
        if !self.down[i] {
            self.pressed[i] = true;
        }
        self.down[i] = true;
    }

    pub(crate) fn release(&mut self, b: MouseButton) {
        let i = b.index();
        if self.down[i] {
            self.released[i] = true;
        }
        self.down[i] = false;
    }

    pub(crate) fn release_all(&mut self) {
        for i in 0..BUTTONS {
            if self.down[i] {
                self.released[i] = true;
                self.down[i] = false;
            }
        }
    }

    pub(crate) fn end_frame(&mut self) {
        self.delta = Vec2::ZERO;
        self.wheel = Vec2::ZERO;
        self.pressed = [false; BUTTONS];
        self.released = [false; BUTTONS];
    }
}
