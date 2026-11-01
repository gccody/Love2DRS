//! Gamepad state (the `love.joystick` equivalent, using its gamepad layout).
//!
//! Controllers are read through [gilrs](https://docs.rs/gilrs) and presented
//! with an Xbox-style layout: [`GamepadButton::A`] is the bottom face button on
//! every controller (Cross on PlayStation, B on Nintendo). Requires the
//! `gamepad` cargo feature (on by default). Without it, or when the platform has
//! no gamepad backend, [`Gamepads`] simply reports no controllers.
//!
//! On Windows and macOS, PlayStation controllers are read straight over HID
//! instead (see `sony.rs`), since gilrs can't rumble them there.

#[cfg(feature = "gamepad")]
use std::time::{Duration, Instant};

#[cfg(feature = "gamepad")]
use super::sony;
use crate::math::{Vec2, vec2};

/// How long a PlayStation pad that gilrs reports may take to show up over HID
/// before it's shown through gilrs instead (e.g. another app holds it open).
#[cfg(feature = "gamepad")]
const HID_GRACE: Duration = Duration::from_secs(2);
/// How often to look for it over HID in the meantime.
#[cfg(feature = "gamepad")]
const HID_RESCAN: Duration = Duration::from_millis(250);

/// Identifies one connected controller. Stays the same while it is connected;
/// a controller that is unplugged and plugged back in usually keeps its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GamepadId(pub(crate) usize);

impl GamepadId {
    /// The id as a plain number, handy for labels and maps.
    pub fn index(self) -> usize {
        self.0
    }
}

/// A digital gamepad button, named after the Xbox controller layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum GamepadButton {
    /// Bottom face button (Xbox A, PlayStation Cross, Nintendo B).
    A,
    /// Right face button (Xbox B, PlayStation Circle, Nintendo A).
    B,
    /// Left face button (Xbox X, PlayStation Square, Nintendo Y).
    X,
    /// Top face button (Xbox Y, PlayStation Triangle, Nintendo X).
    Y,
    /// Back / View / Select / Share / Minus.
    Back,
    /// Guide / Xbox / PS / Home button. Some platforms reserve it for the OS.
    Guide,
    /// Start / Menu / Options / Plus.
    Start,
    /// Pressing the left stick in.
    LeftStick,
    /// Pressing the right stick in.
    RightStick,
    /// Left bumper (LB / L1 / L).
    LeftShoulder,
    /// Right bumper (RB / R1 / R).
    RightShoulder,
    /// Left trigger pulled past its threshold (LT / L2 / ZL). Read
    /// [`GamepadAxis::LeftTrigger`] for the analog value.
    LeftTrigger,
    /// Right trigger pulled past its threshold (RT / R2 / ZR). Read
    /// [`GamepadAxis::RightTrigger`] for the analog value.
    RightTrigger,
    /// D-pad up.
    DPadUp,
    /// D-pad down.
    DPadDown,
    /// D-pad left.
    DPadLeft,
    /// D-pad right.
    DPadRight,
}

pub(crate) const BUTTONS: usize = GamepadButton::DPadRight as usize + 1;

impl GamepadButton {
    /// Every button, in declaration order.
    pub const ALL: [GamepadButton; BUTTONS] = [
        GamepadButton::A,
        GamepadButton::B,
        GamepadButton::X,
        GamepadButton::Y,
        GamepadButton::Back,
        GamepadButton::Guide,
        GamepadButton::Start,
        GamepadButton::LeftStick,
        GamepadButton::RightStick,
        GamepadButton::LeftShoulder,
        GamepadButton::RightShoulder,
        GamepadButton::LeftTrigger,
        GamepadButton::RightTrigger,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
    ];

