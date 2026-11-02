//! Reads Android's input queue directly.
//!
//! winit treats every motion event as a touch, so stick and trigger values never
//! reach a game and a moved stick can look like a moving finger. Its handler has
//! no hook for this, but the queue only has one reader: the app reads it in
//! `new_events`, which winit calls just before it would read the queue itself.
//! That means touches and keys have to be translated here too.

use winit::event::TouchPhase;
use winit::platform::android::activity::AndroidApp;
use winit::platform::android::activity::InputStatus;
use winit::platform::android::activity::input::{
    Axis, InputEvent, KeyAction, KeyEvent, KeyMapChar, Keycode, MotionAction, MotionEvent,
};

use super::{GamepadButton, Key};

/// `InputDevice.SOURCE_GAMEPAD`, without its class bit: gamepad key events are
/// often reported with a combined source such as gamepad + keyboard.
const SOURCE_GAMEPAD_BIT: u32 = 0x400;

/// `MotionEvent.FLAG_CANCELED`: since Android 13, a finger going up with this
/// flag was rejected, for example as a palm, and shouldn't count as a tap.
const FLAG_CANCELED: u32 = 0x20;

pub(crate) enum Event {
    Touch {
        id: u64,
        phase: TouchPhase,
        /// Physical pixels.
        x: f32,
        y: f32,
        pressure: f32,
    },
    Key {
        key: Key,
        pressed: bool,
        repeat: bool,
        text: Option<char>,
    },
    PadButton {
        device: i32,
        button: GamepadButton,
        down: bool,
    },
    PadMotion(PadMotion),
}

/// Every axis of a controller, as Android reports it.
#[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
pub(crate) struct PadMotion {
    pub device: i32,
    pub left: (f32, f32),
    pub right: (f32, f32),
    pub left_trigger: f32,
    pub right_trigger: f32,
    pub hat: (f32, f32),
}

#[derive(Default)]
pub(crate) struct Reader {
    /// A dead key waiting for the letter it combines with.
    accent: Option<char>,
}

impl Reader {
    /// Translates everything waiting in the queue.
    pub(crate) fn drain(&mut self, app: &AndroidApp, mut on_event: impl FnMut(Event)) {
        let Ok(mut events) = app.input_events_iter() else {
            return;
        };
        while events.next(|event| match event {
            InputEvent::MotionEvent(motion) => read_motion(motion, &mut on_event),
            InputEvent::KeyEvent(key) => self.read_key(app, key, &mut on_event),
            _ => InputStatus::Unhandled,
        }) {}
    }

    fn read_key(
        &mut self,
        app: &AndroidApp,
        event: &KeyEvent<'_>,
        on_event: &mut impl FnMut(Event),
    ) -> InputStatus {
        let pressed = match event.action() {
            KeyAction::Down => true,
            KeyAction::Up => false,
            _ => return InputStatus::Unhandled,
        };
        let code = event.key_code();
        let from_pad = u32::from(event.source()) & SOURCE_GAMEPAD_BIT != 0
            || event.source().is_joystick_class();
        if let Some(button) = pad_button(code, from_pad) {
            on_event(Event::PadButton {
                device: event.device_id(),
                button,
                down: pressed,
            });
            return InputStatus::Handled;
        }
        // Leave these to the OS, so they keep working in the game.
        if matches!(
            code,
            Keycode::VolumeUp
                | Keycode::VolumeDown
                | Keycode::VolumeMute
                | Keycode::Power
                | Keycode::Camera
                | Keycode::Home
        ) {
            return InputStatus::Unhandled;
        }
        let text = pressed.then(|| self.typed_char(app, event)).flatten();
        on_event(Event::Key {
            key: key_from_android(code),
            pressed,
            repeat: event.repeat_count() > 0,
            text,
        });
        InputStatus::Handled
    }

