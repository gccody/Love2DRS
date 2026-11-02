//! Controllers and rumble on iOS, through Apple's GameController and CoreHaptics
//! frameworks. Controllers are polled once per frame, like the other backends.

use std::time::Instant;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_haptics::{
    CHHapticAdvancedPatternPlayer, CHHapticEngine, CHHapticEvent, CHHapticEventParameter,
    CHHapticEventParameterIDHapticIntensity, CHHapticEventTypeHapticContinuous, CHHapticPattern,
    CHHapticPatternPlayer, CHHapticTimeImmediate,
};
use objc2_foundation::NSArray;
use objc2_game_controller::{
    GCController, GCControllerButtonInput, GCDevice, GCExtendedGamepad, GCHapticsLocalityDefault,
    GCHapticsLocalityLeftHandle, GCHapticsLocalityRightHandle,
};

use super::gamepad::{AXES, BUTTONS, GamepadAxis, GamepadButton};
use super::sony::{Input, dead_zone};

/// Length of the pattern that loops while a motor runs.
const LOOP_SECONDS: f64 = 1.0;

pub(crate) struct Pad {
    controller: Retained<GCController>,
    /// Started the first time the pad rumbles, since an engine takes a moment to start.
    motors: Vec<Motor>,
    /// When the running rumble should stop.
    until: Option<Instant>,
}

struct Motor {
    engine: Retained<CHHapticEngine>,
    player: Option<Retained<ProtocolObject<dyn CHHapticAdvancedPatternPlayer>>>,
}

/// Every connected controller with a full set of sticks and buttons. Others,
/// such as the Siri Remote, can't fill the Xbox-style layout.
pub(crate) fn controllers() -> Vec<Retained<GCController>> {
    // SAFETY: called on the main thread, where the game's event loop runs on iOS.
    let all = unsafe { GCController::controllers() };
    all.iter()
        .filter(|c| unsafe { c.extendedGamepad() }.is_some())
        .collect()
}

impl Pad {
    pub(crate) fn new(controller: Retained<GCController>) -> Self {
        Pad {
            controller,
            motors: Vec::new(),
            until: None,
        }
    }

    pub(crate) fn is(&self, controller: &GCController) -> bool {
        std::ptr::eq(&*self.controller, controller)
    }

    pub(crate) fn name(&self) -> String {
        unsafe { self.controller.vendorName() }
            .map_or_else(|| "Controller".to_string(), |name| name.to_string())
    }

    pub(crate) fn can_rumble(&self) -> bool {
        unsafe { self.controller.haptics() }.is_some()
    }

    pub(crate) fn input(&self) -> Input {
        let mut input = Input {
            down: [false; BUTTONS],
            axes: [0.0; AXES],
        };
        if let Some(gamepad) = unsafe { self.controller.extendedGamepad() } {
            read_gamepad(&gamepad, &mut input);
        }
        input
    }

    /// Starts or, with zero strength or duration, stops the rumble. `left` is the
    /// strong motor and `right` the weak one.
    pub(crate) fn set(&mut self, left: f32, right: f32, duration: f32) -> bool {
        self.stop();
        if (left <= 0.0 && right <= 0.0) || duration <= 0.0 {
            return true;
        }
        if self.motors.is_empty() {
            self.start_engines();
        }
        // With a single motor, it plays the stronger of the two.
        let strengths = match self.motors.len() {
            1 => [left.max(right), 0.0],
            _ => [left, right],
        };
        let mut started = false;
        for (motor, strength) in self.motors.iter_mut().zip(strengths) {
            if strength > 0.0 {
                match motor.play(strength.min(1.0)) {
                    Ok(player) => {
                        motor.player = Some(player);
                        started = true;
                    }
                    Err(e) => log::warn!("gamepad vibration failed: {e}"),
                }
            }
        }
        if duration.is_finite() {
            self.until = Some(Instant::now() + std::time::Duration::from_secs_f32(duration));
        }
        started
    }

    /// Stops a rumble whose time is up. Called once per frame.
    pub(crate) fn update(&mut self, now: Instant) {
        if self.until.is_some_and(|until| now >= until) {
            self.stop();
        }
    }

    fn stop(&mut self) {
        self.until = None;
        for motor in &mut self.motors {
            if let Some(player) = motor.player.take() {
                let _ = unsafe { player.stopAtTime_error(CHHapticTimeImmediate) };
            }
        }
    }

