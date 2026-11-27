//! The two three-second hold gestures: ending a match and restarting it.
//!
//! # Why holds are snapshotted
//!
//! Ending the match requires both players to hold their own Select input. A
//! short simultaneous tap is a legitimate pair of moves, so the first frame of
//! the gesture snapshots all rollback-sensitive state. If the hold completes,
//! those incidental presses and any provisional automatic end are rolled back
//! before manual scoring; if it is released early, the moves and their
//! automatic result are committed once. While an automatic end is provisional
//! the Match is stopped - both clocks and all ordinary play - and only these
//! two gestures still run, because they are what settle it. See
//! [`EndGestureSnapshot`] for why, and for how long a snapshot lives.
//!
//! # Priority and abandonment
//!
//! The end gesture outranks Restart. While both Select inputs are held the
//! restart hold's timer is cleared - any progress a held Restart had made is
//! discarded, not paused - and a held Restart can neither clear nor pause
//! the end hold. Ending is designed to need both players and saves the
//! Match, while restarting needs one shared input and destroys it, so the
//! gesture meant to take two people is the one given the benefit of the
//! doubt. The cost is that holding Restart no longer lets one player stop
//! the other from forcing a scored end by holding both Select keys alone on
//! a shared keyboard; `docs/GAME_RULES.md` records that trade. What does
//! still end a running hold is a modal: a Select held while Help or the Home
//! confirmation opens and closes stays gated until it is released, and a
//! gated Select stops the gesture exactly as a release would. A modal does
//! one thing more than a release, though: it returns the end hold's timer to
//! zero outright, where a release lets it drain (see below), and any
//! automatic end already parked in the snapshot is committed on the first
//! frame after it closes.
//!
//! Restart is reached only while the end gesture is not running. On that
//! frame, as on every `Playing` frame without a running end gesture, the end
//! hold's timer drains - or, under a modal, is cleared - and the snapshot is
//! dropped unless an automatic end is deferred to it. Kept past its own
//! gesture, a snapshot with nothing deferred would be adopted by the next end
//! hold as its pre-hold state and restored over every round played in
//! between. So with nothing deferred, both Selects coming down again start a
//! fresh hold on a fresh snapshot - from whatever the drain has left of the
//! timer, which is the point of the drain. It follows that a hold assembled
//! over several contacts rolls back, on completing, only the last contact's
//! incidental presses: the rounds the earlier contacts resolved stand, as a
//! released tap's round always has. A deferred end keeps the snapshot
//! with it until Restart either completes or is released without both
//! Selects having come down again; if they do come down first, the end hold
//! resumes on that same snapshot, which cannot have gone stale because a
//! deferred end stops the Match. A completed restart **drops** the recorder
//! before resetting, so an abandoned match is never saved and cannot leak
//! events into the fresh match, and re-arms the input gate so the still held
//! restart input cannot immediately restart again. Only that input stays
//! gated: the gate is per action, so the fresh match is playable at once for
//! anything the players are not holding.
//!
//! A completed end arms no gate, so a Restart still held when the end hold
//! completes is not suppressed; because the end branch had been clearing its
//! timer, it starts from zero in `Ended`, and three more seconds of holding
//! restart the just-scored Match. That is a choice rather than an oversight:
//! the end branch owns the gate and could arm it in one line, but by then the
//! Replay is written when storage allows one, so nothing is destroyed, and
//! gating a completed end would change how ending works rather than which
//! gesture wins.
//!
//! # Why the end hold drains instead of resetting
//!
//! Until 2026-10-05 the end hold's timer returned to zero on any single frame
//! on which both Selects were not read as held. An accessibility review
//! recorded that as a barrier rather than an annoyance: under shipped
//! defaults every automatic end rule is off, so the dual-Select hold is the
//! only route to a scored Match, and a player with a tremor, a weak grip or a
//! flaky key could not finish a game at all. The owner's decision, recorded
//! here and in `docs/GAME_RULES.md`, has five parts.
//!
//! **The end hold drains at half the rate it fills** ([`end_hold_drain`]).
//! Not a reset, and not one-to-one: a player making contact half the time
//! gains nothing at one-to-one and never completes, while at half rate each
//! second of contact is worth a net quarter second, so three seconds of hold
//! take about twelve seconds of trying. That worst case is the one the drain
//! exists for.
//!
//! **The restart hold keeps resetting.** The barrier is entirely on the end
//! gesture: Restart is destructive, takes one shared input rather than two,
//! and nobody is blocked from playing by restart being hard, so making the
//! destructive gesture easier to complete by accident would buy nothing. It
//! is the same asymmetry as the priority rule above - the constructive
//! gesture that needs two people gets the benefit of the doubt.
//!
//! **Three seconds stays, and is not configurable.** The barrier was the
//! cliff, not the length, and changing both at once would have hidden which
//! one helped; `docs/ideas/configurable-hold-duration.md` parks the setting
//! with its reasons.
//!
//! **A modal still ends the gesture outright.** Opening Help mid-hold stops
//! it, and it has to be started again, because a player reads a modal as the
//! gesture having been interrupted - which is what happened. The drain is
//! for a player who let go, not for a modal suppressing a Select they are
//! still physically holding, and [`handle_hold_gestures`] tells the two
//! apart at the one place that knows ([`EndGestureLapse`]).
//!
//! **The bar drains with the timer.** A bar that drains is better feedback
//! than one that vanishes, because it shows a player they are losing
//! progress while they can still act on it. It also means a non-zero end
//! hold no longer implies a running end gesture, so the bar now names the
//! gesture that is *running* rather than the timer that is non-zero; see
//! [`active_hold_progress`] for what that protects.
//!
//! The drain is measured in `playing_delta`, the clock the fill is measured
//! in, so that one clock runs both directions and "half the rate" is exact
//! in the same units; the comment on the time bases in
//! [`handle_hold_gestures`] says why the alternative was not taken.

use bevy::prelude::*;

use super::input::action_pressed;
use super::{
    EndGestureSnapshot, GameState, GameplayPause, HOLD_DURATION, InputGate, MatchPhase,
    begin_round_zero, finish_match, reset_match, restore_end_gesture_snapshot,
};
use crate::audio::{AudioCueQueue, GameCue};
use crate::clock::MatchDuration;
use crate::input::{GameAction, SemanticInput};
use crate::match_settings::MatchSettings;
use crate::replay::EndReason;
#[cfg(test)]
use crate::replay_library::ReplayLibrary;

/// One of the two hold gestures, as the progress bar needs to tell them
/// apart. [`handle_hold_gestures`] records which one is running each frame in
/// `GameState::running_hold`, and [`active_hold_progress`] reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HoldGesture {
    End,
    Restart,
}

/// Why the end gesture is not running on a `Playing` frame. The caller of
/// [`end_gesture_not_running`] decides, because the difference is whether a
/// modal is up, and that function does not know.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EndGestureLapse {
    /// A Select is up, or is still gated from a modal that has since closed.
    /// The timer drains at half the rate it filled ([`end_hold_drain`]), so
    /// the gesture can be resumed from wherever the drain has left it. A
    /// Select held through a modal stays gated until it is released, and the
    /// modal has already returned the timer to zero, so the drain from zero
    /// is a no-op and such a Select behaves exactly as it did before the
    /// hold drained at all.
    Released,
    /// A modal - Help, or the Home confirmation - is open, and the gate reads
    /// every input as released whether or not the player is still physically
    /// holding. The timer returns to zero: a player reads a modal as the
    /// gesture having been interrupted, which is what happened, and starts
    /// it again.
    Interrupted,
    /// A clock boundary - a Round Time timeout - owns this frame:
    /// `input_frozen_this_frame` is set, and [`handle_hold_gestures`] returns
    /// before either hold can advance. The timer stays where it is, as the
    /// restart hold's does, so that the two directions of the end hold are
    /// measured over the same frames and "half the rate it fills" is exact.
    /// A release seen on such a frame still stops the gesture and drops an
    /// unparked snapshot; its drain begins on the next frame.
    Frozen,
}

/// How much slower the end hold drains than it fills. See
/// [`end_hold_drain`].
const END_HOLD_DRAIN_DIVISOR: u64 = 2;

/// How much a released end hold loses over `elapsed` of its own time base:
/// half of it, so the hold drains at half the rate it fills.
///
/// The rate is the owner's, decided on 2026-10-05 against the two obvious
/// alternatives. A reset is the barrier the drain removes. A one-to-one
/// drain looks fairer and is not: a player making contact half the time
/// gains in each contact exactly what the next lapse takes back, and never
/// completes. At half rate the same player nets a quarter second for every
/// second of trying and completes in about twelve, which is the case this
/// exists for.
///
/// `elapsed` is `playing_delta`, the fill's time base, and the drain
/// truncates to the microsecond as every `MatchDuration` operation does.
fn end_hold_drain(elapsed: MatchDuration) -> MatchDuration {
    MatchDuration::from_micros(elapsed.as_micros() / END_HOLD_DRAIN_DIVISOR)
}