    /// The character a key press types, combining dead keys with the next letter.
    fn typed_char(&mut self, app: &AndroidApp, event: &KeyEvent<'_>) -> Option<char> {
        let map = app.device_key_character_map(event.device_id()).ok()?;
        match map.get(event.key_code(), event.meta_state()).ok()? {
            KeyMapChar::Unicode(c) => match self.accent.take() {
                Some(accent) => map.get_dead_char(accent, c).ok().flatten(),
                None => Some(c),
            },
            KeyMapChar::CombiningAccent(accent) => {
                self.accent = Some(accent);
                None
            }
            KeyMapChar::None => None,
        }
    }
}

fn read_motion(event: &MotionEvent<'_>, on_event: &mut impl FnMut(Event)) -> InputStatus {
    if event.source().is_joystick_class() {
        let axes = event.pointer_at_index(0);
        let axis = |a: Axis| axes.axis_value(a);
        on_event(Event::PadMotion(PadMotion {
            device: event.device_id(),
            left: (axis(Axis::X), axis(Axis::Y)),
            right: (axis(Axis::Z), axis(Axis::Rz)),
            // Controllers report triggers on one of these two pairs.
            left_trigger: axis(Axis::Ltrigger).max(axis(Axis::Brake)),
            right_trigger: axis(Axis::Rtrigger).max(axis(Axis::Gas)),
            hat: (axis(Axis::HatX), axis(Axis::HatY)),
        }));
        return InputStatus::Handled;
    }
    let action = event.action();
    let phase = match action {
        MotionAction::Down | MotionAction::PointerDown => TouchPhase::Started,
        MotionAction::Up | MotionAction::PointerUp if event.flags().0 & FLAG_CANCELED != 0 => {
            TouchPhase::Cancelled
        }
        MotionAction::Up | MotionAction::PointerUp => TouchPhase::Ended,
        MotionAction::Move => TouchPhase::Moved,
        MotionAction::Cancel => TouchPhase::Cancelled,
        _ => return InputStatus::Unhandled,
    };
    let mut send = |id: i32, x: f32, y: f32, pressure: f32| {
        on_event(Event::Touch {
            id: id as u64,
            phase,
            x,
            y,
            pressure,
        })
    };
    // These name one finger. Moves and `Cancel` are about all of them.
    if matches!(
        action,
        MotionAction::Down | MotionAction::PointerDown | MotionAction::Up | MotionAction::PointerUp
    ) {
        let p = event.pointer_at_index(event.pointer_index());
        send(p.pointer_id(), p.x(), p.y(), p.pressure());
    } else {
        for p in event.pointers() {
            send(p.pointer_id(), p.x(), p.y(), p.pressure());
        }
    }
    InputStatus::Handled
}

fn pad_button(code: Keycode, from_pad: bool) -> Option<GamepadButton> {
    use GamepadButton as B;
    Some(match code {
        Keycode::ButtonA => B::A,
        Keycode::ButtonB => B::B,
        Keycode::ButtonX => B::X,
        Keycode::ButtonY => B::Y,
        Keycode::ButtonL1 => B::LeftShoulder,
        Keycode::ButtonR1 => B::RightShoulder,
        Keycode::ButtonL2 => B::LeftTrigger,
        Keycode::ButtonR2 => B::RightTrigger,
        Keycode::ButtonThumbl => B::LeftStick,
        Keycode::ButtonThumbr => B::RightStick,
        Keycode::ButtonStart => B::Start,
        Keycode::ButtonSelect => B::Back,
        Keycode::ButtonMode => B::Guide,
        // These also come from keyboards and remote controls.
        Keycode::Back if from_pad => B::Back,
        Keycode::DpadUp if from_pad => B::DPadUp,
        Keycode::DpadDown if from_pad => B::DPadDown,
        Keycode::DpadLeft if from_pad => B::DPadLeft,
        Keycode::DpadRight if from_pad => B::DPadRight,
        _ => return None,
    })
}

