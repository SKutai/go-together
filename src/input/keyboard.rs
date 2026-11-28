//! Physical keys in, semantic [`GameAction`]s out.
//!
//! # Responsibility
//!
//! [`GameAction`] is the semantic vocabulary the rest of the crate speaks -
//! "Player One Select", not "Space" - and it belongs to [`crate::input`], not
//! here. This module owns only the keyboard half: [`KeyboardKey`], the
//! physical allow-list, and [`KeyboardBindings`], the mapping from it to
//! [`GameAction`]. It is one of two sibling device modules under
//! [`crate::input`]; it knows nothing about controllers - which describe
//! themselves in their own [`ProfileAction`](crate::input::controller::ProfileAction)
//! namespace - and nothing about how bindings are persisted.
//!
//! # Rebinding rules
//!
//! [`KeyboardKey`] is a deliberately small allow-list, and the three keys
//! the interface reads for itself during live play are not on it: Escape,
//! which cancels a capture and steps back from every screen; F1, which
//! opens and closes the Help modal on the title screen and during a Match;
//! and F11, which switches the window to fullscreen and back on every
//! screen of a desktop build. A browser keeps F11 for its own fullscreen,
//! and the web page hands it to the browser before the game sees it. None
//! of the three can be bound, persisted, or even represented as a
//! `KeyboardKey`. That is what lets each be read raw during live play, when
//! gameplay reads every bindable key, and why a settings file naming any of
//! them fails to parse rather than loading a key the interface would then read
//! twice. [`KeyboardKey::from_key_code`] refuses the three as reserved, and
//! every other key outside the list as unsupported, so that capture can ignore
//! the first silently and explain the second. Assigning a key that another
//! action already uses swaps the two keys and returns that other action, so
//! every action always has a distinct key.
//!
//! On a Mac, where the default settings keep F11 itself, Control-Command-F
//! switches fullscreen too. It is no reserved key: F stays bindable, since a
//! chord is no binding, and `crate::app::window` keeps the chord's F from the
//! binding while it is held. Left Control is bindable, though, so during an
//! armed capture the chord's Control is taken as the binding before its F
//! arrives; nothing can tell that a chord is coming, so a Mac player switches
//! fullscreen there with the window's green button.
//!
//! The `#[serde]` names on [`GameAction`] and [`KeyboardKey`] are part of the
//! saved configuration format, but this module never touches the file:
//! [`crate::settings`] owns it and passes the entries it read here for
//! validation through [`KeyboardBindings::try_from_entries`].

use std::collections::HashSet;
use std::fmt;

use bevy::prelude::{KeyCode, Resource};
use serde::{Deserialize, Serialize};

use super::GameAction;

/// Font-safe, serializable key allow-list exposed by Settings.
///
/// Escape, F1 and F11 are absent on purpose - see the module docs. `f1`
/// and `f11` were valid serde names in settings files written before F1
/// became the Help key and F11 the fullscreen key;
/// `crate::settings::schema` records what such a file loads as now.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeyboardKey {
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
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    UpArrow,
    DownArrow,
    LeftArrow,
    RightArrow,
    Space,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Backtick,
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    LeftShift,
    RightShift,
    LeftControl,
    RightControl,
    LeftAlt,
    RightAlt,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F12,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
}

