//! A minimal app that runs the real match pipeline, for gameplay tests.
//!
//! The harness registers the systems from
//! [`GameplayPlugin`](super::GameplayPlugin) plus the semantic-input fusion
//! they depend on, in the same order. Tests therefore exercise the shipped
//! ordering - input fusion, clock, gate, holds, actions, replay save - rather
//! than a hand-rolled approximation of it. Left out are the plugin's
//! `run_if(in_state(AppScreen::Match))` conditions and the
//! controller-assignment maintenance that precedes fusion in production, so
//! every system here runs every frame.
//!
//! Each app owns a private replay store directory so replay persistence can be
//! asserted without tests interfering with each other; call
//! [`cleanup_restart_test_root`] when finished.

use bevy::prelude::*;

use super::countdown::{advance_match_clock, begin_active_match, start_opening_countdown};
use super::holds::handle_hold_gestures;
use super::input::{handle_player_input, update_input_gate};
use super::{
    GameState, GameplayPause, InputGate, MatchPhase, begin_round_zero, save_completed_replay,
};
use crate::audio::AudioCueQueue;
use crate::input::controller::{
    ControllerAssignments, ControllerBindings, ControllerHysteresisState,
};
use crate::input::keyboard::KeyboardBindings;
use crate::input::{SemanticInput, update_semantic_input};
use crate::match_settings::MatchSettings;
use crate::replay_library::{ReplayLibrary, ReplayStore};
use crate::settings::GamePreferences;

pub(crate) fn match_input_app(
    keyboard: KeyboardBindings,
    phase: MatchPhase,
) -> (App, std::path::PathBuf) {
    match_input_app_with_settings(keyboard, phase, MatchSettings::default())
}

pub(crate) fn match_input_app_with_settings(
    keyboard: KeyboardBindings,
    phase: MatchPhase,
    match_settings: MatchSettings,
) -> (App, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "go-together-restart-input-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut game = GameState::with_match_rules(super::MatchRules::snapshot(&match_settings));
    match phase {
        MatchPhase::Playing => begin_active_match(&mut game),
        MatchPhase::Ended => game.phase = MatchPhase::Ended,
        MatchPhase::Opening => begin_round_zero(&mut game, &match_settings),
        MatchPhase::OpeningCountdown => {
            begin_round_zero(&mut game, &match_settings);
            for x in 0..3 {
                game.opening_selections[0].add(crate::rules::Point::new(x, 0));
                game.opening_selections[1].add(crate::rules::Point::new(x, 1));
            }
            assert!(start_opening_countdown(&mut game));
        }
    }
    let mut app = App::new();
    app.insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(keyboard)
        .insert_resource(GamePreferences::default())
        .insert_resource(match_settings)
        .insert_resource(ControllerBindings::default())
        .insert_resource(ControllerAssignments::default())
        .insert_resource(ControllerHysteresisState::default())
        .insert_resource(SemanticInput::default())
        .insert_resource(AudioCueQueue::default())
        .insert_resource(Time::<()>::default())
        .insert_resource(game)
        .insert_resource(GameplayPause::default())
        .insert_resource(InputGate::default())
        .insert_resource(ReplayLibrary::from_store(ReplayStore::new(root.clone())))
        .insert_resource(crate::diagnostics::Diagnostics::default())
        .add_systems(
            Update,
            (
                update_semantic_input,
                advance_match_clock,
                update_input_gate,
                handle_hold_gestures,
                handle_player_input,
                save_completed_replay,
            )
                .chain(),
        );
    (app, root)
}

pub(crate) fn ended_match_input_app(keyboard: KeyboardBindings) -> (App, std::path::PathBuf) {
    match_input_app(keyboard, MatchPhase::Ended)
}

pub(crate) fn active_match_input_app(keyboard: KeyboardBindings) -> (App, std::path::PathBuf) {
    match_input_app(keyboard, MatchPhase::Playing)
}

pub(crate) fn opening_match_input_app(keyboard: KeyboardBindings) -> (App, std::path::PathBuf) {
    match_input_app(keyboard, MatchPhase::Opening)
}

pub(crate) fn cleanup_restart_test_root(root: std::path::PathBuf) {
    std::fs::remove_dir_all(root).unwrap();
}
