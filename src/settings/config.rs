//! The live settings resources: what the rest of the client reads and edits,
//! with no JSON anywhere in sight.
//!
//! This is the in-memory half of the boundary described in [`super`]'s docs.
//! [`ControlsConfig`] is the whole settings document as one value; at runtime
//! its four pieces live as four independent Bevy resources, and a
//! `ControlsConfig` exists only at the moment of loading or saving - see
//! `docs/ARCHITECTURE.md`'s "Configuration ownership".
//!
//! Nothing here knows about schema versions or fallback rules. The two
//! methods that bridge to disk, [`ControlsConfig::from_json`] and
//! [`ControlsConfig::to_json`], are implemented in [`super::compat`] beside
//! the migrations they depend on.
//!
//! [`GamePreferences`] and [`VolumeLevel`] do carry `#[serde]` derives,
//! because the preferences section is stored as its own object and
//! `VolumeLevel`'s `snake_case` variant names are on disk. Those names are
//! part of the persisted format even though the types live here.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::input::controller::ControllerBindings;
use crate::input::keyboard::KeyboardBindings;
use crate::match_settings::MatchSettings;

/// Saved sound level shared by Settings and the audio system.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VolumeLevel {
    Mute,
    Low,
    #[default]
    Medium,
    High,
}

impl VolumeLevel {
    pub(crate) const ALL: [Self; 4] = [Self::Mute, Self::Low, Self::Medium, Self::High];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Mute => "Mute",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    pub(crate) const fn gain(self) -> f32 {
        match self {
            Self::Mute => 0.0,
            Self::Low => 0.25,
            Self::Medium => 0.55,
            Self::High => 0.9,
        }
    }
}

/// Non-binding preferences stored in schema v3 and newer.
///
/// Sound fields default to `Medium` so schema v3 values migrate without losing
/// their timer and round choices, and `reduce_motion` defaults to off so a
/// schema v3-v5 file migrates the same way; the field-level defaults are
/// what let a typed load of an older document succeed under
/// `deny_unknown_fields`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Resource, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GamePreferences {
    pub(crate) show_match_timer: bool,
    pub(crate) show_round_number: bool,
    #[serde(default)]
    pub(crate) game_sound_effects: VolumeLevel,
    #[serde(default)]
    pub(crate) ui_sound_effects: VolumeLevel,
    /// Show every change on screen as a step rather than an animation.
    /// Stored from schema v6. This is the player's half of the motion
    /// policy; the platform's half, and what the policy means, are in
    /// `app::motion`.
    #[serde(default)]
    pub(crate) reduce_motion: bool,
}

impl Default for GamePreferences {
    fn default() -> Self {
        Self {
            show_match_timer: false,
            show_round_number: true,
            game_sound_effects: VolumeLevel::Medium,
            ui_sound_effects: VolumeLevel::Medium,
            reduce_motion: false,
        }
    }
}

/// The whole settings document as one in-memory value, produced from any
/// supported schema v1-v6 file on load and reassembled from the four live
/// resources on save. It is never itself inserted as a Bevy resource - see
/// this module's docs.
///
/// Older shapes preserve every valid binding and preference while supplying
/// defaults only for fields that did not exist yet.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ControlsConfig {
    pub(crate) keyboard: KeyboardBindings,
    pub(crate) controller: ControllerBindings,
    pub(crate) preferences: GamePreferences,
    pub(crate) match_settings: MatchSettings,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::GameAction;
    use crate::input::keyboard::KeyboardKey;

    #[test]
    fn controls_config_defaults_match_keyboard_and_controller_defaults() {
        let config = ControlsConfig::default();
        assert_eq!(config.keyboard, KeyboardBindings::default());
        assert_eq!(config.controller, ControllerBindings::default());
        assert_eq!(config.preferences, GamePreferences::default());
        assert_eq!(config.match_settings, MatchSettings::default());
        assert!(!config.preferences.show_match_timer);
        assert!(config.preferences.show_round_number);
        assert!(!config.preferences.reduce_motion);
        assert_eq!(config.keyboard.key(GameAction::PlayerTwoUp), KeyboardKey::E);
        assert_eq!(
            config.keyboard.key(GameAction::PlayerTwoLeft),
            KeyboardKey::S
        );
        assert_eq!(
            config.keyboard.key(GameAction::PlayerTwoDown),
            KeyboardKey::D
        );
        assert_eq!(
            config.keyboard.key(GameAction::PlayerTwoRight),
            KeyboardKey::F
        );
        assert_eq!(
            config.keyboard.key(GameAction::PlayerTwoSelect),
            KeyboardKey::Digit2
        );
        assert_eq!(
            config.keyboard.key(GameAction::PlayerTwoSetMarker),
            KeyboardKey::Backtick
        );
        assert_eq!(
            config.keyboard.key(GameAction::PlayerTwoJumpToMarker),
            KeyboardKey::Digit1
        );
    }
}