impl KeyboardKey {
    pub(crate) const fn key_code(self) -> KeyCode {
        match self {
            Self::A => KeyCode::KeyA,
            Self::B => KeyCode::KeyB,
            Self::C => KeyCode::KeyC,
            Self::D => KeyCode::KeyD,
            Self::E => KeyCode::KeyE,
            Self::F => KeyCode::KeyF,
            Self::G => KeyCode::KeyG,
            Self::H => KeyCode::KeyH,
            Self::I => KeyCode::KeyI,
            Self::J => KeyCode::KeyJ,
            Self::K => KeyCode::KeyK,
            Self::L => KeyCode::KeyL,
            Self::M => KeyCode::KeyM,
            Self::N => KeyCode::KeyN,
            Self::O => KeyCode::KeyO,
            Self::P => KeyCode::KeyP,
            Self::Q => KeyCode::KeyQ,
            Self::R => KeyCode::KeyR,
            Self::S => KeyCode::KeyS,
            Self::T => KeyCode::KeyT,
            Self::U => KeyCode::KeyU,
            Self::V => KeyCode::KeyV,
            Self::W => KeyCode::KeyW,
            Self::X => KeyCode::KeyX,
            Self::Y => KeyCode::KeyY,
            Self::Z => KeyCode::KeyZ,
            Self::Digit0 => KeyCode::Digit0,
            Self::Digit1 => KeyCode::Digit1,
            Self::Digit2 => KeyCode::Digit2,
            Self::Digit3 => KeyCode::Digit3,
            Self::Digit4 => KeyCode::Digit4,
            Self::Digit5 => KeyCode::Digit5,
            Self::Digit6 => KeyCode::Digit6,
            Self::Digit7 => KeyCode::Digit7,
            Self::Digit8 => KeyCode::Digit8,
            Self::Digit9 => KeyCode::Digit9,
            Self::UpArrow => KeyCode::ArrowUp,
            Self::DownArrow => KeyCode::ArrowDown,
            Self::LeftArrow => KeyCode::ArrowLeft,
            Self::RightArrow => KeyCode::ArrowRight,
            Self::Space => KeyCode::Space,
            Self::Enter => KeyCode::Enter,
            Self::Tab => KeyCode::Tab,
            Self::Backspace => KeyCode::Backspace,
            Self::Delete => KeyCode::Delete,
            Self::Insert => KeyCode::Insert,
            Self::Home => KeyCode::Home,
            Self::End => KeyCode::End,
            Self::PageUp => KeyCode::PageUp,
            Self::PageDown => KeyCode::PageDown,
            Self::Backtick => KeyCode::Backquote,
            Self::Minus => KeyCode::Minus,
            Self::Equal => KeyCode::Equal,
            Self::LeftBracket => KeyCode::BracketLeft,
            Self::RightBracket => KeyCode::BracketRight,
            Self::Backslash => KeyCode::Backslash,
            Self::Semicolon => KeyCode::Semicolon,
            Self::Quote => KeyCode::Quote,
            Self::Comma => KeyCode::Comma,
            Self::Period => KeyCode::Period,
            Self::Slash => KeyCode::Slash,
            Self::LeftShift => KeyCode::ShiftLeft,
            Self::RightShift => KeyCode::ShiftRight,
            Self::LeftControl => KeyCode::ControlLeft,
            Self::RightControl => KeyCode::ControlRight,
            Self::LeftAlt => KeyCode::AltLeft,
            Self::RightAlt => KeyCode::AltRight,
            Self::F2 => KeyCode::F2,
            Self::F3 => KeyCode::F3,
            Self::F4 => KeyCode::F4,
            Self::F5 => KeyCode::F5,
            Self::F6 => KeyCode::F6,
            Self::F7 => KeyCode::F7,
            Self::F8 => KeyCode::F8,
            Self::F9 => KeyCode::F9,
            Self::F10 => KeyCode::F10,
            Self::F12 => KeyCode::F12,
            Self::Numpad0 => KeyCode::Numpad0,
            Self::Numpad1 => KeyCode::Numpad1,
            Self::Numpad2 => KeyCode::Numpad2,
            Self::Numpad3 => KeyCode::Numpad3,
            Self::Numpad4 => KeyCode::Numpad4,
            Self::Numpad5 => KeyCode::Numpad5,
            Self::Numpad6 => KeyCode::Numpad6,
            Self::Numpad7 => KeyCode::Numpad7,
            Self::Numpad8 => KeyCode::Numpad8,
            Self::Numpad9 => KeyCode::Numpad9,
            Self::NumpadAdd => KeyCode::NumpadAdd,
            Self::NumpadSubtract => KeyCode::NumpadSubtract,
            Self::NumpadMultiply => KeyCode::NumpadMultiply,
            Self::NumpadDivide => KeyCode::NumpadDivide,
            Self::NumpadDecimal => KeyCode::NumpadDecimal,
            Self::NumpadEnter => KeyCode::NumpadEnter,
        }
    }

