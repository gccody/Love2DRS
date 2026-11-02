//! Keyboard, mouse, gamepad and touch input.

#[cfg(target_os = "android")]
pub(crate) mod android;
pub mod gamepad;
#[cfg(all(target_os = "ios", feature = "gamepad"))]
mod ios_pad;
pub mod keyboard;
pub mod mouse;
#[cfg(feature = "gamepad")]
mod sony;
pub mod touch;

pub use gamepad::{Gamepad, GamepadAxis, GamepadButton, GamepadId, Gamepads};
pub use keyboard::{Key, Keyboard};
pub use mouse::{Mouse, MouseButton};
pub use touch::{Touch, TouchId, TouchPoint};