    /// A short human-readable name, e.g. `"A"` or `"dpup"` (LÖVE's names).
    pub fn name(self) -> &'static str {
        match self {
            GamepadButton::A => "a",
            GamepadButton::B => "b",
            GamepadButton::X => "x",
            GamepadButton::Y => "y",
            GamepadButton::Back => "back",
            GamepadButton::Guide => "guide",
            GamepadButton::Start => "start",
            GamepadButton::LeftStick => "leftstick",
            GamepadButton::RightStick => "rightstick",
            GamepadButton::LeftShoulder => "leftshoulder",
            GamepadButton::RightShoulder => "rightshoulder",
            GamepadButton::LeftTrigger => "lefttrigger",
            GamepadButton::RightTrigger => "righttrigger",
            GamepadButton::DPadUp => "dpup",
            GamepadButton::DPadDown => "dpdown",
            GamepadButton::DPadLeft => "dpleft",
            GamepadButton::DPadRight => "dpright",
        }
    }

    #[cfg(feature = "gamepad")]
    fn from_gilrs(b: gilrs::Button) -> Option<Self> {
        use gilrs::Button as G;
        Some(match b {
            G::South => GamepadButton::A,
            G::East => GamepadButton::B,
            G::West => GamepadButton::X,
            G::North => GamepadButton::Y,
            G::Select => GamepadButton::Back,
            G::Mode => GamepadButton::Guide,
            G::Start => GamepadButton::Start,
            G::LeftThumb => GamepadButton::LeftStick,
            G::RightThumb => GamepadButton::RightStick,
            G::LeftTrigger => GamepadButton::LeftShoulder,
            G::RightTrigger => GamepadButton::RightShoulder,
            G::LeftTrigger2 => GamepadButton::LeftTrigger,
            G::RightTrigger2 => GamepadButton::RightTrigger,
            G::DPadUp => GamepadButton::DPadUp,
            G::DPadDown => GamepadButton::DPadDown,
            G::DPadLeft => GamepadButton::DPadLeft,
            G::DPadRight => GamepadButton::DPadRight,
            _ => return None,
        })
    }
}

/// An analog gamepad input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum GamepadAxis {
    /// Left stick horizontal, -1 (left) to 1 (right).
    LeftX,
    /// Left stick vertical, -1 (up) to 1 (down), matching screen coordinates.
    LeftY,
    /// Right stick horizontal, -1 (left) to 1 (right).
    RightX,
    /// Right stick vertical, -1 (up) to 1 (down), matching screen coordinates.
    RightY,
    /// Left trigger, 0 (released) to 1 (fully pulled).
    LeftTrigger,
    /// Right trigger, 0 (released) to 1 (fully pulled).
    RightTrigger,
}

pub(crate) const AXES: usize = GamepadAxis::RightTrigger as usize + 1;

impl GamepadAxis {
    /// Every axis, in declaration order.
    pub const ALL: [GamepadAxis; AXES] = [
        GamepadAxis::LeftX,
        GamepadAxis::LeftY,
        GamepadAxis::RightX,
        GamepadAxis::RightY,
        GamepadAxis::LeftTrigger,
        GamepadAxis::RightTrigger,
    ];

    /// LÖVE's name for the axis, e.g. `"leftx"` or `"triggerleft"`.
    pub fn name(self) -> &'static str {
        match self {
            GamepadAxis::LeftX => "leftx",
            GamepadAxis::LeftY => "lefty",
            GamepadAxis::RightX => "rightx",
            GamepadAxis::RightY => "righty",
            GamepadAxis::LeftTrigger => "triggerleft",
            GamepadAxis::RightTrigger => "triggerright",
        }
    }
}

/// One connected controller. Get it from [`Gamepads::get`], [`Gamepads::first`]
/// or [`Gamepads::iter`].
pub struct Gamepad {
    id: GamepadId,
    name: String,
    down: [bool; BUTTONS],
    pressed: [bool; BUTTONS],
    released: [bool; BUTTONS],
    axes: [f32; AXES],
    vibration_supported: bool,
    #[cfg(feature = "gamepad")]
    source: Source,
}

/// Where a controller's input comes from and its rumble goes.
#[cfg(feature = "gamepad")]
enum Source {
    Gilrs {
        id: gilrs::GamepadId,
        effect: Option<gilrs::ff::Effect>,
    },
    /// A PlayStation controller driven directly (Windows and macOS).
    Hid(sony::Pad),
    /// Hand-built pads in unit tests.
    #[cfg(test)]
    Test,
}

