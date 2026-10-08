//! Key-name conversions between our config, egui (popup keys) and global-hotkey.

use crate::config::KeyCombo;
use eframe::egui::{self, Key};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};

pub fn egui_key(name: &str) -> Option<Key> {
    let alias = match name {
        "ArrowUp" | "Up" => Some(Key::ArrowUp),
        "ArrowDown" | "Down" => Some(Key::ArrowDown),
        "ArrowLeft" | "Left" => Some(Key::ArrowLeft),
        "ArrowRight" | "Right" => Some(Key::ArrowRight),
        "Enter" | "Return" => Some(Key::Enter),
        "Escape" | "Esc" => Some(Key::Escape),
        "Tab" => Some(Key::Tab),
        "Space" => Some(Key::Space),
        _ => None,
    };
    alias.or_else(|| Key::from_name(name))
}

/// Did the user just press this combo? Removes the key event so text boxes don't also see it.
pub fn consume(input: &mut egui::InputState, k: &KeyCombo) -> bool {
    let Some(key) = egui_key(&k.key) else { return false };
    let mods = egui::Modifiers { alt: k.alt, ctrl: k.ctrl, shift: k.shift, mac_cmd: false, command: k.ctrl };
    input.consume_key(mods, key)
}

pub fn consume_any(input: &mut egui::InputState, list: &[KeyCombo]) -> bool {
    list.iter().any(|k| consume(input, k))
}

/// Convert an egui key press into the name we store.
pub fn name_of(key: Key) -> String {
    match key {
        Key::ArrowUp => "ArrowUp".into(),
        Key::ArrowDown => "ArrowDown".into(),
        Key::ArrowLeft => "ArrowLeft".into(),
        Key::ArrowRight => "ArrowRight".into(),
        other => other.name().to_string(),
    }
}

fn code(name: &str) -> Option<Code> {
    use Code::*;
    let c = match name.to_ascii_uppercase().as_str() {
        "A" => KeyA,
        "B" => KeyB,
        "C" => KeyC,
        "D" => KeyD,
        "E" => KeyE,
        "F" => KeyF,
        "G" => KeyG,
        "H" => KeyH,
        "I" => KeyI,
        "J" => KeyJ,
        "K" => KeyK,
        "L" => KeyL,
        "M" => KeyM,
        "N" => KeyN,
        "O" => KeyO,
        "P" => KeyP,
        "Q" => KeyQ,
        "R" => KeyR,
        "S" => KeyS,
        "T" => KeyT,
        "U" => KeyU,
        "V" => KeyV,
        "W" => KeyW,
        "X" => KeyX,
        "Y" => KeyY,
        "Z" => KeyZ,
        "0" => Digit0,
        "1" => Digit1,
        "2" => Digit2,
        "3" => Digit3,
        "4" => Digit4,
        "5" => Digit5,
        "6" => Digit6,
        "7" => Digit7,
        "8" => Digit8,
        "9" => Digit9,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "F9" => F9,
        "F10" => F10,
        "F11" => F11,
        "F12" => F12,
        "SPACE" => Space,
        "ENTER" => Enter,
        "TAB" => Tab,
        "ESCAPE" => Escape,
        "BACKSPACE" => Backspace,
        "INSERT" => Insert,
        "DELETE" => Delete,
        "HOME" => Home,
        "END" => End,
        "PAGEUP" => PageUp,
        "PAGEDOWN" => PageDown,
        "ARROWUP" | "UP" => ArrowUp,
        "ARROWDOWN" | "DOWN" => ArrowDown,
        "ARROWLEFT" | "LEFT" => ArrowLeft,
        "ARROWRIGHT" | "RIGHT" => ArrowRight,
        "MINUS" => Minus,
        "EQUALS" | "PLUS" => Equal,
        "BACKTICK" => Backquote,
        "COMMA" => Comma,
        "PERIOD" => Period,
        "SLASH" => Slash,
        "BACKSLASH" => Backslash,
        "SEMICOLON" => Semicolon,
        "QUOTE" => Quote,
        "OPENBRACKET" => BracketLeft,
        "CLOSEBRACKET" => BracketRight,
        _ => return None,
    };
    Some(c)
}

pub fn global_hotkey(k: &KeyCombo) -> Option<HotKey> {
    if k.key.is_empty() {
        return None;
    }
    let c = code(&k.key)?;
    let mut m = Modifiers::empty();
    if k.ctrl {
        m |= Modifiers::CONTROL;
    }
    if k.alt {
        m |= Modifiers::ALT;
    }
    if k.shift {
        m |= Modifiers::SHIFT;
    }
    if k.win {
        m |= Modifiers::SUPER;
    }
    Some(HotKey::new(if m.is_empty() { None } else { Some(m) }, c))
}

/// Keys that can be used for a global hotkey (shown in settings).
pub fn hotkey_key_supported(name: &str) -> bool {
    code(name).is_some()
}
