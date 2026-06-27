//! Keyboard and mouse input, exposed through [`Context`](crate::Context) with a
//! Raylib-inspired API (`is_key_pressed`, `is_mouse_button_down`,
//! `mouse_position`, …).

use crate::math::Vec2D;
use std::collections::HashSet;
use winit::keyboard::KeyCode;

/// A keyboard key. Mirrors Raylib's `KeyboardKey` for the commonly used keys.
///
/// Pass these to [`Context::is_key_pressed`](crate::Context::is_key_pressed)
/// and friends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Key {
    // Letters.
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    // Number row.
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    // Function keys.
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    // Navigation / editing.
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    // Punctuation.
    BracketLeft,
    BracketRight,
    Comma,
    Period,
    // Modifiers.
    LeftShift,
    RightShift,
    LeftControl,
    RightControl,
    LeftAlt,
    RightAlt,
}

impl Key {
    /// Map a winit [`KeyCode`] to a [`Key`], if we expose it. Unmapped keys
    /// return `None` and are ignored.
    pub(crate) fn from_code(code: KeyCode) -> Option<Key> {
        use KeyCode as C;
        Some(match code {
            C::KeyA => Key::A,
            C::KeyB => Key::B,
            C::KeyC => Key::C,
            C::KeyD => Key::D,
            C::KeyE => Key::E,
            C::KeyF => Key::F,
            C::KeyG => Key::G,
            C::KeyH => Key::H,
            C::KeyI => Key::I,
            C::KeyJ => Key::J,
            C::KeyK => Key::K,
            C::KeyL => Key::L,
            C::KeyM => Key::M,
            C::KeyN => Key::N,
            C::KeyO => Key::O,
            C::KeyP => Key::P,
            C::KeyQ => Key::Q,
            C::KeyR => Key::R,
            C::KeyS => Key::S,
            C::KeyT => Key::T,
            C::KeyU => Key::U,
            C::KeyV => Key::V,
            C::KeyW => Key::W,
            C::KeyX => Key::X,
            C::KeyY => Key::Y,
            C::KeyZ => Key::Z,

            C::Digit0 => Key::Num0,
            C::Digit1 => Key::Num1,
            C::Digit2 => Key::Num2,
            C::Digit3 => Key::Num3,
            C::Digit4 => Key::Num4,
            C::Digit5 => Key::Num5,
            C::Digit6 => Key::Num6,
            C::Digit7 => Key::Num7,
            C::Digit8 => Key::Num8,
            C::Digit9 => Key::Num9,

            C::F1 => Key::F1,
            C::F2 => Key::F2,
            C::F3 => Key::F3,
            C::F4 => Key::F4,
            C::F5 => Key::F5,
            C::F6 => Key::F6,
            C::F7 => Key::F7,
            C::F8 => Key::F8,
            C::F9 => Key::F9,
            C::F10 => Key::F10,
            C::F11 => Key::F11,
            C::F12 => Key::F12,

            C::Space => Key::Space,
            C::Enter => Key::Enter,
            C::Escape => Key::Escape,
            C::Tab => Key::Tab,
            C::Backspace => Key::Backspace,
            C::Delete => Key::Delete,
            C::ArrowLeft => Key::Left,
            C::ArrowRight => Key::Right,
            C::ArrowUp => Key::Up,
            C::ArrowDown => Key::Down,

            C::BracketLeft => Key::BracketLeft,
            C::BracketRight => Key::BracketRight,
            C::Comma => Key::Comma,
            C::Period => Key::Period,

            C::ShiftLeft => Key::LeftShift,
            C::ShiftRight => Key::RightShift,
            C::ControlLeft => Key::LeftControl,
            C::ControlRight => Key::RightControl,
            C::AltLeft => Key::LeftAlt,
            C::AltRight => Key::RightAlt,

            _ => return None,
        })
    }
}