/// A PlayStation controller gilrs reports, kept out of the list because it's
/// read over HID instead.
#[cfg(feature = "gamepad")]
struct Hidden {
    gilrs: gilrs::GamepadId,
    ids: (u16, u16),
    /// When it was last expected to show up over HID.
    since: Instant,
}

impl Gamepad {
    /// This controller's id.
    pub fn id(&self) -> GamepadId {
        self.id
    }

    /// The controller's name as reported by the OS, e.g. `"Xbox Controller"`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether `button` is currently held down.
    pub fn is_down(&self, button: GamepadButton) -> bool {
        self.down[button as usize]
    }

    /// Whether any of `buttons` is held down.
    pub fn is_any_down(&self, buttons: &[GamepadButton]) -> bool {
        buttons.iter().any(|&b| self.is_down(b))
    }

    /// Whether `button` went down since the previous frame.
    pub fn was_pressed(&self, button: GamepadButton) -> bool {
        self.pressed[button as usize]
    }

    /// Whether `button` was let go since the previous frame.
    pub fn was_released(&self, button: GamepadButton) -> bool {
        self.released[button as usize]
    }

    /// The current value of `axis`: -1..1 for sticks, 0..1 for triggers.
    /// Small values near the center are already filtered out (dead zone).
    pub fn axis(&self, axis: GamepadAxis) -> f32 {
        self.axes[axis as usize]
    }

    /// The left stick as a vector, `y` pointing down like screen coordinates.
    pub fn left_stick(&self) -> Vec2 {
        vec2(self.axis(GamepadAxis::LeftX), self.axis(GamepadAxis::LeftY))
    }

    /// The right stick as a vector, `y` pointing down like screen coordinates.
    pub fn right_stick(&self) -> Vec2 {
        vec2(
            self.axis(GamepadAxis::RightX),
            self.axis(GamepadAxis::RightY),
        )
    }

    /// The d-pad as a vector of -1, 0 or 1 per component, `y` pointing down.
    pub fn dpad(&self) -> Vec2 {
        let a = |neg, pos| self.is_down(pos) as i32 as f32 - self.is_down(neg) as i32 as f32;
        vec2(
            a(GamepadButton::DPadLeft, GamepadButton::DPadRight),
            a(GamepadButton::DPadUp, GamepadButton::DPadDown),
        )
    }

    /// Whether [`Gamepads::set_vibration`] can rumble this controller.
    ///
    /// Xbox-style controllers rumble on Windows and Linux. PlayStation
    /// controllers (DualShock 4, DualSense, DualSense Edge) rumble on Windows,
    /// macOS and Linux, over USB or Bluetooth. Other controllers on macOS
    /// report `false`.
    pub fn is_vibration_supported(&self) -> bool {
        self.vibration_supported
    }

    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    fn press(&mut self, b: GamepadButton) {
        let i = b as usize;
        if !self.down[i] {
            self.pressed[i] = true;
        }
        self.down[i] = true;
    }

    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    fn release(&mut self, b: GamepadButton) {
        let i = b as usize;
        if self.down[i] {
            self.released[i] = true;
        }
        self.down[i] = false;
    }

    fn end_frame(&mut self) {
        self.pressed = [false; BUTTONS];
        self.released = [false; BUTTONS];
    }

    #[cfg(feature = "gamepad")]
    fn gilrs_id(&self) -> Option<gilrs::GamepadId> {
        match self.source {
            Source::Gilrs { id, .. } => Some(id),
            _ => None,
        }
    }

    #[cfg(feature = "gamepad")]
    fn hid(&self) -> Option<&sony::Pad> {
        match &self.source {
            Source::Hid(hid) => Some(hid),
            _ => None,
        }
    }

    /// Brings the state up to date with a HID snapshot, queuing events for
    /// what changed.
    #[cfg(feature = "gamepad")]
    fn apply(&mut self, input: &sony::Input, events: &mut Vec<GamepadEvent>) {
        for b in GamepadButton::ALL {
            let down = input.down[b as usize];
            if down != self.is_down(b) {
                if down {
                    self.press(b);
                    events.push(GamepadEvent::Pressed(self.id, b));
                } else {
                    self.release(b);
                    events.push(GamepadEvent::Released(self.id, b));
                }
            }
        }
        for a in GamepadAxis::ALL {
            let v = input.axes[a as usize];
            if self.axes[a as usize] != v {
                self.axes[a as usize] = v;
                events.push(GamepadEvent::Axis(self.id, a, v));
            }
        }
    }
}

