//! Touchscreen state (the `love.touch` equivalent).
//!
//! Touch events come from touchscreens on Windows and Linux (X11 and Wayland).
//! macOS has no touchscreens; trackpad gestures arrive as mouse and wheel
//! events, so `ctx.touch` stays empty there.
//!
//! Operating systems also turn the first finger into emulated mouse events for
//! older programs, so a tap usually reaches both `touch_pressed` and
//! `mouse_pressed`. Handle one or the other for the same action.

use crate::math::Vec2;

/// Identifies one finger for as long as it stays on the screen. Ids may be
/// reused for later touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TouchId(pub(crate) u64);

impl TouchId {
    /// The id as a plain number.
    pub fn index(self) -> u64 {
        self.0
    }
}

/// A finger on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchPoint {
    /// Which finger this is.
    pub id: TouchId,
    /// Current position in logical pixels.
    pub position: Vec2,
    /// Where the finger first touched down, in logical pixels.
    pub start: Vec2,
    /// How far the finger moved since the previous frame.
    pub delta: Vec2,
    /// Pressure from 0 to 1, or 1 when the hardware doesn't report pressure.
    pub pressure: f32,
}

/// Touchscreen state, updated before each `update`. Reach it through `ctx.touch`.
#[derive(Default)]
pub struct Touch {
    active: Vec<TouchPoint>,
    pressed: Vec<TouchId>,
    released: Vec<TouchPoint>,
}

impl Touch {
    pub(crate) fn new() -> Self {
        Touch::default()
    }

    /// Every finger currently on the screen, oldest first.
    pub fn touches(&self) -> &[TouchPoint] {
        &self.active
    }

    /// How many fingers are on the screen.
    pub fn count(&self) -> usize {
        self.active.len()
    }

    /// The finger with this id, if it is still on the screen.
    pub fn get(&self, id: TouchId) -> Option<&TouchPoint> {
        self.active.iter().find(|t| t.id == id)
    }

    /// Where the finger with this id is, if it is still on the screen.
    pub fn position(&self, id: TouchId) -> Option<Vec2> {
        self.get(id).map(|t| t.position)
    }

    /// Whether this finger touched down since the previous frame.
    pub fn was_pressed(&self, id: TouchId) -> bool {
        self.pressed.contains(&id)
    }

    /// Fingers that touched down since the previous frame.
    pub fn pressed(&self) -> impl Iterator<Item = &TouchPoint> {
        self.active.iter().filter(|t| self.pressed.contains(&t.id))
    }

    /// Fingers lifted since the previous frame, with their last position.
    /// Compare `position` with `start` to tell taps from swipes.
    pub fn released(&self) -> &[TouchPoint] {
        &self.released
    }

    pub(crate) fn begin(&mut self, id: TouchId, pos: Vec2, pressure: f32) {
        self.active.retain(|t| t.id != id);
        self.active.push(TouchPoint {
            id,
            position: pos,
            start: pos,
            delta: Vec2::ZERO,
            pressure,
        });
        self.pressed.push(id);
    }

    /// Moves a finger and returns how far it went, or `None` for an unknown id.
    pub(crate) fn moved(&mut self, id: TouchId, pos: Vec2, pressure: f32) -> Option<Vec2> {
        let t = self.active.iter_mut().find(|t| t.id == id)?;
        let d = pos - t.position;
        t.position = pos;
        t.delta += d;
        t.pressure = pressure;
        Some(d)
    }

    /// Lifts a finger, returning whether it was down.
    pub(crate) fn end(&mut self, id: TouchId, pos: Vec2, pressure: f32) -> bool {
        let Some(i) = self.active.iter().position(|t| t.id == id) else {
            return false;
        };
        let mut t = self.active.remove(i);
        t.delta += pos - t.position;
        t.position = pos;
        t.pressure = pressure;
        self.released.push(t);
        true
    }

    pub(crate) fn release_all(&mut self) {
        self.released.append(&mut self.active);
    }

    pub(crate) fn end_frame(&mut self) {
        for t in &mut self.active {
            t.delta = Vec2::ZERO;
        }
        self.pressed.clear();
        self.released.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vec2;

    #[test]
    fn touch_lifecycle() {
        let mut t = Touch::new();
        let id = TouchId(7);
        t.begin(id, vec2(10.0, 20.0), 0.5);
        assert_eq!(t.count(), 1);
        assert!(t.was_pressed(id));
        assert_eq!(t.pressed().count(), 1);

        assert_eq!(t.moved(id, vec2(15.0, 18.0), 0.6), Some(vec2(5.0, -2.0)));
        assert_eq!(t.moved(id, vec2(16.0, 18.0), 0.6), Some(vec2(1.0, 0.0)));
        assert_eq!(t.get(id).unwrap().delta, vec2(6.0, -2.0));
        assert_eq!(t.moved(TouchId(99), vec2(0.0, 0.0), 1.0), None);

        t.end_frame();
        assert!(!t.was_pressed(id));
        assert_eq!(t.get(id).unwrap().delta, Vec2::ZERO);

        assert!(t.end(id, vec2(30.0, 18.0), 0.0));
        assert!(!t.end(id, vec2(30.0, 18.0), 0.0));
        assert_eq!(t.count(), 0);
        let r = t.released()[0];
        assert_eq!(
            (r.start, r.position, r.delta),
            (vec2(10.0, 20.0), vec2(30.0, 18.0), vec2(14.0, 0.0))
        );

        t.end_frame();
        assert!(t.released().is_empty());
    }

    #[test]
    fn release_all_reports_released() {
        let mut t = Touch::new();
        t.begin(TouchId(1), vec2(0.0, 0.0), 1.0);
        t.begin(TouchId(2), vec2(5.0, 5.0), 1.0);
        t.release_all();
        assert_eq!(t.count(), 0);
        assert_eq!(t.released().len(), 2);
    }
}
