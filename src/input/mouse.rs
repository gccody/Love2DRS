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

/// Which buttons are held and what changed since the previous frame.
#[derive(Default)]
struct Buttons {
    down: [bool; BUTTONS],
    pressed: [bool; BUTTONS],
    released: [bool; BUTTONS],
    cancelled: [bool; BUTTONS],
}

impl Buttons {
    fn press(&mut self, i: usize) {
        if !self.down[i] {
            self.pressed[i] = true;
        }
        self.down[i] = true;
    }

    fn release(&mut self, i: usize) {
        if self.down[i] {
            self.released[i] = true;
        }
        self.down[i] = false;
    }

    fn cancel(&mut self, i: usize) {
        if self.down[i] {
            self.cancelled[i] = true;
        }
        self.down[i] = false;
    }

    fn release_all(&mut self) {
        for i in 0..BUTTONS {
            self.release(i);
        }
    }

    fn end_frame(&mut self) {
        self.pressed = [false; BUTTONS];
        self.released = [false; BUTTONS];
        self.cancelled = [false; BUTTONS];
    }
}

/// Mouse state, updated before each `update`. Reach it through `ctx.mouse`.
pub struct Mouse {
    window: Arc<Window>,
    pub(crate) position: Vec2,
    delta: Vec2,
    wheel: Vec2,
    buttons: Buttons,
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
            buttons: Buttons::default(),
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
        self.buttons.down[button.index()]
    }

    /// Whether `button` went down since the previous frame.
    pub fn was_pressed(&self, button: MouseButton) -> bool {
        self.buttons.pressed[button.index()]
    }

    /// Whether `button` was let go since the previous frame.
    pub fn was_released(&self, button: MouseButton) -> bool {
        self.buttons.released[button.index()]
    }

    /// Whether `button` stopped being held since the previous frame without being
    /// let go. This only happens on mobile, when the OS takes the finger that acts
    /// as the mouse, and doesn't count as [`was_released`](Mouse::was_released).
    pub fn was_cancelled(&self, button: MouseButton) -> bool {
        self.buttons.cancelled[button.index()]
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
        self.buttons.press(b.index());
    }

    pub(crate) fn release(&mut self, b: MouseButton) {
        self.buttons.release(b.index());
    }

    pub(crate) fn cancel(&mut self, b: MouseButton) {
        self.buttons.cancel(b.index());
    }

    pub(crate) fn release_all(&mut self) {
        self.buttons.release_all();
    }

    pub(crate) fn end_frame(&mut self) {
        self.delta = Vec2::ZERO;
        self.wheel = Vec2::ZERO;
        self.buttons.end_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_button_is_not_released() {
        let mut b = Buttons::default();
        b.press(0);
        b.end_frame();
        b.cancel(0);
        assert!(!b.down[0]);
        assert!(b.cancelled[0]);
        assert!(!b.released[0]);

        b.end_frame();
        assert!(!b.cancelled[0]);
        b.cancel(0);
        assert!(!b.cancelled[0], "wasn't down");
    }

    #[test]
    fn release_all_only_releases_what_is_down() {
        let mut b = Buttons::default();
        b.press(1);
        b.release_all();
        assert!(b.released[1]);
        assert!(!b.released[0]);
        assert!(!b.down[1]);
    }
}