fn key_from_android(code: Keycode) -> Key {
    use Keycode as K;
    match code {
        K::A => Key::A,
        K::B => Key::B,
        K::C => Key::C,
        K::D => Key::D,
        K::E => Key::E,
        K::F => Key::F,
        K::G => Key::G,
        K::H => Key::H,
        K::I => Key::I,
        K::J => Key::J,
        K::K => Key::K,
        K::L => Key::L,
        K::M => Key::M,
        K::N => Key::N,
        K::O => Key::O,
        K::P => Key::P,
        K::Q => Key::Q,
        K::R => Key::R,
        K::S => Key::S,
        K::T => Key::T,
        K::U => Key::U,
        K::V => Key::V,
        K::W => Key::W,
        K::X => Key::X,
        K::Y => Key::Y,
        K::Z => Key::Z,
        K::Keycode0 => Key::Num0,
        K::Keycode1 => Key::Num1,
        K::Keycode2 => Key::Num2,
        K::Keycode3 => Key::Num3,
        K::Keycode4 => Key::Num4,
        K::Keycode5 => Key::Num5,
        K::Keycode6 => Key::Num6,
        K::Keycode7 => Key::Num7,
        K::Keycode8 => Key::Num8,
        K::Keycode9 => Key::Num9,
        K::F1 => Key::F1,
        K::F2 => Key::F2,
        K::F3 => Key::F3,
        K::F4 => Key::F4,
        K::F5 => Key::F5,
        K::F6 => Key::F6,
        K::F7 => Key::F7,
        K::F8 => Key::F8,
        K::F9 => Key::F9,
        K::F10 => Key::F10,
        K::F11 => Key::F11,
        K::F12 => Key::F12,
        K::DpadUp => Key::Up,
        K::DpadDown => Key::Down,
        K::DpadLeft => Key::Left,
        K::DpadRight => Key::Right,
        K::Space => Key::Space,
        K::Enter | K::DpadCenter => Key::Enter,
        // Android's back gesture is its Escape key.
        K::Escape | K::Back => Key::Escape,
        K::Tab => Key::Tab,
        K::Del => Key::Backspace,
        K::ForwardDel => Key::Delete,
        K::Insert => Key::Insert,
        K::MoveHome => Key::Home,
        K::MoveEnd => Key::End,
        K::PageUp => Key::PageUp,
        K::PageDown => Key::PageDown,
        K::ShiftLeft => Key::LShift,
        K::ShiftRight => Key::RShift,
        K::CtrlLeft => Key::LCtrl,
        K::CtrlRight => Key::RCtrl,
        K::AltLeft => Key::LAlt,
        K::AltRight => Key::RAlt,
        K::MetaLeft => Key::LSuper,
        K::MetaRight => Key::RSuper,
        K::Minus => Key::Minus,
        K::Equals => Key::Equals,
        K::LeftBracket => Key::LeftBracket,
        K::RightBracket => Key::RightBracket,
        K::Backslash => Key::Backslash,
        K::Semicolon => Key::Semicolon,
        K::Apostrophe => Key::Quote,
        K::Comma => Key::Comma,
        K::Period => Key::Period,
        K::Slash => Key::Slash,
        K::Grave => Key::Backtick,
        K::CapsLock => Key::CapsLock,
        K::NumLock => Key::NumLock,
        K::ScrollLock => Key::ScrollLock,
        K::Sysrq => Key::PrintScreen,
        K::Break => Key::Pause,
        K::Menu => Key::Menu,
        K::Numpad0 => Key::Kp0,
        K::Numpad1 => Key::Kp1,
        K::Numpad2 => Key::Kp2,
        K::Numpad3 => Key::Kp3,
        K::Numpad4 => Key::Kp4,
        K::Numpad5 => Key::Kp5,
        K::Numpad6 => Key::Kp6,
        K::Numpad7 => Key::Kp7,
        K::Numpad8 => Key::Kp8,
        K::Numpad9 => Key::Kp9,
        K::NumpadAdd => Key::KpAdd,
        K::NumpadSubtract => Key::KpSubtract,
        K::NumpadMultiply => Key::KpMultiply,
        K::NumpadDivide => Key::KpDivide,
        K::NumpadEnter => Key::KpEnter,
        K::NumpadDot => Key::KpDecimal,
        _ => Key::Unknown,
    }
}