pub(super) fn handle_hold_gestures(
    semantic: Res<SemanticInput>,
    time: Res<Time>,
    pause: Res<GameplayPause>,
    mut input_gate: ResMut<InputGate>,
    mut game: ResMut<GameState>,
    match_settings: Res<MatchSettings>,
    mut audio: ResMut<AudioCueQueue>,
) {
    // Mixed-source end hold: each player's own Select may come from their
    // keyboard key or their assigned controller. A Select still held from
    // before a modal is gated, so it cannot keep the gesture running while
    // the other player's inputs keep working.
    let end_gesture_running = game.phase == MatchPhase::Playing
        && action_pressed(&semantic, &input_gate, GameAction::PlayerOneSelect)
        && action_pressed(&semantic, &input_gate, GameAction::PlayerTwoSelect);
    let restart_pressed = action_pressed(&semantic, &input_gate, GameAction::RestartMatch);

    // Recorded for the progress bar before anything can return early, so a
    // frame on which a clock boundary stops the gestures from acting still
    // reports the gesture that is held, and the frame on which the end hold
    // completes - read as held here, before it fires below - still reports
    // it running, with its bar full. Under a modal the gate reads every
    // input as released, so nothing is running and the bar has nothing to
    // show. A completed restart clears this with the rest of the Match.
    game.running_hold = running_hold(end_gesture_running, restart_pressed);

    // Settled before the early return below, not after it: the snapshot's
    // lifetime follows the gesture on every frame, including the ones on
    // which a modal or a clock boundary stops the gestures from acting. The
    // clock runs before this system, so a snapshot left in place on such a
    // frame is the one it would park the next frame's automatic end into -
    // and the one the next end hold would adopt.
    //
    // The timer is settled here too, but by why the gesture stopped, which
    // only this caller knows: under a modal the gesture was interrupted and
    // its timer returns to zero; on a frame the clock owns neither hold
    // moves, so it stays where it is; and on any other frame the player let
    // go and the timer drains. The decision is made here and handed down
    // rather than inferred inside, so that a reader of the call site can see
    // which case this is.
    if game.phase == MatchPhase::Playing && !end_gesture_running {
        let lapse = if pause.input_blocked {
            EndGestureLapse::Interrupted
        } else if game.input_frozen_this_frame {
            EndGestureLapse::Frozen
        } else {
            EndGestureLapse::Released
        };
        end_gesture_not_running(&mut game, lapse);
    }

    // A modal is a real pause and still stops the restart hold's timer
    // outright; the end hold's has just been returned to zero, because the
    // gate stays fully armed while a modal is open, so the lines above always
    // read a modal frame as an interruption. A frame on which the clock
    // resolved or ended play first stops both holds where they stand - the
    // end hold's drain included, which is what `EndGestureLapse::Frozen` is
    // for - but it cannot keep a finished end gesture alive either: the lines
    // above have already seen whether both Selects are down, and have dropped
    // an unparked snapshot if they are not.
    if pause.input_blocked || game.input_frozen_this_frame {
        return;
    }

    // The two holds deliberately advance on different time bases. The restart
    // hold uses the whole frame delta; the end hold below uses
    // `game.playing_delta`, the part of this frame spent in ordinary play.
    // That asymmetry is required, not an oversight: `playing_delta` is
    // credited only with the time `advance_match_state_by`'s clock steps
    // spend in `Playing` - or, while an automatic end is parked and the clock
    // is frozen, with the whole frame - and reset to zero at the top of every
    // call, so it is always zero during Round 0
    // (`Opening`/`OpeningCountdown`) and after the Match has `Ended` - the
    // phases below where restart is still offered. A restart hold measured in
    // `playing_delta` could therefore never complete outside ordinary play.
    // The end hold, which only exists during `Playing`, wants the narrower
    // base so a long frame crossing reveal cannot credit pre-reveal time to
    // the gesture.
    //
    // The end hold's drain, in `end_gesture_not_running` above, uses
    // `playing_delta` as well, so that one clock measures the hold in both
    // directions and "half the rate it fills" is exact in the same units -
    // the frame a clock boundary owns, on which neither direction moves,
    // included. The frame delta would have been observably the same today:
    // during `Playing` the two bases differ only on the frame that crosses
    // reveal, when the end hold is still zero; on a frame that ends the
    // Match, when it no longer matters; and under a modal, which returns the
    // timer to zero before any delta is read. It would, though, have let a
    // Match paused without its input blocked - a state no current caller
    // creates, since `ui::hud` sets both flags together - drain a hold it
    // could not fill. The shared clock rules that out by construction rather
    // than by the current set of callers.
    let delta = MatchDuration::from(time.delta());

    match game.phase {
        MatchPhase::Opening | MatchPhase::OpeningCountdown => {
            game.end_hold = MatchDuration::ZERO;
            game.end_snapshot = None;
            if restart_pressed {
                if advance_restart_hold(&mut game, &mut input_gate, &match_settings, delta) {
                    audio.game(GameCue::MatchRestart);
                }
            } else {
                game.restart_hold = MatchDuration::ZERO;
            }
        }
        MatchPhase::Playing => {
            if end_gesture_running {
                // Ending outranks restarting: this branch is tested first, so
                // a held Restart cannot clear or pause the end hold, and the
                // restart hold's timer is cleared for as long as both Selects
                // are held.
                game.restart_hold = MatchDuration::ZERO;
                if game.end_snapshot.is_none() {
                    // Short simultaneous taps still submit moves. A completed hold
                    // rolls those incidental lock presses back before scoring.
                    game.end_snapshot = Some(EndGestureSnapshot::capture(&game));
                }
                game.end_hold = (game.end_hold + game.playing_delta).min(HOLD_DURATION);
                if game.end_hold >= HOLD_DURATION {
                    restore_end_gesture_snapshot(&mut game);
                    game.pending_replay = finish_match(&mut game, vec![EndReason::Manual]);
                }
            } else if restart_pressed {
                // Restart deliberately abandons the match, which is why it is
                // the losing branch: it runs only while the scoring gesture is
                // not. An automatic end parked in the snapshot stays parked
                // until Restart either completes, dropping it, or is released,
                // committing it.
                if advance_restart_hold(&mut game, &mut input_gate, &match_settings, delta) {
                    audio.game(GameCue::MatchRestart);
                }
            } else {
                commit_provisional_end(&mut game);
                game.end_snapshot = None;
                game.restart_hold = MatchDuration::ZERO;
            }
        }
        MatchPhase::Ended => {
            game.end_hold = MatchDuration::ZERO;
            game.end_snapshot = None;
            // RestartMatch is already OR'd across keyboard and both
            // assigned controllers' profile-local Restart action.
            if restart_pressed {
                if advance_restart_hold(&mut game, &mut input_gate, &match_settings, delta) {
                    audio.game(GameCue::MatchRestart);
                }
            } else {
                game.restart_hold = MatchDuration::ZERO;
            }
        }
    }
}

/// Which hold gesture is running this frame: the one whose branch of
/// [`handle_hold_gestures`] would advance its timer if nothing stopped the
/// gestures from acting. The end gesture outranks Restart, so a held Restart
/// is not running while both Selects are down; `end_gesture_running` is
/// already false outside `Playing`, where the end gesture does not exist.
fn running_hold(end_gesture_running: bool, restart_pressed: bool) -> Option<HoldGesture> {
    if end_gesture_running {
        Some(HoldGesture::End)
    } else if restart_pressed {
        Some(HoldGesture::Restart)
    } else {
        None
    }
}

/// What every `Playing` frame without a running end gesture does to it -
/// whether Restart is held or not, and whether or not a modal or a clock
/// boundary then stops the gestures from acting: its timer drains, stays
/// where it is, or returns to zero, as `lapse` says; and its snapshot is
/// dropped unless an automatic end is parked in it.
///
/// This is the one place that decides whether a snapshot with nothing parked
/// outlives its gesture, which is why [`handle_hold_gestures`] calls it for
/// every such frame before anything can return early. The defect it closes
/// came from paths that kept a snapshot the release branch would have
/// dropped: the Restart branch, and the modal and clock-boundary frames on
/// which the gestures do not run at all. A snapshot is only the rollback point of
/// the gesture that captured it. Kept any longer, the clock would park a
/// later automatic end into it, and the next end hold would find it already
/// there, adopt it as its own pre-hold state, and on completing restore it -
/// erasing every round played in between from the score and the Replay.
/// With nothing parked there is nothing for it to wait for, and a hold that
/// begins later captures its own. See [`EndGestureSnapshot`] for why a
/// parked end is the exception and why keeping it is safe. (The release
/// branch drops the snapshot again once it has committed what was parked.)
///
/// The timer is the half that changed when the end hold began to drain
/// rather than reset, and only the timer: the gesture is "not running" the
/// moment a Select comes up, even while its timer is still draining, and the
/// snapshot goes exactly when it went before. A drained timer that is later
/// refilled therefore runs on a fresh snapshot, not the one its earlier
/// progress was made on - unless an end is parked, in which case the Match
/// is stopped and the kept snapshot cannot have gone stale.
fn end_gesture_not_running(game: &mut GameState, lapse: EndGestureLapse) {
    game.end_hold = match lapse {
        EndGestureLapse::Released => game.end_hold - end_hold_drain(game.playing_delta),
        EndGestureLapse::Frozen => game.end_hold,
        EndGestureLapse::Interrupted => MatchDuration::ZERO,
    };
    if !game.provisional_end_pending() {
        game.end_snapshot = None;
    }
}

fn commit_provisional_end(game: &mut GameState) {
    let reasons = game
        .end_snapshot
        .as_mut()
        .and_then(|snapshot| snapshot.provisional_end_reasons.take());
    if let Some(reasons) = reasons {
        game.pending_replay = finish_match(game, reasons);
    }
}

/// The label and 0..=1 fraction of the hold gesture that is running, or of
/// an end hold still draining after its gesture stopped.
///
/// The running gesture is preferred to the timer that is non-zero, and that
/// is what keeps the progress bar from ever disagreeing with the gesture that
/// is actually going to fire. While the end hold reset, a non-zero `end_hold`
/// meant the end gesture was running, because the timer was zeroed the
/// instant it was not; a released end hold now stays non-zero for up to six
/// seconds while Restart may be running underneath it, so `end_hold` alone
/// would name the gesture that is *not* going to fire. [`handle_hold_gestures`]
/// records which gesture is running each frame in `GameState::running_hold`,
/// before anything can stop the gestures from acting, and this reads that
/// first.
///
/// With nothing running, a draining end hold is still reported, during
/// `Playing` only: it is the one gesture that can be resumed, and a bar that
/// drains tells a player they are losing progress while they can still act on
/// it, where a bar that vanished told them nothing. Once the Match has left
/// `Playing` there is nothing to resume, so nothing is reported, and the
/// `Ended` arm zeroes the timer a frame later in any case. A restart hold
/// frozen behind a modal is not reported either; the modal covers the bar.
///
/// The frame on which the end hold completes still reports it running, at
/// 1.0: the gesture was read as held at the top of that frame, before it
/// fired, so the bar is seen to reach full before the end banner replaces it.
/// That was already so when the timer alone decided, and is kept on purpose.
pub(crate) fn active_hold_progress(game: &GameState) -> Option<(&'static str, f32)> {
    match game.running_hold {
        Some(HoldGesture::End) => Some(("Ending Match", hold_fraction(game.end_hold))),
        Some(HoldGesture::Restart) => Some(("Restarting Match", hold_fraction(game.restart_hold))),
        None if game.phase == MatchPhase::Playing && !game.end_hold.is_zero() => {
            Some(("Ending Match", hold_fraction(game.end_hold)))
        }
        None => None,
    }
}

fn hold_fraction(hold: MatchDuration) -> f32 {
    (hold.as_secs_f64() / HOLD_DURATION.as_secs_f64()).clamp(0.0, 1.0) as f32
}