    pub(crate) fn from_key_code(key_code: KeyCode) -> Result<Self, KeyCaptureError> {
        let key = match key_code {
            KeyCode::KeyA => Self::A,
            KeyCode::KeyB => Self::B,
            KeyCode::KeyC => Self::C,
            KeyCode::KeyD => Self::D,
            KeyCode::KeyE => Self::E,
            KeyCode::KeyF => Self::F,
            KeyCode::KeyG => Self::G,
            KeyCode::KeyH => Self::H,
            KeyCode::KeyI => Self::I,
            KeyCode::KeyJ => Self::J,
            KeyCode::KeyK => Self::K,
            KeyCode::KeyL => Self::L,
            KeyCode::KeyM => Self::M,
            KeyCode::KeyN => Self::N,
            KeyCode::KeyO => Self::O,
            KeyCode::KeyP => Self::P,
            KeyCode::KeyQ => Self::Q,
            KeyCode::KeyR => Self::R,
            KeyCode::KeyS => Self::S,
            KeyCode::KeyT => Self::T,
            KeyCode::KeyU => Self::U,
            KeyCode::KeyV => Self::V,
            KeyCode::KeyW => Self::W,
            KeyCode::KeyX => Self::X,
            KeyCode::KeyY => Self::Y,
            KeyCode::KeyZ => Self::Z,
            KeyCode::Digit0 => Self::Digit0,
            KeyCode::Digit1 => Self::Digit1,
            KeyCode::Digit2 => Self::Digit2,
            KeyCode::Digit3 => Self::Digit3,
            KeyCode::Digit4 => Self::Digit4,
            KeyCode::Digit5 => Self::Digit5,
            KeyCode::Digit6 => Self::Digit6,
            KeyCode::Digit7 => Self::Digit7,
            KeyCode::Digit8 => Self::Digit8,
            KeyCode::Digit9 => Self::Digit9,
            KeyCode::ArrowUp => Self::UpArrow,
            KeyCode::ArrowDown => Self::DownArrow,
            KeyCode::ArrowLeft => Self::LeftArrow,
            KeyCode::ArrowRight => Self::RightArrow,
            KeyCode::Space => Self::Space,
            KeyCode::Enter => Self::Enter,
            KeyCode::Tab => Self::Tab,
            KeyCode::Backspace => Self::Backspace,
            KeyCode::Delete => Self::Delete,
            KeyCode::Insert => Self::Insert,
            KeyCode::Home => Self::Home,
            KeyCode::End => Self::End,
            KeyCode::PageUp => Self::PageUp,
            KeyCode::PageDown => Self::PageDown,
            KeyCode::Backquote => Self::Backtick,
            KeyCode::Minus => Self::Minus,
            KeyCode::Equal => Self::Equal,
            KeyCode::BracketLeft => Self::LeftBracket,
            KeyCode::BracketRight => Self::RightBracket,
            KeyCode::Backslash => Self::Backslash,
            KeyCode::Semicolon => Self::Semicolon,
            KeyCode::Quote => Self::Quote,
            KeyCode::Comma => Self::Comma,
            KeyCode::Period => Self::Period,
            KeyCode::Slash => Self::Slash,
            KeyCode::ShiftLeft => Self::LeftShift,
            KeyCode::ShiftRight => Self::RightShift,
            KeyCode::ControlLeft => Self::LeftControl,
            KeyCode::ControlRight => Self::RightControl,
            KeyCode::AltLeft => Self::LeftAlt,
            KeyCode::AltRight => Self::RightAlt,
            KeyCode::F2 => Self::F2,
            KeyCode::F3 => Self::F3,
            KeyCode::F4 => Self::F4,
            KeyCode::F5 => Self::F5,
            KeyCode::F6 => Self::F6,
            KeyCode::F7 => Self::F7,
            KeyCode::F8 => Self::F8,
            KeyCode::F9 => Self::F9,
            KeyCode::F10 => Self::F10,
            KeyCode::F12 => Self::F12,
            KeyCode::Numpad0 => Self::Numpad0,
            KeyCode::Numpad1 => Self::Numpad1,
            KeyCode::Numpad2 => Self::Numpad2,
            KeyCode::Numpad3 => Self::Numpad3,
            KeyCode::Numpad4 => Self::Numpad4,
            KeyCode::Numpad5 => Self::Numpad5,
            KeyCode::Numpad6 => Self::Numpad6,
            KeyCode::Numpad7 => Self::Numpad7,
            KeyCode::Numpad8 => Self::Numpad8,
            KeyCode::Numpad9 => Self::Numpad9,
            KeyCode::NumpadAdd => Self::NumpadAdd,
            KeyCode::NumpadSubtract => Self::NumpadSubtract,
            KeyCode::NumpadMultiply => Self::NumpadMultiply,
            KeyCode::NumpadDivide => Self::NumpadDivide,
            KeyCode::NumpadDecimal => Self::NumpadDecimal,
            KeyCode::NumpadEnter => Self::NumpadEnter,
            KeyCode::Escape | KeyCode::F1 | KeyCode::F11 => {
                return Err(KeyCaptureError::Reserved);
            }
            _ => return Err(KeyCaptureError::Unsupported),
        };
        Ok(key)
    }