/// Controllers and rumble, through Android's Java APIs.
#[cfg(feature = "gamepad")]
pub(crate) mod pads {
    use jni::JNIEnv;
    use jni::objects::{JIntArray, JLongArray, JObject, JString, JValue};

    use crate::platform::{sdk_version, with_jni};

    const INPUT_DEVICE: &str = "android/view/InputDevice";
    const EFFECT: &str = "android/os/VibrationEffect";
    const GET_EFFECT: &str = "Landroid/os/VibrationEffect;";
    const SOURCE_GAMEPAD: i32 = 0x401;
    const SOURCE_JOYSTICK: i32 = 0x1000010;
    /// Android 12, which added separate access to each motor.
    const VIBRATOR_MANAGER_SDK: i32 = 31;

    pub(crate) struct Device {
        pub id: i32,
        pub name: String,
        pub can_rumble: bool,
    }

    /// Every connected controller.
    pub(crate) fn devices() -> Vec<Device> {
        with_jni(|env, _| {
            let ids = env
                .call_static_method(INPUT_DEVICE, "getDeviceIds", "()[I", &[])?
                .l()?;
            let ids = JIntArray::from(ids);
            let mut buffer = vec![0; env.get_array_length(&ids)? as usize];
            env.get_int_array_region(&ids, 0, &mut buffer)?;
            let mut found = Vec::new();
            for id in buffer {
                if let Some(device) = env.with_local_frame(8, |env| read_device(env, id))? {
                    found.push(device);
                }
            }
            Ok(found)
        })
        .unwrap_or_default()
    }

    /// The controller with this id, or `None` if it isn't one.
    pub(crate) fn device(id: i32) -> Option<Device> {
        with_jni(|env, _| read_device(env, id)).flatten()
    }

    fn read_device(env: &mut JNIEnv, id: i32) -> jni::errors::Result<Option<Device>> {
        let device = input_device(env, id)?;
        if device.is_null() {
            return Ok(None);
        }
        let sources = env.call_method(&device, "getSources", "()I", &[])?.i()?;
        let has_source = |source: i32| sources & source == source;
        if !has_source(SOURCE_GAMEPAD) && !has_source(SOURCE_JOYSTICK) {
            return Ok(None);
        }
        let name = env
            .call_method(&device, "getName", "()Ljava/lang/String;", &[])?
            .l()?;
        let name: String = env.get_string(&JString::from(name))?.into();
        let vibrator = env
            .call_method(&device, "getVibrator", "()Landroid/os/Vibrator;", &[])?
            .l()?;
        let can_rumble = env.call_method(&vibrator, "hasVibrator", "()Z", &[])?.z()?;
        Ok(Some(Device {
            id,
            name,
            can_rumble,
        }))
    }

