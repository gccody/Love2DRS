//! Touchscreen state (the `love.touch` equivalent).
//!
//! Touch events come from touchscreens on Android, iOS, Windows and Linux (X11
//! and Wayland). macOS has no touchscreens; trackpad gestures arrive as mouse
//! and wheel events, so `ctx.touch` stays empty there.
//!
//! The first finger also drives emulated mouse events for programs that only
//! know about the mouse: the OS does this on desktop and love2drs on mobile. A
//! tap therefore usually reaches both `touch_pressed` and `mouse_pressed`.
//! Handle one or the other for the same action.
//!
//! A finger can also end without being lifted: the OS takes it for a system
//! gesture, an incoming call or a notification shade, or the window loses focus.
//! That reaches `touch_cancelled` and [`Touch::cancelled`], never `touch_released`
//! or [`Touch::released`], so a game that acts on release doesn't act on these.
//! The emulated mouse does the same through `mouse_cancelled`.

use winit::event::TouchPhase;

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
    cancelled: Vec<TouchPoint>,
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

    /// Fingers the OS took away since the previous frame, with their last
    /// position. They weren't lifted, so don't treat them as taps.
    pub fn cancelled(&self) -> &[TouchPoint] {
        &self.cancelled
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

    /// Removes a finger from the screen, updated to where it was last seen.
    fn take(&mut self, id: TouchId, pos: Vec2, pressure: f32) -> Option<TouchPoint> {
        let i = self.active.iter().position(|t| t.id == id)?;
        let mut t = self.active.remove(i);
        t.delta += pos - t.position;
        t.position = pos;
        t.pressure = pressure;
        Some(t)
    }

    /// Lifts a finger, returning whether it was down.
    pub(crate) fn end(&mut self, id: TouchId, pos: Vec2, pressure: f32) -> bool {
        let t = self.take(id, pos, pressure);
        let was_down = t.is_some();
        self.released.extend(t);
        was_down
    }

    /// Ends a finger the OS took away, returning whether it was down.
    pub(crate) fn cancel(&mut self, id: TouchId, pos: Vec2, pressure: f32) -> bool {
        let t = self.take(id, pos, pressure);
        let was_down = t.is_some();
        self.cancelled.extend(t);
        was_down
    }

    /// Ends every finger, for when focus is lost or the app is suspended, and
    /// returns them so their callbacks can be sent.
    pub(crate) fn cancel_all(&mut self) -> Vec<TouchPoint> {
        let fingers = std::mem::take(&mut self.active);
        self.cancelled.extend_from_slice(&fingers);
        fingers
    }

    pub(crate) fn end_frame(&mut self) {
        for t in &mut self.active {
            t.delta = Vec2::ZERO;
        }
        self.pressed.clear();
        self.released.clear();
        self.cancelled.clear();
    }
}

/// What a touch does to the emulated mouse on mobile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseAction {
    Press,
    Move,
    Release,
    Cancel,
}

/// Tracks the one finger that acts as the mouse on mobile: the first one down.
#[derive(Default)]
pub(crate) struct MouseFinger(Option<TouchId>);

impl MouseFinger {
    /// Works out what this touch does to the mouse, if anything.
    pub(crate) fn route(&mut self, id: TouchId, phase: TouchPhase) -> Option<MouseAction> {
        match phase {
            TouchPhase::Started if self.0.is_none() => {
                self.0 = Some(id);
                Some(MouseAction::Press)
            }
            TouchPhase::Moved if self.0 == Some(id) => Some(MouseAction::Move),
            TouchPhase::Ended if self.0 == Some(id) => {
                self.0 = None;
                Some(MouseAction::Release)
            }
            TouchPhase::Cancelled if self.0 == Some(id) => {
                self.0 = None;
                Some(MouseAction::Cancel)
            }
            _ => None,
        }
    }

    /// Forgets the finger, returning it. Its later events no longer reach the mouse.
    pub(crate) fn take(&mut self) -> Option<TouchId> {
        self.0.take()
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
    fn cancelling_is_not_releasing() {
        let mut t = Touch::new();
        let id = TouchId(3);
        t.begin(id, vec2(10.0, 10.0), 1.0);
        assert!(t.cancel(id, vec2(12.0, 11.0), 1.0));
        assert!(!t.cancel(id, vec2(12.0, 11.0), 1.0), "already gone");
        assert!(!t.end(id, vec2(12.0, 11.0), 1.0), "already gone");
        assert_eq!(t.count(), 0);
        assert!(t.released().is_empty());
        let c = t.cancelled()[0];
        assert_eq!((c.start, c.position), (vec2(10.0, 10.0), vec2(12.0, 11.0)));

        t.end_frame();
        assert!(t.cancelled().is_empty());
    }

    #[test]
    fn cancel_all_reports_cancelled() {
        let mut t = Touch::new();
        t.begin(TouchId(1), vec2(0.0, 0.0), 1.0);
        t.begin(TouchId(2), vec2(5.0, 5.0), 1.0);
        let fingers = t.cancel_all();
        assert_eq!(fingers.len(), 2);
        assert_eq!(t.count(), 0);
        assert_eq!(t.cancelled().len(), 2);
        assert!(t.released().is_empty());
    }

    #[test]
    fn only_the_first_finger_drives_the_mouse() {
        let (first, second) = (TouchId(1), TouchId(2));
        let mut m = MouseFinger::default();
        let press = Some(MouseAction::Press);
        assert_eq!(m.route(first, TouchPhase::Started), press);
        assert_eq!(m.route(second, TouchPhase::Started), None);
        assert_eq!(m.route(second, TouchPhase::Moved), None);
        assert_eq!(m.route(first, TouchPhase::Moved), Some(MouseAction::Move));
        assert_eq!(m.route(second, TouchPhase::Ended), None);
        assert_eq!(
            m.route(first, TouchPhase::Ended),
            Some(MouseAction::Release)
        );
        assert_eq!(m.route(second, TouchPhase::Started), press);
        assert_eq!(
            m.route(second, TouchPhase::Cancelled),
            Some(MouseAction::Cancel)
        );
        assert_eq!(m.route(second, TouchPhase::Cancelled), None);
    }

    #[test]
    fn a_forgotten_finger_cannot_release_the_mouse() {
        // Focus was lost while the finger was down, and it lifts afterwards.
        let id = TouchId(1);
        let mut m = MouseFinger::default();
        m.route(id, TouchPhase::Started);
        assert_eq!(m.take(), Some(id));
        assert_eq!(m.route(id, TouchPhase::Moved), None);
        assert_eq!(m.route(id, TouchPhase::Ended), None);
    }
}
