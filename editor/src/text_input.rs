//! Keyboard text-entry helpers (deleted in PR2 when egui owns text input).

use juni::prelude::*;

/// Collect characters suitable for typing a file path: letters, digits, and
/// common path separators/punctuation. Letters are lowercase unless `shift`.
pub(crate) fn collect_path_chars(ctx: &Context, shift: bool) -> String {
    let mut s = String::new();
    for &(key, lo, hi) in &[
        (Key::A, 'a', 'A'),
        (Key::B, 'b', 'B'),
        (Key::C, 'c', 'C'),
        (Key::D, 'd', 'D'),
        (Key::E, 'e', 'E'),
        (Key::F, 'f', 'F'),
        (Key::G, 'g', 'G'),
        (Key::H, 'h', 'H'),
        (Key::I, 'i', 'I'),
        (Key::J, 'j', 'J'),
        (Key::K, 'k', 'K'),
        (Key::L, 'l', 'L'),
        (Key::M, 'm', 'M'),
        (Key::N, 'n', 'N'),
        (Key::O, 'o', 'O'),
        (Key::P, 'p', 'P'),
        (Key::Q, 'q', 'Q'),
        (Key::R, 'r', 'R'),
        (Key::S, 's', 'S'),
        (Key::T, 't', 'T'),
        (Key::U, 'u', 'U'),
        (Key::V, 'v', 'V'),
        (Key::W, 'w', 'W'),
        (Key::X, 'x', 'X'),
        (Key::Y, 'y', 'Y'),
        (Key::Z, 'z', 'Z'),
        (Key::Num0, '0', ')'),
        (Key::Num1, '1', '!'),
        (Key::Num2, '2', '@'),
        (Key::Num3, '3', '#'),
        (Key::Num4, '4', '$'),
        (Key::Num5, '5', '%'),
        (Key::Num6, '6', '^'),
        (Key::Num7, '7', '&'),
        (Key::Num8, '8', '*'),
        (Key::Num9, '9', '('),
        (Key::Space, ' ', ' '),
        (Key::Minus, '-', '_'),
        (Key::Period, '.', '>'),
        (Key::Comma, ',', '<'),
        (Key::Slash, '/', '?'),
        (Key::Backslash, '\\', '|'),
        (Key::Semicolon, ';', ':'),
        (Key::Apostrophe, '\'', '"'),
        (Key::Equal, '=', '+'),
        (Key::BracketLeft, '[', '{'),
        (Key::BracketRight, ']', '}'),
        (Key::Grave, '`', '~'),
    ] {
        if ctx.is_key_pressed(key) {
            s.push(if shift { hi } else { lo });
        }
    }
    s
}

/// Collect all printable characters pressed this frame (letters, digits, space,
/// comma, period). Letters are lowercase unless `shift` is true.
pub(crate) fn collect_typed_chars(ctx: &Context, shift: bool) -> String {
    let mut s = String::new();
    for &(key, lo, hi) in &[
        (Key::A, 'a', 'A'),
        (Key::B, 'b', 'B'),
        (Key::C, 'c', 'C'),
        (Key::D, 'd', 'D'),
        (Key::E, 'e', 'E'),
        (Key::F, 'f', 'F'),
        (Key::G, 'g', 'G'),
        (Key::H, 'h', 'H'),
        (Key::I, 'i', 'I'),
        (Key::J, 'j', 'J'),
        (Key::K, 'k', 'K'),
        (Key::L, 'l', 'L'),
        (Key::M, 'm', 'M'),
        (Key::N, 'n', 'N'),
        (Key::O, 'o', 'O'),
        (Key::P, 'p', 'P'),
        (Key::Q, 'q', 'Q'),
        (Key::R, 'r', 'R'),
        (Key::S, 's', 'S'),
        (Key::T, 't', 'T'),
        (Key::U, 'u', 'U'),
        (Key::V, 'v', 'V'),
        (Key::W, 'w', 'W'),
        (Key::X, 'x', 'X'),
        (Key::Y, 'y', 'Y'),
        (Key::Z, 'z', 'Z'),
        (Key::Num0, '0', '0'),
        (Key::Num1, '1', '1'),
        (Key::Num2, '2', '2'),
        (Key::Num3, '3', '3'),
        (Key::Num4, '4', '4'),
        (Key::Num5, '5', '5'),
        (Key::Num6, '6', '6'),
        (Key::Num7, '7', '7'),
        (Key::Num8, '8', '8'),
        (Key::Num9, '9', '9'),
        (Key::Space, ' ', ' '),
        (Key::Comma, ',', '<'),
        (Key::Period, '.', '>'),
    ] {
        if ctx.is_key_pressed(key) {
            s.push(if shift { hi } else { lo });
        }
    }
    s
}