pub(super) fn advance_restart_hold(
    game: &mut GameState,
    input_gate: &mut InputGate,
    match_settings: &MatchSettings,
    delta: MatchDuration,
) -> bool {
    game.restart_hold = (game.restart_hold + delta).min(HOLD_DURATION);
    if game.restart_hold >= HOLD_DURATION {
        // An abandoned recorder is dropped before reset, so it cannot be
        // completed, saved, or leak events into the fresh match.
        drop(game.recorder.take());
        reset_match(game);
        begin_round_zero(game, match_settings);
        input_gate.arm();
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        active_match_input_app, cleanup_restart_test_root, ended_match_input_app, match_input_app,
        opening_match_input_app,
    };
    use super::*;
    use crate::audio;
    use crate::clock::MatchInstant;
    use crate::gameplay::countdown::begin_active_match;
    use crate::gameplay::input::{
        jump_to_teleport_marker, move_cursor, store_teleport_marker, try_lock,
    };
    use crate::gameplay::{
        PLAYER_STARTS, initialize_match_state, player_index, record_game_event, replay_frame,
    };
    use crate::input::controller::{
        ControllerAssignments, ControllerBindings, ControllerInput, DeviceFamily, ProfileAction,
    };
    use crate::input::keyboard::{KeyboardBindings, KeyboardKey};
    use crate::match_settings::MatchSettingField;
    use crate::replay::ReplayEventKind;
    use crate::rules::{Board, Cell, LockedMove, Player, Point, Score};
    use crate::settings::GamePreferences;
    use crate::test_support::advance_test_app;
    use crate::ui::hud::match_hud_visibility;
    use bevy::input::ButtonInput;

    fn hold_secs(duration: MatchDuration) -> f32 {
        duration.as_secs_f64() as f32
    }

    #[test]
    fn match_reset_preserves_custom_keyboard_bindings() {
        let mut game = GameState::default();
        let mut bindings = KeyboardBindings::default();
        bindings.assign(GameAction::RestartMatch, KeyboardKey::Enter);
        let expected = bindings.clone();
        game.phase = MatchPhase::Ended;

        reset_match(&mut game);

        assert_eq!(bindings, expected);
        assert_eq!(bindings.key(GameAction::RestartMatch), KeyboardKey::Enter);
        assert_eq!(game.phase, MatchPhase::Opening);
    }

    #[test]
    fn round_zero_disables_end_hold_but_keeps_restart_available() {
        let keyboard = KeyboardBindings::default();
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        let (mut end_app, end_root) = opening_match_input_app(keyboard.clone());
        {
            let mut keys = end_app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(first_select);
            keys.press(second_select);
        }
        advance_test_app(
            &mut end_app,
            hold_secs(HOLD_DURATION + MatchDuration::from_secs_f64(0.5)),
        );
        let game = end_app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.end_hold, MatchDuration::from_secs_f64(0.0));
        assert_eq!(game.opening_selections[0].len(), 1);
        assert_eq!(game.opening_selections[1].len(), 1);
        assert!(
            end_app
                .world()
                .resource::<ReplayLibrary>()
                .entries
                .is_empty()
        );
        cleanup_restart_test_root(end_root);

        let (mut restart_app, restart_root) = opening_match_input_app(keyboard);
        let abandoned_id = restart_app
            .world()
            .resource::<GameState>()
            .recorder
            .as_ref()
            .unwrap()
            .id()
            .to_string();
        restart_app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        advance_test_app(&mut restart_app, hold_secs(HOLD_DURATION));
        let game = restart_app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.round, 0);
        assert!(
            game.opening_selections
                .iter()
                .all(|selection| selection.is_empty())
        );
        assert!(game.recorder.is_some());
        assert_ne!(game.recorder.as_ref().unwrap().id(), abandoned_id);
        assert!(
            restart_app
                .world()
                .resource::<ReplayLibrary>()
                .entries
                .is_empty()
        );
        cleanup_restart_test_root(restart_root);
    }

    #[test]
    fn frame_crossing_reveal_counts_only_round_one_time_toward_end_hold() {
        let keyboard = KeyboardBindings::default();
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        let (mut app, root) = match_input_app(keyboard, MatchPhase::OpeningCountdown);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(first_select);
            keys.press(second_select);
        }

        advance_test_app(&mut app, 3.25);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.end_hold, MatchDuration::from_secs_f64(0.25));
        cleanup_restart_test_root(root);
    }

    #[test]
    fn manual_end_rollback_preserves_the_go_overlay_finish_timeline() {
        let keyboard = KeyboardBindings::default();
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        let (mut app, root) = match_input_app(keyboard, MatchPhase::OpeningCountdown);

        advance_test_app(&mut app, 3.0);
        assert_eq!(
            super::super::countdown::countdown_text(app.world().resource::<GameState>()),
            Some("Go!")
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(first_select);
            keys.press(second_select);
        }
        advance_test_app(&mut app, 0.25);
        advance_test_app(&mut app, 2.75);

        let library = app.world().resource::<ReplayLibrary>();
        let replay = library.load_full(&library.entries[0].id).unwrap();
        replay.validate().unwrap();
        assert_eq!(replay.duration_seconds, 6.0);
        assert_eq!(
            replay
                .events
                .iter()
                .filter_map(
                    |event| matches!(event.kind, ReplayEventKind::OpeningCountdownFinished)
                        .then_some(event.at_seconds)
                )
                .collect::<Vec<_>>(),
            vec![3.75]
        );
        assert_eq!(
            replay.frame_at(3.74).opening_countdown,
            Some(crate::opening::OpeningCountdownStage::Go)
        );
        assert_eq!(replay.frame_at(3.75).opening_countdown, None);
        assert!(replay.events.iter().all(|event| !matches!(
            event.kind,
            ReplayEventKind::SelectionChanged { .. }
                | ReplayEventKind::RoundResolved { .. }
                | ReplayEventKind::RoundTimedOut { .. }
        )));
        cleanup_restart_test_root(root);
    }

    #[test]
    fn default_space_restarts_through_semantic_pipeline_after_three_seconds() {
        let (mut app, root) = ended_match_input_app(KeyboardBindings::default());
        app.world_mut().resource_mut::<InputGate>().arm();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyM);
        advance_test_app(&mut app, 0.1);
        let gate = app.world().resource::<InputGate>();
        assert!(
            !gate.is_gated(GameAction::RestartMatch),
            "restart gating must not wait on unrelated held gameplay inputs"
        );
        assert!(
            gate.is_gated(GameAction::PlayerOneSelect),
            "the held Select is the one action that stays gated"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);

        for expected in [1.0, 2.0] {
            advance_test_app(&mut app, 1.0);
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Ended);
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(expected));
        }
        advance_test_app(&mut app, 1.0);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.countdown_elapsed, MatchDuration::from_secs_f64(0.0));
        assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(0.0));
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Game(GameCue::MatchRestart)]
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        advance_test_app(&mut app, 0.5);
        assert_eq!(
            app.world().resource::<GameState>().phase,
            MatchPhase::Opening
        );
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        cleanup_restart_test_root(root);
    }

    #[test]
    fn active_default_space_restart_resets_state_discards_replay_and_starts_fresh_recording() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        let expected_preferences = GamePreferences {
            show_match_timer: true,
            show_round_number: false,
            ..default()
        };
        *app.world_mut().resource_mut::<GamePreferences>() = expected_preferences;
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.board.set(Point::new(4, 4), Cell::Black);
            game.cursors = [Point::new(3, 3), Point::new(5, 5)];
            game.teleport_markers = [Some(Point::new(2, 2)), Some(Point::new(6, 6))];
            game.locks[0] = Some(LockedMove {
                player: Player::One,
                point: Point::new(3, 3),
                at: MatchInstant::from_secs_f64(4.0),
            });
            let dirty_board = game.board.clone();
            game.completed_history.insert(dirty_board);
            game.round = 8;
            game.elapsed = MatchInstant::from_secs_f64(17.0);
            record_game_event(
                &mut game,
                ReplayEventKind::MarkerChanged {
                    player: Player::One,
                    point: Point::new(2, 2),
                },
            );
        }

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            active_hold_progress(app.world().resource::<GameState>()),
            Some((
                "Restarting Match",
                (1.0 / HOLD_DURATION.as_secs_f64()) as f32
            ))
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        advance_test_app(&mut app, 0.25);
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(0.0)
        );
        assert_eq!(
            active_hold_progress(app.world().resource::<GameState>()),
            None
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        advance_test_app(&mut app, hold_secs(HOLD_DURATION));

        let game = app.world().resource::<GameState>();
        assert_eq!(game.board, Board::default());
        assert_eq!(game.cursors, PLAYER_STARTS);
        assert_eq!(game.teleport_markers, [None, None]);
        assert_eq!(game.control_hints_visible, [true, true]);
        assert_eq!(game.locks, [None, None]);
        assert_eq!(
            game.completed_history,
            std::collections::HashSet::from([Board::default()])
        );
        assert_eq!(game.round, 0);
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.countdown_elapsed, MatchDuration::from_secs_f64(0.0));
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        assert_eq!(game.current_score(), Score::default());
        assert_eq!(game.end_hold, MatchDuration::from_secs_f64(0.0));
        assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(0.0));
        assert!(game.end_snapshot.is_none());
        assert!(game.recorder.is_some());
        assert_eq!(
            *app.world().resource::<GamePreferences>(),
            expected_preferences
        );
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        advance_test_app(&mut app, 1.0);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Opening);
        // The fresh match's clock has been running since the restart landed -
        // it does not wait for the still-held Restart key to come up. Only
        // that key is gated, and the clock is shared by both players.
        assert_eq!(game.elapsed.as_secs_f64(), 1.0);
        let mut recorder = game.recorder.clone().unwrap();
        let replay = recorder.complete(1, 0.0, replay_frame(game)).unwrap();
        assert_eq!(replay.events.len(), 3);
        assert_eq!(replay.events[0].kind, ReplayEventKind::MatchStart);
        assert_eq!(replay.events[0].at_seconds, 0.0);
        assert_eq!(replay.events[1].kind, ReplayEventKind::OpeningStarted);
        assert_eq!(replay.events[2].kind, ReplayEventKind::MatchEnd);
        cleanup_restart_test_root(root);
    }

    #[test]
    fn remapped_keyboard_and_either_assigned_controller_restart_active_match() {
        let mut keyboard = KeyboardBindings::default();
        keyboard.assign(GameAction::RestartMatch, KeyboardKey::Enter);
        let expected_keyboard = keyboard.clone();
        let (mut keyboard_app, keyboard_root) = active_match_input_app(keyboard);
        keyboard_app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        advance_test_app(&mut keyboard_app, hold_secs(HOLD_DURATION));
        assert_eq!(
            keyboard_app.world().resource::<GameState>().phase,
            MatchPhase::Opening
        );
        assert_eq!(
            *keyboard_app.world().resource::<KeyboardBindings>(),
            expected_keyboard
        );
        cleanup_restart_test_root(keyboard_root);

        for active_player in Player::ALL {
            let (mut controller_app, controller_root) =
                active_match_input_app(KeyboardBindings::default());
            let mut assignments = ControllerAssignments::default();
            for player in [Player::Two, Player::One] {
                let mut gamepad = Gamepad::default();
                if player == active_player {
                    gamepad.digital_mut().press(GamepadButton::North);
                }
                let entity = controller_app.world_mut().spawn(gamepad).id();
                assignments.connect(
                    entity,
                    DeviceFamily::Unknown,
                    format!("{player:?} controller"),
                );
            }
            *controller_app
                .world_mut()
                .resource_mut::<ControllerAssignments>() = assignments;
            let expected_assignments = Player::ALL.map(|player| {
                controller_app
                    .world()
                    .resource::<ControllerAssignments>()
                    .assigned_entity(player)
            });
            let expected_bindings = controller_app
                .world()
                .resource::<ControllerBindings>()
                .clone();
            assert_eq!(
                controller_app
                    .world()
                    .resource::<ControllerBindings>()
                    .input(active_player, ProfileAction::Restart),
                ControllerInput::North
            );

            advance_test_app(&mut controller_app, hold_secs(HOLD_DURATION));
            assert_eq!(
                controller_app.world().resource::<GameState>().phase,
                MatchPhase::Opening
            );
            assert_eq!(
                Player::ALL.map(|player| {
                    controller_app
                        .world()
                        .resource::<ControllerAssignments>()
                        .assigned_entity(player)
                }),
                expected_assignments
            );
            assert_eq!(
                *controller_app.world().resource::<ControllerBindings>(),
                expected_bindings
            );
            cleanup_restart_test_root(controller_root);
        }
    }

    #[test]
    fn mixed_keyboard_and_controller_restart_sources_remain_held_until_both_release() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::North);
        let entity = app.world_mut().spawn(gamepad).id();
        app.world_mut()
            .resource_mut::<ControllerAssignments>()
            .connect(
                entity,
                DeviceFamily::Unknown,
                "Mixed source controller".into(),
            );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);

        advance_test_app(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(1.0)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(2.0)
        );
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<GameState>().phase,
            MatchPhase::Opening
        );
        cleanup_restart_test_root(root);
    }

    #[test]
    fn restart_hold_resets_on_release_and_pauses_behind_modal_gate() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(1.0)
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        advance_test_app(&mut app, 0.25);
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(0.0)
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        advance_test_app(&mut app, 1.0);
        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::blocking();
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(1.0)
        );

        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::default();
        advance_test_app(&mut app, 1.0);
        // Restart was held when the modal opened, so it is gated on the way
        // out and the interrupted hold drops back to zero rather than sitting
        // frozen at its pre-modal value. The player has to release and press
        // again either way; the per-action gate just does not keep the stale
        // duration around while they do.
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(0.0)
        );
        assert!(
            app.world()
                .resource::<InputGate>()
                .is_gated(GameAction::RestartMatch)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyM);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        advance_test_app(&mut app, 0.1);
        assert_eq!(
            app.world().resource::<GameState>().restart_hold,
            MatchDuration::from_secs_f64(0.0)
        );
        assert!(
            !app.world()
                .resource::<InputGate>()
                .is_gated(GameAction::RestartMatch),
            "closing a modal during restart waits for semantic Restart, not unrelated actions"
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        advance_test_app(&mut app, hold_secs(HOLD_DURATION));
        assert_eq!(
            app.world().resource::<GameState>().phase,
            MatchPhase::Opening
        );
        cleanup_restart_test_root(root);
    }

    #[test]
    fn a_select_gated_from_before_a_modal_cannot_complete_the_end_gesture() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());

        // Player One is resting a finger on Select when a modal opens, so that
        // Select is gated on the way out.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyM);
        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::blocking();
        advance_test_app(&mut app, 0.1);
        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::default();

        // Player Two now holds their own Select. The end gesture reads press
        // *level*, not the press edge, so without the gate this held key would
        // score a match Player One never asked to end.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Digit2);
        advance_test_app(&mut app, hold_secs(HOLD_DURATION) + 0.5);
        assert!(
            app.world()
                .resource::<InputGate>()
                .is_gated(GameAction::PlayerOneSelect)
        );
        let game = app.world().resource::<GameState>();
        assert_eq!(game.end_hold, MatchDuration::ZERO);
        assert_eq!(game.phase, MatchPhase::Playing);

        // Releasing and pressing again is exactly what the gate asks for, and
        // the gesture then runs normally.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyM);
        advance_test_app(&mut app, 0.1);
        assert!(
            !app.world()
                .resource::<InputGate>()
                .is_gated(GameAction::PlayerOneSelect)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyM);
        advance_test_app(&mut app, hold_secs(HOLD_DURATION));
        assert_eq!(app.world().resource::<GameState>().phase, MatchPhase::Ended);
        cleanup_restart_test_root(root);
    }

    #[test]
    fn one_players_gated_select_leaves_the_other_player_free_to_play() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());

        // Player One holds Select through a modal, so it is gated afterwards.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyM);
        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::blocking();
        advance_test_app(&mut app, 0.1);
        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::default();
        advance_test_app(&mut app, 0.1);
        assert!(
            app.world()
                .resource::<InputGate>()
                .is_gated(GameAction::PlayerOneSelect)
        );

        // Player Two moves and selects while that gate is still up. Under the
        // old single flag neither player could do anything at all.
        let before = app.world().resource::<GameState>().cursors[player_index(Player::Two)];
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyF);
        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert_eq!(
            game.cursors[player_index(Player::Two)],
            Point::new(before.x + 1, before.y)
        );
        assert!(game.locks[player_index(Player::One)].is_none());

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Digit2);
        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert!(game.locks[player_index(Player::Two)].is_some());
        assert!(game.locks[player_index(Player::One)].is_none());
        cleanup_restart_test_root(root);
    }

    /// Inverted when ending was given priority over restarting. Its
    /// predecessor pressed Backquote - Player Two's Set Teleport Marker key -
    /// rather than their Select, so it never ran an end hold at all and
    /// would have passed under either ordering. This one takes all three
    /// keys from the bindings, holds them from the same frame, and then lets
    /// the Selects up to prove Restart was really down the whole time.
    #[test]
    fn active_end_hold_takes_priority_over_concurrent_restart() {
        let keyboard = KeyboardBindings::default();
        let restart = keyboard.key(GameAction::RestartMatch).key_code();
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        let (mut app, root) = active_match_input_app(keyboard);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(restart);
            keys.press(first_select);
            keys.press(second_select);
        }

        advance_test_app(&mut app, 1.0);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.end_hold, MatchDuration::from_secs_f64(1.0));
        assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(0.0));
        assert!(game.end_snapshot.is_some());
        assert_eq!(
            active_hold_progress(game),
            Some(("Ending Match", (1.0 / HOLD_DURATION.as_secs_f64()) as f32))
        );

        // Everything above would also hold with no Restart pressed at all.
        // Letting the Selects up is what proves Restart was down throughout:
        // its hold starts the moment the end gesture stops outranking it.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(first_select);
            keys.release(second_select);
        }
        advance_test_app(&mut app, 0.5);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(
            game.end_hold,
            MatchDuration::from_secs_f64(0.75),
            "the released end hold drains at half rate rather than resetting"
        );
        assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(0.5));
        assert!(
            game.end_snapshot.is_none(),
            "nothing was parked, so the snapshot went with the gesture"
        );
        // The end hold is the larger timer, but Restart is the gesture that
        // is running, and the bar follows the gesture.
        assert_eq!(game.running_hold, Some(HoldGesture::Restart));
        assert_eq!(
            active_hold_progress(game),
            Some((
                "Restarting Match",
                (0.5 / HOLD_DURATION.as_secs_f64()) as f32
            ))
        );
        cleanup_restart_test_root(root);
    }

    /// The counter that survives the flip, and the one `docs/GAME_RULES.md`
    /// names: a modal opened and closed during the hold. Every input held
    /// while a modal is up stays gated until it is released, and a gated
    /// Select cannot feed the end hold, so the hold drops to zero - and a
    /// Restart released and pressed again then runs to completion over the
    /// two Selects that are still physically down.
    #[test]
    fn a_modal_round_trip_ends_a_running_end_hold_and_frees_restart_to_run_over_held_selects() {
        let keyboard = KeyboardBindings::default();
        let restart = keyboard.key(GameAction::RestartMatch).key_code();
        let (mut app, root) = active_match_input_app(keyboard.clone());
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(restart);
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::from_secs_f64(1.0)
        );

        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::blocking();
        advance_test_app(&mut app, 0.1);
        *app.world_mut().resource_mut::<GameplayPause>() = GameplayPause::default();
        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            let gate = app.world().resource::<InputGate>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::ZERO);
            assert!(game.end_snapshot.is_none());
            assert_eq!(game.restart_hold, MatchDuration::ZERO);
            assert!(gate.is_gated(GameAction::PlayerOneSelect));
            assert!(gate.is_gated(GameAction::PlayerTwoSelect));
            assert!(gate.is_gated(GameAction::RestartMatch));
        }

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(restart);
        advance_test_app(&mut app, 0.1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(restart);
        advance_test_app(&mut app, hold_secs(HOLD_DURATION));
        assert_eq!(
            app.world().resource::<GameState>().phase,
            MatchPhase::Opening
        );
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        cleanup_restart_test_root(root);
    }

    /// A restart hold begun in Round 0 with both Selects also down is
    /// cleared on the frame the Match reveals into Round 1: the end gesture
    /// exists from that moment and outranks it, even on the frame the
    /// restart would have completed. The end hold then runs from the reveal.
    #[test]
    fn restart_hold_carried_into_round_one_with_both_selects_held_is_cleared_at_the_reveal() {
        let keyboard = KeyboardBindings::default();
        let (mut app, root) = match_input_app(keyboard.clone(), MatchPhase::OpeningCountdown);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::RestartMatch).key_code());
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 0.5);
        advance_test_app(&mut app, 1.0);
        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::OpeningCountdown);
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(2.5));
            assert_eq!(game.end_hold, MatchDuration::ZERO);
        }

        // This frame crosses the three-second reveal with half a second to
        // spare - enough for the restart to have completed under the old
        // ordering. Only the Round 1 part of the frame credits the end hold.
        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.round, 1);
            assert_eq!(game.restart_hold, MatchDuration::ZERO);
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(0.5));
            assert!(game.end_snapshot.is_some());
        }
        advance_test_app(&mut app, 2.5);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::Manual]);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        cleanup_restart_test_root(root);
    }

    /// The case the priority flip exists for: two players who have agreed
    /// to score cannot have the Match destroyed by a Restart held throughout.
    /// The second half pins a consequence that was weighed and left alone: a
    /// completed end arms no input gate, so the Restart still held after
    /// scoring counts from zero in `Ended` and restarts three seconds later,
    /// by which time the Replay is already saved.
    #[test]
    fn end_hold_completes_and_scores_while_restart_is_held_throughout() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::Space);
            keys.press(KeyCode::KeyM);
            keys.press(KeyCode::Digit2);
        }

        for expected in [1.0, 2.0] {
            advance_test_app(&mut app, 1.0);
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(expected));
            assert_eq!(game.restart_hold, MatchDuration::ZERO);
            assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        }
        advance_test_app(&mut app, 1.0);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::Manual]);
        assert!(game.end_snapshot.is_none());
        assert_eq!(game.restart_hold, MatchDuration::ZERO);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].end_reasons, vec![EndReason::Manual]);
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Game(GameCue::MatchEnd)]
        );

        // Nothing released. The end gesture no longer exists after scoring,
        // so the still-held Restart now runs from zero, and the just-scored
        // Match restarts with its Replay intact.
        for expected in [1.0, 2.0] {
            advance_test_app(&mut app, 1.0);
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Ended);
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(expected));
        }
        advance_test_app(&mut app, 1.0);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.round, 0);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Game(GameCue::MatchRestart)]
        );
        cleanup_restart_test_root(root);
    }

    #[test]
    fn live_actions_record_semantic_replay_frames_through_completion() {
        let mut game = GameState::default();
        begin_active_match(&mut game);
        game.elapsed = MatchInstant::from_secs_f64(0.5);
        move_cursor(&mut game, Player::One, -1, 0);
        game.elapsed = MatchInstant::from_secs_f64(0.75);
        store_teleport_marker(&mut game, Player::One);
        game.elapsed = MatchInstant::from_secs_f64(1.0);
        move_cursor(&mut game, Player::One, 0, -1);
        game.elapsed = MatchInstant::from_secs_f64(1.25);
        jump_to_teleport_marker(&mut game, Player::One);
        game.elapsed = MatchInstant::from_secs_f64(1.5);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.5));
        game.elapsed = MatchInstant::from_secs_f64(1.55);
        try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.55));
        game.elapsed = MatchInstant::from_secs_f64(4.0);

        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();
        let kinds = replay
            .events
            .iter()
            .map(|event| &event.kind)
            .collect::<Vec<_>>();

        assert!(matches!(kinds[0], ReplayEventKind::MatchStart));
        assert!(
            kinds
                .iter()
                .any(|kind| matches!(kind, ReplayEventKind::CursorMoved { .. }))
        );
        assert!(
            kinds
                .iter()
                .any(|kind| matches!(kind, ReplayEventKind::MarkerChanged { .. }))
        );
        assert!(
            kinds
                .iter()
                .any(|kind| matches!(kind, ReplayEventKind::JumpToMarker { .. }))
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| matches!(kind, ReplayEventKind::SelectionChanged { .. }))
                .count(),
            2
        );
        assert!(
            kinds
                .iter()
                .any(|kind| matches!(kind, ReplayEventKind::RoundResolved { .. }))
        );
        assert!(matches!(kinds.last(), Some(ReplayEventKind::MatchEnd)));
        assert_eq!(replay.duration_seconds, 4.0);
        assert_eq!(replay.final_board, game.board);
        assert!(finish_match(&mut game, vec![EndReason::Manual]).is_none());
    }

    #[test]
    fn fresh_match_and_restart_preserve_preferences() {
        let mut game = GameState::default();
        let preferences = GamePreferences {
            show_match_timer: true,
            show_round_number: false,
            ..default()
        };
        let mut gate = InputGate::default();
        initialize_match_state(&mut game, &mut gate, &MatchSettings::default());

        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.round, 0);
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        assert_eq!(match_hud_visibility(&preferences), (true, false));
        assert!(
            GameAction::ALL
                .into_iter()
                .all(|action| gate.is_gated(action))
        );

        begin_active_match(&mut game);
        game.round = 7;
        game.elapsed = MatchInstant::from_secs_f64(42.0);
        game.board.set(Point::new(4, 4), Cell::Gray);
        reset_match(&mut game);
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.round, 0);
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        assert_eq!(game.board, Board::default());
        assert!(game.recorder.is_none());
        assert_eq!(match_hud_visibility(&preferences), (true, false));
    }

    #[test]
    fn restart_snapshots_the_latest_persisted_match_settings() {
        let mut game = GameState::default();
        begin_active_match(&mut game);
        let mut gate = InputGate::default();
        let mut settings = MatchSettings::default();
        settings.set_value(crate::match_settings::MatchSettingField::RoundLimit, 7);
        settings.set_enabled(crate::match_settings::MatchSettingField::RoundLimit, true);

        assert!(advance_restart_hold(
            &mut game,
            &mut gate,
            &settings,
            HOLD_DURATION
        ));

        assert_eq!(
            game.match_rules
                .value(crate::match_settings::MatchSettingField::RoundLimit),
            7
        );
        let mut recorder = game.recorder.clone().unwrap();
        let replay = recorder.complete(2, 0.0, replay_frame(&game)).unwrap();
        assert_eq!(replay.match_settings, settings);
    }

    #[test]
    fn completed_replay_round_count_excludes_pending_and_empty_rounds() {
        let mut empty = GameState::default();
        begin_active_match(&mut empty);
        let empty_replay = finish_match(&mut empty, vec![EndReason::Manual]).unwrap();
        assert_eq!(empty_replay.round_count, 0);

        let mut game = GameState::default();
        begin_active_match(&mut game);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.2));
        move_cursor(&mut game, Player::One, -1, 0);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(2.0));
        assert_eq!(game.round, 2);
        assert!(game.locks[0].is_some());

        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();
        replay.validate().unwrap();
        assert_eq!(replay.round_count, 1);
        assert_eq!(replay.completed_round_count(), 1);
    }

    #[test]
    fn releasing_provisional_round_limit_commits_and_saves_once() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }

        advance_test_app(&mut app, 0.25);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.round, 2);
        assert!(game.pending_replay.is_none());
        assert_eq!(
            game.end_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.provisional_end_reasons.as_deref()),
            Some([EndReason::RoundLimit].as_slice())
        );
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .into_iter()
                .all(|cue| cue != audio::AudioCue::Game(GameCue::MatchEnd))
        );

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.release(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 0.1);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].round_count, 1);
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Game(GameCue::MatchEnd)]
        );
        advance_test_app(&mut app, 0.25);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        cleanup_restart_test_root(root);
    }

    #[test]
    fn provisional_round_limit_freezes_gameplay_clocks_past_wall_time_deadlines() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        for (field, value) in [
            (MatchSettingField::RoundLimit, 1),
            (MatchSettingField::RoundTime, 1),
            (MatchSettingField::MatchTime, 2),
        ] {
            settings.set_value(field, value);
            settings.set_enabled(field, true);
        }
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(first_select);
            keys.press(second_select);
        }

        advance_test_app(&mut app, 0.25);
        let ended_at = app.world().resource::<GameState>().elapsed.as_secs_f64();
        assert_eq!(ended_at, 0.25);
        assert!(
            app.world()
                .resource::<GameState>()
                .end_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
        );

        advance_test_app(&mut app, 2.5);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.elapsed.as_secs_f64(), ended_at);
            assert_eq!(game.round_elapsed.as_secs_f64(), 0.0);
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(2.75));
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(first_select);
            keys.release(second_select);
        }
        advance_test_app(&mut app, 0.5);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.elapsed.as_secs_f64(), ended_at);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        assert!(game.round_elapsed.as_secs_f64() <= 1.0);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        let replay = library.load_full(&library.entries[0].id).unwrap();
        assert_eq!(replay.duration_seconds, ended_at);
        assert_eq!(replay.end_reasons, vec![EndReason::RoundLimit]);
        assert!(
            replay
                .events
                .iter()
                .all(|event| !matches!(event.kind, ReplayEventKind::RoundTimedOut { .. }))
        );
        replay.validate().unwrap();
        cleanup_restart_test_root(root);
    }

    #[test]
    fn provisional_end_during_go_freezes_countdown_until_release() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::OpeningCountdown,
            settings,
        );
        advance_test_app(&mut app, 3.0);
        assert_eq!(
            super::super::countdown::countdown_text(app.world().resource::<GameState>()),
            Some("Go!")
        );
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(first_select);
            keys.press(second_select);
        }
        advance_test_app(&mut app, 0.1);
        assert!(
            app.world()
                .resource::<GameState>()
                .provisional_end_pending()
        );
        let ended_at = app.world().resource::<GameState>().elapsed.as_secs_f64();
        let countdown_at_end = app.world().resource::<GameState>().countdown_elapsed;

        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.elapsed.as_secs_f64(), ended_at);
            assert_eq!(game.countdown_elapsed, countdown_at_end);
            assert_eq!(super::super::countdown::countdown_text(game), Some("Go!"));
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(first_select);
            keys.release(second_select);
        }
        advance_test_app(&mut app, 0.1);

        assert_eq!(
            app.world().resource::<GameState>().end_reasons,
            vec![EndReason::RoundLimit]
        );
        let library = app.world().resource::<ReplayLibrary>();
        let replay = library.load_full(&library.entries[0].id).unwrap();
        assert_eq!(replay.duration_seconds, ended_at);
        assert!(
            replay
                .events
                .iter()
                .all(|event| !matches!(event.kind, ReplayEventKind::OpeningCountdownFinished))
        );
        replay.validate().unwrap();
        cleanup_restart_test_root(root);
    }

    #[test]
    fn completed_hold_rolls_back_provisional_capture_target_and_ends_manual() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        settings.set_capture_points(true);
        settings.set_value(MatchSettingField::PointTarget, 5);
        settings.set_enabled(MatchSettingField::PointTarget, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        let baseline = {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.board.set(Point::new(1, 1), Cell::White);
            for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
                game.board.set(point, Cell::Black);
            }
            game.cursors = [Point::new(1, 2), Point::new(8, 8)];
            game.completed_history = std::collections::HashSet::from([game.board.clone()]);
            game.elapsed = MatchInstant::from_secs_f64(0.4);
            game.round_elapsed = MatchDuration::from_secs_f64(0.4);
            game.board.clone()
        };
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }

        advance_test_app(&mut app, 1.0);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.capture_totals, Score { black: 1, white: 0 });
        assert_eq!(
            game.end_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.provisional_end_reasons.as_deref()),
            Some([EndReason::PointTarget].as_slice())
        );
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .into_iter()
                .all(|cue| cue != audio::AudioCue::Game(GameCue::MatchEnd))
        );

        advance_test_app(&mut app, 2.0);

        let game = app.world().resource::<GameState>();
        let mut scored_baseline = baseline.clone();
        scored_baseline.fill_territory();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::Manual]);
        assert_eq!(game.board, scored_baseline);
        assert_eq!(game.capture_totals, Score::default());
        assert_eq!(
            game.last_removals,
            crate::authority::RoundRemovals::default(),
            "the rolled-back round's capture is no longer the last resolution's"
        );
        assert_eq!(game.round, 1);
        assert!((game.round_elapsed.as_secs_f64() - 1.4).abs() < 0.000_001);
        assert_eq!(
            game.completed_history,
            std::collections::HashSet::from([baseline])
        );
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].round_count, 0);
        assert_eq!(library.entries[0].end_reasons, vec![EndReason::Manual]);
        let saved = library.load_full(&library.entries[0].id).unwrap();
        assert!(saved.events.iter().all(|event| !matches!(
            event.kind,
            ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
        )));
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Game(GameCue::MatchEnd)]
        );
        advance_test_app(&mut app, 0.25);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        cleanup_restart_test_root(root);
    }

    #[test]
    fn completed_restart_abandons_a_provisional_automatic_end_without_saving() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 0.25);
        assert!(
            app.world()
                .resource::<GameState>()
                .end_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
        );

        // The end gesture outranks Restart, so the Restart hold can only
        // begin once both Selects are up. What keeps the deferred end alive
        // is that Restart is already down when they come up - pressed in
        // that same frame, as here, or earlier, as
        // `restart_already_held_when_the_selects_come_up_keeps_a_deferred_end_alive`
        // shows - so no frame passes with neither gesture held, which is
        // what would commit it.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::RestartMatch).key_code());
            keys.release(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.release(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::ZERO);
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(1.0));
            assert!(
                game.end_snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
            );
            assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        }
        for _ in 0..2 {
            advance_test_app(&mut app, 1.0);
        }

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.round, 0);
        assert!(game.end_snapshot.is_none());
        assert!(game.end_reasons.is_empty());
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        cleanup_restart_test_root(root);
    }

    #[test]
    fn cancelled_restart_commits_a_deferred_automatic_end_once() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        let restart = keyboard.key(GameAction::RestartMatch).key_code();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(first_select);
            keys.press(second_select);
        }
        advance_test_app(&mut app, 0.25);
        assert!(
            app.world()
                .resource::<GameState>()
                .end_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
        );

        // With ending outranking restarting, a Restart pressed while both
        // Selects are still down does not start, so with the Selects held
        // there is nothing to cancel. Releasing them in the same frame as
        // the press lets the Restart hold run alone with the end still
        // parked; a frame earlier would have committed it, and a frame later
        // is the sequence
        // `restart_already_held_when_the_selects_come_up_keeps_a_deferred_end_alive`
        // pins.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(restart);
            keys.release(first_select);
            keys.release(second_select);
        }
        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::ZERO);
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(1.0));
            assert!(
                game.end_snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
            );
            assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(restart);
        advance_test_app(&mut app, 0.1);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        assert!(game.end_snapshot.is_none());
        assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(0.0));
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].end_reasons, vec![EndReason::RoundLimit]);
        let saved = library.load_full(&library.entries[0].id).unwrap();
        assert_eq!(
            saved
                .events
                .iter()
                .filter(|event| matches!(event.kind, ReplayEventKind::MatchEnd))
                .count(),
            1
        );
        advance_test_app(&mut app, 0.25);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        cleanup_restart_test_root(root);
    }

    /// Replaces a test in which a held Restart interrupted the end hold and
    /// releasing it resumed the hold from zero. Under end-first priority the
    /// hold is never interrupted, so there is nothing to resume; what matters
    /// now is that a Restart pressed and released mid-hold changes nothing -
    /// not the timer, and not the automatic end already deferred to it.
    #[test]
    fn restart_pressed_mid_end_hold_neither_pauses_it_nor_disturbs_its_deferred_end() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 0.25);
        assert!(
            app.world()
                .resource::<GameState>()
                .end_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(keyboard.key(GameAction::RestartMatch).key_code());
        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(1.25));
            assert_eq!(game.restart_hold, MatchDuration::ZERO);
            assert_eq!(
                active_hold_progress(game),
                Some(("Ending Match", (1.25 / HOLD_DURATION.as_secs_f64()) as f32))
            );
        }
        assert!(
            restart_held(&app),
            "Restart really is down; it changes nothing because the end gesture outranks it"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(keyboard.key(GameAction::RestartMatch).key_code());
        advance_test_app(&mut app, 0.5);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.end_hold, MatchDuration::from_secs_f64(1.75));
        assert_eq!(game.restart_hold, MatchDuration::ZERO);
        assert!(
            game.end_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
        );
        assert!(game.end_reasons.is_empty());
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());

        // Restart down again, then everything let go in one frame: the end
        // branch hands straight to the release branch, which commits the
        // deferred end exactly once. The test this one replaced pinned that
        // sequence, so it is kept here rather than lost with it.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(keyboard.key(GameAction::RestartMatch).key_code());
        advance_test_app(&mut app, 0.25);
        assert!(restart_held(&app));
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::from_secs_f64(2.0),
            "the end branch is still the one running"
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(keyboard.key(GameAction::RestartMatch).key_code());
            keys.release(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.release(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        assert!(game.end_snapshot.is_none());
        assert_eq!(game.restart_hold, MatchDuration::ZERO);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].end_reasons, vec![EndReason::RoundLimit]);
        advance_test_app(&mut app, 0.25);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        cleanup_restart_test_root(root);
    }

    /// The natural human order: Restart goes down while both Selects are
    /// still held, and the Selects come up a moment later. The first frame
    /// runs the end branch, which keeps the snapshot and ignores Restart;
    /// on the next, the Restart branch runs, and the snapshot survives only
    /// because an end is parked in it - `end_gesture_not_running` drops an
    /// unparked one before any branch is reached. The deferred end therefore
    /// survives the handover between the two branches and is still committed
    /// exactly once when Restart is released.
    #[test]
    fn restart_already_held_when_the_selects_come_up_keeps_a_deferred_end_alive() {
        let keyboard = KeyboardBindings::default();
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            keyboard.clone(),
            MatchPhase::Playing,
            settings,
        );
        let first_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let second_select = keyboard.key(GameAction::PlayerTwoSelect).key_code();
        let restart = keyboard.key(GameAction::RestartMatch).key_code();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(first_select);
            keys.press(second_select);
        }
        advance_test_app(&mut app, 0.25);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(restart);
        advance_test_app(&mut app, 0.25);
        assert!(
            restart_held(&app),
            "Restart is down before the Selects come up, not in the same frame"
        );
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(0.5));
            assert_eq!(game.restart_hold, MatchDuration::ZERO);
            assert!(
                game.end_snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
            );
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(first_select);
            keys.release(second_select);
        }
        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::ZERO);
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(1.0));
            assert!(
                game.end_snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
            );
            assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(restart);
        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        assert!(game.end_snapshot.is_none());
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        advance_test_app(&mut app, 0.25);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        cleanup_restart_test_root(root);
    }

    #[test]
    fn match_end_and_restart_queue_once_at_completed_transitions() {
        let keyboard = KeyboardBindings::default();
        let (mut end_app, end_root) = active_match_input_app(keyboard.clone());
        {
            let mut keys = end_app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(keyboard.key(GameAction::PlayerOneSelect).key_code());
            keys.press(keyboard.key(GameAction::PlayerTwoSelect).key_code());
        }
        for _ in 0..3 {
            advance_test_app(&mut end_app, 1.0);
        }
        assert_eq!(
            end_app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Game(GameCue::MatchEnd)]
        );
        assert_eq!(
            end_app.world().resource::<ReplayLibrary>().entries[0].end_reasons,
            vec![EndReason::Manual]
        );
        advance_test_app(&mut end_app, 1.0);
        assert!(
            end_app
                .world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        cleanup_restart_test_root(end_root);

        let (mut restart_app, restart_root) = active_match_input_app(keyboard);
        restart_app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        for _ in 0..3 {
            advance_test_app(&mut restart_app, 1.0);
        }
        assert_eq!(
            restart_app
                .world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain(),
            vec![audio::AudioCue::Game(GameCue::MatchRestart)]
        );
        restart_app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        advance_test_app(&mut restart_app, 0.49);
        assert!(
            restart_app
                .world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        advance_test_app(&mut restart_app, 0.01);
        assert_eq!(
            restart_app.world().resource::<GameState>().phase,
            MatchPhase::Opening
        );
        assert!(
            restart_app
                .world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        cleanup_restart_test_root(restart_root);
    }

    fn hold_fraction_of(fraction: f64) -> MatchDuration {
        MatchDuration::from_secs_f64(HOLD_DURATION.as_secs_f64() * fraction)
    }

    /// The decision table behind the progress bar, driven directly rather
    /// than through the input pipeline. The running gesture is reported
    /// first; with nothing running, a draining end hold is reported during
    /// `Playing` and nothing else is.
    #[test]
    fn center_out_hold_progress_clamps_and_uses_exact_labels() {
        let mut game = GameState::default();
        begin_active_match(&mut game);
        assert_eq!(active_hold_progress(&game), None);

        game.running_hold = Some(HoldGesture::End);
        game.end_hold = hold_fraction_of(0.25);
        assert_eq!(active_hold_progress(&game), Some(("Ending Match", 0.25)));
        game.end_hold = hold_fraction_of(0.5);
        assert_eq!(active_hold_progress(&game), Some(("Ending Match", 0.5)));
        game.end_hold = HOLD_DURATION;
        assert_eq!(active_hold_progress(&game), Some(("Ending Match", 1.0)));
        game.end_hold = hold_fraction_of(2.0);
        assert_eq!(active_hold_progress(&game), Some(("Ending Match", 1.0)));

        // MatchDuration can never go negative, unlike the old f32 field, so the
        // clamp-at-zero behavior this test used to exercise via a negative
        // hold is now a structural guarantee of the type itself.
        game.end_hold = MatchDuration::ZERO;
        game.running_hold = Some(HoldGesture::Restart);
        game.restart_hold = hold_fraction_of(0.5);
        assert_eq!(active_hold_progress(&game), Some(("Restarting Match", 0.5)));
        // Both non-zero with Restart running: the end hold is draining, not
        // running, and the bar names the gesture that is going to fire - a
        // non-zero timer used to be enough, and since the drain it is not.
        game.end_hold = HOLD_DURATION;
        assert_eq!(active_hold_progress(&game), Some(("Restarting Match", 0.5)));
        game.running_hold = Some(HoldGesture::End);
        assert_eq!(active_hold_progress(&game), Some(("Ending Match", 1.0)));

        // Nothing running: a draining end hold is still shown, so the player
        // sees progress leaving while they can still act on it...
        game.running_hold = None;
        game.end_hold = hold_fraction_of(0.5);
        assert_eq!(active_hold_progress(&game), Some(("Ending Match", 0.5)));
        // ...a restart hold frozen behind a modal is not, since the modal
        // covers the bar...
        game.end_hold = MatchDuration::ZERO;
        assert_eq!(active_hold_progress(&game), None);
        // ...and once the Match has left `Playing`, nothing can be resumed,
        // so a not-yet-zeroed end hold is not shown either.
        game.end_hold = hold_fraction_of(0.5);
        game.phase = MatchPhase::Ended;
        assert_eq!(active_hold_progress(&game), None);
        game.restart_hold = MatchDuration::ZERO;
        game.end_hold = MatchDuration::ZERO;
        assert_eq!(active_hold_progress(&game), None);
    }

    /// Each action's key under the default bindings, so a sequence names the
    /// action it means rather than a key code that might belong to another -
    /// the mistake that once left an end-gesture test pressing a Teleport
    /// Marker key instead of a Select.
    fn default_key(action: GameAction) -> KeyCode {
        KeyboardBindings::default().key(action).key_code()
    }

    fn press(app: &mut App, actions: &[GameAction]) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        for action in actions {
            keys.press(default_key(*action));
        }
    }

    fn release(app: &mut App, actions: &[GameAction]) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        for action in actions {
            keys.release(default_key(*action));
        }
    }

    /// Whether Restart is held and ungated this frame - what the hold
    /// gestures themselves ask. Tests in which a held Restart is meant to
    /// change nothing check this, because "held and ignored" and "never
    /// pressed" otherwise look the same.
    fn restart_held(app: &App) -> bool {
        action_pressed(
            app.world().resource::<SemanticInput>(),
            app.world().resource::<InputGate>(),
            GameAction::RestartMatch,
        )
    }

    const BOTH_SELECTS: [GameAction; 2] =
        [GameAction::PlayerOneSelect, GameAction::PlayerTwoSelect];

    /// One full ordinary round with no end gesture in it: both cursors step
    /// right, off the stones their players placed last round, and then each
    /// player selects in turn, so the two Selects are never down together.
    /// Each release shares a frame with the next press.
    fn play_a_round_one_select_at_a_time(app: &mut App) {
        let steps = [
            GameAction::PlayerOneRight,
            GameAction::PlayerTwoRight,
            GameAction::PlayerOneSelect,
            GameAction::PlayerTwoSelect,
        ];
        for (index, action) in steps.into_iter().enumerate() {
            if index > 0 {
                release(app, &[steps[index - 1]]);
            }
            press(app, &[action]);
            advance_test_app(app, 0.1);
        }
        release(app, &[GameAction::PlayerTwoSelect]);
        advance_test_app(app, 0.1);
    }

    fn resolved_rounds(replay: &crate::replay::Replay) -> usize {
        replay
            .events
            .iter()
            .filter(|event| matches!(event.kind, ReplayEventKind::RoundResolved { .. }))
            .count()
    }

    /// Recovered from the review that found the stale-snapshot rollback,
    /// with its assertions inverted by the fix. A short tap of both Selects
    /// captures a snapshot and resolves Round 1 as an ordinary round; Restart
    /// goes down as the Selects come up, and a whole round is played while it
    /// is held. Restart used to keep that snapshot alive, so pressing both
    /// Selects again - Restart still held, inside its three seconds - ran an
    /// end hold that restored it and erased the round played in between from
    /// the score and from the saved Replay. The snapshot is now dropped on
    /// the first frame the end gesture is not running, so the second hold
    /// captures its own and rolls back nothing that was really played.
    #[test]
    fn a_round_played_while_restart_is_held_survives_a_later_end_hold() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        assert_eq!(
            app.world().resource::<GameState>().round,
            2,
            "the incidental tap resolved Round 1"
        );

        press(&mut app, &[GameAction::RestartMatch]);
        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.end_hold, MatchDuration::ZERO);
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(0.5));
            assert!(
                game.end_snapshot.is_none(),
                "with nothing parked in it, the snapshot does not outlive its gesture"
            );
        }

        play_a_round_one_select_at_a_time(&mut app);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(
                game.round, 3,
                "a real round was played while Restart was held"
            );
            assert!(game.end_snapshot.is_none());
            assert!(game.restart_hold < HOLD_DURATION);
        }

        // Both Selects again with Restart still held: the end branch, which
        // now captures a fresh snapshot at Round 3.
        press(&mut app, &BOTH_SELECTS);
        for _ in 0..3 {
            advance_test_app(&mut app, 1.0);
        }
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::Manual]);
        assert_eq!(game.round, 3, "both played rounds are kept");
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].round_count, 2);
        let saved = library.load_full(&library.entries[0].id).unwrap();
        assert_eq!(resolved_rounds(&saved), 2);
        saved.validate().unwrap();
        cleanup_restart_test_root(root);
    }

    /// The same sequence, with Restart released in the frame the Selects go
    /// down again. Recovered alongside the test above: under the old
    /// ordering, where Restart outranked the end gesture, this was the only
    /// way into the end branch with the stale snapshot still held, which is
    /// how the review showed the rollback predates the priority flip. Kept so
    /// that both routes stay closed.
    #[test]
    fn a_round_played_while_restart_is_held_survives_an_end_hold_begun_as_restart_is_released() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        press(&mut app, &[GameAction::RestartMatch]);
        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        play_a_round_one_select_at_a_time(&mut app);
        assert_eq!(app.world().resource::<GameState>().round, 3);
        assert!(app.world().resource::<GameState>().end_snapshot.is_none());

        press(&mut app, &BOTH_SELECTS);
        release(&mut app, &[GameAction::RestartMatch]);
        for _ in 0..3 {
            advance_test_app(&mut app, 1.0);
        }
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::Manual]);
        assert_eq!(game.round, 3);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries[0].round_count, 2);
        assert_eq!(
            resolved_rounds(&library.load_full(&library.entries[0].id).unwrap()),
            2
        );
        cleanup_restart_test_root(root);
    }

    /// Holding Restart alone defers nothing. The end is parked only while an
    /// end gesture could still roll back the round that reached it; with no
    /// gesture running there is nothing to wait for, so a limit crossed with
    /// Restart held ends the Match at once, exactly as it would with Restart
    /// up. It used to be parked in the snapshot a previous tap had left
    /// behind, stopping both clocks with the Match unended until Restart
    /// resolved it one way or the other.
    #[test]
    fn a_limit_crossed_while_restart_alone_is_held_ends_the_match_at_once() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 2);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        // A tap of both Selects resolves Round 1 without reaching the limit,
        // then Restart goes down as they come up - the sequence that used to
        // leave a snapshot behind for the next round to park into.
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        press(&mut app, &[GameAction::RestartMatch]);
        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.round, 2);
            assert!(game.end_reasons.is_empty());
        }

        play_a_round_one_select_at_a_time(&mut app);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        assert!(!game.provisional_end_pending());
        assert!(game.restart_hold < HOLD_DURATION);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].round_count, 2);
        assert_eq!(library.entries[0].end_reasons, vec![EndReason::RoundLimit]);
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .contains(&audio::AudioCue::Game(GameCue::MatchEnd))
        );
        cleanup_restart_test_root(root);
    }

    /// The deadlock guard. A parked automatic end stops play, but it must
    /// never stop the hold gestures, because they are the only thing that
    /// settles it: releasing commits it, completing the end hold rolls it
    /// back, and a completed Restart drops it. Freezing them as well - by
    /// adding the parked end to `handle_hold_gestures`' early return, or by
    /// setting `input_frozen_this_frame`, which that early return also reads -
    /// would leave the end parked for good, with both clocks stopped and no
    /// input able to reach it, and the Match would never end. So this holds
    /// both Selects until an end parks, keeps holding well past it, and then
    /// requires the release to end the Match.
    #[test]
    fn releasing_the_selects_ends_a_match_however_long_its_automatic_end_was_parked() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        assert!(
            app.world()
                .resource::<GameState>()
                .provisional_end_pending()
        );
        let parked_at = app.world().resource::<GameState>().elapsed;

        for _ in 0..5 {
            advance_test_app(&mut app, 0.5);
        }
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert!(game.provisional_end_pending());
            assert_eq!(game.elapsed, parked_at);
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(2.75));
        }

        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        assert!(game.end_snapshot.is_none());
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        cleanup_restart_test_root(root);
    }

    /// A parked automatic end stops play, not only the clocks: whether the
    /// Match has already ended is undecided, so a move made now could belong
    /// to a Match that is over. Restart held alone is the state this matters
    /// in - the one in which an end waits with no end hold running and both
    /// players' hands free - so every ordinary action is tried there, one
    /// player at a time so the end gesture never resumes, and none may
    /// change the Match or reach its Replay. The cursors step onto empty
    /// points before the Selects, so a whole round is on offer: without the
    /// freeze it would resolve, which `complete_ordinary_round` asserts
    /// cannot happen while an end is parked. Releasing Restart then commits
    /// the end as though nothing had been pressed.
    #[test]
    fn a_parked_automatic_end_ignores_ordinary_play_until_it_is_settled() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        press(&mut app, &[GameAction::RestartMatch]);
        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        let (cursors, markers, events) = {
            let game = app.world().resource::<GameState>();
            assert!(game.provisional_end_pending());
            assert_eq!(game.round, 2);
            // The clock is frozen behind the parked end, so the whole frame
            // is credited to the gestures - and the released end hold drains
            // by half of it.
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(0.125));
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(0.25));
            (
                game.cursors,
                game.teleport_markers,
                game.recorder.as_ref().unwrap().events().len(),
            )
        };

        for action in [
            GameAction::PlayerOneRight,
            GameAction::PlayerTwoDown,
            GameAction::PlayerOneSetMarker,
            GameAction::PlayerTwoSetMarker,
            GameAction::PlayerOneJumpToMarker,
            GameAction::PlayerTwoJumpToMarker,
            GameAction::PlayerOneSelect,
            GameAction::PlayerTwoSelect,
        ] {
            press(&mut app, &[action]);
            advance_test_app(&mut app, 0.1);
            release(&mut app, &[action]);
            advance_test_app(&mut app, 0.1);
        }
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert!(game.provisional_end_pending());
            assert!(game.restart_hold < HOLD_DURATION);
            assert_eq!(
                game.end_hold,
                MatchDuration::ZERO,
                "1.6 s of lapse drains the 0.125 s that was left"
            );
            assert_eq!(game.round, 2);
            assert_eq!(game.cursors, cursors);
            assert_eq!(game.teleport_markers, markers);
            assert_eq!(game.locks, [None, None]);
            assert_eq!(game.recorder.as_ref().unwrap().events().len(), events);
        }

        release(&mut app, &[GameAction::RestartMatch]);
        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        let saved = library.load_full(&library.entries[0].id).unwrap();
        let resolved_at = saved
            .events
            .iter()
            .position(|event| matches!(event.kind, ReplayEventKind::RoundResolved { .. }))
            .unwrap();
        assert!(saved.events[resolved_at..].iter().all(|event| !matches!(
            event.kind,
            ReplayEventKind::CursorMoved { .. }
                | ReplayEventKind::MarkerChanged { .. }
                | ReplayEventKind::SelectionChanged { .. }
        )));
        saved.validate().unwrap();
        cleanup_restart_test_root(root);
    }

    /// The freeze holds within a frame as well as across frames. Player Two
    /// has already selected; both players then press Select together, Player
    /// Two stepping to an empty point in the same frame. Player One's Select
    /// resolves Round 1, which reaches the Round Limit and parks it, and
    /// Player Two's must then be ignored rather than select for Round 2 - as
    /// it already is when an end is immediate, because the Match has `Ended`
    /// by the time the second Select is read. Without that, the saved Replay
    /// carried a Round 2 selection recorded after the round that ended the
    /// Match.
    #[test]
    fn a_select_in_the_frame_that_parks_an_automatic_end_does_not_select_for_the_next_round() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        press(&mut app, &[GameAction::PlayerTwoSelect]);
        advance_test_app(&mut app, 0.1);
        release(&mut app, &[GameAction::PlayerTwoSelect]);
        advance_test_app(&mut app, 0.1);
        assert!(app.world().resource::<GameState>().locks[player_index(Player::Two)].is_some());

        press(
            &mut app,
            &[
                GameAction::PlayerOneSelect,
                GameAction::PlayerTwoSelect,
                GameAction::PlayerTwoRight,
            ],
        );
        advance_test_app(&mut app, 0.1);
        {
            let game = app.world().resource::<GameState>();
            assert!(game.provisional_end_pending());
            assert_eq!(game.round, 2);
            assert_eq!(game.locks, [None, None]);
        }

        release(
            &mut app,
            &[
                GameAction::PlayerOneSelect,
                GameAction::PlayerTwoSelect,
                GameAction::PlayerTwoRight,
            ],
        );
        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
        let library = app.world().resource::<ReplayLibrary>();
        let saved = library.load_full(&library.entries[0].id).unwrap();
        let resolved_at = saved
            .events
            .iter()
            .position(|event| matches!(event.kind, ReplayEventKind::RoundResolved { .. }))
            .unwrap();
        assert!(
            saved.events[resolved_at..]
                .iter()
                .all(|event| !matches!(event.kind, ReplayEventKind::SelectionChanged { .. }))
        );
        saved.validate().unwrap();
        cleanup_restart_test_root(root);
    }

    /// Opens or closes a modal the way `ui::hud::apply_hud_action` does:
    /// the pause flags change together and the gate is armed, so every input
    /// held across the change is suppressed until it is released.
    fn set_modal_open(app: &mut App, open: bool) {
        *app.world_mut().resource_mut::<GameplayPause>() = if open {
            GameplayPause::blocking()
        } else {
            GameplayPause::default()
        };
        app.world_mut().resource_mut::<InputGate>().arm();
    }

    /// A release on the frame a Round Time timeout freezes input still ends
    /// the end gesture. The clock resolves that round before the holds run,
    /// and the holds then skip the rest of the frame - which used to include
    /// noticing that both Selects had come up. The snapshot survived, and
    /// pressing both Selects again on the next frame resumed the old hold
    /// with its timer still running and, on completing, rolled back the round
    /// that release should have committed as well as the one that timed out.
    /// Since the end hold began to drain, the timer legitimately carries what
    /// the drain has left of it into the next hold; what has to be fresh is
    /// the snapshot.
    #[test]
    fn a_release_on_the_frame_a_round_times_out_still_ends_the_end_gesture() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundTime, 1);
        settings.set_enabled(MatchSettingField::RoundTime, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        // The tap resolves Round 1 at 0.25 s, so Round 2 runs out at 1.25 s.
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        advance_test_app(&mut app, 0.75);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.round, 2);
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(1.0));
        }

        // Both Selects come up in the frame that crosses 1.25 s.
        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        {
            let game = app.world().resource::<GameState>();
            assert!(
                game.input_frozen_this_frame,
                "the timeout froze this frame's input, so the holds did not act on it"
            );
            assert_eq!(game.round, 3);
            assert_eq!(
                game.end_hold,
                MatchDuration::from_secs_f64(1.0),
                "the release was seen on the frozen frame - the snapshot is gone - but the \
                 clock owns that frame, so neither hold moved and nothing drained"
            );
            assert!(game.end_snapshot.is_none());
        }

        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.125);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(
                game.end_hold,
                MatchDuration::from_secs_f64(1.125),
                "the timer resumes from what was left; the snapshot is what must be fresh"
            );
            assert_eq!(game.end_snapshot.as_ref().unwrap().progress.round, 3);
        }
        for _ in 0..40 {
            if app.world().resource::<GameState>().phase == MatchPhase::Ended {
                break;
            }
            advance_test_app(&mut app, 0.25);
        }
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::Manual]);
        assert_eq!(game.round, 3, "the rounds before the second hold stand");
        let library = app.world().resource::<ReplayLibrary>();
        let saved = library.load_full(&library.entries[0].id).unwrap();
        assert_eq!(resolved_rounds(&saved), 1);
        saved.validate().unwrap();
        cleanup_restart_test_root(root);
    }

    /// A modal ends a running end gesture as it opens, snapshot and all,
    /// because the gate suppresses the Selects held into it. It used to pause
    /// the gesture instead, keeping the snapshot for as long as the modal was
    /// up - and the clock, which runs before the holds, then found that
    /// snapshot on the frame the modal closed. A Round Time timeout on that
    /// frame parked the automatic end it reached into a snapshot no gesture
    /// owned, and a Restart pressed on the next frame discarded it unsaved.
    #[test]
    fn a_limit_reached_as_a_modal_closes_ends_the_match_instead_of_parking() {
        let mut settings = MatchSettings::default();
        for (field, value) in [
            (MatchSettingField::RoundTime, 1),
            (MatchSettingField::RoundLimit, 2),
        ] {
            settings.set_value(field, value);
            settings.set_enabled(field, true);
        }
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        // The tap resolves Round 1 at 0.25 s without reaching the limit, and
        // Round 2 runs out at 1.25 s of Match time.
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        advance_test_app(&mut app, 0.75);
        assert!(app.world().resource::<GameState>().end_snapshot.is_some());

        set_modal_open(&mut app, true);
        advance_test_app(&mut app, 0.25);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(
                game.elapsed.as_secs_f64(),
                1.0,
                "the modal pauses the Match"
            );
            assert_eq!(game.end_hold, MatchDuration::ZERO);
            assert!(
                game.end_snapshot.is_none(),
                "the gated Selects ended the gesture as the modal opened"
            );
        }
        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);

        set_modal_open(&mut app, false);
        advance_test_app(&mut app, 0.5);
        {
            let game = app.world().resource::<GameState>();
            assert!(
                game.input_frozen_this_frame,
                "Round 2 timed out on this frame"
            );
            assert_eq!(game.phase, MatchPhase::Ended);
            assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
            assert!(!game.provisional_end_pending());
        }
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);

        // Restart pressed on the very next frame now restarts a scored Match.
        press(&mut app, &[GameAction::RestartMatch]);
        for _ in 0..3 {
            advance_test_app(&mut app, 1.0);
        }
        assert_eq!(
            app.world().resource::<GameState>().phase,
            MatchPhase::Opening
        );
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 1);
        cleanup_restart_test_root(root);
    }

    /// Whether `action` is held and ungated this frame - what the hold
    /// gestures themselves ask. See [`restart_held`].
    fn held(app: &App, action: GameAction) -> bool {
        action_pressed(
            app.world().resource::<SemanticInput>(),
            app.world().resource::<InputGate>(),
            action,
        )
    }

    /// The case the drain exists for. Player One holds steadily; Player Two's
    /// Select makes contact half the time, half a second down and half a
    /// second up - a tremor, a weak grip or a flaky key. While the hold reset
    /// on a single unheld frame it could never pass half a second here and
    /// the Match could never be scored. Draining at half rate, each cycle
    /// nets a quarter second, and the eleventh contact completes the hold.
    /// The first contact is also a pair of Select presses and resolves Round
    /// 1, as any tap of both Selects does; after that only Player Two's
    /// Select is re-pressed, and a lock is submitted on the press edge, so
    /// Player One's was spent on that first round and no further round
    /// resolves on the way.
    #[test]
    fn an_end_hold_interrupted_repeatedly_completes_where_a_reset_never_would() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &[GameAction::PlayerOneSelect]);
        advance_test_app(&mut app, 0.1);

        let mut expected = 0.0;
        for contact in 1..=10 {
            press(&mut app, &[GameAction::PlayerTwoSelect]);
            advance_test_app(&mut app, 0.5);
            expected += 0.5;
            {
                let game = app.world().resource::<GameState>();
                assert_eq!(game.phase, MatchPhase::Playing, "contact {contact}");
                assert_eq!(game.running_hold, Some(HoldGesture::End));
                assert_eq!(
                    game.end_hold,
                    MatchDuration::from_secs_f64(expected),
                    "contact {contact}: the hold resumes from what the drain left"
                );
                assert!(game.end_snapshot.is_some());
            }

            release(&mut app, &[GameAction::PlayerTwoSelect]);
            advance_test_app(&mut app, 0.5);
            expected -= 0.25;
            {
                assert!(
                    held(&app, GameAction::PlayerOneSelect)
                        && !held(&app, GameAction::PlayerTwoSelect),
                    "lapse {contact}: Player Two's Select really is up"
                );
                let game = app.world().resource::<GameState>();
                assert_eq!(game.phase, MatchPhase::Playing, "lapse {contact}");
                assert_eq!(game.running_hold, None);
                assert_eq!(
                    game.end_hold,
                    MatchDuration::from_secs_f64(expected),
                    "lapse {contact}: half a second up drains a quarter"
                );
                assert!(
                    game.end_snapshot.is_none(),
                    "the snapshot goes with the gesture, drain or no drain"
                );
                assert_eq!(
                    active_hold_progress(game),
                    Some(("Ending Match", hold_fraction(game.end_hold))),
                    "the draining bar is shown, so the player sees progress leaving"
                );
            }
        }
        assert_eq!(expected, 2.5);
        assert_eq!(
            app.world().resource::<GameState>().round,
            2,
            "the first contact resolved Round 1; the flicker resolved nothing more"
        );
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());

        press(&mut app, &[GameAction::PlayerTwoSelect]);
        advance_test_app(&mut app, 0.5);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(game.end_reasons, vec![EndReason::Manual]);
        let library = app.world().resource::<ReplayLibrary>();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].end_reasons, vec![EndReason::Manual]);
        cleanup_restart_test_root(root);
    }

    /// The bar lie the drain makes possible, and the rule that prevents it.
    /// Both Selects come up with Restart going down, so the end hold drains
    /// while the restart hold runs beneath it. The end hold is the larger
    /// timer for most of a second, but Restart is the gesture that is going
    /// to fire, and the bar must say so throughout - until the restart
    /// completes with the end hold long since drained.
    #[test]
    fn an_end_hold_draining_while_restart_runs_reports_the_restart() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            active_hold_progress(app.world().resource::<GameState>()),
            Some(("Ending Match", (1.0 / HOLD_DURATION.as_secs_f64()) as f32))
        );

        press(&mut app, &[GameAction::RestartMatch]);
        release(&mut app, &BOTH_SELECTS);
        let restarting = |restart_secs: f64| {
            Some((
                "Restarting Match",
                (restart_secs / HOLD_DURATION.as_secs_f64()) as f32,
            ))
        };
        for (lapse, end, restart) in [(0.5, 0.75, 0.5), (1.0, 0.25, 1.5), (1.0, 0.0, 2.5)] {
            advance_test_app(&mut app, lapse);
            assert!(restart_held(&app), "Restart really is down");
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(end));
            assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(restart));
            assert_eq!(game.running_hold, Some(HoldGesture::Restart));
            assert_eq!(active_hold_progress(game), restarting(restart));
            assert!(game.end_snapshot.is_none());
        }

        advance_test_app(&mut app, 0.5);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.round, 0);
        // A completed restart resets the whole `GameState`, this field
        // included, so the frame it completes on reports no gesture.
        assert_eq!(game.running_hold, None);
        assert_eq!(active_hold_progress(game), None);
        assert!(app.world().resource::<ReplayLibrary>().entries.is_empty());
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Game(GameCue::MatchRestart)]
        );
        cleanup_restart_test_root(root);
    }

    /// The decision behind `EndGestureLapse::Interrupted`, at its edge: a
    /// modal clears a draining end hold even when nobody is holding a Select
    /// at all. The owner's rule was stated for a running gesture - a modal
    /// interrupts it, and a player reads it so - and this extends it to the
    /// drain, where the alternative would have been a timer left frozen
    /// through the modal (a modal pauses the clock, so a release lapse would
    /// have drained nothing) and resumed afterwards. One rule, "a modal
    /// clears the end hold", is easier to hold in mind than two, and the
    /// owner confirmed the extension on 2026-10-06. This test makes
    /// reversing it a visible edit.
    #[test]
    fn a_modal_clears_a_draining_end_hold_even_with_nobody_holding() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 1.0);
        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        {
            let semantic = app.world().resource::<SemanticInput>();
            assert!(
                !semantic.pressed(GameAction::PlayerOneSelect)
                    && !semantic.pressed(GameAction::PlayerTwoSelect),
                "nobody is holding anything"
            );
            assert_eq!(
                app.world().resource::<GameState>().end_hold,
                MatchDuration::from_secs_f64(0.75),
                "draining when the modal opens"
            );
        }

        set_modal_open(&mut app, true);
        advance_test_app(&mut app, 0.1);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::ZERO,
            "cleared, not left at 0.75"
        );

        set_modal_open(&mut app, false);
        advance_test_app(&mut app, 0.5);
        {
            let gate = app.world().resource::<InputGate>();
            assert!(
                !gate.is_gated(GameAction::PlayerOneSelect)
                    && !gate.is_gated(GameAction::PlayerTwoSelect),
                "nothing was held through the modal, so nothing is gated after it"
            );
            assert_eq!(
                app.world().resource::<GameState>().end_hold,
                MatchDuration::ZERO
            );
        }
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::from_secs_f64(0.5),
            "a fresh hold from zero"
        );
        cleanup_restart_test_root(root);
    }

    /// The arithmetic of the drain, and that it is a drain and not a pause:
    /// a second of hold loses a quarter in half a second up, resumes from
    /// there when both Selects come down again, and runs out after two
    /// seconds up, staying at zero rather than wrapping or reviving.
    #[test]
    fn a_released_end_hold_drains_at_half_the_rate_it_filled() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::from_secs_f64(1.0)
        );

        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        {
            assert!(!held(&app, GameAction::PlayerOneSelect));
            assert!(!held(&app, GameAction::PlayerTwoSelect));
            let game = app.world().resource::<GameState>();
            assert_eq!(game.end_hold, MatchDuration::from_secs_f64(0.75));
            assert_eq!(game.running_hold, None);
            assert!(game.end_snapshot.is_none());
            assert_eq!(
                active_hold_progress(game),
                Some(("Ending Match", (0.75 / HOLD_DURATION.as_secs_f64()) as f32))
            );
        }
        advance_test_app(&mut app, 0.5);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::from_secs_f64(0.5)
        );

        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(
                game.end_hold,
                MatchDuration::from_secs_f64(1.0),
                "the hold resumes from what the drain left, on a fresh snapshot"
            );
            assert!(game.end_snapshot.is_some());
            assert_eq!(game.running_hold, Some(HoldGesture::End));
        }

        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 2.0);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::ZERO,
            "two seconds up drain a second of hold"
        );
        advance_test_app(&mut app, 1.0);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.end_hold, MatchDuration::ZERO);
        assert_eq!(active_hold_progress(game), None);
        cleanup_restart_test_root(root);
    }

    /// A modal ends the gesture outright, draining or not: Player Two lets
    /// go and the hold is draining when Help opens over Player One's Select,
    /// which is still physically down. The timer returns to zero rather than
    /// being left where it stood - a modal pauses the clock, so there would
    /// have been nothing to drain by - and after the modal closes that Select
    /// stays gated until it is released, so the frames on which it is "not
    /// running" drain a timer that is already at zero - the behaviour a
    /// Select held through a modal had before the drain existed.
    #[test]
    fn a_modal_zeroes_a_draining_end_hold_and_a_select_held_through_it_drains_nothing_after() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 1.0);
        release(&mut app, &[GameAction::PlayerTwoSelect]);
        advance_test_app(&mut app, 0.5);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::from_secs_f64(0.75),
            "draining when the modal opens"
        );

        set_modal_open(&mut app, true);
        advance_test_app(&mut app, 0.1);
        {
            assert!(
                app.world()
                    .resource::<SemanticInput>()
                    .pressed(GameAction::PlayerOneSelect),
                "Player One is still physically holding"
            );
            assert!(
                !held(&app, GameAction::PlayerOneSelect),
                "...and the gate reads that Select as released"
            );
            let game = app.world().resource::<GameState>();
            assert_eq!(
                game.end_hold,
                MatchDuration::ZERO,
                "interrupted: returned to zero, not left at 0.75 - a modal pauses the \
                 clock, so `playing_delta` is zero and a drain would have taken nothing"
            );
            assert_eq!(game.running_hold, None);
            assert!(game.end_snapshot.is_none());
        }

        set_modal_open(&mut app, false);
        let paused_at = app.world().resource::<GameState>().elapsed;
        advance_test_app(&mut app, 1.0);
        {
            assert!(
                app.world()
                    .resource::<InputGate>()
                    .is_gated(GameAction::PlayerOneSelect)
            );
            let game = app.world().resource::<GameState>();
            assert_eq!(
                game.elapsed,
                paused_at + MatchDuration::from_secs_f64(1.0),
                "the frame was live - the Match clock ran - so the drain really was a no-op"
            );
            assert_eq!(game.phase, MatchPhase::Playing);
            assert_eq!(game.end_hold, MatchDuration::ZERO);
            assert_eq!(active_hold_progress(game), None);
        }

        release(&mut app, &[GameAction::PlayerOneSelect]);
        advance_test_app(&mut app, 0.1);
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.5);
        let game = app.world().resource::<GameState>();
        assert_eq!(
            game.end_hold,
            MatchDuration::from_secs_f64(0.5),
            "a fresh hold from zero once the Select has been released and pressed again"
        );
        assert!(game.end_snapshot.is_some());
        cleanup_restart_test_root(root);
    }

    /// Decided rather than inherited: on the frame the end hold completes,
    /// the bar still reports the end gesture, at full. The gesture was read
    /// as held at the top of that frame, before it fired, and a bar that is
    /// seen to reach full is the natural picture of a gesture completing.
    /// Preferring the running gesture could have made that frame show
    /// nothing instead; the next frame does, once the `Ended` arm has zeroed
    /// the timer.
    #[test]
    fn the_bar_reaches_full_on_the_frame_the_end_hold_fires() {
        let (mut app, root) = active_match_input_app(KeyboardBindings::default());
        press(&mut app, &BOTH_SELECTS);
        for _ in 0..2 {
            advance_test_app(&mut app, 1.0);
        }
        assert_eq!(
            app.world().resource::<GameState>().phase,
            MatchPhase::Playing
        );

        advance_test_app(&mut app, 1.0);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Ended);
            assert_eq!(game.end_reasons, vec![EndReason::Manual]);
            assert_eq!(game.end_hold, HOLD_DURATION);
            assert_eq!(game.running_hold, Some(HoldGesture::End));
            assert_eq!(active_hold_progress(game), Some(("Ending Match", 1.0)));
        }

        advance_test_app(&mut app, 0.1);
        let game = app.world().resource::<GameState>();
        assert_eq!(game.end_hold, MatchDuration::ZERO);
        assert_eq!(game.running_hold, None);
        assert_eq!(active_hold_progress(game), None);
        cleanup_restart_test_root(root);
    }

    /// A draining end hold is shown only while there is something to resume.
    /// Releasing both Selects commits the automatic end parked in the
    /// snapshot, so the Match has `Ended` on the very frame the drain begins;
    /// the timer is non-zero for that frame - and shows that the whole frame
    /// was credited to the gestures while the clock stood frozen behind the
    /// parked end - but the bar reports nothing, because nothing can be
    /// resumed. The `Ended` arm zeroes the timer on the next frame.
    #[test]
    fn a_draining_end_hold_is_not_reported_once_the_match_has_ended() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 1);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let (mut app, root) = super::super::test_support::match_input_app_with_settings(
            KeyboardBindings::default(),
            MatchPhase::Playing,
            settings,
        );
        press(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.25);
        assert!(
            app.world()
                .resource::<GameState>()
                .provisional_end_pending()
        );

        release(&mut app, &BOTH_SELECTS);
        advance_test_app(&mut app, 0.1);
        {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.phase, MatchPhase::Ended);
            assert_eq!(game.end_reasons, vec![EndReason::RoundLimit]);
            assert_eq!(
                game.end_hold,
                MatchDuration::from_secs_f64(0.2),
                "the release frame drained half of its whole 0.1 s"
            );
            assert_eq!(game.running_hold, None);
            assert_eq!(active_hold_progress(game), None);
        }

        advance_test_app(&mut app, 0.1);
        assert_eq!(
            app.world().resource::<GameState>().end_hold,
            MatchDuration::ZERO
        );
        cleanup_restart_test_root(root);
    }
}
