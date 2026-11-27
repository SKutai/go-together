//! The Bevy client's side of Round 0's closing 3-2-1-Go sequence and the
//! continuous Match clock.
//!
//! A fresh Match is interactive immediately. Once both opening windows contain
//! three points, [`crate::authority::MatchState::start_opening_countdown`]
//! begins a countdown without disabling Round 0 actions. At the Go boundary
//! the six final slots are revealed atomically, ordinary Round 1 begins, and
//! a short Go card remains visible while normal play is already active.
//!
//! None of that timing is decided here.
//! [`crate::authority::MatchState::advance_clock`] owns
//! the thresholds, the reveal, and both deadlines, and stops at each one; this
//! module converts Bevy's frame delta once, drives that method to the end of
//! the frame, and maps every [`ClockBoundary`] it reports onto what only the
//! client has: the Replay recorder, the audio queue, the end-gesture
//! snapshot an automatic end is deferred to, and the per-frame input freeze -
//! exactly as `gameplay::input` does for every command outcome.

use bevy::prelude::*;

#[cfg(test)]
use super::InputGate;
#[cfg(test)]
use super::OrdinaryCompletion;
#[cfg(test)]
use super::replay_frame;
use super::{
    GameState, GameplayPause, MatchPhase, complete_ordinary_round, finish_match, record_game_event,
};
use crate::audio::{AudioCueQueue, GameCue};
use crate::authority::ClockBoundary;
use crate::clock::MatchDuration;
use crate::opening::OpeningCountdownStage;
#[cfg(test)]
use crate::replay::MatchRecorder;
use crate::replay::{EndReason, ReplayEventKind};
#[cfg(test)]
use crate::rules::Score;

pub(crate) fn advance_match_clock(
    time: Res<Time>,
    pause: Res<GameplayPause>,
    mut game: ResMut<GameState>,
    mut audio: ResMut<AudioCueQueue>,
) {
    // The clock reads `match_paused` and nothing else. It deliberately does
    // not consult [`super::InputGate`]: the gate suppresses one action at a
    // time, and there is one Match clock, so letting it freeze the clock
    // would hand any single held key the power to stop time for both
    // players - the shared-state half of the bug the per-action gate fixes.
    // `input_blocked` is the other player-local flag it ignores, for the same
    // reason [`GameplayPause`] keeps the two apart: a client that cannot
    // accept input has not stopped the authoritative Match.
    //
    // `Time::delta()` is already an exact `std::time::Duration`; converting
    // it directly avoids the floating-point round-trip `delta_secs_f64()`
    // would introduce for every single frame. See `crate::clock`.
    for cue in advance_match_state_by(
        &mut game,
        MatchDuration::from(time.delta()),
        pause.match_paused,
    ) {
        audio.game(cue);
    }
}

/// Advances every interactive Match phase through one frame and returns the
/// newly crossed audio cues.
///
/// [`crate::authority::MatchState::advance_clock`] stops at every authoritative boundary
/// individually, so even one unusually long frame records each countdown
/// stage and each timeout at its exact Match-clock timestamp, with the frame
/// snapshot that boundary produced. Between steps this function does what the
/// authoritative core deliberately leaves to its caller: it resolves the round
/// a timeout ended through [`complete_ordinary_round`] - which is where an
/// automatic end the round crossed may be deferred to an in-flight end
/// gesture instead of finishing the Match - records Replay events, freezes
/// this frame's input, and keeps `game.playing_delta`, the hold gestures'
/// time base.
///
/// Deferring an end also freezes the Match clock: this function returns
/// before its first step while one is pending, crediting the whole frame to
/// the gesture instead, and breaks straight after creating one, so the clock
/// spends none of the rest of that frame. That remainder is credited to
/// nothing - the timeout that deferred the end also froze the frame's input,
/// so the holds skip it - and the hold simply carries on from the next
/// frame. Ordinary play stops at the same time, in `input`'s
/// `match_takes_play`; this is the clock's half of that freeze.
/// [`complete_ordinary_round`] depends on both; this module's
/// `a_pending_provisional_end_freezes_the_match_clock_short_of_the_match_time_limit`
/// test pins this half.
pub(crate) fn advance_match_state_by(
    game: &mut GameState,
    delta: MatchDuration,
    match_paused: bool,
) -> Vec<GameCue> {
    game.playing_delta = MatchDuration::ZERO;
    game.input_frozen_this_frame = false;
    if match_paused || game.phase == MatchPhase::Ended {
        return Vec::new();
    }
    let mut cues = Vec::new();
    if game.provisional_end_pending() {
        advance_provisional_end_clock(game, delta);
        return cues;
    }

    let mut budget = delta;
    while game.phase != MatchPhase::Ended {
        // The end hold is credited only with time spent in ordinary play,
        // and a step never leaves the phase it started in until it stops.
        let was_playing = game.phase == MatchPhase::Playing;
        let step = game.state.advance_clock(budget);
        budget = step.remaining;
        if was_playing {
            game.playing_delta += step.consumed;
        }
        let Some(boundary) = step.crossed else {
            break;
        };
        match boundary {
            ClockBoundary::OpeningCountdown(stage) => {
                if stage == OpeningCountdownStage::Go {
                    record_game_event(game, ReplayEventKind::OpeningReveal);
                }
                record_countdown_stage(game, stage, &mut cues);
            }
            ClockBoundary::OpeningCountdownFinished => {
                record_game_event(game, ReplayEventKind::OpeningCountdownFinished);
            }
            ClockBoundary::MatchTimeExpiredInRoundZero => {
                game.input_frozen_this_frame = true;
                record_game_event(game, ReplayEventKind::OpeningReveal);
                game.pending_replay = finish_match(game, vec![EndReason::MatchTimeLimit]);
            }
            ClockBoundary::MatchTimeExpired | ClockBoundary::RoundTimeExpired => {
                game.input_frozen_this_frame = true;
                let completion = boundary
                    .completion()
                    .expect("timeout boundaries name their completion");
                if let Some(cue) = complete_ordinary_round(game, completion) {
                    cues.push(cue);
                }
                // A round timeout that crossed an automatic limit during an
                // end hold has just been deferred as a provisional end; the
                // Match clock freezes there until the gesture settles it. The
                // rest of the frame goes to `playing_delta` rather than to the
                // clock, but no gesture consumes it: this timeout also froze
                // the frame's input, so the holds skip it.
                if game.provisional_end_pending() {
                    advance_provisional_end_clock(game, budget);
                    break;
                }
            }
        }
    }
    cues
}