    fn input_device<'a>(env: &mut JNIEnv<'a>, id: i32) -> jni::errors::Result<JObject<'a>> {
        env.call_static_method(
            INPUT_DEVICE,
            "getDevice",
            "(I)Landroid/view/InputDevice;",
            &[JValue::Int(id)],
        )?
        .l()
    }

    /// Starts or, with zero strength or duration, stops a controller's rumble.
    /// `left` is the strong motor and `right` the weak one. Replaces what's playing.
    pub(crate) fn vibrate(id: i32, left: f32, right: f32, duration: f32) -> bool {
        with_jni(|env, _| {
            let device = input_device(env, id)?;
            if device.is_null() {
                return Ok(false);
            }
            let vibrator = env
                .call_method(&device, "getVibrator", "()Landroid/os/Vibrator;", &[])?
                .l()?;
            if !env.call_method(&vibrator, "hasVibrator", "()Z", &[])?.z()? {
                return Ok(false);
            }
            let stop = (left <= 0.0 && right <= 0.0) || duration <= 0.0;
            if let Some((manager, motors)) = vibrator_manager(env, &device)?
                && motors.len() >= 2
            {
                if stop {
                    env.call_method(&manager, "cancel", "()V", &[])?;
                } else {
                    play_on_each_motor(env, &manager, &motors, left, right, duration)?;
                }
                return Ok(true);
            }
            if stop {
                env.call_method(&vibrator, "cancel", "()V", &[])?;
            } else {
                let effect = effect(env, left.max(right), duration)?;
                env.call_method(
                    &vibrator,
                    "vibrate",
                    "(Landroid/os/VibrationEffect;)V",
                    &[JValue::Object(&effect)],
                )?;
            }
            Ok(true)
        })
        .unwrap_or(false)
    }

    /// The device's `VibratorManager` and its motor ids, on Android 12 and up.
    fn vibrator_manager<'a>(
        env: &mut JNIEnv<'a>,
        device: &JObject,
    ) -> jni::errors::Result<Option<(JObject<'a>, Vec<i32>)>> {
        if sdk_version(env)? < VIBRATOR_MANAGER_SDK {
            return Ok(None);
        }
        let manager = env
            .call_method(
                device,
                "getVibratorManager",
                "()Landroid/os/VibratorManager;",
                &[],
            )?
            .l()?;
        let ids = env
            .call_method(&manager, "getVibratorIds", "()[I", &[])?
            .l()?;
        let ids = JIntArray::from(ids);
        let mut motors = vec![0; env.get_array_length(&ids)? as usize];
        env.get_int_array_region(&ids, 0, &mut motors)?;
        Ok(Some((manager, motors)))
    }

    fn play_on_each_motor(
        env: &mut JNIEnv,
        manager: &JObject,
        motors: &[i32],
        left: f32,
        right: f32,
        duration: f32,
    ) -> jni::errors::Result<()> {
        const PARALLEL: &str = "Landroid/os/CombinedVibration$ParallelCombination;";
        let parallel = env
            .call_static_method(
                "android/os/CombinedVibration",
                "startParallel",
                format!("(){PARALLEL}"),
                &[],
            )?
            .l()?;
        for (&motor, strength) in motors.iter().zip([left, right]) {
            // A vibration effect can't have a strength of zero.
            if strength > 0.0 {
                let effect = effect(env, strength, duration)?;
                env.call_method(
                    &parallel,
                    "addVibrator",
                    format!("(I{GET_EFFECT}){PARALLEL}"),
                    &[JValue::Int(motor), JValue::Object(&effect)],
                )?;
            }
        }
        let combined = env
            .call_method(
                &parallel,
                "combine",
                "()Landroid/os/CombinedVibration;",
                &[],
            )?
            .l()?;
        env.call_method(
            manager,
            "vibrate",
            "(Landroid/os/CombinedVibration;)V",
            &[JValue::Object(&combined)],
        )?;
        Ok(())
    }

    fn effect<'a>(
        env: &mut JNIEnv<'a>,
        strength: f32,
        duration: f32,
    ) -> jni::errors::Result<JObject<'a>> {
        let amplitude = (strength * 255.0).round().clamp(1.0, 255.0) as i32;
        if duration.is_finite() {
            let millis = (duration as f64 * 1000.0).max(1.0) as i64;
            return env
                .call_static_method(
                    EFFECT,
                    "createOneShot",
                    format!("(JI){GET_EFFECT}"),
                    &[JValue::Long(millis), JValue::Int(amplitude)],
                )?
                .l();
        }
        // One step repeating forever.
        let timings: JLongArray = env.new_long_array(1)?;
        env.set_long_array_region(&timings, 0, &[1000])?;
        let amplitudes: JIntArray = env.new_int_array(1)?;
        env.set_int_array_region(&amplitudes, 0, &[amplitude])?;
        env.call_static_method(
            EFFECT,
            "createWaveform",
            format!("([J[II){GET_EFFECT}"),
            &[
                JValue::Object(&timings),
                JValue::Object(&amplitudes),
                JValue::Int(0),
            ],
        )?
        .l()
    }
}