/// A gamepad event, translated from the backend and replayed as `Game` callbacks.
#[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
pub(crate) enum GamepadEvent {
    Added(GamepadId),
    Removed(GamepadId),
    Pressed(GamepadId, GamepadButton),
    Released(GamepadId, GamepadButton),
    Axis(GamepadId, GamepadAxis, f32),
}

/// All connected gamepads. Reach it through `ctx.gamepad`.
///
/// The convenience queries on this type ([`is_down`](Self::is_down),
/// [`left_stick`](Self::left_stick), ...) look at every connected controller,
/// which is what a single-player game usually wants. For local multiplayer,
/// look up each player's controller with [`get`](Self::get).
pub struct Gamepads {
    pads: Vec<Gamepad>,
    pub(crate) events: Vec<GamepadEvent>,
    #[cfg(feature = "gamepad")]
    gilrs: Option<gilrs::Gilrs>,
    #[cfg(feature = "gamepad")]
    sony: sony::Hid,
    #[cfg(feature = "gamepad")]
    hidden: Vec<Hidden>,
    #[cfg(feature = "gamepad")]
    next_scan: Option<Instant>,
}

impl Gamepads {
    pub(crate) fn new() -> Self {
        #[allow(unused_mut)]
        let mut pads = Gamepads {
            pads: Vec::new(),
            events: Vec::new(),
            #[cfg(feature = "gamepad")]
            gilrs: match gilrs::Gilrs::new() {
                Ok(g) => Some(g),
                Err(gilrs::Error::NotImplemented(g)) => {
                    log::warn!("gamepads are not supported on this platform");
                    Some(g)
                }
                Err(e) => {
                    log::warn!("gamepad support unavailable: {e}");
                    None
                }
            },
            #[cfg(feature = "gamepad")]
            sony: sony::Hid::new(),
            #[cfg(feature = "gamepad")]
            hidden: Vec::new(),
            #[cfg(feature = "gamepad")]
            next_scan: None,
        };
        // Controllers that were plugged in before the game started are
        // announced as `gamepad_added` on the first frame, like LÖVE does.
        #[cfg(feature = "gamepad")]
        if let Some(gilrs) = &pads.gilrs {
            let ids: Vec<_> = gilrs.gamepads().map(|(id, _)| id).collect();
            for id in ids {
                pads.connect(id);
            }
        }
        #[cfg(feature = "gamepad")]
        {
            pads.scan_hid();
            pads.next_scan = None;
        }
        pads
    }

    /// The number of connected controllers.
    pub fn count(&self) -> usize {
        self.pads.len()
    }

    /// Whether at least one controller is connected.
    pub fn is_connected(&self) -> bool {
        !self.pads.is_empty()
    }

    /// Every connected controller, in the order they were connected.
    pub fn iter(&self) -> impl Iterator<Item = &Gamepad> {
        self.pads.iter()
    }

