//! Keyboard state (the `love.keyboard` equivalent).

use std::sync::Arc;

use winit::keyboard::KeyCode;
use winit::window::Window as WinitWindow;

/// A physical key, named after its position on a US QWERTY layout
/// (so `Key::W` is the same physical key on AZERTY keyboards). Use the
/// `text_input` callback for typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Key {
    /// The A key.
    A,
    /// The B key.
    B,
    /// The C key.
    C,
    /// The D key.
    D,
    /// The E key.
    E,
    /// The F key.
    F,
    /// The G key.
    G,
    /// The H key.
    H,
    /// The I key.
    I,
    /// The J key.
    J,
    /// The K key.
    K,
    /// The L key.
    L,
    /// The M key.
    M,
    /// The N key.
    N,
    /// The O key.
    O,
    /// The P key.
    P,
    /// The Q key.
    Q,
    /// The R key.
    R,
    /// The S key.
    S,
    /// The T key.
    T,
    /// The U key.
    U,
    /// The V key.
    V,
    /// The W key.
    W,
    /// The X key.
    X,
    /// The Y key.
    Y,
    /// The Z key.
    Z,
    /// The 0 key on the main row.
    Num0,
    /// The 1 key on the main row.
    Num1,
    /// The 2 key on the main row.
    Num2,
    /// The 3 key on the main row.
    Num3,
    /// The 4 key on the main row.
    Num4,
    /// The 5 key on the main row.
    Num5,
    /// The 6 key on the main row.
    Num6,
    /// The 7 key on the main row.
    Num7,
    /// The 8 key on the main row.
    Num8,
    /// The 9 key on the main row.
    Num9,
    /// Function key F1.
    F1,
    /// Function key F2.
    F2,
    /// Function key F3.
    F3,
    /// Function key F4.
    F4,
    /// Function key F5.
    F5,
    /// Function key F6.
    F6,
    /// Function key F7.
    F7,
    /// Function key F8.
    F8,
    /// Function key F9.
    F9,
    /// Function key F10.
    F10,
    /// Function key F11.
    F11,
    /// Function key F12.
    F12,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Space bar.
    Space,
    /// Enter / Return (main keyboard).
    Enter,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Left Shift.
    LShift,
    /// Right Shift.
    RShift,
    /// Left Control.
    LCtrl,
    /// Right Control.
    RCtrl,
    /// Left Alt (Option on macOS).
    LAlt,
    /// Right Alt (AltGr on many layouts).
    RAlt,
    /// Left Super (Windows / Command key).
    LSuper,
    /// Right Super (Windows / Command key).
    RSuper,
    /// `-` key.
    Minus,
    /// `=` key.
    Equals,
    /// `[` key.
    LeftBracket,
    /// `]` key.
    RightBracket,
    /// `\` key.
    Backslash,
    /// `;` key.
    Semicolon,
    /// `'` key.
    Quote,
    /// `,` key.
    Comma,
    /// `.` key.
    Period,
    /// `/` key.
    Slash,
    /// `` ` `` key (left of 1).
    Backtick,
    /// Caps Lock.
    CapsLock,
    /// Num Lock.
    NumLock,
    /// Scroll Lock.
    ScrollLock,
    /// Print Screen.
    PrintScreen,
    /// Pause / Break.
    Pause,
    /// Context menu key.
    Menu,
    /// Keypad 0.
    Kp0,
    /// Keypad 1.
    Kp1,
    /// Keypad 2.
    Kp2,
    /// Keypad 3.
    Kp3,
    /// Keypad 4.
    Kp4,
    /// Keypad 5.
    Kp5,
    /// Keypad 6.
    Kp6,
    /// Keypad 7.
    Kp7,
    /// Keypad 8.
    Kp8,
    /// Keypad 9.
    Kp9,
    /// Keypad `+`.
    KpAdd,
    /// Keypad `-`.
    KpSubtract,
    /// Keypad `*`.
    KpMultiply,
    /// Keypad `/`.
    KpDivide,
    /// Keypad Enter.
    KpEnter,
    /// Keypad `.`.
    KpDecimal,
    /// Any key love2drs does not have a name for.
    Unknown,
}

pub(crate) const KEY_COUNT: usize = Key::Unknown as usize + 1;