    pub(crate) const fn display_name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::Digit0 => "0",
            Self::Digit1 => "1",
            Self::Digit2 => "2",
            Self::Digit3 => "3",
            Self::Digit4 => "4",
            Self::Digit5 => "5",
            Self::Digit6 => "6",
            Self::Digit7 => "7",
            Self::Digit8 => "8",
            Self::Digit9 => "9",
            Self::UpArrow => "Up Arrow",
            Self::DownArrow => "Down Arrow",
            Self::LeftArrow => "Left Arrow",
            Self::RightArrow => "Right Arrow",
            Self::Space => "Space",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Insert => "Insert",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
            Self::Backtick => "Backtick",
            Self::Minus => "-",
            Self::Equal => "=",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
            Self::Backslash => "\\",
            Self::Semicolon => ";",
            Self::Quote => "'",
            Self::Comma => ",",
            Self::Period => ".",
            Self::Slash => "/",
            Self::LeftShift => "Left Shift",
            Self::RightShift => "Right Shift",
            Self::LeftControl => "Left Control",
            Self::RightControl => "Right Control",
            Self::LeftAlt => "Left Alt",
            Self::RightAlt => "Right Alt",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F12 => "F12",
            Self::Numpad0 => "Numpad 0",
            Self::Numpad1 => "Numpad 1",
            Self::Numpad2 => "Numpad 2",
            Self::Numpad3 => "Numpad 3",
            Self::Numpad4 => "Numpad 4",
            Self::Numpad5 => "Numpad 5",
            Self::Numpad6 => "Numpad 6",
            Self::Numpad7 => "Numpad 7",
            Self::Numpad8 => "Numpad 8",
            Self::Numpad9 => "Numpad 9",
            Self::NumpadAdd => "Numpad +",
            Self::NumpadSubtract => "Numpad -",
            Self::NumpadMultiply => "Numpad *",
            Self::NumpadDivide => "Numpad /",
            Self::NumpadDecimal => "Numpad .",
            Self::NumpadEnter => "Numpad Enter",
        }
    }

    pub(crate) const fn compact_display_name(self) -> &'static str {
        match self {
            Self::UpArrow => "Up",
            Self::DownArrow => "Down",
            Self::LeftArrow => "Left",
            Self::RightArrow => "Right",
            _ => self.display_name(),
        }
    }
}

