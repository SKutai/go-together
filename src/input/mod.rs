//! Physical devices in, semantic actions out.
//!
//! # Responsibility
//!
//! No gameplay code reads a key code or a gamepad button. Devices are
//! translated here into one vocabulary - [`GameAction`] - and one per-frame
//! snapshot of it, [`SemanticInput`]. Gameplay and the Match HUD only ever
//! see that snapshot, which is why an action behaves identically whether it
//! came from a keyboard or a pad, and why a recorded Replay, which stores the
//! resulting events, carries no trace of which device produced them.
//!
//! Screens still read raw input for their own chrome, and since menus became
//! operable without a pointer that includes pads as well as keys: Escape or a
//! pad's Start to leave a screen or dismiss a modal, with East doing the same
//! on the menu screens, F1 to open and close the Help modal on Home and
//! during a Match, Tab, the arrows, Enter, Space and a pad's D-pad and
//! South to drive the
//! focus ring, modifier keys in a numeric field, the Home marimba, the browser
//! audio unlock. The Settings capture systems additionally read raw keys and
//! pads in order to *create* bindings. None of it reaches gameplay: chrome
//! input is fixed rather than remappable, precisely so it belongs to neither
//! player - and the two keys read during live play, Escape and F1, are
//! reserved outright, so no binding can ever hold them either.
//!
//! # Layout
//!
//! This module owns the vocabulary itself - [`GameAction`] and the
//! [`SemanticInput`] snapshot built from it - and the fusion that produces
//! that snapshot each frame:
//!
//! * [`fusion`] - [`update_semantic_input`], the sole writer of
//!   [`SemanticInput`], ORing the keyboard with each player's assigned
//!   controller.
//!
//! Below that sit two *sibling* device modules. Each translates one class of
//! physical hardware into the vocabulary above, and neither knows the other
//! exists:
//!
//! * [`keyboard`] - the physical [`KeyboardKey`](keyboard::KeyboardKey)
//!   allow-list and the [`KeyboardBindings`](keyboard::KeyboardBindings)
//!   between it and [`GameAction`].
//! * [`controller`] - gamepad assignment, per-profile bindings, analog
//!   hysteresis, and rebinding capture, all expressed in the profile-local
//!   [`ProfileAction`](controller::ProfileAction) namespace.
//!
//! # Direction
//!
//! Dependencies point inward, never sideways. `keyboard` and `controller`
//! both read this module's vocabulary; neither imports from the other, and
//! `fusion` is the single place where a keyboard binding and a controller
//! profile meet. Both device modules are also read by [`crate::settings`],
//! which owns the persisted file - neither knows how bindings are stored.

pub(crate) mod controller;
mod fusion;
pub(crate) mod keyboard;

pub(crate) use fusion::update_semantic_input;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::presentation::{SELECT_ACTION, SET_TELEPORT_MARKER_ACTION, TELEPORT_TO_MARKER_ACTION};

/// Semantic gameplay action independent of any physical device.
///
/// Variant serde names are persisted by `controls.json`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub(crate) enum GameAction {
    PlayerOneUp,
    PlayerOneDown,
    PlayerOneLeft,
    PlayerOneRight,
    PlayerOneSelect,
    PlayerOneSetMarker,
    PlayerOneJumpToMarker,
    PlayerTwoUp,
    PlayerTwoDown,
    PlayerTwoLeft,
    PlayerTwoRight,
    PlayerTwoSelect,
    PlayerTwoSetMarker,
    PlayerTwoJumpToMarker,
    RestartMatch,
}