fn advance_provisional_end_clock(game: &mut GameState, delta: MatchDuration) {
    game.playing_delta += delta;
}

/// Test-only convenience combining the pure
/// [`crate::authority::MatchState::start_opening_countdown`] phase transition
/// with its Replay side effect, for tests that build up Round 0 selections
/// directly rather than through [`super::input::try_opening_select`].
#[cfg(test)]
pub(super) fn start_opening_countdown(game: &mut GameState) -> bool {
    if !game.state.start_opening_countdown() {
        return false;
    }
    record_countdown_stage(game, OpeningCountdownStage::Three, &mut Vec::new());
    true
}

/// The audio cue each closing-countdown stage plays as it is reached.
const fn countdown_cue(stage: OpeningCountdownStage) -> GameCue {
    match stage {
        OpeningCountdownStage::Three => GameCue::Countdown3,
        OpeningCountdownStage::Two => GameCue::Countdown2,
        OpeningCountdownStage::One => GameCue::Countdown1,
        OpeningCountdownStage::Go => GameCue::Go,
    }
}

fn record_countdown_stage(
    game: &mut GameState,
    stage: OpeningCountdownStage,
    cues: &mut Vec<GameCue>,
) {
    let cue = countdown_cue(stage);
    record_game_event(game, ReplayEventKind::OpeningCountdown { stage });
    record_game_event(game, ReplayEventKind::MatchSound { cue });
    cues.push(cue);
}

/// Test-only convenience combining the pure
/// [`crate::authority::MatchState::reveal_round_zero`] phase transition with its Replay side
/// effect, for tests that reveal directly rather than through the clock.
#[cfg(test)]
fn reveal_round_zero(game: &mut GameState) {
    game.state.reveal_round_zero();
    record_game_event(game, ReplayEventKind::OpeningReveal);
}

/// Test and setup helper for entering ordinary play without constructing a
/// Round 0 timeline. Production Matches reach ordinary play only through the
/// clock: `MatchState::advance_clock` reveals Round 0 at the Go boundary, or
/// when Match Time expires during Round 0, and [`advance_match_state_by`]
/// records the reveal. [`reveal_round_zero`] is the test-only shortcut past
/// that same pair of steps.
#[cfg(test)]
pub(crate) fn begin_active_match(game: &mut GameState) {
    game.phase = MatchPhase::Playing;
    game.round = 1;
    game.countdown_elapsed = crate::opening::OPENING_COUNTDOWN_TOTAL;
    game.opening_selections = Default::default();
    game.completed_history = std::collections::HashSet::from([game.board.clone()]);
    if game.recorder.is_none() {
        let started_at_ms = crate::platform::now_millis();
        game.recorder = Some(MatchRecorder::new_with_settings(
            started_at_ms,
            crate::replay_library::format_local_timestamp(started_at_ms),
            replay_frame(game),
            game.match_rules.settings().clone(),
        ));
    }
}

pub(crate) fn countdown_text(game: &GameState) -> Option<&'static str> {
    game.opening_countdown_stage()
        .map(OpeningCountdownStage::label)
}