/// Why [`KeyboardKey::from_key_code`] refused a key. `Reserved` is one of
/// the three keys the interface reads for itself during live play, where
/// every bindable key is gameplay's - Escape, which cancels a capture and
/// leaves the screen; F1, which opens and closes the Help modal on Home and
/// during a Match; and F11, which switches fullscreen on every screen - so
/// capture ignores it silently; `Unsupported` is any other [`KeyCode`]
/// outside the allow-list, which capture reports so the player can tell
/// "nothing happened" from "that key will not work".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyCaptureError {
    Reserved,
    Unsupported,
}

/// One unique physical key for every [`GameAction`].
#[derive(Clone, Debug, Eq, PartialEq, Resource)]
pub(crate) struct KeyboardBindings {
    keys: [KeyboardKey; GameAction::ALL.len()],
}

// Positional: `keys[action as usize]`, so this list must follow
// `GameAction`'s declaration order, which `GameAction::ALL` also mirrors. A
// new variant goes at the end of the enum, of `ALL`, and of this list, or
// every later key shifts onto the wrong action. The test
// `action_set_is_complete_and_defaults_match_current_controls` pins the
// pairing.
impl Default for KeyboardBindings {
    fn default() -> Self {
        Self {
            keys: [
                KeyboardKey::UpArrow,
                KeyboardKey::DownArrow,
                KeyboardKey::LeftArrow,
                KeyboardKey::RightArrow,
                KeyboardKey::M,
                KeyboardKey::B,
                KeyboardKey::N,
                KeyboardKey::E,
                KeyboardKey::D,
                KeyboardKey::S,
                KeyboardKey::F,
                KeyboardKey::Digit2,
                KeyboardKey::Backtick,
                KeyboardKey::Digit1,
                KeyboardKey::Space,
            ],
        }
    }
}

impl KeyboardBindings {
    pub(crate) fn key(&self, action: GameAction) -> KeyboardKey {
        self.keys[action as usize]
    }

    pub(crate) fn key_code(&self, action: GameAction) -> KeyCode {
        self.key(action).key_code()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (GameAction, KeyboardKey)> + '_ {
        GameAction::ALL
            .into_iter()
            .map(|action| (action, self.key(action)))
    }

    pub(crate) fn assign(&mut self, action: GameAction, key: KeyboardKey) -> Option<GameAction> {
        let old_key = self.key(action);
        if old_key == key {
            return None;
        }

        let conflict = GameAction::ALL
            .into_iter()
            .find(|candidate| *candidate != action && self.key(*candidate) == key);
        self.keys[action as usize] = key;
        if let Some(conflicting_action) = conflict {
            self.keys[conflicting_action as usize] = old_key;
        }
        conflict
    }

    pub(crate) fn reset_to_defaults(&mut self) {
        *self = Self::default();
    }

    /// Validates a flat list of persisted `(action, key)` entries (action
    /// coverage, no duplicate actions/keys) independent of the surrounding
    /// file envelope. Shared by every settings schema's keyboard section.
    pub(crate) fn try_from_entries(
        entries: Vec<(GameAction, KeyboardKey)>,
    ) -> Result<Self, KeyboardBindingsError> {
        if entries.len() != GameAction::ALL.len() {
            return Err(KeyboardBindingsError::IncompleteActions);
        }

        let mut actions = HashSet::new();
        let mut keys = HashSet::new();
        let mut result = Self::default();
        for (action, key) in entries {
            if !actions.insert(action) {
                return Err(KeyboardBindingsError::DuplicateAction(action));
            }
            if !keys.insert(key) {
                return Err(KeyboardBindingsError::DuplicateKey(key));
            }
            result.keys[action as usize] = key;
        }
        if GameAction::ALL
            .into_iter()
            .any(|action| !actions.contains(&action))
        {
            return Err(KeyboardBindingsError::IncompleteActions);
        }
        Ok(result)
    }
}