    /// One engine per grip if the controller has a motor in each, otherwise one.
    fn start_engines(&mut self) {
        let Some(haptics) = (unsafe { self.controller.haptics() }) else {
            return;
        };
        let supported = unsafe { haptics.supportedLocalities() };
        let (left, right, default) = unsafe {
            (
                GCHapticsLocalityLeftHandle,
                GCHapticsLocalityRightHandle,
                GCHapticsLocalityDefault,
            )
        };
        let localities = if supported.containsObject(left) && supported.containsObject(right) {
            vec![left, right]
        } else {
            vec![default]
        };
        for locality in localities {
            let Some(engine) = (unsafe { haptics.createEngineWithLocality(locality) }) else {
                continue;
            };
            match unsafe { engine.startAndReturnError() } {
                Ok(()) => self.motors.push(Motor {
                    engine,
                    player: None,
                }),
                Err(e) => log::warn!("can't start a gamepad's haptic engine: {e}"),
            }
        }
    }
}

impl Motor {
    /// Loops a continuous vibration until the player is stopped.
    fn play(
        &self,
        strength: f32,
    ) -> Result<
        Retained<ProtocolObject<dyn CHHapticAdvancedPatternPlayer>>,
        Retained<objc2_foundation::NSError>,
    > {
        unsafe {
            // iOS stops engines when the app goes to the background.
            self.engine.startAndReturnError()?;
            let intensity = CHHapticEventParameter::initWithParameterID_value(
                CHHapticEventParameter::alloc(),
                CHHapticEventParameterIDHapticIntensity,
                strength,
            );
            let event = CHHapticEvent::initWithEventType_parameters_relativeTime_duration(
                CHHapticEvent::alloc(),
                CHHapticEventTypeHapticContinuous,
                &NSArray::from_retained_slice(&[intensity]),
                0.0,
                LOOP_SECONDS,
            );
            let pattern = CHHapticPattern::initWithEvents_parameters_error(
                CHHapticPattern::alloc(),
                &NSArray::from_retained_slice(&[event]),
                &NSArray::new(),
            )?;
            let player = self
                .engine
                .createAdvancedPlayerWithPattern_error(&pattern)?;
            player.setLoopEnabled(true);
            player.startAtTime_error(CHHapticTimeImmediate)?;
            Ok(player)
        }
    }
}

fn read_gamepad(pad: &GCExtendedGamepad, input: &mut Input) {
    use GamepadAxis as A;
    use GamepadButton as B;

    let mut set = |button: B, state: Option<Retained<GCControllerButtonInput>>| {
        input.down[button as usize] = state.is_some_and(|s| unsafe { s.isPressed() });
    };
    unsafe {
        set(B::A, Some(pad.buttonA()));
        set(B::B, Some(pad.buttonB()));
        set(B::X, Some(pad.buttonX()));
        set(B::Y, Some(pad.buttonY()));
        set(B::LeftShoulder, Some(pad.leftShoulder()));
        set(B::RightShoulder, Some(pad.rightShoulder()));
        set(B::LeftTrigger, Some(pad.leftTrigger()));
        set(B::RightTrigger, Some(pad.rightTrigger()));
        set(B::Start, Some(pad.buttonMenu()));
        set(B::Back, pad.buttonOptions());
        set(B::Guide, pad.buttonHome());
        set(B::LeftStick, pad.leftThumbstickButton());
        set(B::RightStick, pad.rightThumbstickButton());
        let dpad = pad.dpad();
        set(B::DPadUp, Some(dpad.up()));
        set(B::DPadDown, Some(dpad.down()));
        set(B::DPadLeft, Some(dpad.left()));
        set(B::DPadRight, Some(dpad.right()));

        // GameController's sticks point up. love2drs's point down, like the screen.
        let stick = pad.leftThumbstick();
        let (x, y) = dead_zone(stick.xAxis().value(), -stick.yAxis().value());
        input.axes[A::LeftX as usize] = x;
        input.axes[A::LeftY as usize] = y;
        let stick = pad.rightThumbstick();
        let (x, y) = dead_zone(stick.xAxis().value(), -stick.yAxis().value());
        input.axes[A::RightX as usize] = x;
        input.axes[A::RightY as usize] = y;
        input.axes[A::LeftTrigger as usize] = pad.leftTrigger().value();
        input.axes[A::RightTrigger as usize] = pad.rightTrigger().value();
    }
}