#[cfg(test)]
pub(crate) fn match_timer_running(phase: MatchPhase, paused: bool) -> bool {
    matches!(
        phase,
        MatchPhase::Opening | MatchPhase::OpeningCountdown | MatchPhase::Playing
    ) && !paused
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameplay::begin_round_zero;
    use crate::gameplay::input::apply_player_input;
    use crate::gameplay::test_support::{cleanup_restart_test_root, opening_match_input_app};
    use crate::input::keyboard::KeyboardBindings;
    use crate::input::{GameAction, SemanticInput};
    use crate::match_settings::{MatchSettingField, MatchSettings};
    use crate::opening::{OPENING_COUNTDOWN_TOTAL, OPENING_REVEAL, OpeningSelectionOutcome};
    use crate::replay::{ReplayEventKind, ReplayPhase};
    use crate::rules::{Cell, Player, Point};
    use crate::test_support::advance_test_app;
    use bevy::input::ButtonInput;

    /// Test-only convenience: seconds are the readable unit in test bodies,
    /// converted once at this boundary. `f64::NAN`/negative inputs clamp to
    /// zero at the type boundary - see [`crate::clock`] - which is the only
    /// tolerance `advance_match_state_by` has for an implausible frame delta.
    fn secs(seconds: f64) -> MatchDuration {
        MatchDuration::from_secs_f64(seconds)
    }

    fn fill_opening(game: &mut GameState) {
        for x in 0..3 {
            assert!(matches!(
                game.opening_selections[Player::One.index()].add(Point::new(x, 0)),
                OpeningSelectionOutcome::Added { .. }
            ));
            assert!(matches!(
                game.opening_selections[Player::Two.index()].add(Point::new(x + 3, 0)),
                OpeningSelectionOutcome::Added { .. }
            ));
        }
    }

    fn settings_with(field: MatchSettingField, value: u16) -> MatchSettings {
        let mut settings = MatchSettings::default();
        settings.set_value(field, u64::from(value));
        settings.set_enabled(field, true);
        settings
    }

    fn timed_active_game(seconds: u16) -> GameState {
        let settings = settings_with(MatchSettingField::RoundTime, seconds);
        let mut game =
            GameState::with_match_rules(crate::gameplay::MatchRules::snapshot(&settings));
        begin_active_match(&mut game);
        game
    }

    #[test]
    fn round_zero_clock_starts_immediately_and_pauses_only_for_a_modal() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &crate::match_settings::MatchSettings::default());

        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        assert!(game.recorder.is_some());
        assert!(advance_match_state_by(&mut game, secs(1.25), false).is_empty());
        assert_eq!(game.elapsed.as_secs_f64(), 1.25);

        advance_match_state_by(&mut game, secs(2.0), true);
        assert_eq!(game.elapsed.as_secs_f64(), 1.25);
        assert!(match_timer_running(game.phase, false));
        assert!(!match_timer_running(game.phase, true));
    }

    #[test]
    fn a_gated_action_does_not_stop_the_match_clock() {
        // The release gate suppresses one action at a time, but there is one
        // Match clock shared by both players, so it keeps running while an
        // input waits to be released. It used to freeze, which handed any
        // single held key the power to stop Round 0 for everybody - the
        // shared-state half of the bug the per-action gate fixes.
        let (mut app, root) = opening_match_input_app(KeyboardBindings::default());
        app.world_mut().resource_mut::<InputGate>().arm();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyM);
        assert_eq!(
            app.world().resource::<GameState>().elapsed.as_secs_f64(),
            0.0
        );

        advance_test_app(&mut app, 0.5);

        let gate = app.world().resource::<InputGate>();
        assert!(
            gate.is_gated(GameAction::PlayerOneSelect),
            "the held Select is still gated"
        );
        assert!(
            !gate.is_gated(GameAction::PlayerTwoSelect),
            "and it gates nothing else"
        );
        assert_eq!(
            app.world().resource::<GameState>().elapsed.as_secs_f64(),
            0.5,
            "the Match clock advances through the whole frame anyway"
        );
        cleanup_restart_test_root(root);
    }

    #[test]
    fn identical_total_delta_produces_the_same_result_regardless_of_frame_subdivision() {
        let mut single_frame = GameState::default();
        begin_round_zero(
            &mut single_frame,
            &crate::match_settings::MatchSettings::default(),
        );
        fill_opening(&mut single_frame);
        assert!(start_opening_countdown(&mut single_frame));

        let mut many_frames = GameState::default();
        begin_round_zero(
            &mut many_frames,
            &crate::match_settings::MatchSettings::default(),
        );
        fill_opening(&mut many_frames);
        assert!(start_opening_countdown(&mut many_frames));

        // One frame covering the whole 3-2-1-Go countdown plus the Go overlay
        // in a single step...
        advance_match_state_by(&mut single_frame, secs(4.25), false);

        // ...versus several odd-sized frames summing to the exact same total,
        // deliberately crossing every countdown threshold (3, 2, 1, Go, and
        // the end of the Go overlay) at a different point within a frame.
        let subdivisions = [0.37, 0.13, 0.5, 1.0, 0.6, 0.9, 0.75];
        assert!((subdivisions.iter().sum::<f64>() - 4.25).abs() < 1e-9);
        for chunk in subdivisions {
            advance_match_state_by(&mut many_frames, secs(chunk), false);
        }

        assert_eq!(single_frame.phase, many_frames.phase);
        assert_eq!(single_frame.phase, MatchPhase::Playing);
        assert_eq!(single_frame.elapsed, many_frames.elapsed);
        assert_eq!(
            single_frame.countdown_elapsed,
            many_frames.countdown_elapsed
        );
        assert_eq!(single_frame.board, many_frames.board);
        assert_eq!(single_frame.round, many_frames.round);
    }

    #[test]
    fn countdown_starts_only_when_both_players_have_three() {
        let mut game = GameState::default();
        for x in 0..3 {
            game.opening_selections[0].add(Point::new(x, 0));
        }
        for x in 0..2 {
            game.opening_selections[1].add(Point::new(x, 1));
        }

        assert!(!start_opening_countdown(&mut game));
        assert_eq!(game.phase, MatchPhase::Opening);
        game.opening_selections[1].add(Point::new(2, 1));
        assert!(start_opening_countdown(&mut game));
        assert_eq!(game.phase, MatchPhase::OpeningCountdown);
        assert_eq!(countdown_text(&game), Some("3"));
        assert!(!start_opening_countdown(&mut game));
    }

    #[test]
    fn countdown_records_each_note_once_and_reveals_at_go() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &crate::match_settings::MatchSettings::default());
        fill_opening(&mut game);
        assert!(start_opening_countdown(&mut game));

        assert!(advance_match_state_by(&mut game, secs(0.99), false).is_empty());
        assert_eq!(
            advance_match_state_by(&mut game, secs(0.01), false),
            vec![GameCue::Countdown2]
        );
        assert_eq!(
            advance_match_state_by(&mut game, secs(1.0), false),
            vec![GameCue::Countdown1]
        );
        assert_eq!(
            advance_match_state_by(&mut game, secs(1.0), false),
            vec![GameCue::Go]
        );
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.round, 1);
        assert_eq!(countdown_text(&game), Some("Go!"));
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(game.board.get(Point::new(3, 0)), Cell::White);
        assert_eq!(
            game.completed_history,
            std::collections::HashSet::from([game.board.clone()])
        );

        assert!(advance_match_state_by(&mut game, secs(0.75), false).is_empty());
        assert_eq!(countdown_text(&game), None);
        let mut recorder = game.recorder.clone().unwrap();
        let replay = recorder
            .complete(2, game.elapsed.as_secs_f64(), replay_frame(&game))
            .unwrap();
        let sounds = replay
            .events
            .iter()
            .filter_map(|event| match event.kind {
                ReplayEventKind::MatchSound { cue } => Some(cue),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sounds,
            vec![
                GameCue::Countdown3,
                GameCue::Countdown2,
                GameCue::Countdown1,
                GameCue::Go
            ]
        );
        assert_eq!(replay.frame_at(0.0).phase, ReplayPhase::Opening);
    }

    #[test]
    fn reveal_captures_zero_liberty_group_before_setting_the_repetition_baseline() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &crate::match_settings::MatchSettings::default());
        // Black's lone corner selection is surrounded by White's two
        // selections, so it must be captured as part of the reveal itself.
        game.opening_selections[Player::One.index()].add(Point::new(0, 0));
        game.opening_selections[Player::Two.index()].add(Point::new(1, 0));
        game.opening_selections[Player::Two.index()].add(Point::new(0, 1));

        reveal_round_zero(&mut game);

        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(1, 0)), Cell::White);
        assert_eq!(game.board.get(Point::new(0, 1)), Cell::White);
        assert_eq!(game.capture_totals, Score { black: 0, white: 1 });
        // The repetition baseline must be the cleaned board, not the raw
        // six-slot reveal that still had the captured stone on it.
        assert_eq!(
            game.completed_history,
            std::collections::HashSet::from([game.board.clone()])
        );
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.round, 1);
    }

    #[test]
    fn reveal_captures_are_credited_only_when_capture_points_is_enabled() {
        fn revealed_game_with_corner_capture(settings: &MatchSettings) -> GameState {
            let mut game = GameState::default();
            begin_round_zero(&mut game, settings);
            game.opening_selections[Player::One.index()].add(Point::new(0, 0));
            game.opening_selections[Player::Two.index()].add(Point::new(1, 0));
            game.opening_selections[Player::Two.index()].add(Point::new(0, 1));
            reveal_round_zero(&mut game);
            game
        }

        let mut enabled_settings = MatchSettings::default();
        enabled_settings.set_capture_points(true);
        let enabled = revealed_game_with_corner_capture(&enabled_settings);
        let disabled = revealed_game_with_corner_capture(&MatchSettings::default());

        // The raw credit is recorded identically either way...
        assert_eq!(enabled.capture_totals, Score { black: 0, white: 1 });
        assert_eq!(disabled.capture_totals, Score { black: 0, white: 1 });
        // ...but it only reaches the score when Capture Points is on.
        assert_eq!(enabled.current_score(), Score { black: 0, white: 3 });
        assert_eq!(disabled.current_score(), Score { black: 0, white: 2 });
    }

    #[test]
    fn modal_pause_does_not_duplicate_countdown_cues() {
        let mut game = GameState::default();
        fill_opening(&mut game);
        start_opening_countdown(&mut game);
        advance_match_state_by(&mut game, secs(0.8), false);
        let paused_at = game.countdown_elapsed;

        assert!(advance_match_state_by(&mut game, secs(10.0), true).is_empty());
        assert_eq!(game.countdown_elapsed, paused_at);
        assert_eq!(
            advance_match_state_by(&mut game, secs(0.2), false),
            vec![GameCue::Countdown2]
        );
        assert!(advance_match_state_by(&mut game, secs(0.2), false).is_empty());
    }

    #[test]
    fn long_frame_preserves_countdown_order_and_match_time() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &crate::match_settings::MatchSettings::default());
        fill_opening(&mut game);
        start_opening_countdown(&mut game);

        assert_eq!(
            advance_match_state_by(&mut game, secs(4.5), false),
            vec![GameCue::Countdown2, GameCue::Countdown1, GameCue::Go]
        );
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.countdown_elapsed, OPENING_COUNTDOWN_TOTAL);
        assert_eq!(game.elapsed.as_secs_f64(), 4.5);
        assert_eq!(game.round_elapsed.as_secs_f64(), 1.5);
        assert_eq!(countdown_text(&game), None);
    }

    #[test]
    fn round_timeout_with_zero_moves_advances_once_resets_and_has_no_round_audio() {
        let mut game = timed_active_game(1);

        let cues = advance_match_state_by(&mut game, secs(1.0), false);

        assert!(cues.is_empty());
        assert_eq!(game.round, 2);
        assert_eq!(game.round_elapsed.as_secs_f64(), 0.0);
        assert_eq!(game.no_stone_streak, 1);
        assert_eq!(game.completed_history.len(), 1);
        assert!(game.input_frozen_this_frame);
        let recorder = game.recorder.as_ref().unwrap();
        assert!(recorder.events().iter().any(|event| matches!(
            event.kind,
            ReplayEventKind::RoundTimedOut {
                submitted: [false, false],
                resolution: crate::rules::ResolutionKind::NoPlacement,
                ..
            }
        )));

        advance_match_state_by(&mut game, secs(0.0), false);
        assert_eq!(game.round, 2);
        assert!(!game.input_frozen_this_frame);
    }

    #[test]
    fn long_frame_processes_every_round_timeout_at_exact_boundaries() {
        let mut game = timed_active_game(1);

        let cues = advance_match_state_by(&mut game, secs(2.1), false);

        assert!(cues.is_empty());
        assert_eq!(game.elapsed.as_secs_f64(), 2.1);
        assert_eq!(game.round, 3);
        // The exact integer clock leaves precisely 100,000 microseconds;
        // only the `f64` round-trip back to seconds needs any tolerance.
        assert!((game.round_elapsed.as_secs_f64() - 0.1).abs() < 1e-9);
        assert!(game.input_frozen_this_frame);
        let timeout_times = game
            .recorder
            .as_ref()
            .unwrap()
            .events()
            .iter()
            .filter_map(|event| {
                matches!(event.kind, ReplayEventKind::RoundTimedOut { .. })
                    .then_some(event.at_seconds)
            })
            .collect::<Vec<_>>();
        assert_eq!(timeout_times, vec![1.0, 2.0]);

        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();
        replay.validate().unwrap();
    }

    #[test]
    fn long_frame_stops_on_round_limit_at_the_exact_second_boundary() {
        let mut settings = settings_with(MatchSettingField::RoundTime, 1);
        settings.set_value(MatchSettingField::RoundLimit, 2);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let mut game =
            GameState::with_match_rules(crate::gameplay::MatchRules::snapshot(&settings));
        begin_active_match(&mut game);

        advance_match_state_by(&mut game, secs(2.1), false);

        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.elapsed.as_secs_f64(), 2.0);
        assert_eq!(game.round, 3);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        let replay = game.pending_replay.as_ref().unwrap();
        let timeout_times = replay
            .events
            .iter()
            .filter_map(|event| {
                matches!(event.kind, ReplayEventKind::RoundTimedOut { .. })
                    .then_some(event.at_seconds)
            })
            .collect::<Vec<_>>();
        assert_eq!(timeout_times, vec![1.0, 2.0]);
        assert_eq!(replay.duration_seconds, 2.0);
        replay.validate().unwrap();
    }

    #[test]
    fn match_time_wins_a_simultaneous_round_deadline_once() {
        let mut settings = settings_with(MatchSettingField::RoundTime, 1);
        settings.set_value(MatchSettingField::MatchTime, 1);
        settings.set_enabled(MatchSettingField::MatchTime, true);
        let mut game =
            GameState::with_match_rules(crate::gameplay::MatchRules::snapshot(&settings));
        begin_active_match(&mut game);

        advance_match_state_by(&mut game, secs(2.1), false);

        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.elapsed.as_secs_f64(), 1.0);
        assert_eq!(game.end_reasons, vec![EndReason::MatchTimeLimit]);
        let replay = game.pending_replay.as_ref().unwrap();
        let timeouts = replay
            .events
            .iter()
            .filter_map(|event| match event.kind {
                ReplayEventKind::RoundTimedOut { cause, .. } => Some(cause),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(timeouts, vec![crate::replay::TimeoutCause::MatchTime]);
        replay.validate().unwrap();
    }

    #[test]
    fn round_timeout_resolves_one_legal_or_illegal_move_and_freezes_input_frame() {
        let mut legal = timed_active_game(1);
        legal.cursors[0] = Point::new(3, 3);
        super::super::input::try_lock(
            &mut legal,
            Player::One,
            crate::clock::MatchInstant::from_secs_f64(0.4),
        );
        advance_match_state_by(&mut legal, secs(1.0), false);
        assert_eq!(legal.board.get(Point::new(3, 3)), Cell::Black);
        assert_eq!(legal.round, 2);
        assert_eq!(legal.no_stone_streak, 0);

        let mut audio = AudioCueQueue::default();
        let semantic = SemanticInput::from_pressed(&[GameAction::PlayerTwoSelect]);
        apply_player_input(
            &semantic,
            &GameplayPause::default(),
            &InputGate::default(),
            &mut legal,
            &mut audio,
        );
        assert_eq!(legal.locks, [None, None]);

        let mut illegal = timed_active_game(1);
        let occupied = Point::new(2, 2);
        illegal.board.set(occupied, Cell::White);
        illegal.completed_history = std::collections::HashSet::from([illegal.board.clone()]);
        illegal.locks[0] = Some(crate::rules::LockedMove {
            player: Player::One,
            point: occupied,
            at: crate::clock::MatchInstant::from_secs_f64(0.25),
        });
        advance_match_state_by(&mut illegal, secs(1.0), false);
        assert_eq!(illegal.board.get(occupied), Cell::White);
        assert_eq!(illegal.no_stone_streak, 1);
        assert_eq!(illegal.round, 2);
    }

    #[test]
    fn round_timeout_keeps_two_move_timestamps_and_resets_on_pause_boundary() {
        let mut game = timed_active_game(2);
        game.locks = [
            Some(crate::rules::LockedMove {
                player: Player::One,
                point: Point::new(0, 0),
                at: crate::clock::MatchInstant::from_secs_f64(0.1),
            }),
            Some(crate::rules::LockedMove {
                player: Player::Two,
                point: Point::new(8, 8),
                at: crate::clock::MatchInstant::from_secs_f64(0.4),
            }),
        ];

        advance_match_state_by(&mut game, secs(1.0), true);
        assert_eq!(game.round_elapsed.as_secs_f64(), 0.0);
        advance_match_state_by(&mut game, secs(1.999), false);
        assert_eq!(game.round, 1);
        assert_eq!(game.round_time_remaining_seconds(), Some(1));
        advance_match_state_by(&mut game, secs(0.001), false);

        assert_eq!(game.round, 2);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(game.board.get(Point::new(8, 8)), Cell::White);
        assert!(
            game.recorder
                .as_ref()
                .unwrap()
                .events()
                .iter()
                .any(|event| {
                    matches!(
                        event.kind,
                        ReplayEventKind::RoundTimedOut {
                            resolution: crate::rules::ResolutionKind::Sequential,
                            submitted: [true, true],
                            ..
                        }
                    )
                })
        );
    }

    #[test]
    fn timeout_is_significant_and_empty_forfeit_adds_only_match_end_audio() {
        let settings = settings_with(MatchSettingField::RoundLimit, 1);
        let mut game =
            GameState::with_match_rules(crate::gameplay::MatchRules::snapshot(&settings));
        begin_active_match(&mut game);

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        let replay = game.pending_replay.as_ref().unwrap();

        assert!(replay.events.iter().any(|event| {
            matches!(event.kind, ReplayEventKind::RoundTimedOut { .. })
                && event.kind.is_significant()
        }));
        assert_eq!(
            replay
                .events
                .iter()
                .filter_map(|event| match event.kind {
                    ReplayEventKind::MatchSound { cue } => Some(cue),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![GameCue::MatchEnd]
        );
    }

    #[test]
    fn match_time_expiry_reveals_partial_or_empty_round_zero_and_scores_once() {
        for with_partial_selections in [false, true] {
            let settings = settings_with(MatchSettingField::MatchTime, 1);
            let mut game = GameState::default();
            begin_round_zero(&mut game, &settings);
            if with_partial_selections {
                game.cursors[0] = Point::new(1, 1);
                super::super::input::try_opening_select(&mut game, Player::One);
                game.cursors[1] = Point::new(1, 1);
                super::super::input::try_opening_select(&mut game, Player::Two);
                game.cursors[1] = Point::new(8, 8);
                super::super::input::try_opening_select(&mut game, Player::Two);
            }

            advance_match_state_by(&mut game, secs(1.5), false);

            assert_eq!(game.phase, MatchPhase::Ended);
            assert_eq!(game.elapsed.as_secs_f64(), 1.0);
            assert_eq!(game.round, 1);
            assert_eq!(game.end_reasons, vec![EndReason::MatchTimeLimit]);
            let replay = game.pending_replay.as_ref().unwrap();
            replay.validate().unwrap();
            assert_eq!(
                replay
                    .events
                    .iter()
                    .filter(|event| matches!(event.kind, ReplayEventKind::MatchEnd))
                    .count(),
                1
            );
            let reveal = replay
                .events
                .iter()
                .find(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))
                .unwrap();
            if with_partial_selections {
                assert_eq!(reveal.frame.board.get(Point::new(1, 1)), Cell::Gray);
                assert_eq!(reveal.frame.board.get(Point::new(8, 8)), Cell::White);
            } else {
                assert_eq!(reveal.frame.board, crate::rules::Board::default());
            }
        }
    }

    /// A legal Round 0 whose reveal sweeps a stone must record a Replay that
    /// passes its own validation - the shape that regressed unnoticed because
    /// no test drove it. Black takes the corner while White, blind to that,
    /// takes both adjacent points, so `reveal_round_zero` sweeps the
    /// zero-liberty Black stone and credits White before the `OpeningReveal`
    /// frame is recorded. The recorded frame must carry that cleaned board
    /// and those credits, and `Replay::validate` must accept both - it is
    /// what `ReplayLibrary::save_completed` runs before writing, so a
    /// rejection here silently costs the player the whole recording.
    #[test]
    fn sweeping_go_reveal_records_the_cleaned_board_and_validates() {
        let settings = settings_with(MatchSettingField::MatchTime, 5);
        let mut game = GameState::default();
        begin_round_zero(&mut game, &settings);
        for (player, point) in [
            (Player::One, Point::new(0, 0)),
            (Player::Two, Point::new(1, 0)),
            (Player::One, Point::new(4, 4)),
            (Player::Two, Point::new(0, 1)),
            (Player::One, Point::new(6, 6)),
            (Player::Two, Point::new(8, 8)),
        ] {
            game.cursors[player.index()] = point;
            super::super::input::try_opening_select(&mut game, player);
        }
        assert_eq!(game.phase, MatchPhase::OpeningCountdown);

        advance_match_state_by(&mut game, secs(6.0), false);

        assert_eq!(game.phase, MatchPhase::Ended);
        let replay = game.pending_replay.as_ref().unwrap();
        let reveal = replay
            .events
            .iter()
            .find(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))
            .unwrap();
        assert_eq!(reveal.frame.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(reveal.frame.board.get(Point::new(1, 0)), Cell::White);
        assert_eq!(reveal.frame.board.get(Point::new(0, 1)), Cell::White);
        assert_eq!(
            reveal.frame.capture_totals,
            crate::rules::Score { black: 0, white: 1 }
        );
        replay.validate().unwrap();
    }

    /// The same sweeping shape through the other reveal path: Match Time
    /// expiring during Round 0 reveals a partial opening, and the board
    /// equality check applies there too.
    #[test]
    fn sweeping_deadline_reveal_records_the_cleaned_board_and_validates() {
        let settings = settings_with(MatchSettingField::MatchTime, 1);
        let mut game = GameState::default();
        begin_round_zero(&mut game, &settings);
        game.cursors[0] = Point::new(0, 0);
        super::super::input::try_opening_select(&mut game, Player::One);
        game.cursors[1] = Point::new(1, 0);
        super::super::input::try_opening_select(&mut game, Player::Two);
        game.cursors[1] = Point::new(0, 1);
        super::super::input::try_opening_select(&mut game, Player::Two);

        advance_match_state_by(&mut game, secs(1.5), false);

        assert_eq!(game.phase, MatchPhase::Ended);
        let replay = game.pending_replay.as_ref().unwrap();
        let reveal = replay
            .events
            .iter()
            .find(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))
            .unwrap();
        assert_eq!(reveal.frame.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(
            reveal.frame.capture_totals,
            crate::rules::Score { black: 0, white: 1 }
        );
        replay.validate().unwrap();
    }

    #[test]
    fn match_time_uses_exact_boundary_and_bounded_finite_deltas() {
        let settings = settings_with(MatchSettingField::MatchTime, 1);
        let mut game = GameState::default();
        begin_round_zero(&mut game, &settings);

        advance_match_state_by(&mut game, secs(f64::NAN), false);
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        advance_match_state_by(&mut game, secs(0.999), false);
        assert_eq!(game.phase, MatchPhase::Opening);
        advance_match_state_by(&mut game, secs(0.001), false);

        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.elapsed.as_secs_f64(), 1.0);
    }

    #[test]
    fn match_time_precedes_all_simultaneous_post_round_reasons() {
        let mut settings = MatchSettings::default();
        for (field, value) in [
            (MatchSettingField::MatchTime, 1),
            (MatchSettingField::RoundLimit, 1),
            (MatchSettingField::NoStoneRoundLimit, 1),
            (MatchSettingField::PointTarget, 1),
        ] {
            settings.set_value(field, value);
            settings.set_enabled(field, true);
        }
        let mut game = GameState::default();
        game.board.set(Point::new(0, 0), Cell::Black);
        game.match_rules = crate::gameplay::MatchRules::snapshot(&settings);
        begin_active_match(&mut game);

        advance_match_state_by(&mut game, secs(1.0), false);

        assert_eq!(
            game.end_reasons,
            vec![
                EndReason::MatchTimeLimit,
                EndReason::RoundLimit,
                EndReason::NoStoneRoundLimit,
                EndReason::PointTarget,
            ]
        );
        assert_eq!(game.round, 2);
        assert_eq!(
            game.recorder
                .as_ref()
                .unwrap()
                .events()
                .iter()
                .filter(|event| matches!(event.kind, ReplayEventKind::RoundTimedOut { .. }))
                .count(),
            1
        );
        game.pending_replay.as_ref().unwrap().validate().unwrap();
    }

    #[test]
    fn round_zero_never_counts_toward_ordinary_limits() {
        let mut settings = MatchSettings::default();
        for field in [
            MatchSettingField::RoundLimit,
            MatchSettingField::NoStoneRoundLimit,
            MatchSettingField::PointTarget,
        ] {
            settings.set_value(field, 1);
            settings.set_enabled(field, true);
        }
        let mut game = GameState::default();
        begin_round_zero(&mut game, &settings);
        fill_opening(&mut game);
        assert!(start_opening_countdown(&mut game));

        advance_match_state_by(&mut game, OPENING_REVEAL, false);

        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.round, 1);
        assert_eq!(game.no_stone_streak, 0);
        assert!(game.end_reasons.is_empty());
    }

    #[test]
    fn automatic_end_during_go_overlay_produces_a_valid_replay() {
        let settings = settings_with(MatchSettingField::RoundLimit, 1);
        let mut game = GameState::default();
        begin_round_zero(&mut game, &settings);
        fill_opening(&mut game);
        assert!(start_opening_countdown(&mut game));
        advance_match_state_by(&mut game, OPENING_REVEAL, false);
        game.locks = [
            Some(crate::rules::LockedMove {
                player: Player::One,
                point: Point::new(0, 8),
                at: game.elapsed,
            }),
            Some(crate::rules::LockedMove {
                player: Player::Two,
                point: Point::new(8, 8),
                at: game.elapsed,
            }),
        ];

        complete_ordinary_round(&mut game, OrdinaryCompletion::Normal);

        assert_eq!(game.phase, MatchPhase::Ended);
        assert!(game.countdown_elapsed < OPENING_COUNTDOWN_TOTAL);
        game.pending_replay.as_ref().unwrap().validate().unwrap();
    }

    #[test]
    fn match_timeout_at_go_boundary_records_the_countdown_finish() {
        let settings = settings_with(MatchSettingField::MatchTime, 4);
        let mut game = GameState::default();
        begin_round_zero(&mut game, &settings);
        advance_match_state_by(&mut game, secs(0.25), false);
        fill_opening(&mut game);
        assert!(start_opening_countdown(&mut game));

        advance_match_state_by(&mut game, secs(3.75), false);

        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.elapsed.as_secs_f64(), 4.0);
        let replay = game.pending_replay.as_ref().unwrap();
        assert!(replay.events.iter().any(|event| {
            matches!(event.kind, ReplayEventKind::OpeningCountdownFinished)
                && event.at_seconds == 4.0
        }));
        replay.validate().unwrap();
    }

    #[test]
    fn automatic_timeout_scores_and_saves_exactly_once_in_the_ecs_pipeline() {
        let mut settings = settings_with(MatchSettingField::RoundTime, 1);
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = crate::gameplay::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );

        advance_test_app(&mut app, 1.0);
        advance_test_app(&mut app, 1.0);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        let library = app
            .world()
            .resource::<crate::replay_library::ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        let saved = library.load_full(&library.entries[0].id).unwrap();
        assert_eq!(
            saved
                .events
                .iter()
                .filter(|event| matches!(event.kind, ReplayEventKind::MatchEnd))
                .count(),
            1
        );
        crate::gameplay::test_support::cleanup_restart_test_root(root);
    }

    #[test]
    fn match_time_expiry_saves_once_and_blocks_same_frame_input() {
        let settings = settings_with(MatchSettingField::MatchTime, 1);
        let (mut app, root) = crate::gameplay::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        let black_select = app
            .world()
            .resource::<KeyboardBindings>()
            .key(GameAction::PlayerOneSelect)
            .key_code();
        app.world_mut()
            .resource_mut::<bevy::input::ButtonInput<KeyCode>>()
            .press(black_select);

        advance_test_app(&mut app, 1.0);
        advance_test_app(&mut app, 1.0);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::MatchTimeLimit]);
        assert_eq!(game.round, 2);
        assert_eq!(game.board, crate::rules::Board::default());
        assert_eq!(
            app.world()
                .resource::<crate::replay_library::ReplayLibrary>()
                .entries
                .len(),
            1
        );
        crate::gameplay::test_support::cleanup_restart_test_root(root);
    }

    #[test]
    fn match_time_interrupts_an_end_hold_and_saves_once_at_the_deadline() {
        let settings = settings_with(MatchSettingField::MatchTime, 1);
        let keyboard = KeyboardBindings::default();
        let (mut app, root) = crate::gameplay::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        advance_test_app(&mut app, 0.99);
        {
            let mut keys = app
                .world_mut()
                .resource_mut::<bevy::input::ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 0.005);
        assert!(app.world().resource::<GameState>().end_snapshot.is_some());

        advance_test_app(&mut app, 0.01);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.elapsed.as_secs_f64(), 1.0);
        assert_eq!(game.end_hold, MatchDuration::ZERO);
        assert!(game.end_snapshot.is_none());
        assert_eq!(game.end_reasons, vec![EndReason::MatchTimeLimit]);
        let library = app
            .world()
            .resource::<crate::replay_library::ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        let saved = library.load_full(&library.entries[0].id).unwrap();
        assert_eq!(
            saved
                .events
                .iter()
                .filter(|event| matches!(event.kind, ReplayEventKind::MatchEnd))
                .count(),
            1
        );
        saved.validate().unwrap();
        let match_end_cues = app
            .world_mut()
            .resource_mut::<AudioCueQueue>()
            .drain()
            .into_iter()
            .filter(|cue| *cue == crate::audio::AudioCue::Game(GameCue::MatchEnd))
            .count();
        assert_eq!(match_end_cues, 1);
        advance_test_app(&mut app, 0.25);
        assert_eq!(
            app.world()
                .resource::<crate::replay_library::ReplayLibrary>()
                .entries
                .len(),
            1
        );
        crate::gameplay::test_support::cleanup_restart_test_root(root);
    }

    /// Both Selects go down on the very frame that reaches the Match Time
    /// deadline. The clock runs first and ends the Match, so neither Select
    /// is read: no selection is taken and no end hold starts, which is why
    /// the end cannot become provisional. It is not the test for a deadline
    /// landing *during* an end hold - no hold exists here when the deadline
    /// lands - which is
    /// `match_time_interrupts_an_end_hold_and_saves_once_at_the_deadline`.
    #[test]
    fn exact_active_match_deadline_preempts_input_without_becoming_provisional() {
        let settings = settings_with(MatchSettingField::MatchTime, 1);
        let keyboard = KeyboardBindings::default();
        let (mut app, root) = crate::gameplay::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        {
            let mut keys = app
                .world_mut()
                .resource_mut::<bevy::input::ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }

        advance_test_app(&mut app, 1.0);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.elapsed.as_secs_f64(), 1.0);
        assert_eq!(game.end_reasons, vec![EndReason::MatchTimeLimit]);
        assert!(game.end_snapshot.is_none());
        assert_eq!(game.end_hold, MatchDuration::ZERO);
        let library = app
            .world()
            .resource::<crate::replay_library::ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        let saved = library.load_full(&library.entries[0].id).unwrap();
        assert!(saved.events.iter().any(|event| matches!(
            event.kind,
            ReplayEventKind::RoundTimedOut {
                cause: crate::replay::TimeoutCause::MatchTime,
                ..
            }
        )));
        assert!(
            saved
                .events
                .iter()
                .all(|event| !matches!(event.kind, ReplayEventKind::SelectionChanged { .. })),
            "the deadline preempted both Selects"
        );
        saved.validate().unwrap();
        crate::gameplay::test_support::cleanup_restart_test_root(root);
    }

    /// Pins the invariant that a deferred automatic end freezes the Match
    /// clock, which is what makes a Match Time timeout landing on an already
    /// deferred end an impossible state rather than merely an unlikely one.
    ///
    /// `gameplay::complete_ordinary_round` relies on this: it handles a Match
    /// Time timeout during an end hold by discarding the snapshot, with no
    /// rollback path for one that has already deferred an end, because
    /// `elapsed` cannot grow to the Match Time limit while
    /// `provisional_end_reasons` is set. The freeze is an emergent property of
    /// two placements in [`advance_match_state_by`] - the early return before
    /// the first clock step, and the `break` right after a deferral - so this
    /// test exercises both rather than the states they produce, and both legs
    /// use a frame three times the Match Time limit so an un-freeze cannot
    /// hide behind a short budget. See the comment at that site in
    /// `gameplay::complete_ordinary_round`, which also relies on ordinary play
    /// stopping while an end is deferred; `holds`' tests pin that half.
    ///
    /// An edit that un-freezes the clock is likely to fail first inside
    /// `complete_ordinary_round` rather than at the assertions here: the next
    /// Round Time deadline resolves a round with the end still deferred, and
    /// that function's debug assertion refuses it. Either failure names the
    /// same broken freeze.
    #[test]
    fn a_pending_provisional_end_freezes_the_match_clock_short_of_the_match_time_limit() {
        let mut settings = MatchSettings::default();
        for (field, value) in [
            (MatchSettingField::RoundLimit, 1),
            (MatchSettingField::RoundTime, 1),
            (MatchSettingField::MatchTime, 3),
        ] {
            settings.set_value(field, value);
            settings.set_enabled(field, true);
        }
        let mut game = GameState::with_match_settings(&settings);
        begin_active_match(&mut game);
        // An end hold in flight with nothing deferred yet, exactly as
        // `holds::handle_hold_gestures` captures it on the gesture's first
        // frame.
        game.end_snapshot = Some(crate::gameplay::EndGestureSnapshot::capture(&game));

        // Leg one: the `break` after a deferral. This nine-second frame would
        // reach the three-second Match Time limit twice over, but Round Time
        // expires first at one second and the Round Limit end it crosses is
        // deferred to the hold - so the clock must stop there and put the
        // unspent eight seconds into `playing_delta` rather than into itself.
        // (In the running game no gesture consumes them: the timeout froze
        // that frame's input, so the holds skip it.)
        assert!(advance_match_state_by(&mut game, secs(9.0), false).is_empty());
        assert!(game.provisional_end_pending());
        assert_eq!(
            game.end_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.provisional_end_reasons.as_deref()),
            Some([EndReason::RoundLimit].as_slice())
        );
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.elapsed.as_secs_f64(), 1.0);
        assert_eq!(game.playing_delta, secs(9.0));

        // Leg two: the early return. A second oversized frame must not advance
        // the Match clock at all, however long the provisional end stays
        // pending, so `elapsed` never reaches the limit and the Match Time
        // timeout that would resolve this round never happens.
        assert!(advance_match_state_by(&mut game, secs(9.0), false).is_empty());
        assert!(game.provisional_end_pending());
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.elapsed.as_secs_f64(), 1.0);
        assert_eq!(game.playing_delta, secs(9.0));
        assert!(game.end_reasons.is_empty());
    }
}