/// Errors from validating a set of keyboard bindings, used by the
/// persistence layer in [`crate::settings`] to enforce action coverage and
/// uniqueness. Supported-key parsing is handled by serde before entries
/// reach [`KeyboardBindings::try_from_entries`], and that is also where a
/// reserved key is refused: none of Escape, F1 and F11 has a serde name, so
/// none can arrive here. The `Display` text is what the settings loader
/// puts in its warning when the section falls back to defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyboardBindingsError {
    IncompleteActions,
    DuplicateAction(GameAction),
    DuplicateKey(KeyboardKey),
}

impl fmt::Display for KeyboardBindingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompleteActions => write!(formatter, "not every gameplay action is bound"),
            Self::DuplicateAction(action) => write!(formatter, "duplicate action {action:?}"),
            Self::DuplicateKey(key) => {
                write!(formatter, "duplicate active key {}", key.display_name())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_set_is_complete_and_defaults_match_current_controls() {
        assert_eq!(GameAction::ALL.len(), 15);
        assert_eq!(GameAction::PLAYER_ONE.len(), 7);
        assert_eq!(GameAction::PLAYER_TWO.len(), 7);
        assert_eq!(GameAction::SHARED, [GameAction::RestartMatch]);

        let bindings = KeyboardBindings::default();
        let expected = [
            (GameAction::PlayerOneUp, KeyboardKey::UpArrow),
            (GameAction::PlayerOneDown, KeyboardKey::DownArrow),
            (GameAction::PlayerOneLeft, KeyboardKey::LeftArrow),
            (GameAction::PlayerOneRight, KeyboardKey::RightArrow),
            (GameAction::PlayerOneSelect, KeyboardKey::M),
            (GameAction::PlayerOneSetMarker, KeyboardKey::B),
            (GameAction::PlayerOneJumpToMarker, KeyboardKey::N),
            (GameAction::PlayerTwoUp, KeyboardKey::E),
            (GameAction::PlayerTwoDown, KeyboardKey::D),
            (GameAction::PlayerTwoLeft, KeyboardKey::S),
            (GameAction::PlayerTwoRight, KeyboardKey::F),
            (GameAction::PlayerTwoSelect, KeyboardKey::Digit2),
            (GameAction::PlayerTwoSetMarker, KeyboardKey::Backtick),
            (GameAction::PlayerTwoJumpToMarker, KeyboardKey::Digit1),
            (GameAction::RestartMatch, KeyboardKey::Space),
        ];
        assert_eq!(bindings.iter().collect::<Vec<_>>(), expected);
        assert_eq!(
            bindings
                .iter()
                .map(|(_, key)| key)
                .collect::<HashSet<_>>()
                .len(),
            GameAction::ALL.len()
        );
    }

    #[test]
    fn assigning_an_in_use_key_swaps_atomically() {
        let mut bindings = KeyboardBindings::default();

        let swapped = bindings.assign(GameAction::PlayerOneSelect, KeyboardKey::Digit2);

        assert_eq!(swapped, Some(GameAction::PlayerTwoSelect));
        assert_eq!(
            bindings.key(GameAction::PlayerOneSelect),
            KeyboardKey::Digit2
        );
        assert_eq!(bindings.key(GameAction::PlayerTwoSelect), KeyboardKey::M);
        assert_eq!(
            bindings
                .iter()
                .map(|(_, key)| key)
                .collect::<HashSet<_>>()
                .len(),
            GameAction::ALL.len()
        );
    }

    #[test]
    fn reset_restores_defaults() {
        let mut bindings = KeyboardBindings::default();
        bindings.assign(GameAction::PlayerOneSelect, KeyboardKey::F8);

        bindings.reset_to_defaults();

        assert_eq!(bindings, KeyboardBindings::default());
    }

    #[test]
    fn key_names_are_human_readable_and_the_reserved_keys_are_refused() {
        assert_eq!(KeyboardKey::UpArrow.display_name(), "Up Arrow");
        assert_eq!(KeyboardKey::Backtick.display_name(), "Backtick");
        assert_eq!(KeyboardKey::Space.display_name(), "Space");
        assert_eq!(KeyboardKey::Digit1.display_name(), "1");
        for reserved in [KeyCode::Escape, KeyCode::F1, KeyCode::F11] {
            assert_eq!(
                KeyboardKey::from_key_code(reserved),
                Err(KeyCaptureError::Reserved),
                "{reserved:?}"
            );
        }
        // Reserved is narrower than unsupported: the function keys beside
        // each reserved one are still bindable, and a key with no
        // allow-list entry is reported.
        for (code, key) in [
            (KeyCode::F2, KeyboardKey::F2),
            (KeyCode::F10, KeyboardKey::F10),
            (KeyCode::F12, KeyboardKey::F12),
        ] {
            assert_eq!(KeyboardKey::from_key_code(code), Ok(key), "{code:?}");
            assert_eq!(key.key_code(), code);
        }
        assert_eq!(
            KeyboardKey::from_key_code(KeyCode::CapsLock),
            Err(KeyCaptureError::Unsupported)
        );
    }

    /// The reservation is structural, not a check: no reserved key has a
    /// `KeyboardKey` variant, so no binding, no `assign` and no settings
    /// file can hold one. Serde refuses all three names, which is what the
    /// settings loader relies on for a hand-edited file, and still reads
    /// the names of the function keys beside them.
    #[test]
    fn reserved_keys_have_no_serde_name() {
        for name in ["\"escape\"", "\"f1\"", "\"f11\""] {
            assert!(
                serde_json::from_str::<KeyboardKey>(name).is_err(),
                "{name} must not deserialize"
            );
        }
        for (name, key) in [
            ("\"f2\"", KeyboardKey::F2),
            ("\"f10\"", KeyboardKey::F10),
            ("\"f12\"", KeyboardKey::F12),
        ] {
            assert_eq!(serde_json::from_str::<KeyboardKey>(name).unwrap(), key);
        }
        for key in [
            KeyboardKey::F2,
            KeyboardKey::F10,
            KeyboardKey::F12,
            KeyboardKey::Backtick,
            KeyboardKey::NumpadEnter,
        ] {
            for reserved in [KeyCode::Escape, KeyCode::F1, KeyCode::F11] {
                assert_ne!(key.key_code(), reserved, "{key:?}");
            }
        }
    }

    #[test]
    fn binding_errors_name_their_cause() {
        assert_eq!(
            KeyboardBindingsError::IncompleteActions.to_string(),
            "not every gameplay action is bound"
        );
        assert_eq!(
            KeyboardBindingsError::DuplicateKey(KeyboardKey::Space).to_string(),
            "duplicate active key Space"
        );
    }

    #[test]
    fn compact_key_names_shorten_only_arrows() {
        assert_eq!(KeyboardKey::UpArrow.compact_display_name(), "Up");
        assert_eq!(KeyboardKey::DownArrow.compact_display_name(), "Down");
        assert_eq!(KeyboardKey::LeftArrow.compact_display_name(), "Left");
        assert_eq!(KeyboardKey::RightArrow.compact_display_name(), "Right");
        assert_eq!(
            KeyboardKey::LeftControl.compact_display_name(),
            KeyboardKey::LeftControl.display_name()
        );
        assert_eq!(
            KeyboardKey::NumpadEnter.compact_display_name(),
            KeyboardKey::NumpadEnter.display_name()
        );
    }
}