impl GameAction {
    pub(crate) const ALL: [Self; 15] = [
        Self::PlayerOneUp,
        Self::PlayerOneDown,
        Self::PlayerOneLeft,
        Self::PlayerOneRight,
        Self::PlayerOneSelect,
        Self::PlayerOneSetMarker,
        Self::PlayerOneJumpToMarker,
        Self::PlayerTwoUp,
        Self::PlayerTwoDown,
        Self::PlayerTwoLeft,
        Self::PlayerTwoRight,
        Self::PlayerTwoSelect,
        Self::PlayerTwoSetMarker,
        Self::PlayerTwoJumpToMarker,
        Self::RestartMatch,
    ];
    pub(crate) const PLAYER_ONE: [Self; 7] = [
        Self::PlayerOneUp,
        Self::PlayerOneDown,
        Self::PlayerOneLeft,
        Self::PlayerOneRight,
        Self::PlayerOneSelect,
        Self::PlayerOneSetMarker,
        Self::PlayerOneJumpToMarker,
    ];
    pub(crate) const PLAYER_TWO: [Self; 7] = [
        Self::PlayerTwoUp,
        Self::PlayerTwoDown,
        Self::PlayerTwoLeft,
        Self::PlayerTwoRight,
        Self::PlayerTwoSelect,
        Self::PlayerTwoSetMarker,
        Self::PlayerTwoJumpToMarker,
    ];
    pub(crate) const SHARED: [Self; 1] = [Self::RestartMatch];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::PlayerOneUp | Self::PlayerTwoUp => "Move Up",
            Self::PlayerOneDown | Self::PlayerTwoDown => "Move Down",
            Self::PlayerOneLeft | Self::PlayerTwoLeft => "Move Left",
            Self::PlayerOneRight | Self::PlayerTwoRight => "Move Right",
            Self::PlayerOneSelect | Self::PlayerTwoSelect => SELECT_ACTION,
            Self::PlayerOneSetMarker | Self::PlayerTwoSetMarker => SET_TELEPORT_MARKER_ACTION,
            Self::PlayerOneJumpToMarker | Self::PlayerTwoJumpToMarker => TELEPORT_TO_MARKER_ACTION,
            Self::RestartMatch => "Restart Match",
        }
    }
}

/// A frame's resolved state for every [`GameAction`], aggregating keyboard
/// (always simultaneous) with whichever controller is assigned to each
/// action's player. `RestartMatch` is the logical OR of the keyboard restart
/// key and both assigned controllers' profile-local restart input, so either
/// assigned controller can restart an active or finished match. Gameplay only
/// ever consumes these semantic booleans, so the events it records into a
/// Replay are identical regardless of which input source produced them.
///
/// [`update_semantic_input`] is the only writer.
#[derive(Resource, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemanticInput {
    current: [bool; GameAction::ALL.len()],
    previous: [bool; GameAction::ALL.len()],
}

impl Default for SemanticInput {
    fn default() -> Self {
        Self {
            current: [false; GameAction::ALL.len()],
            previous: [false; GameAction::ALL.len()],
        }
    }
}

impl SemanticInput {
    pub(crate) fn pressed(&self, action: GameAction) -> bool {
        self.current[action as usize]
    }

    pub(crate) fn just_pressed(&self, action: GameAction) -> bool {
        self.current[action as usize] && !self.previous[action as usize]
    }

    #[cfg(test)]
    /// Builds a snapshot resource useful for tests: only the given actions
    /// are pressed, and since `previous` is left empty, they also read as
    /// freshly pressed - matching how a bare single-frame `ButtonInput`
    /// snapshot behaves in the existing keyboard-only tests.
    pub(crate) fn from_pressed(actions: &[GameAction]) -> Self {
        let mut semantic = Self::default();
        for action in actions {
            semantic.current[*action as usize] = true;
        }
        semantic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_input_tracks_pressed_and_just_pressed_states() {
        let mut semantic = SemanticInput::default();
        semantic.current[GameAction::PlayerOneUp as usize] = true;
        assert!(semantic.pressed(GameAction::PlayerOneUp));
        assert!(semantic.just_pressed(GameAction::PlayerOneUp));

        semantic.previous = semantic.current;
        assert!(semantic.pressed(GameAction::PlayerOneUp));
        assert!(!semantic.just_pressed(GameAction::PlayerOneUp));

        semantic.previous = semantic.current;
        semantic.current[GameAction::PlayerOneUp as usize] = false;
        assert!(!semantic.pressed(GameAction::PlayerOneUp));
        assert!(!semantic.just_pressed(GameAction::PlayerOneUp));
    }
}