impl Key {
    /// Every named key.
    pub const ALL: &'static [Key] = &[
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
        Key::Num0,
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::F10,
        Key::F11,
        Key::F12,
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::Space,
        Key::Enter,
        Key::Escape,
        Key::Tab,
        Key::Backspace,
        Key::Delete,
        Key::Insert,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
        Key::LShift,
        Key::RShift,
        Key::LCtrl,
        Key::RCtrl,
        Key::LAlt,
        Key::RAlt,
        Key::LSuper,
        Key::RSuper,
        Key::Minus,
        Key::Equals,
        Key::LeftBracket,
        Key::RightBracket,
        Key::Backslash,
        Key::Semicolon,
        Key::Quote,
        Key::Comma,
        Key::Period,
        Key::Slash,
        Key::Backtick,
        Key::CapsLock,
        Key::NumLock,
        Key::ScrollLock,
        Key::PrintScreen,
        Key::Pause,
        Key::Menu,
        Key::Kp0,
        Key::Kp1,
        Key::Kp2,
        Key::Kp3,
        Key::Kp4,
        Key::Kp5,
        Key::Kp6,
        Key::Kp7,
        Key::Kp8,
        Key::Kp9,
        Key::KpAdd,
        Key::KpSubtract,
        Key::KpMultiply,
        Key::KpDivide,
        Key::KpEnter,
        Key::KpDecimal,
    ];

    pub(crate) fn from_winit(code: KeyCode) -> Key {
        match code {
            KeyCode::KeyA => Key::A,
            KeyCode::KeyB => Key::B,
            KeyCode::KeyC => Key::C,
            KeyCode::KeyD => Key::D,
            KeyCode::KeyE => Key::E,
            KeyCode::KeyF => Key::F,
            KeyCode::KeyG => Key::G,
            KeyCode::KeyH => Key::H,
            KeyCode::KeyI => Key::I,
            KeyCode::KeyJ => Key::J,
            KeyCode::KeyK => Key::K,
            KeyCode::KeyL => Key::L,
            KeyCode::KeyM => Key::M,
            KeyCode::KeyN => Key::N,
            KeyCode::KeyO => Key::O,
            KeyCode::KeyP => Key::P,
            KeyCode::KeyQ => Key::Q,
            KeyCode::KeyR => Key::R,
            KeyCode::KeyS => Key::S,
            KeyCode::KeyT => Key::T,
            KeyCode::KeyU => Key::U,
            KeyCode::KeyV => Key::V,
            KeyCode::KeyW => Key::W,
            KeyCode::KeyX => Key::X,
            KeyCode::KeyY => Key::Y,
            KeyCode::KeyZ => Key::Z,
            KeyCode::Digit0 => Key::Num0,
            KeyCode::Digit1 => Key::Num1,
            KeyCode::Digit2 => Key::Num2,
            KeyCode::Digit3 => Key::Num3,
            KeyCode::Digit4 => Key::Num4,
            KeyCode::Digit5 => Key::Num5,
            KeyCode::Digit6 => Key::Num6,
            KeyCode::Digit7 => Key::Num7,
            KeyCode::Digit8 => Key::Num8,
            KeyCode::Digit9 => Key::Num9,
            KeyCode::F1 => Key::F1,
            KeyCode::F2 => Key::F2,
            KeyCode::F3 => Key::F3,
            KeyCode::F4 => Key::F4,
            KeyCode::F5 => Key::F5,
            KeyCode::F6 => Key::F6,
            KeyCode::F7 => Key::F7,
            KeyCode::F8 => Key::F8,
            KeyCode::F9 => Key::F9,
            KeyCode::F10 => Key::F10,
            KeyCode::F11 => Key::F11,
            KeyCode::F12 => Key::F12,
            KeyCode::ArrowUp => Key::Up,
            KeyCode::ArrowDown => Key::Down,
            KeyCode::ArrowLeft => Key::Left,
            KeyCode::ArrowRight => Key::Right,
            KeyCode::Space => Key::Space,
            KeyCode::Enter => Key::Enter,
            KeyCode::Escape => Key::Escape,
            KeyCode::Tab => Key::Tab,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Delete => Key::Delete,
            KeyCode::Insert => Key::Insert,
            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            KeyCode::ShiftLeft => Key::LShift,
            KeyCode::ShiftRight => Key::RShift,
            KeyCode::ControlLeft => Key::LCtrl,
            KeyCode::ControlRight => Key::RCtrl,
            KeyCode::AltLeft => Key::LAlt,
            KeyCode::AltRight => Key::RAlt,
            KeyCode::SuperLeft => Key::LSuper,
            KeyCode::SuperRight => Key::RSuper,
            KeyCode::Minus => Key::Minus,
            KeyCode::Equal => Key::Equals,
            KeyCode::BracketLeft => Key::LeftBracket,
            KeyCode::BracketRight => Key::RightBracket,
            KeyCode::Backslash => Key::Backslash,
            KeyCode::Semicolon => Key::Semicolon,
            KeyCode::Quote => Key::Quote,
            KeyCode::Comma => Key::Comma,
            KeyCode::Period => Key::Period,
            KeyCode::Slash => Key::Slash,
            KeyCode::Backquote => Key::Backtick,
            KeyCode::CapsLock => Key::CapsLock,
            KeyCode::NumLock => Key::NumLock,
            KeyCode::ScrollLock => Key::ScrollLock,
            KeyCode::PrintScreen => Key::PrintScreen,
            KeyCode::Pause => Key::Pause,
            KeyCode::ContextMenu => Key::Menu,
            KeyCode::Numpad0 => Key::Kp0,
            KeyCode::Numpad1 => Key::Kp1,
            KeyCode::Numpad2 => Key::Kp2,
            KeyCode::Numpad3 => Key::Kp3,
            KeyCode::Numpad4 => Key::Kp4,
            KeyCode::Numpad5 => Key::Kp5,
            KeyCode::Numpad6 => Key::Kp6,
            KeyCode::Numpad7 => Key::Kp7,
            KeyCode::Numpad8 => Key::Kp8,
            KeyCode::Numpad9 => Key::Kp9,
            KeyCode::NumpadAdd => Key::KpAdd,
            KeyCode::NumpadSubtract => Key::KpSubtract,
            KeyCode::NumpadMultiply => Key::KpMultiply,
            KeyCode::NumpadDivide => Key::KpDivide,
            KeyCode::NumpadEnter => Key::KpEnter,
            KeyCode::NumpadDecimal => Key::KpDecimal,
            _ => Key::Unknown,
        }
    }

    /// Lowercase name of the key, e.g. `"space"`, `"a"`, `"lshift"`.
    pub fn name(self) -> &'static str {
        match self {
            Key::A => "a",
            Key::B => "b",
            Key::C => "c",
            Key::D => "d",
            Key::E => "e",
            Key::F => "f",
            Key::G => "g",
            Key::H => "h",
            Key::I => "i",
            Key::J => "j",
            Key::K => "k",
            Key::L => "l",
            Key::M => "m",
            Key::N => "n",
            Key::O => "o",
            Key::P => "p",
            Key::Q => "q",
            Key::R => "r",
            Key::S => "s",
            Key::T => "t",
            Key::U => "u",
            Key::V => "v",
            Key::W => "w",
            Key::X => "x",
            Key::Y => "y",
            Key::Z => "z",
            Key::Num0 => "num0",
            Key::Num1 => "num1",
            Key::Num2 => "num2",
            Key::Num3 => "num3",
            Key::Num4 => "num4",
            Key::Num5 => "num5",
            Key::Num6 => "num6",
            Key::Num7 => "num7",
            Key::Num8 => "num8",
            Key::Num9 => "num9",
            Key::F1 => "f1",
            Key::F2 => "f2",
            Key::F3 => "f3",
            Key::F4 => "f4",
            Key::F5 => "f5",
            Key::F6 => "f6",
            Key::F7 => "f7",
            Key::F8 => "f8",
            Key::F9 => "f9",
            Key::F10 => "f10",
            Key::F11 => "f11",
            Key::F12 => "f12",
            Key::Up => "up",
            Key::Down => "down",
            Key::Left => "left",
            Key::Right => "right",
            Key::Space => "space",
            Key::Enter => "enter",
            Key::Escape => "escape",
            Key::Tab => "tab",
            Key::Backspace => "backspace",
            Key::Delete => "delete",
            Key::Insert => "insert",
            Key::Home => "home",
            Key::End => "end",
            Key::PageUp => "pageup",
            Key::PageDown => "pagedown",
            Key::LShift => "lshift",
            Key::RShift => "rshift",
            Key::LCtrl => "lctrl",
            Key::RCtrl => "rctrl",
            Key::LAlt => "lalt",
            Key::RAlt => "ralt",
            Key::LSuper => "lsuper",
            Key::RSuper => "rsuper",
            Key::Minus => "minus",
            Key::Equals => "equals",
            Key::LeftBracket => "leftbracket",
            Key::RightBracket => "rightbracket",
            Key::Backslash => "backslash",
            Key::Semicolon => "semicolon",
            Key::Quote => "quote",
            Key::Comma => "comma",
            Key::Period => "period",
            Key::Slash => "slash",
            Key::Backtick => "backtick",
            Key::CapsLock => "capslock",
            Key::NumLock => "numlock",
            Key::ScrollLock => "scrolllock",
            Key::PrintScreen => "printscreen",
            Key::Pause => "pause",
            Key::Menu => "menu",
            Key::Kp0 => "kp0",
            Key::Kp1 => "kp1",
            Key::Kp2 => "kp2",
            Key::Kp3 => "kp3",
            Key::Kp4 => "kp4",
            Key::Kp5 => "kp5",
            Key::Kp6 => "kp6",
            Key::Kp7 => "kp7",
            Key::Kp8 => "kp8",
            Key::Kp9 => "kp9",
            Key::KpAdd => "kpadd",
            Key::KpSubtract => "kpsubtract",
            Key::KpMultiply => "kpmultiply",
            Key::KpDivide => "kpdivide",
            Key::KpEnter => "kpenter",
            Key::KpDecimal => "kpdecimal",
            Key::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Keyboard state, updated before each `update`. Reach it through `ctx.keyboard`.
pub struct Keyboard {
    down: [bool; KEY_COUNT],
    pressed: [bool; KEY_COUNT],
    released: [bool; KEY_COUNT],
    pub(crate) key_repeat: bool,
    pub(crate) text_input: bool,
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    window: Arc<WinitWindow>,
}

impl Keyboard {
    pub(crate) fn new(window: Arc<WinitWindow>) -> Self {
        Keyboard {
            window,
            down: [false; KEY_COUNT],
            pressed: [false; KEY_COUNT],
            released: [false; KEY_COUNT],
            key_repeat: false,
            text_input: true,
        }
    }

    /// Whether `key` is currently held down.
    #[inline]
    pub fn is_down(&self, key: Key) -> bool {
        self.down[key as usize]
    }

    /// Whether any of `keys` is held down (`love.keyboard.isDown(a, b, ...)`).
    pub fn is_any_down(&self, keys: &[Key]) -> bool {
        keys.iter().any(|k| self.is_down(*k))
    }

    /// Whether `key` went down since the previous frame.
    #[inline]
    pub fn was_pressed(&self, key: Key) -> bool {
        self.pressed[key as usize]
    }

    /// Whether `key` was let go since the previous frame.
    #[inline]
    pub fn was_released(&self, key: Key) -> bool {
        self.released[key as usize]
    }

    /// Either shift key is down.
    pub fn shift(&self) -> bool {
        self.is_any_down(&[Key::LShift, Key::RShift])
    }

    /// Either control key is down.
    pub fn ctrl(&self) -> bool {
        self.is_any_down(&[Key::LCtrl, Key::RCtrl])
    }

    /// Either alt key is down.
    pub fn alt(&self) -> bool {
        self.is_any_down(&[Key::LAlt, Key::RAlt])
    }

    /// -1, 0 or 1 depending on which of two keys is held: handy for movement.
    pub fn axis(&self, negative: Key, positive: Key) -> f32 {
        (self.is_down(positive) as i32 - self.is_down(negative) as i32) as f32
    }

    /// Deliver repeated `key_pressed` events while a key is held (off by default, like LÖVE).
    pub fn set_key_repeat(&mut self, enabled: bool) {
        self.key_repeat = enabled;
    }

    /// Whether key repeat is enabled.
    pub fn has_key_repeat(&self) -> bool {
        self.key_repeat
    }

    /// Enable or disable `text_input` callbacks (on by default).
    pub fn set_text_input(&mut self, enabled: bool) {
        self.text_input = enabled;
    }

    /// Shows or hides the on-screen keyboard on Android and iOS, and does
    /// nothing on desktop. Typing arrives through the `text_input` callback, and
    /// `Key::Backspace` and `Key::Enter` through `key_pressed`. While the keyboard
    /// is up, [`safe_area`](crate::window::Window::safe_area) leaves it out, so
    /// keep your text field inside that.
    ///
    /// The user can also close the keyboard without the game finding out, and
    /// Android may not show it while a hardware keyboard is connected.
    ///
    /// On Android, love2drs only receives text that maps to key presses, which
    /// covers Latin letters, digits and punctuation. Emoji, other scripts and
    /// autocorrect or swipe typing may not arrive there.
    pub fn set_screen_keyboard(&mut self, visible: bool) {
        #[cfg(target_os = "android")]
        {
            let app = crate::platform::android_app();
            if visible {
                app.show_soft_input(false);
            } else {
                app.hide_soft_input(false);
            }
        }
        #[cfg(target_os = "ios")]
        self.window.set_ime_allowed(visible);
        let _ = visible;
    }

    /// Whether this platform has an on-screen keyboard
    /// (`love.keyboard.hasScreenKeyboard`).
    pub fn has_screen_keyboard(&self) -> bool {
        crate::platform::MOBILE
    }

    pub(crate) fn press(&mut self, key: Key) {
        let i = key as usize;
        if !self.down[i] {
            self.pressed[i] = true;
        }
        self.down[i] = true;
    }

    pub(crate) fn release(&mut self, key: Key) {
        let i = key as usize;
        if self.down[i] {
            self.released[i] = true;
        }
        self.down[i] = false;
    }

    /// Releases everything (e.g. when the window loses focus).
    pub(crate) fn release_all(&mut self) {
        for i in 0..KEY_COUNT {
            if self.down[i] {
                self.released[i] = true;
                self.down[i] = false;
            }
        }
    }

    pub(crate) fn end_frame(&mut self) {
        self.pressed = [false; KEY_COUNT];
        self.released = [false; KEY_COUNT];
    }
}
