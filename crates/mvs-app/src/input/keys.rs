//! The keyboard's FKey names: a raw input key's virtual key to the name UI_Manager's key tables and
//! the bindings hold.

/// The FKey name of a keyboard key from raw input: its virtual key, and the E0 prefix (the right-hand Ctrl / Alt,
/// the arrows' and the edit block's own keys, the keypad's Enter) and its scan code (the right Shift).
// Unverified: the build's FWindowsApplication maps the virtual key and its character through
// FInputKeyManager::GetKeyFromCodes (layout-dependent for the letter and punctuation keys); here the virtual key on
// any layout gives the US name of its key (a browser would use the key's position, KeyboardEvent.code).
pub fn key_name(vk: u16, scan: u16, e0: bool) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V",
        "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];
    const PAD: [&str; 10] = [
        "NumPadZero",
        "NumPadOne",
        "NumPadTwo",
        "NumPadThree",
        "NumPadFour",
        "NumPadFive",
        "NumPadSix",
        "NumPadSeven",
        "NumPadEight",
        "NumPadNine",
    ];
    const F: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "F16", "F17",
        "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    Some(match vk {
        0x41..=0x5a => LETTERS[usize::from(vk - 0x41)],
        0x30..=0x39 => DIGITS[usize::from(vk - 0x30)],
        0x60..=0x69 => PAD[usize::from(vk - 0x60)],
        0x70..=0x87 => F[usize::from(vk - 0x70)],
        0x08 => "BackSpace",
        0x09 => "Tab",
        0x0d => "Enter",
        0x10 => {
            if scan == 0x36 {
                "RightShift"
            } else {
                "LeftShift"
            }
        }
        0x11 => {
            if e0 {
                "RightControl"
            } else {
                "LeftControl"
            }
        }
        0x12 => {
            if e0 {
                "RightAlt"
            } else {
                "LeftAlt"
            }
        }
        0xa0 => "LeftShift",
        0xa1 => "RightShift",
        0xa2 => "LeftControl",
        0xa3 => "RightControl",
        0xa4 => "LeftAlt",
        0xa5 => "RightAlt",
        0x13 => "Pause",
        0x14 => "CapsLock",
        0x1b => "Escape",
        0x20 => "SpaceBar",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2d => "Insert",
        0x2e => "Delete",
        0x6a => "Multiply",
        0x6b => "Add",
        0x6d => "Subtract",
        0x6e => "Decimal",
        0x6f => "Divide",
        0x90 => "NumLock",
        0x91 => "ScrollLock",
        0xba => "Semicolon",
        0xbb => "Equals",
        0xbc => "Comma",
        0xbd => "Hyphen",
        0xbe => "Period",
        0xbf => "Slash",
        0xc0 => "Tilde",
        0xdb => "LeftBracket",
        0xdc => "Backslash",
        0xdd => "RightBracket",
        0xde => "Apostrophe",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_keys_name_their_fkeys() {
        assert_eq!(key_name(0x41, 0x1e, false), Some("A"));
        assert_eq!(key_name(0x10, 0x36, false), Some("RightShift"));
        assert_eq!(key_name(0x11, 0x1d, true), Some("RightControl"));
        assert_eq!(key_name(0x25, 0x4b, true), Some("Left"));
        assert_eq!(key_name(0xff, 0, false), None);
    }
}