/// A mouse button. Mirrors Raylib's `MouseButton` for the buttons winit
/// reports.
///
/// Pass these to
/// [`Context::is_mouse_button_pressed`](crate::Context::is_mouse_button_pressed)
/// and friends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButton {
    /// Map a winit [`MouseButton`](winit::event::MouseButton) to ours, if we
    /// expose it. Unmapped buttons return `None` and are ignored.
    pub(crate) fn from_winit(button: winit::event::MouseButton) -> Option<MouseButton> {
        use winit::event::MouseButton as B;
        Some(match button {
            B::Left => MouseButton::Left,
            B::Right => MouseButton::Right,
            B::Middle => MouseButton::Middle,
            B::Back => MouseButton::Back,
            B::Forward => MouseButton::Forward,
            B::Other(_) => return None,
        })
    }
}

/// Per-frame keyboard and mouse state. The engine feeds window events into the
/// `process*` methods and calls [`new_frame`] after the frame's updates, so
/// edge events (`pressed`/`released`) and per-frame deltas hold for exactly one
/// frame.
///
/// [`new_frame`]: Input::new_frame
#[derive(Default)]
pub struct Input {
    down: HashSet<Key>,
    pressed: HashSet<Key>,
    released: HashSet<Key>,

    mouse_down: HashSet<MouseButton>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_released: HashSet<MouseButton>,
    /// Cursor position in physical window pixels (untransformed).
    mouse_pos: Vec2D,
    /// Cursor movement accumulated this frame, in physical pixels.
    mouse_delta: Vec2D,
    /// Wheel movement accumulated this frame.
    wheel: f32,
}

impl Input {
    /// Record a key state change from a window event.
    pub fn process(&mut self, key: Key, is_down: bool) {
        if is_down {
            // `insert` returns false if it was already down: this is an OS key
            // repeat, not a fresh press, so don't re-fire `pressed`.
            if self.down.insert(key) {
                self.pressed.insert(key);
            }
        } else if self.down.remove(&key) {
            self.released.insert(key);
        }
    }

    /// Record a mouse button state change from a window event.
    pub fn process_mouse_button(&mut self, button: MouseButton, is_down: bool) {
        if is_down {
            if self.mouse_down.insert(button) {
                self.mouse_pressed.insert(button);
            }
        } else if self.mouse_down.remove(&button) {
            self.mouse_released.insert(button);
        }
    }

    /// Record a new cursor position (physical pixels), accumulating the delta.
    pub fn set_mouse_pos(&mut self, x: f32, y: f32) {
        let new = Vec2D::new(x, y);
        self.mouse_delta += new - self.mouse_pos;
        self.mouse_pos = new;
    }

    /// Accumulate wheel movement for this frame.
    pub fn add_wheel(&mut self, amount: f32) {
        self.wheel += amount;
    }

    /// Clear the one-frame edge sets and per-frame deltas. Call once per
    /// rendered frame, after the fixed-update loop has had a chance to observe
    /// them.
    pub fn new_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse_pressed.clear();
        self.mouse_released.clear();
        self.mouse_delta = Vec2D::ZERO;
        self.wheel = 0.0;
    }

    /// `true` while `key` is held down.
    pub fn is_key_down(&self, key: Key) -> bool {
        self.down.contains(&key)
    }

    /// `true` on the frame `key` was pressed.
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.pressed.contains(&key)
    }

    /// `true` on the frame `key` was released.
    pub fn is_key_released(&self, key: Key) -> bool {
        self.released.contains(&key)
    }

    /// `true` while `button` is held down.
    pub fn is_mouse_button_down(&self, button: MouseButton) -> bool {
        self.mouse_down.contains(&button)
    }

    /// `true` on the frame `button` was pressed.
    pub fn is_mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed.contains(&button)
    }

    /// `true` on the frame `button` was released.
    pub fn is_mouse_button_released(&self, button: MouseButton) -> bool {
        self.mouse_released.contains(&button)
    }

    /// Cursor position in physical window pixels (untransformed).
    pub fn mouse_pos(&self) -> Vec2D {
        self.mouse_pos
    }

    /// Cursor movement this frame, in physical pixels.
    pub fn mouse_delta(&self) -> Vec2D {
        self.mouse_delta
    }

    /// Wheel movement this frame.
    pub fn wheel(&self) -> f32 {
        self.wheel
    }
}