    /// The ids of every connected controller, in the order they were connected.
    pub fn ids(&self) -> impl Iterator<Item = GamepadId> + '_ {
        self.pads.iter().map(|p| p.id)
    }

    /// The controller with this id, if it is still connected.
    pub fn get(&self, id: GamepadId) -> Option<&Gamepad> {
        self.pads.iter().find(|p| p.id == id)
    }

    /// The controller that has been connected the longest.
    pub fn first(&self) -> Option<&Gamepad> {
        self.pads.first()
    }

    /// Whether `button` is held on any controller.
    pub fn is_down(&self, button: GamepadButton) -> bool {
        self.pads.iter().any(|p| p.is_down(button))
    }

    /// Whether any of `buttons` is held on any controller.
    pub fn is_any_down(&self, buttons: &[GamepadButton]) -> bool {
        self.pads.iter().any(|p| p.is_any_down(buttons))
    }

    /// Whether `button` went down on any controller since the previous frame.
    pub fn was_pressed(&self, button: GamepadButton) -> bool {
        self.pads.iter().any(|p| p.was_pressed(button))
    }

    /// Whether `button` was let go on any controller since the previous frame.
    pub fn was_released(&self, button: GamepadButton) -> bool {
        self.pads.iter().any(|p| p.was_released(button))
    }

    /// The value of `axis` on whichever controller pushes it furthest.
    pub fn axis(&self, axis: GamepadAxis) -> f32 {
        self.pads
            .iter()
            .map(|p| p.axis(axis))
            .fold(0.0, |best, v| if v.abs() > best.abs() { v } else { best })
    }

    /// The left stick of whichever controller pushes it furthest (`y` down).
    pub fn left_stick(&self) -> Vec2 {
        self.strongest(Gamepad::left_stick)
    }

    /// The right stick of whichever controller pushes it furthest (`y` down).
    pub fn right_stick(&self) -> Vec2 {
        self.strongest(Gamepad::right_stick)
    }

    /// The d-pad of whichever controller has one held (`y` down).
    pub fn dpad(&self) -> Vec2 {
        self.strongest(Gamepad::dpad)
    }

    fn strongest(&self, f: impl Fn(&Gamepad) -> Vec2) -> Vec2 {
        self.pads.iter().map(f).fold(Vec2::ZERO, |best, v| {
            if v.length_squared() > best.length_squared() {
                v
            } else {
                best
            }
        })
    }

    /// Rumbles a controller (`love.joystick`'s `setVibration`). `left` drives the
    /// strong low-frequency motor and `right` the weak high-frequency one, both
    /// 0..1. `duration` is in seconds; pass `f32::INFINITY` to rumble until
    /// [`stop_vibration`](Self::stop_vibration). Replaces any rumble already
    /// playing on that controller.
    ///
    /// Returns `false` if the controller is gone or can't rumble (see
    /// [`Gamepad::is_vibration_supported`]).
    pub fn set_vibration(&mut self, id: GamepadId, left: f32, right: f32, duration: f32) -> bool {
        #[cfg(feature = "gamepad")]
        {
            use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Repeat, Replay, Ticks};

            let Some(pad) = self.pads.iter_mut().find(|p| p.id == id) else {
                return false;
            };
            if !pad.vibration_supported {
                return false;
            }
            let (gilrs_id, slot) = match &mut pad.source {
                Source::Hid(hid) => return hid.set(left, right, duration),
                Source::Gilrs { id, effect } => (*id, effect),
                #[cfg(test)]
                Source::Test => return false,
            };
            let Some(gilrs) = &mut self.gilrs else {
                return false;
            };
            if let Some(old) = slot.take() {
                let _ = old.stop();
            }
            if left <= 0.0 && right <= 0.0 || duration <= 0.0 {
                return true;
            }
            let magnitude = |v: f32| (v.clamp(0.0, 1.0) * u16::MAX as f32) as u16;
            let (play_ms, repeat) = if duration.is_finite() {
                let ms = (duration * 1000.0).clamp(1.0, u32::MAX as f32) as u32;
                (ms, Repeat::For(Ticks::from_ms(ms)))
            } else {
                (1000, Repeat::Infinitely)
            };
            let scheduling = Replay {
                play_for: Ticks::from_ms(play_ms),
                ..Default::default()
            };
            let effect = EffectBuilder::new()
                .add_effect(BaseEffect {
                    kind: BaseEffectType::Strong {
                        magnitude: magnitude(left),
                    },
                    scheduling,
                    ..Default::default()
                })
                .add_effect(BaseEffect {
                    kind: BaseEffectType::Weak {
                        magnitude: magnitude(right),
                    },
                    scheduling,
                    ..Default::default()
                })
                .gamepads(&[gilrs_id])
                .repeat(repeat)
                .finish(gilrs);
            match effect.and_then(|e| e.play().map(|()| e)) {
                Ok(e) => {
                    *slot = Some(e);
                    true
                }
                Err(e) => {
                    log::warn!("gamepad vibration failed: {e}");
                    false
                }
            }
        }
        #[cfg(not(feature = "gamepad"))]
        {
            let _ = (id, left, right, duration);
            false
        }
    }

    /// Stops any rumble started with [`set_vibration`](Self::set_vibration).
    pub fn stop_vibration(&mut self, id: GamepadId) {
        self.set_vibration(id, 0.0, 0.0, 0.0);
    }

    /// Reads pending backend events into `self.events` and updates state.
    pub(crate) fn poll(&mut self) {
        #[cfg(feature = "gamepad")]
        {
            self.poll_gilrs();
            self.poll_hid();
            self.reconcile_hid(Instant::now());
        }
    }

    #[cfg(feature = "gamepad")]
    fn poll_gilrs(&mut self) {
        use gilrs::EventType as E;

        while let Some(ev) = self.gilrs.as_mut().and_then(|g| g.next_event()) {
            match ev.event {
                E::Connected => {
                    self.connect(ev.id);
                    continue;
                }
                E::Disconnected => {
                    self.hidden.retain(|h| h.gilrs != ev.id);
                    if let Some(i) = self.pads.iter().position(|p| p.gilrs_id() == Some(ev.id)) {
                        let id = self.pads.remove(i).id;
                        self.events.push(GamepadEvent::Removed(id));
                    }
                    continue;
                }
                _ => {}
            }
            // Pads read over HID are hidden here, so their events are dropped.
            let Some(id) = self
                .pads
                .iter()
                .find(|p| p.gilrs_id() == Some(ev.id))
                .map(|p| p.id)
            else {
                continue;
            };
            match ev.event {
                E::ButtonPressed(b, _) => {
                    if let Some(b) = GamepadButton::from_gilrs(b)
                        && let Some(pad) = self.pad_mut(id)
                        && !pad.is_down(b)
                    {
                        pad.press(b);
                        self.events.push(GamepadEvent::Pressed(id, b));
                    }
                }
                E::ButtonReleased(b, _) => {
                    if let Some(b) = GamepadButton::from_gilrs(b)
                        && let Some(pad) = self.pad_mut(id)
                        && pad.is_down(b)
                    {
                        pad.release(b);
                        self.events.push(GamepadEvent::Released(id, b));
                    }
                }
                E::ButtonChanged(b, value, _) => {
                    let axis = match b {
                        gilrs::Button::LeftTrigger2 => GamepadAxis::LeftTrigger,
                        gilrs::Button::RightTrigger2 => GamepadAxis::RightTrigger,
                        _ => continue,
                    };
                    self.set_axis(id, axis, value.clamp(0.0, 1.0));
                }
                E::AxisChanged(a, value, _) => {
                    let (axis, value) = match a {
                        gilrs::Axis::LeftStickX => (GamepadAxis::LeftX, value),
                        gilrs::Axis::LeftStickY => (GamepadAxis::LeftY, -value),
                        gilrs::Axis::RightStickX => (GamepadAxis::RightX, value),
                        gilrs::Axis::RightStickY => (GamepadAxis::RightY, -value),
                        _ => continue,
                    };
                    self.set_axis(id, axis, value.clamp(-1.0, 1.0));
                }
                _ => {}
            }
        }
    }

    /// Applies what the PlayStation pads' worker threads have read.
    #[cfg(feature = "gamepad")]
    fn poll_hid(&mut self) {
        let mut i = 0;
        while i < self.pads.len() {
            let pad = &mut self.pads[i];
            let Some(hid) = pad.hid() else {
                i += 1;
                continue;
            };
            let msgs: Vec<_> = hid.messages().collect();
            let mut gone = false;
            for msg in msgs {
                match msg {
                    sony::Msg::Input(input) => pad.apply(&input, &mut self.events),
                    sony::Msg::Gone => gone = true,
                }
            }
            if !gone {
                i += 1;
                continue;
            }
            let pad = self.pads.remove(i);
            self.events.push(GamepadEvent::Removed(pad.id));
            // If gilrs still sees it, it may come back (or need gilrs instead).
            let ids = pad.hid().map(sony::Pad::ids);
            let now = Instant::now();
            for h in self.hidden.iter_mut().filter(|h| Some(h.ids) == ids) {
                h.since = now;
                self.next_scan = Some(now + HID_RESCAN);
            }
        }
    }

    /// Makes sure every PlayStation pad gilrs reports is covered by a HID pad:
    /// scans for it while it isn't, and shows gilrs's copy instead once
    /// [`HID_GRACE`] runs out.
    #[cfg(feature = "gamepad")]
    fn reconcile_hid(&mut self, now: Instant) {
        if self.next_scan.is_some_and(|t| now >= t) {
            self.next_scan = None;
            self.scan_hid();
        }
        let mut uncovered = Vec::new();
        for (i, h) in self.hidden.iter().enumerate() {
            let hid_pads = self
                .pads
                .iter()
                .filter(|p| p.hid().map(sony::Pad::ids) == Some(h.ids))
                .count();
            let before = self.hidden[..i].iter().filter(|o| o.ids == h.ids).count();
            if before >= hid_pads {
                uncovered.push(i);
            }
        }
        for &i in uncovered.iter().rev() {
            if now.duration_since(self.hidden[i].since) >= HID_GRACE {
                let h = self.hidden.remove(i);
                log::warn!(
                    "couldn't read a PlayStation controller over HID; using it without rumble"
                );
                self.add_gilrs_pad(h.gilrs);
            } else if self.next_scan.is_none() {
                self.next_scan = Some(now + HID_RESCAN);
            }
        }
    }

    /// Opens PlayStation controllers that aren't open yet.
    #[cfg(feature = "gamepad")]
    fn scan_hid(&mut self) {
        let found = {
            let open: Vec<_> = self
                .pads
                .iter()
                .filter_map(|p| p.hid().map(sony::Pad::path))
                .collect();
            self.sony.scan(&open)
        };
        for hid in found {
            let id = self.free_id();
            self.pads.push(Gamepad {
                id,
                name: hid.name().to_owned(),
                down: [false; BUTTONS],
                pressed: [false; BUTTONS],
                released: [false; BUTTONS],
                axes: [0.0; AXES],
                vibration_supported: true,
                source: Source::Hid(hid),
            });
            self.events.push(GamepadEvent::Added(id));
        }
    }

    #[cfg(feature = "gamepad")]
    fn connect(&mut self, gilrs_id: gilrs::GamepadId) {
        let Some(gilrs) = &self.gilrs else { return };
        if self.pads.iter().any(|p| p.gilrs_id() == Some(gilrs_id))
            || self.hidden.iter().any(|h| h.gilrs == gilrs_id)
        {
            return;
        }
        let Some(gp) = gilrs.connected_gamepad(gilrs_id) else {
            return;
        };
        if let (Some(vid), Some(pid)) = (gp.vendor_id(), gp.product_id())
            && sony::is_supported(vid, pid)
            && self.sony.available()
        {
            let now = Instant::now();
            self.hidden.push(Hidden {
                gilrs: gilrs_id,
                ids: (vid, pid),
                since: now,
            });
            self.next_scan = Some(now);
            return;
        }
        self.add_gilrs_pad(gilrs_id);
    }

    #[cfg(feature = "gamepad")]
    fn add_gilrs_pad(&mut self, gilrs_id: gilrs::GamepadId) {
        let Some(gp) = self
            .gilrs
            .as_ref()
            .and_then(|g| g.connected_gamepad(gilrs_id))
        else {
            return;
        };
        let (name, ff) = (gp.name().to_owned(), gp.is_ff_supported());
        let id = self.free_id();
        self.pads.push(Gamepad {
            id,
            name,
            down: [false; BUTTONS],
            pressed: [false; BUTTONS],
            released: [false; BUTTONS],
            axes: [0.0; AXES],
            vibration_supported: ff,
            source: Source::Gilrs {
                id: gilrs_id,
                effect: None,
            },
        });
        self.events.push(GamepadEvent::Added(id));
    }

    /// The lowest id no connected pad uses, so a replugged pad usually gets
    /// its old id back.
    #[cfg(feature = "gamepad")]
    fn free_id(&self) -> GamepadId {
        let used = |i: usize| self.pads.iter().any(|p| p.id.0 == i);
        GamepadId((0..).find(|&i| !used(i)).unwrap_or(0))
    }

    #[cfg(feature = "gamepad")]
    fn pad_mut(&mut self, id: GamepadId) -> Option<&mut Gamepad> {
        self.pads.iter_mut().find(|p| p.id == id)
    }

    #[cfg(feature = "gamepad")]
    fn set_axis(&mut self, id: GamepadId, axis: GamepadAxis, value: f32) {
        if let Some(pad) = self.pad_mut(id)
            && pad.axes[axis as usize] != value
        {
            pad.axes[axis as usize] = value;
            self.events.push(GamepadEvent::Axis(id, axis, value));
        }
    }

    pub(crate) fn end_frame(&mut self) {
        for pad in &mut self.pads {
            pad.end_frame();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(id: usize) -> Gamepad {
        Gamepad {
            id: GamepadId(id),
            name: format!("pad {id}"),
            down: [false; BUTTONS],
            pressed: [false; BUTTONS],
            released: [false; BUTTONS],
            axes: [0.0; AXES],
            vibration_supported: false,
            #[cfg(feature = "gamepad")]
            source: Source::Test,
        }
    }

    fn gamepads(pads: Vec<Gamepad>) -> Gamepads {
        Gamepads {
            pads,
            events: Vec::new(),
            #[cfg(feature = "gamepad")]
            gilrs: None,
            #[cfg(feature = "gamepad")]
            sony: sony::Hid::disabled(),
            #[cfg(feature = "gamepad")]
            hidden: Vec::new(),
            #[cfg(feature = "gamepad")]
            next_scan: None,
        }
    }

    #[test]
    fn button_edges() {
        let mut p = pad(0);
        p.press(GamepadButton::A);
        p.press(GamepadButton::A);
        assert!(p.is_down(GamepadButton::A) && p.was_pressed(GamepadButton::A));
        p.end_frame();
        assert!(p.is_down(GamepadButton::A) && !p.was_pressed(GamepadButton::A));
        p.release(GamepadButton::A);
        assert!(!p.is_down(GamepadButton::A) && p.was_released(GamepadButton::A));
    }

    #[test]
    fn dpad_and_sticks() {
        let mut p = pad(0);
        p.press(GamepadButton::DPadLeft);
        p.press(GamepadButton::DPadDown);
        assert_eq!(p.dpad(), vec2(-1.0, 1.0));
        p.axes[GamepadAxis::LeftX as usize] = 0.5;
        p.axes[GamepadAxis::LeftY as usize] = -0.25;
        assert_eq!(p.left_stick(), vec2(0.5, -0.25));
    }

    #[test]
    fn any_pad_queries_pick_the_strongest() {
        let mut a = pad(0);
        let mut b = pad(1);
        a.axes[GamepadAxis::LeftX as usize] = 0.3;
        b.axes[GamepadAxis::LeftX as usize] = -0.8;
        b.press(GamepadButton::Start);
        let pads = gamepads(vec![a, b]);
        assert_eq!(pads.axis(GamepadAxis::LeftX), -0.8);
        assert_eq!(pads.left_stick(), vec2(-0.8, 0.0));
        assert!(pads.is_down(GamepadButton::Start));
        assert!(pads.was_pressed(GamepadButton::Start));
        assert_eq!(pads.first().map(|p| p.id()), Some(GamepadId(0)));
        assert!(pads.get(GamepadId(1)).is_some() && pads.get(GamepadId(2)).is_none());
    }

    #[test]
    fn no_backend_means_no_pads() {
        let mut pads = gamepads(Vec::new());
        pads.poll();
        assert!(!pads.is_connected());
        assert!(!pads.set_vibration(GamepadId(0), 1.0, 1.0, 1.0));
        assert_eq!(pads.left_stick(), Vec2::ZERO);
    }

    #[test]
    fn all_lists_match_enums() {
        for (i, b) in GamepadButton::ALL.iter().enumerate() {
            assert_eq!(*b as usize, i);
        }
        for (i, a) in GamepadAxis::ALL.iter().enumerate() {
            assert_eq!(*a as usize, i);
        }
    }
}
