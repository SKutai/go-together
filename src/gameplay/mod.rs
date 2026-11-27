//! Live match state and the systems that drive a match from Round 0 to final
//! score.
//!
//! # Responsibility
//!
//! [`crate::authority::MatchState`] is the authoritative, Bevy-independent
//! record of a match in progress: board, cursors, opening choices, hidden
//! moves, round number, clocks, Match rules, and official end reasons. It
//! lives in the top-level [`crate::authority`] module - outside `gameplay`
//! entirely - so it compiles as part of this crate's headless library API
//! with no Bevy dependency; see `docs/ARCHITECTURE.md` for the full
//! core/client split. [`GameState`] is the single Bevy resource the rest of
//! the app reaches for; it wraps one `MatchState` plus local runtime concerns
//! that a hypothetical networked peer would never need to agree on -
//! display-only hints, hold-gesture progress and its rollback snapshot, the
//! optional [`MatchRecorder`], and storage status. Device-release gating is
//! not among them: it is the sibling [`InputGate`] resource, which the same
//! plugin owns.
//!
//! # Pausing without a UI dependency
//!
//! Gameplay has no idea what a "modal" is. [`GameplayPause`] is the single
//! neutral signal UI or application-level orchestration sets to say whether
//! *this client* should stop accepting input ([`GameplayPause::input_blocked`])
//! and whether the *authoritative Match* should stop advancing
//! ([`GameplayPause::match_paused`]). The two are kept distinct - not because
//! today's shared-screen game ever sets them differently, but so that one
//! client opening a modal in a future networked mode can block only its own
//! input without silently pausing the Match for every player. `crate::ui::hud`
//! is the current writer, translating its own modal state into this type;
//! gameplay only ever reads it.
//!
//! The pure game *rules* (captures, liberties, positional repetition, board
//! scoring) live one level further down in [`crate::rules`], independent of
//! both Bevy and this module. [`crate::authority`] owns *when* those rules
//! apply during a live match; this module and its submodules own translating
//! Bevy input into that timing and recording what happened for Replay.
//!
//! # Submodules
//!
//! * [`countdown`] - the client side of Round 0's closing 3-2-1-Go sequence
//!   and the match clock: drives `MatchState::advance_clock` through each
//!   frame and maps the boundaries it reports onto Replay events, audio
//!   cues, an end deferred to an in-flight end gesture, and the per-frame
//!   input freeze.
//! * [`input`] - the input-release gate plus semantic actions to cursor moves,
//!   teleport markers and hidden selections.
//! * [`holds`] - the two three-second hold gestures: end match and restart.
//!
//! # Bevy lifecycle
//!
//! [`GameplayPlugin`] owns [`GameState`], [`InputGate`], and [`GameplayPause`]
//! and schedules the match pipeline into [`AppSystems::MatchUpdate`], in the
//! order clock -> gate -> holds -> actions -> replay save. That order is a
//! chain of individually justified data dependencies, not a convention; see
//! [`GameplayPlugin`]'s doc comment for the dependency behind each link.
//! Screen entry resets Match state
//! through [`initialize_match_state`] / [`initialize_home_state`], which the
//! owning UI screen chains before it spawns entities; the HUD's modal state
//! is reset by `crate::ui::hud` on the same screen change.
//!
//! # Invariants
//!
//! * A [`MatchRecorder`] exists from interactive Round 0 until scoring. It is
//!   dropped - never completed - when a match is abandoned, so an abandoned
//!   match cannot be saved and cannot leak events into the next one.
//! * Every mutation that a viewer must be able to reconstruct goes through
//!   [`record_game_event`], which snapshots the post-change frame. Replays are
//!   therefore reconstructible by seeking to a frame rather than by re-running
//!   the rules.
//! * Each player's temporary control hint is dismissed only by that player's
//!   first successful movement, selection, marker placement, or teleport.
//! * Gameplay never reads devices directly. It only sees the fused
//!   [`SemanticInput`] booleans, so keyboard and controller sources behave
//!   identically.
//! * An end-gesture snapshot exists only while the end gesture is running, or
//!   while an automatic end is parked in it - as of each frame's hold
//!   handling, which the clock precedes. See [`EndGestureSnapshot`] for why
//!   the two cases get different lifetimes.
//! * An automatic end deferred to an in-flight end gesture stops the Match
//!   until the gesture resolves: both clocks and all ordinary play, leaving
//!   only the two hold gestures running, since they are what resolve it. So
//!   no clock deadline and no selection can land on top of a deferred end,
//!   and no round resolves while one is waiting. [`complete_ordinary_round`]
//!   relies on this; see the comment there for what it rules out and for the
//!   tests that hold it.

pub(crate) mod countdown;
mod holds;
mod input;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod save_timing;

use std::ops::{Deref, DerefMut};

use bevy::prelude::*;

use crate::app::{AppScreen, AppSystems};
use crate::audio::GameCue;
use crate::authority::{MatchProgress, MatchState};
use crate::clock::{MatchDuration, MatchInstant};
use crate::input::{GameAction, SemanticInput};
use crate::match_settings::MatchSettings;
use crate::platform::now_millis;
use crate::replay::{EndReason, MatchRecorder, Replay, ReplayEventKind, ReplayFrame, ReplayPhase};
use crate::replay_library::{ReplayLibrary, format_local_timestamp};
#[cfg(test)]
use crate::rules::{Player, Point};

use countdown::advance_match_clock;
use holds::handle_hold_gestures;
use input::{handle_player_input, update_input_gate};

pub(crate) use crate::authority::{
    MatchPhase, OpeningAttempt, OpeningAttemptOutcome, OrdinaryCompletion, RoundRemovals,
    player_index,
};
#[cfg(test)]
pub(crate) use crate::authority::{MatchRules, PLAYER_STARTS};
pub(crate) use countdown::countdown_text;
pub(crate) use holds::{HoldGesture, active_hold_progress};

/// Owns live-match resources and the ordered match pipeline.
///
/// # Why the match pipeline is a chain
///
/// The five systems in [`AppSystems::MatchUpdate`] run in one `.chain()`.
/// Held to the same standard as the [`AppSystems`] edges one level up, every
/// link is a real data dependency rather than "the frame order". The edges
/// are listed by *producer*, because several consumers depend on the same
/// writer and a reorder that preserved only the adjacent pairs would still
/// break them:
///
/// * [`advance_match_clock`] -> everything after it. Its one call to
///   [`countdown::advance_match_state_by`] writes `game.phase` (a countdown
///   completing into [`MatchPhase::Playing`], or a clock boundary ending the
///   Match - Match Time in Round 0 or in play, or a Round Time timeout whose
///   resolution crosses an automatic limit), `game.playing_delta`,
///   `game.input_frozen_this_frame`, and - whenever one of those boundaries
///   ends the Match - `game.pending_replay`, or instead, when a Round Time
///   timeout crosses an automatic limit during an end hold, the end it parks
///   in `game.end_snapshot`. So it must precede
///   [`handle_hold_gestures`], which reads `playing_delta`,
///   `input_frozen_this_frame` and the parked end; [`handle_player_input`],
///   which reads `input_frozen_this_frame`, `phase`, and whether an end is
///   parked, since a parked end stops play; and [`save_completed_replay`],
///   which consumes `pending_replay`. [`update_input_gate`] is the one
///   system after it that depends on none of those: it reads only
///   [`SemanticInput`] and [`GameplayPause`], so its position in the chain is
///   fixed by its own consumers below, not by the clock.
/// * [`update_input_gate`] -> [`handle_hold_gestures`] and
///   [`handle_player_input`]. It writes [`InputGate`], and both consumers ask
///   it, action by action, whether a press counts, so they need this frame's
///   gate rather than last frame's. They are its only readers:
///   [`advance_match_clock`] deliberately does not consult the gate at all,
///   so no held input can stop the shared Match clock.
/// * [`handle_hold_gestures`] -> [`handle_player_input`]. A completed restart
///   hold re-arms [`InputGate`] (`advance_restart_hold` calls
///   [`InputGate::arm`]) so the still-held restart input cannot also act as
///   gameplay input; player input must observe that the same frame.
/// * [`handle_hold_gestures`] -> [`save_completed_replay`]. A completed end
///   hold, and committing a deferred provisional end when the gesture is
///   released, both set `game.pending_replay`.
/// * [`handle_player_input`] -> [`save_completed_replay`]. A selection that
///   fills both locks resolves the round, which can end the Match and
///   produce `game.pending_replay`; the save consumes it the same frame.
///
/// `game.playing_delta` is worth calling out: it exists precisely so a single
/// long frame that crosses Round 0's reveal does not credit the pre-reveal
/// part of that frame to the end-gesture hold. That is what makes the
/// [`advance_match_clock`] -> [`handle_hold_gestures`] edge load-bearing
/// rather than cosmetic.
pub(crate) struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameState>()
            .init_resource::<InputGate>()
            .init_resource::<GameplayPause>()
            .add_systems(
                Update,
                (
                    advance_match_clock.run_if(in_state(AppScreen::Match)),
                    update_input_gate,
                    handle_hold_gestures.run_if(in_state(AppScreen::Match)),
                    handle_player_input.run_if(in_state(AppScreen::Match)),
                    save_completed_replay.run_if(in_state(AppScreen::Match)),
                )
                    // Not a blanket ordering: see this plugin's doc comment
                    // for the data dependency behind each link.
                    .chain()
                    .in_set(AppSystems::MatchUpdate),
            );
    }
}

/// How long both hold gestures must be held. Long enough that neither can
/// be triggered by an accidental press during play.
///
/// The player-facing copy does not read this constant. Four code sites spell
/// "3 seconds" out as text, and their tests assert the literal: `ui::help`'s
/// keyboard, rules and controller pages; `ui::match_screen`'s in-play
/// end-gesture hint; its ended-match restart status line; and `ui::hud`'s
/// return-home confirmation. `README.md`, `docs/GAME_RULES.md` and
/// `docs/ARCHITECTURE.md` state the duration too, spelled "three seconds".
/// Changing the duration means changing all of them together.
pub(crate) const HOLD_DURATION: MatchDuration = MatchDuration::from_secs(3);

/// The single Bevy resource through which the whole app reaches the live
/// Match: one authoritative [`MatchState`] plus everything that is a
/// property of this local client rather than of the Match itself.
///
/// Local fields are `pub(crate)` where the interface renders them,
/// `pub(super)` where a submodule drives them, and otherwise private; the
/// authoritative fields live on [`MatchState`] and are reached transparently
/// through [`Deref`]/[`DerefMut`], so `game.board`, `game.round`,
/// `game.phase` and so on keep working exactly as before this split - only
/// the *ownership* moved, not the access.
#[derive(Resource)]
pub(crate) struct GameState {
    pub(super) state: MatchState,
    /// Temporary movement/Select hints, independently dismissed for each player.
    pub(crate) control_hints_visible: [bool; 2],
    pub(crate) end_hold: MatchDuration,
    /// Rollback point captured on the first frame of an end hold, kept only
    /// while that hold runs or while an automatic end is parked in it. See
    /// [`EndGestureSnapshot`].
    end_snapshot: Option<EndGestureSnapshot>,
    pub(crate) restart_hold: MatchDuration,
    /// The hold gesture that is held this frame - the one whose timer would
    /// be advancing if nothing stopped the gestures from acting - if either
    /// is. [`handle_hold_gestures`] records it before anything can stop the
    /// gestures from acting, and [`active_hold_progress`] reads it so the
    /// progress bar names the gesture that is going to fire rather than the
    /// timer that happens to be non-zero - which, since a released end hold
    /// drains rather than resets, may belong to the other gesture. A restart
    /// clears it with the rest of the Match, so the frame a restart completes
    /// reports no gesture: the fresh Match has none in it.
    pub(crate) running_hold: Option<HoldGesture>,
    /// Portion of the current frame spent in ordinary play - or the whole
    /// frame while an automatic end is parked and the clock is frozen. Hold
    /// handling uses this instead of the whole frame delta when a long frame
    /// crosses reveal.
    playing_delta: MatchDuration,
    /// Set when a clock boundary resolves or ends play before input systems run.
    pub(super) input_frozen_this_frame: bool,
    /// Present from interactive Round 0 through scoring.
    pub(super) recorder: Option<MatchRecorder>,
    pending_replay: Option<Replay>,
    /// A replay-save problem worth telling the player about, shown after
    /// scoring. Empty when everything worked; otherwise
    /// [`REPLAY_SAVE_FAILED_NOTICE`] - a player-safe sentence, never the
    /// underlying storage error, because the Match screen renders this
    /// verbatim.
    pub(crate) replay_notice: String,
}

impl Deref for GameState {
    type Target = MatchState;

    fn deref(&self) -> &MatchState {
        &self.state
    }
}

impl DerefMut for GameState {
    fn deref_mut(&mut self) -> &mut MatchState {
        &mut self.state
    }
}

/// What an end hold rolls back if it completes, and any automatic end that
/// is waiting on it.
///
/// A brief simultaneous tap of both Select inputs is a legitimate pair of
/// moves, so the state from before the gesture is kept until the hold either
/// completes (restore, then score) or is released (discard, moves stand).
///
/// The authoritative half of the rollback is one clone of
/// [`crate::authority::MatchProgress`] - exactly the part of [`MatchState`] that
/// changes as a result of moves and round resolution, and not the clocks,
/// cursors, or phase that keep advancing for the real time the hold takes.
/// Adding a new field to `MatchProgress` can never be forgotten here, because
/// there is no per-field list to update. Only the handful of local fields
/// this gesture also happens to touch - hints and the in-flight
/// recorder/pending Replay - are still named individually.
///
/// # Two jobs, two lifetimes
///
/// A snapshot is first the **rollback point** of the end gesture that is
/// running: the state immediately before it, restored if the hold completes.
/// `provisional_end_reasons` gives it a second job, as the **parking slot**
/// for an automatic end that a round reached while the hold was in flight.
/// [`complete_ordinary_round`] writes the end here instead of finishing,
/// because completing the hold would roll that round back and the end would
/// never have happened. `holds` then settles it: releasing the hold commits
/// it, completing the hold discards it with the rest of the snapshot and ends
/// the Match Manual, and completing Restart drops it unsaved. If Restart is
/// what is held as the Selects come up, it waits for Restart to complete or
/// be released, or for both Selects to come down again and resume the hold.
///
/// The two jobs need different lifetimes, and giving them one was a defect. A
/// rollback point means something only while its own gesture runs: kept past
/// it, the next end hold found it already there, adopted it as its own
/// pre-hold state, and on completing erased every round played in between. So
/// `holds` drops the snapshot on every `Playing` frame its gesture is not
/// running - including frames on which a modal or a clock boundary stops the
/// gestures from acting, since the gate stays fully armed while a modal is
/// open and the gesture therefore always reads as released - unless an end
/// is parked in it. The parked end
/// cannot move out into a field of its own, because it is a statement about
/// this rollback point - restore it, and this end never happened - and a
/// resumed hold that completes has to roll back past the round that parked
/// it. So when something is parked, the rollback point waits with it.
///
/// What keeps a waiting rollback point correct is that nothing happens to the
/// Match while an end is parked. Both clocks stop
/// ([`countdown::advance_match_state_by`]), and so does ordinary play
/// (`input`'s `match_takes_play`), leaving only the two hold gestures running,
/// since they are what settle it. The snapshot therefore still describes the
/// state immediately before the gesture that captured it, and what separates
/// it from the live state is exactly what resolved while that gesture ran -
/// the incidental round, any Round Time expiry, and the round that parked the
/// end - which is what a completed hold exists to remove. Together the two
/// rules keep one invariant: **a snapshot exists only while the end gesture is
/// running, or while an automatic end is parked in it and the Match is
/// stopped.** [`complete_ordinary_round`] relies on it.
///
/// The invariant is kept at frame granularity. The clock runs before `holds`
/// in each frame, so it sees the gesture as `holds` last left it: a Round Time
/// expiry on the frame the Selects come up still lands during the hold, as
/// though it had come a moment before the release.
///
/// `opening_countdown_finish` is bookkeeping rather than rollback state: the
/// Match-clock instant the Go card was due to clear when the hold began. The
/// clocks are not rolled back, so if the card cleared during the hold,
/// restoring the pre-hold recorder on its own would lose the
/// `OpeningCountdownFinished` event the current recorder holds;
/// [`restore_end_gesture_snapshot`] puts that one event back at its original
/// timestamp.
struct EndGestureSnapshot {
    progress: MatchProgress,
    control_hints_visible: [bool; 2],
    recorder: Option<MatchRecorder>,
    pending_replay: Option<Replay>,
    provisional_end_reasons: Option<Vec<EndReason>>,
    opening_countdown_finish: Option<OpeningCountdownFinishSnapshot>,
}

#[derive(Clone, Copy)]
struct OpeningCountdownFinishSnapshot {
    at: MatchInstant,
    round_elapsed: MatchDuration,
}

impl EndGestureSnapshot {
    fn capture(game: &GameState) -> Self {
        let opening_countdown_finish =
            game.opening_go_remaining()
                .map(|remaining| OpeningCountdownFinishSnapshot {
                    at: game.elapsed + remaining,
                    round_elapsed: game.round_elapsed + remaining,
                });
        Self {
            progress: game.state.progress.clone(),
            control_hints_visible: game.control_hints_visible,
            recorder: game.recorder.clone(),
            pending_replay: game.pending_replay.clone(),
            provisional_end_reasons: None,
            opening_countdown_finish,
        }
    }
}

impl GameState {
    /// Whether an automatic end is parked in the end-gesture snapshot,
    /// waiting for the gesture to settle it - the state in which both clocks
    /// and all ordinary play are stopped, and only the two hold gestures still
    /// run. See [`EndGestureSnapshot`].
    pub(super) fn provisional_end_pending(&self) -> bool {
        self.end_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.provisional_end_reasons.is_some())
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            state: MatchState::default(),
            control_hints_visible: [true, true],
            end_hold: MatchDuration::ZERO,
            end_snapshot: None,
            restart_hold: MatchDuration::ZERO,
            running_hold: None,
            playing_delta: MatchDuration::ZERO,
            input_frozen_this_frame: false,
            recorder: None,
            pending_replay: None,
            replay_notice: String::new(),
        }
    }
}

/// Suppresses each [`GameAction`] individually until the input holding it is
/// released.
///
/// Without this, a key still held from the previous screen - or the very press
/// that closed a modal - would act on the board the instant play resumes.
///
/// The suppression is per action rather than one flag for the whole gate
/// because both players share one gate: a single flag meant one player resting
/// a finger on Select froze the other player too, most often at the very start
/// of Round 0. An action is gated if and only if its own bit is set, so
/// [`GameAction::RestartMatch`] - which is one action OR'd across the keyboard
/// and both assigned controllers rather than either player's - needs no
/// special case, and neither does a Match that has ended, where the actions
/// nothing reads any more simply gate nobody.
///
/// [`arm`](Self::arm) gates *every* action, and the next
/// [`update_input_gate`] with no modal open releases the ones that are not
/// actually held. What survives that first update is therefore exactly the
/// set of inputs that were already down when the gate armed, which is why
/// callers with no [`SemanticInput`] to hand - screen entry, a HUD button, a
/// completed restart - can arm without one. While a modal is open nothing is
/// released, so the mask stays fully armed until the frame it closes.
///
/// Two readers see the armed-but-not-yet-narrowed mask, and both want to.
/// [`handle_player_input`] sees it on the frame a restart completes, which is
/// what stops the input that restarted the match also playing it.
/// [`handle_hold_gestures`] sees it on every frame a modal is open, where it
/// reads the two Selects before returning, so neither can keep the end
/// gesture running under a modal - a frame the hold system treats as an
/// interruption rather than a release, returning the end hold's timer to
/// zero where a release would let it drain. Every other read happens after
/// [`update_input_gate`] has narrowed the mask to what is really held.
///
/// Nothing here touches the Match clock. The gate is a property of one local
/// device and one action; the clock is shared by both players, so
/// [`advance_match_clock`] reads only [`GameplayPause::match_paused`]. A held
/// key gates that key, and time keeps running.
#[derive(Resource, Default)]
pub(crate) struct InputGate {
    gated: [bool; GameAction::ALL.len()],
}

impl InputGate {
    /// Gates every action, leaving the next gate update to release the ones
    /// that are not held. See the type's doc comment for why arming is
    /// deliberately coarse and narrowing is not.
    pub(crate) fn arm(&mut self) {
        self.gated = [true; GameAction::ALL.len()];
    }

    /// Opens the gate for every action at once.
    pub(crate) fn open(&mut self) {
        self.gated = [false; GameAction::ALL.len()];
    }

    /// Whether this one action is still waiting to be released.
    pub(crate) fn is_gated(&self, action: GameAction) -> bool {
        self.gated[action as usize]
    }

    /// Whether any action at all is still waiting to be released.
    ///
    /// Test-only, and deliberately so: no running system asks this question.
    /// The gate suppresses one action at a time, and every consumer names the
    /// action it cares about. The Match clock used to ask it, which is how a
    /// single held key came to stop time for both players; it now reads only
    /// [`GameplayPause::match_paused`].
    #[cfg(test)]
    pub(crate) fn any_gated(&self) -> bool {
        self.gated.iter().any(|gated| *gated)
    }

    /// Gates every action currently held, leaving the rest as they are.
    pub(super) fn gate_held(&mut self, semantic: &SemanticInput) {
        for action in GameAction::ALL {
            self.gated[action as usize] |= semantic.pressed(action);
        }
    }

    /// Releases every action that is no longer held.
    pub(super) fn release_unheld(&mut self, semantic: &SemanticInput) {
        for action in GameAction::ALL {
            self.gated[action as usize] &= semantic.pressed(action);
        }
    }
}

/// The neutral, UI-independent pause signal gameplay reads instead of any
/// concrete modal type. See the module documentation for why the two flags
/// are kept separate.
///
/// `crate::ui::hud` is today's sole writer, deriving both flags together from
/// its modal state; a future networked mode could instead have only a local
/// client set `input_blocked` while `match_paused` comes from the server.
#[derive(Resource, Default, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GameplayPause {
    /// This running client should not accept or submit player actions.
    pub(crate) input_blocked: bool,
    /// Match clocks and authoritative progression are paused for all players.
    pub(crate) match_paused: bool,
}

impl GameplayPause {
    /// Blocks local input and pauses the Match together - what opening the
    /// current shared-screen modal does today.
    pub(crate) const fn blocking() -> Self {
        Self {
            input_blocked: true,
            match_paused: true,
        }
    }
}

/// Resets to a clean, unstarted match for the title screen.
///
/// The gate is left open because nothing on Home can be triggered by a held
/// gameplay input. HUD/modal state is reset separately, by `crate::ui::hud`,
/// on the same screen change.
pub(crate) fn initialize_home_state(game: &mut GameState, input_gate: &mut InputGate) {
    reset_match(game);
    input_gate.open();
}

/// Resets for a fresh match and arms the input gate, so an input held while
/// navigating into the match cannot act once play begins. HUD/modal state
/// is reset separately, by `crate::ui::hud`, on the same screen change.
pub(crate) fn initialize_match_state(
    game: &mut GameState,
    input_gate: &mut InputGate,
    settings: &MatchSettings,
) {
    reset_match(game);
    begin_round_zero(game, settings);
    input_gate.arm();
}

pub(super) fn reset_match(game: &mut GameState) {
    *game = GameState::default();
}

/// Starts the interactive opening and its Replay timeline at elapsed zero.
pub(super) fn begin_round_zero(game: &mut GameState, settings: &MatchSettings) {
    game.state.begin_round_zero(settings);
    game.pending_replay = None;
    let started_at_ms = now_millis();
    game.recorder = Some(MatchRecorder::new_with_settings(
        started_at_ms,
        format_local_timestamp(started_at_ms),
        replay_frame(game),
        game.match_rules.settings().clone(),
    ));
    record_game_event(game, ReplayEventKind::OpeningStarted);
}

/// Scores the match and completes the recording.
///
/// Territory is filled first, so the final board that is scored is also the
/// final board that is recorded and later replayed. Returns `None` when there
/// was nothing to finish or nothing was being recorded.
///
/// Nothing here writes the result for the player to read. Who won and why
/// are already authoritative state - the final score and
/// [`MatchProgress::end_reasons`] - and `crate::ui::match_screen` words them
/// from there, so a sentence about the result cannot disagree with the
/// result it describes.
pub(super) fn finish_match(game: &mut GameState, end_reasons: Vec<EndReason>) -> Option<Replay> {
    if !game.state.finish(end_reasons) {
        return None;
    }
    record_game_event(
        game,
        ReplayEventKind::MatchSound {
            cue: GameCue::MatchEnd,
        },
    );
    let final_frame = replay_frame(game);
    let elapsed_seconds = game.elapsed.as_secs_f64();
    let end_reasons = game.end_reasons.clone();
    game.recorder.as_mut()?.complete_with_reasons(
        now_millis(),
        elapsed_seconds,
        final_frame,
        end_reasons,
    )
}

/// Applies one authoritative ordinary-round result, updates every counter,
/// records the post-mutation frame, and evaluates automatic limits.
///
/// The authoritative resolution itself - the pure-rules call, counter
/// updates, and automatic-limit evaluation - is
/// [`MatchState::resolve_ordinary_round`]; this wrapper only adds what that
/// pure step cannot own: recording the result for Replay, and deciding what
/// becomes of an automatic end the round crossed - a Match Time timeout
/// finishes the Match at once, discarding any end-gesture snapshot; an end
/// gesture in flight instead has the end deferred to its snapshot; with
/// neither, the Match finishes immediately.
pub(super) fn complete_ordinary_round(
    game: &mut GameState,
    completion: OrdinaryCompletion,
) -> Option<GameCue> {
    if game.phase != MatchPhase::Playing {
        return None;
    }
    // There is deliberately no rollback here for a Match Time timeout that
    // lands while this hold has *already* deferred an automatic end. That
    // state cannot occur: deferring an end freezes the Match clock, so
    // `elapsed` can never grow to the Match Time limit while
    // `provisional_end_reasons` is set. `countdown::advance_match_state_by`
    // returns before its first clock step when one is pending and breaks
    // immediately after creating one, so the clock spends none of the rest of
    // that frame (see its doc for where that time goes); `MatchState::advance_clock` has exactly
    // that one caller, so nothing else can move `elapsed` either. The other
    // way a round resolves, both players selecting, is stopped the same way:
    // `input`'s `match_takes_play` refuses every ordinary action while an end
    // is parked, asking again between the two Selects of a single frame. So no
    // round of any kind resolves while one is parked. Every exit from a
    // pending provisional end finishes the Match, restarts it, or abandons it
    // by returning Home, rather than resuming play, so the freeze is
    // terminal, not a pause.
    //
    // Those are emergent properties of a few statement placements, so they
    // are pinned directly: the clock by `countdown`'s
    // `a_pending_provisional_end_freezes_the_match_clock_short_of_the_match_time_limit`,
    // and play by `holds`'
    // `a_parked_automatic_end_ignores_ordinary_play_until_it_is_settled` and
    // `a_select_in_the_frame_that_parks_an_automatic_end_does_not_select_for_the_next_round`.
    // An edit that un-freezes either fails there instead of quietly making
    // this case live again. A branch guarding it here could never be
    // covered, and would sit oddly beside the deferral logic below, which
    // discards the snapshot on a Match Time timeout rather than restoring it.
    //
    // A Match Time timeout during an end hold that has deferred *nothing* is
    // ordinary and reachable - see `countdown`'s
    // `match_time_interrupts_an_end_hold_and_saves_once_at_the_deadline` -
    // and is handled below.
    let outcome = game.state.resolve_ordinary_round(completion)?;
    record_game_event(
        game,
        match completion {
            OrdinaryCompletion::Normal => ReplayEventKind::RoundResolved {
                resolution: outcome.kind,
                captures: outcome.capture_credits,
            },
            OrdinaryCompletion::RoundTimeout | OrdinaryCompletion::MatchTimeTimeout => {
                ReplayEventKind::RoundTimedOut {
                    resolution: outcome.kind,
                    submitted: outcome.submitted,
                    captures: outcome.capture_credits,
                    cause: completion
                        .timeout_cause()
                        .expect("timeout completions always have a cause"),
                }
            }
        },
    );
    let cue = GameCue::from_round_sound(outcome.sound);
    if let Some(cue) = cue {
        record_game_event(game, ReplayEventKind::MatchSound { cue });
    }
    if !outcome.end_reasons.is_empty() {
        if completion == OrdinaryCompletion::MatchTimeTimeout {
            game.end_hold = MatchDuration::ZERO;
            game.end_snapshot = None;
            game.pending_replay = finish_match(game, outcome.end_reasons);
        } else if let Some(snapshot) = &mut game.end_snapshot {
            // The existence test is the question this branch means - could
            // completing the end gesture still roll back the round that
            // reached this end? - and not a stand-in for "is a gesture
            // running". A snapshot exists only while the gesture runs or has
            // an end parked in it (see `EndGestureSnapshot`), and no round
            // resolves while one is parked (see the top of this function),
            // so this is the running gesture's own rollback point with nothing
            // in it yet. The assertion names that second half, which the
            // snapshot's shape alone cannot: a round resolving here with an
            // end already parked would overwrite it.
            debug_assert!(
                snapshot.provisional_end_reasons.is_none(),
                "no round resolves while an automatic end is parked"
            );
            snapshot.provisional_end_reasons = Some(outcome.end_reasons);
        } else {
            game.pending_replay = finish_match(game, outcome.end_reasons);
        }
    }
    cue
}

/// Restores the pre-hold [`EndGestureSnapshot`] when an end hold completes.
///
/// Re-records `OpeningCountdownFinished` when the Go card cleared during the
/// hold - the one recording in this module that bypasses
/// [`record_game_event`], because the frame it belongs to is not the current
/// one. See [`EndGestureSnapshot`] for why that event has to be put back.
pub(super) fn restore_end_gesture_snapshot(game: &mut GameState) {
    let Some(snapshot) = game.end_snapshot.take() else {
        return;
    };
    let opening_countdown_finish = snapshot.opening_countdown_finish.filter(|finish| {
        game.recorder.as_ref().is_some_and(|recorder| {
            recorder.has_opening_countdown_finished_at(finish.at.as_secs_f64())
        })
    });
    game.state.progress = snapshot.progress;
    game.control_hints_visible = snapshot.control_hints_visible;
    game.recorder = snapshot.recorder;
    game.pending_replay = snapshot.pending_replay;
    if let Some(finish) = opening_countdown_finish {
        let mut frame = replay_frame(game);
        frame.round_elapsed_seconds = finish.round_elapsed.as_secs_f64();
        if let Some(recorder) = &mut game.recorder {
            recorder.record(
                finish.at.as_secs_f64(),
                ReplayEventKind::OpeningCountdownFinished,
                frame,
            );
        }
    }
}

impl GameState {
    #[cfg(test)]
    pub(crate) fn with_match_settings(settings: &MatchSettings) -> Self {
        Self::with_match_rules(MatchRules::snapshot(settings))
    }

    /// Test-only convenience: a fresh `GameState` with only its authoritative
    /// [`MatchRules`] overridden. Prefer this over the struct-update-syntax
    /// literal `GameState { match_rules: ..., ..Default::default() }`, which
    /// no longer compiles now that `match_rules` lives on the nested
    /// [`MatchState`], not on `GameState` directly.
    #[cfg(test)]
    pub(crate) fn with_match_rules(match_rules: MatchRules) -> Self {
        Self {
            state: MatchState {
                match_rules,
                ..MatchState::default()
            },
            ..Default::default()
        }
    }

    /// Test-only convenience: a fresh `GameState` with only its authoritative
    /// cursor positions overridden. See [`Self::with_match_rules`].
    #[cfg(test)]
    pub(crate) fn with_cursors(cursors: [Point; 2]) -> Self {
        Self {
            state: MatchState {
                cursors,
                ..MatchState::default()
            },
            ..Default::default()
        }
    }
}

/// What the player is told when a completed Match's Replay could not be
/// saved, both in the ended-match status line and on
/// [`Diagnostics`](crate::diagnostics::Diagnostics).
///
/// Deliberately a sentence rather than the
/// [`ReplayError`](crate::replay::ReplayError) itself:
/// [`GameState::replay_notice`] is rendered verbatim into the ended-match
/// status line by `ui::match_screen::replay_notice_suffix`, and
/// [`ReplayError::Storage`](crate::replay::ReplayError::Storage) carries a
/// full filesystem path and raw OS text. The detail is not lost - it stays
/// in the `bevy::log` line
/// [`report_recoverable`](crate::diagnostics::report_recoverable) makes from
/// its `technical` argument.
pub(crate) const REPLAY_SAVE_FAILED_NOTICE: &str =
    "Could not save this match's Replay. The match result itself is unaffected.";

/// The last link of the match pipeline: consumes `game.pending_replay` on the
/// same frame an earlier system set it, queues the live [`GameCue::MatchEnd`]
/// cue, and saves through [`ReplayLibrary::save_completed`]; a save failure
/// becomes [`REPLAY_SAVE_FAILED_NOTICE`].
///
/// This is the only place the Match End cue reaches the audio queue -
/// [`finish_match`] records the `MatchSound` event into the Replay but plays
/// nothing - so an automatic end deferred to an in-flight end gesture stays
/// silent until the gesture commits it.
fn save_completed_replay(
    mut game: ResMut<GameState>,
    mut replay_library: ResMut<ReplayLibrary>,
    mut audio: ResMut<crate::audio::AudioCueQueue>,
    mut diagnostics: ResMut<crate::diagnostics::Diagnostics>,
) {
    let Some(replay) = game.pending_replay.take() else {
        return;
    };
    audio.game(GameCue::MatchEnd);
    match replay_library.save_completed(replay) {
        Ok(()) => game.replay_notice.clear(),
        Err(error) => {
            crate::diagnostics::report_recoverable(
                &mut diagnostics,
                format!("Could not save replay: {error}"),
                REPLAY_SAVE_FAILED_NOTICE,
            );
            game.replay_notice = REPLAY_SAVE_FAILED_NOTICE.into();
        }
    }
}

pub(super) fn replay_frame(game: &GameState) -> ReplayFrame {
    ReplayFrame {
        board: game.board.clone(),
        cursors: game.cursors,
        teleport_markers: game.teleport_markers,
        selected_moves: game
            .locks
            .map(|locked| locked.map(|selection| selection.point)),
        opening_selections: game.opening_selections.clone(),
        phase: match game.phase {
            MatchPhase::Opening | MatchPhase::OpeningCountdown => ReplayPhase::Opening,
            MatchPhase::Playing => ReplayPhase::Playing,
            MatchPhase::Ended => ReplayPhase::Ended,
        },
        opening_countdown: game.opening_countdown_stage(),
        // `ReplayFrame::score` is the *final* score, not a live one, and
        // `crate::replay::validation` enforces exactly that: a frame that is
        // not `Ended` must carry `Score::default()`, and an `Ended` one must
        // carry its `current_score`. So this stays phase-aware rather than
        // becoming a plain `game.current_score()`.
        score: if game.phase == MatchPhase::Ended {
            game.current_score()
        } else {
            crate::rules::Score::default()
        },
        current_round: game.round,
        capture_totals: game.capture_totals,
        no_stone_streak: game.no_stone_streak,
        round_elapsed_seconds: game.round_elapsed.as_secs_f64(),
    }
}

/// Records an event together with the frame that resulted from it.
///
/// Snapshotting *after* the change is what lets playback seek to a time and
/// simply draw, instead of replaying the rules. A no-op when no recording is
/// in progress, which is how Home-screen and abandoned state stay out of
/// Replays.
pub(super) fn record_game_event(game: &mut GameState, kind: ReplayEventKind) {
    let frame = replay_frame(game);
    let elapsed_seconds = game.elapsed.as_secs_f64();
    if let Some(recorder) = &mut game.recorder {
        recorder.record(elapsed_seconds, kind, frame);
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashSet;

    use super::*;

    use crate::gameplay::countdown::{advance_match_state_by, begin_active_match};
    use crate::gameplay::input::{
        jump_to_teleport_marker, move_cursor, store_teleport_marker, try_lock, try_opening_select,
    };
    use crate::match_settings::MatchSettingField;
    use crate::replay::ReplayEventKind;
    use crate::rules::{Board, Cell, LockedMove, Score};

    use crate::ui::help::HELP_PAGE_COUNT;
    use crate::ui::hud::{HudAction, HudState, apply_hud_action};
    use crate::ui::match_screen::player_label;

    fn secs(seconds: f64) -> MatchDuration {
        MatchDuration::from_secs_f64(seconds)
    }

    /// `replay_notice` is the one string here a player actually reads: the
    /// Match screen renders it verbatim into the ended-match status line,
    /// under "Hold ... to restart". Neither it nor `Diagnostics` may carry
    /// what `ReplayError::Storage` does.
    #[test]
    fn a_failed_replay_save_tells_the_player_a_sentence_not_a_storage_path() {
        let mut app = App::new();
        app.insert_resource(GameState {
            pending_replay: Some(crate::replay::test_support::sample_replay(1, 2, "Doomed")),
            ..GameState::default()
        })
        .insert_resource(ReplayLibrary::from_store(
            crate::replay_library::test_support::PathLeakingReplayStorage,
        ))
        .insert_resource(crate::audio::AudioCueQueue::default())
        .init_resource::<crate::diagnostics::Diagnostics>()
        .add_systems(Update, save_completed_replay);

        app.update();

        let notice = app.world().resource::<GameState>().replay_notice.clone();
        assert_eq!(notice, REPLAY_SAVE_FAILED_NOTICE);
        crate::diagnostics::assert_carries_no_internal_detail(&notice);
        assert_eq!(
            crate::ui::match_screen::replay_notice_suffix(&notice),
            format!("  |  {REPLAY_SAVE_FAILED_NOTICE}"),
            "the ended-match status line appends the player-safe sentence"
        );

        let diagnostics = app.world().resource::<crate::diagnostics::Diagnostics>();
        assert_eq!(
            diagnostics.active(),
            [crate::diagnostics::Diagnostic {
                severity: crate::diagnostics::Severity::Warning,
                message: REPLAY_SAVE_FAILED_NOTICE.into(),
            }]
        );
        crate::diagnostics::assert_carries_no_internal_detail(&diagnostics.active()[0].message);
        assert!(
            crate::diagnostics::carries_internal_detail(
                crate::replay_library::test_support::LEAKED_REPLAY_STORAGE_ERROR
            ),
            "the failure this drove really does produce something unfit to show a player, \
             so the assertions above cannot pass because the setup was harmless"
        );
    }

    fn settings_with(field: MatchSettingField, value: u16) -> MatchSettings {
        let mut settings = MatchSettings::default();
        settings.set_value(field, u64::from(value));
        settings.set_enabled(field, true);
        settings
    }

    fn active_game(settings: &MatchSettings) -> GameState {
        let mut game = GameState::with_match_rules(MatchRules::snapshot(settings));
        begin_active_match(&mut game);
        game
    }

    fn set_black_corner_suicide(board: &mut Board) {
        board.set(Point::new(0, 0), Cell::Black);
        for point in [Point::new(0, 1), Point::new(1, 1), Point::new(2, 0)] {
            board.set(point, Cell::White);
        }
    }

    fn set_white_corner_suicide(board: &mut Board) {
        board.set(Point::new(8, 8), Cell::White);
        for point in [Point::new(8, 7), Point::new(7, 7), Point::new(6, 8)] {
            board.set(point, Cell::Black);
        }
    }

    /// What a player does is recorded for the Replay. That it is not also
    /// announced on the live screen is structural rather than asserted:
    /// `GameState` has had no message field to announce it in since the
    /// end-of-match string moved out to the Match screen on 2026-10-07.
    #[test]
    fn player_activity_is_recorded_for_the_replay() {
        let mut game = GameState::default();
        begin_active_match(&mut game);

        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        assert_eq!(player_label(Player::One), "BLACK");

        move_cursor(&mut game, Player::One, -1, 0);
        store_teleport_marker(&mut game, Player::One);

        move_cursor(&mut game, Player::One, 0, -1);
        jump_to_teleport_marker(&mut game, Player::One);
        let mut recorder = game.recorder.clone().unwrap();
        let replay = recorder
            .complete(2, game.elapsed.as_secs_f64(), replay_frame(&game))
            .expect("an active recording should complete");
        assert!(replay.events.iter().any(|event| matches!(
            &event.kind,
            ReplayEventKind::SelectionChanged {
                player: Player::One,
                ..
            }
        )));
        assert!(replay.events.iter().any(|event| matches!(
            &event.kind,
            ReplayEventKind::MarkerChanged {
                player: Player::One,
                ..
            }
        )));
        assert!(replay.events.iter().any(|event| matches!(
            &event.kind,
            ReplayEventKind::JumpToMarker {
                player: Player::One,
                ..
            }
        )));
    }

    #[test]
    fn reset_clears_both_teleport_markers() {
        let mut game = GameState::default();
        store_teleport_marker(&mut game, Player::One);
        store_teleport_marker(&mut game, Player::Two);

        reset_match(&mut game);

        assert_eq!(game.teleport_markers, [None, None]);
    }

    #[test]
    fn match_starts_and_resets_cursors_at_requested_points() {
        let mut game = GameState::default();

        assert_eq!(game.cursors[player_index(Player::One)], Point::new(7, 4));
        assert_eq!(game.cursors[player_index(Player::Two)], Point::new(1, 4));
        assert!(game.recorder.is_none());
        begin_active_match(&mut game);
        let mut initial_recorder = game.recorder.clone().unwrap();
        let initial_replay = initial_recorder
            .complete(1, 0.0, replay_frame(&game))
            .unwrap();
        assert_eq!(initial_replay.events[0].frame.cursors, PLAYER_STARTS);

        game.cursors = [Point::new(0, 0), Point::new(8, 8)];
        game.elapsed = MatchInstant::from_secs_f64(87.0);
        reset_match(&mut game);

        assert_eq!(game.cursors[player_index(Player::One)], Point::new(7, 4));
        assert_eq!(game.cursors[player_index(Player::Two)], Point::new(1, 4));
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        assert!(game.recorder.is_none());
        begin_active_match(&mut game);
        let mut reset_recorder = game.recorder.clone().unwrap();
        let reset_replay = reset_recorder
            .complete(1, 0.0, replay_frame(&game))
            .unwrap();
        assert_eq!(reset_replay.events[0].frame.cursors, PLAYER_STARTS);
    }

    #[test]
    fn fresh_match_navigation_resets_every_transient_match_value() {
        let mut game = GameState::default();
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();
        let dirty_board = {
            let mut board = Board::default();
            board.set(Point::new(4, 4), Cell::Gray);
            // A scoring stone, so the derived score is genuinely non-default
            // before the reset: Gray scores for neither player.
            board.set(Point::new(0, 0), Cell::Black);
            board
        };
        game.board = dirty_board.clone();
        game.cursors = [Point::new(0, 0), Point::new(8, 8)];
        game.teleport_markers = [Some(Point::new(2, 2)), Some(Point::new(6, 6))];
        game.control_hints_visible = [false, false];
        game.locks = [
            Some(LockedMove {
                player: Player::One,
                point: Point::new(2, 2),
                at: MatchInstant::from_secs_f64(12.0),
            }),
            None,
        ];
        game.completed_history.insert(dirty_board.clone());
        game.round = 9;
        game.phase = MatchPhase::Ended;
        game.end_hold = secs(1.5);
        game.end_snapshot = Some(EndGestureSnapshot::capture(&game));
        game.restart_hold = secs(2.5);
        game.end_reasons = vec![EndReason::RoundLimit];
        game.elapsed = MatchInstant::from_secs_f64(143.0);
        hud.help_open = true;
        hud.help_page = HELP_PAGE_COUNT - 1;
        hud.return_home_open = true;

        assert_eq!(
            apply_hud_action(&mut hud, &mut input_gate, HudAction::PlayMatch),
            Some(AppScreen::Match)
        );
        // Mirrors entering the Match: `prepare_match_screen` resets the Match
        // state and `ui::hud`'s screen-change system resets the HUD - two
        // systems on the same transition, rather than one gameplay function
        // reaching into UI state.
        initialize_match_state(&mut game, &mut input_gate, &MatchSettings::default());
        hud = HudState::default();

        assert_eq!(game.board, Board::default());
        assert_eq!(game.cursors, PLAYER_STARTS);
        assert_eq!(game.teleport_markers, [None, None]);
        assert_eq!(game.control_hints_visible, [true, true]);
        assert_eq!(game.locks, [None, None]);
        assert_eq!(game.completed_history, HashSet::from([Board::default()]));
        assert_eq!(game.round, 0);
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.countdown_elapsed, MatchDuration::ZERO);
        assert!(game.recorder.is_some());
        assert_eq!(game.end_hold, MatchDuration::ZERO);
        assert!(game.end_snapshot.is_none());
        assert_eq!(game.restart_hold, MatchDuration::ZERO);
        assert_eq!(game.current_score(), Score::default());
        assert!(game.end_reasons.is_empty());
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        assert!(!hud.help_open);
        assert_eq!(hud.help_page, 0);
        assert!(!hud.return_home_open);
        assert!(
            GameAction::ALL
                .into_iter()
                .all(|action| input_gate.is_gated(action)),
            "entering the Match screen arms every action, and the gate update \
             then keeps only the ones actually held"
        );
    }

    #[test]
    fn completed_live_match_replays_round_zero_and_counts_only_ordinary_rounds() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &MatchSettings::default());
        for (player, points) in [
            (
                Player::One,
                [Point::new(0, 0), Point::new(1, 0), Point::new(2, 0)],
            ),
            (
                Player::Two,
                [Point::new(0, 0), Point::new(7, 8), Point::new(8, 8)],
            ),
        ] {
            for point in points {
                game.cursors[player.index()] = point;
                try_opening_select(&mut game, player);
            }
        }
        countdown::advance_match_state_by(&mut game, secs(3.75), false);
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.round, 1);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Gray);
        assert_eq!(game.completed_history, HashSet::from([game.board.clone()]));

        game.cursors = [Point::new(4, 4), Point::new(5, 5)];
        let first_time = game.elapsed;
        try_lock(&mut game, Player::One, first_time);
        try_lock(
            &mut game,
            Player::Two,
            first_time + MatchDuration::from_secs_f64(0.2),
        );
        assert_eq!(game.round, 2);
        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();

        replay.validate().unwrap();
        assert!(replay.opening_completed());
        assert_eq!(replay.round_count, 1);
        assert_eq!(replay.completed_round_count(), 1);
        assert!(
            replay
                .events
                .iter()
                .any(|event| matches!(event.kind, ReplayEventKind::OpeningStarted))
        );
        assert!(
            replay
                .events
                .iter()
                .any(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))
        );
    }

    #[test]
    fn completed_match_with_a_cleanup_round_produces_a_valid_replay() {
        let mut game = GameState::with_match_rules(MatchRules::snapshot(&MatchSettings::default()));
        game.board.set(Point::new(0, 0), Cell::White);
        game.board.set(Point::new(1, 0), Cell::Black);
        game.board.set(Point::new(0, 1), Cell::Black);
        game.cursors = [Point::new(1, 0), Point::new(8, 8)];
        begin_active_match(&mut game);

        let now = game.elapsed;
        try_lock(&mut game, Player::One, now);
        try_lock(
            &mut game,
            Player::Two,
            now + MatchDuration::from_secs_f64(0.2),
        );
        assert_eq!(game.round, 2);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(game.capture_totals, Score { black: 1, white: 0 });

        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();

        replay.validate().unwrap();
        assert_eq!(replay.score.black, game.current_score().black);
    }

    #[test]
    fn modal_transitions_never_restore_dismissed_control_hints() {
        let mut game = GameState::default();
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();
        move_cursor(&mut game, Player::Two, 1, 0);
        assert_eq!(game.control_hints_visible, [true, false]);

        apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenHelp);
        apply_hud_action(&mut hud, &mut input_gate, HudAction::CloseHelp);
        apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenReturnHome);
        apply_hud_action(&mut hud, &mut input_gate, HudAction::CancelReturnHome);

        assert_eq!(game.control_hints_visible, [true, false]);
    }

    #[test]
    fn match_rules_are_snapshotted_and_restart_uses_new_defaults() {
        let mut persisted = settings_with(MatchSettingField::RoundLimit, 2);
        let mut game = GameState::default();
        begin_round_zero(&mut game, &persisted);
        assert_eq!(game.match_rules.value(MatchSettingField::RoundLimit), 2);

        persisted.set_value(MatchSettingField::RoundLimit, 9);
        assert_eq!(game.match_rules.value(MatchSettingField::RoundLimit), 2);

        reset_match(&mut game);
        begin_round_zero(&mut game, &persisted);
        assert_eq!(game.match_rules.value(MatchSettingField::RoundLimit), 9);
        assert_eq!(game.completed_history, HashSet::from([Board::default()]));
        assert_eq!(game.capture_totals, Score::default());
        assert_eq!(game.no_stone_streak, 0);
    }

    #[test]
    fn authoritative_capture_counts_are_cumulative_and_simultaneous() {
        let mut game = active_game(&MatchSettings::default());
        game.board.set(Point::new(0, 0), Cell::White);
        game.board.set(Point::new(1, 0), Cell::Black);
        game.board.set(Point::new(8, 8), Cell::Black);
        game.board.set(Point::new(7, 8), Cell::White);
        game.completed_history = HashSet::from([game.board.clone()]);
        game.no_stone_streak = 3;
        game.locks = [
            Some(LockedMove {
                player: Player::One,
                point: Point::new(0, 1),
                at: MatchInstant::from_secs_f64(1.0),
            }),
            Some(LockedMove {
                player: Player::Two,
                point: Point::new(8, 7),
                at: MatchInstant::from_secs_f64(1.05),
            }),
        ];

        assert_eq!(
            complete_ordinary_round(&mut game, OrdinaryCompletion::Normal),
            Some(GameCue::BothCapture)
        );
        assert_eq!(game.capture_totals, Score { black: 1, white: 1 });
        assert_eq!(game.no_stone_streak, 0);
    }

    #[test]
    fn timeout_suicide_resolves_self_removal_and_resets_no_stone_streak() {
        let settings = settings_with(MatchSettingField::RoundTime, 1);
        let mut game = GameState::with_match_rules(MatchRules::snapshot(&settings));
        set_black_corner_suicide(&mut game.board);
        begin_active_match(&mut game);
        assert!(advance_match_state_by(&mut game, secs(1.0), false).is_empty());
        assert_eq!(game.no_stone_streak, 1);
        game.cursors[Player::One.index()] = Point::new(1, 0);
        advance_match_state_by(&mut game, secs(0.5), false);
        let now = game.elapsed;
        try_lock(&mut game, Player::One, now);

        assert_eq!(
            advance_match_state_by(&mut game, secs(0.5), false),
            vec![GameCue::WhiteCapture]
        );

        assert_eq!(game.round, 3);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(1, 0)), Cell::Empty);
        assert_eq!(game.capture_totals, Score { black: 0, white: 2 });
        assert_eq!(game.no_stone_streak, 0);
        assert!(
            game.recorder
                .as_ref()
                .unwrap()
                .events()
                .iter()
                .any(|event| {
                    event.kind
                        == ReplayEventKind::RoundTimedOut {
                            resolution: crate::rules::ResolutionKind::Single,
                            submitted: [true, false],
                            captures: [0, 2],
                            cause: crate::replay::TimeoutCause::RoundTime,
                        }
                })
        );
        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();
        replay.validate().unwrap();
        assert_eq!(
            replay
                .audio_cues_between(0.0, replay.duration_seconds)
                .into_iter()
                .filter(|cue| *cue == GameCue::WhiteCapture)
                .count(),
            1
        );
    }

    #[test]
    fn simultaneous_suicides_advance_once_and_replay_exact_boards_totals_and_audio() {
        let mut settings = MatchSettings::default();
        settings.set_capture_points(true);
        let mut game = GameState::with_match_rules(MatchRules::snapshot(&settings));
        set_black_corner_suicide(&mut game.board);
        set_white_corner_suicide(&mut game.board);
        game.cursors = [Point::new(1, 0), Point::new(7, 8)];
        begin_active_match(&mut game);

        game.elapsed = MatchInstant::from_secs_f64(1.0);
        game.round_elapsed = MatchDuration::from_secs_f64(1.0);
        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        game.elapsed = MatchInstant::from_secs_f64(1.05);
        game.round_elapsed = MatchDuration::from_secs_f64(1.05);
        assert_eq!(
            try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.05)),
            Some(GameCue::BothCapture)
        );

        assert_eq!(game.round, 2);
        assert_eq!(game.capture_totals, Score { black: 2, white: 2 });
        assert_eq!(game.no_stone_streak, 0);
        for point in [
            Point::new(0, 0),
            Point::new(1, 0),
            Point::new(8, 8),
            Point::new(7, 8),
        ] {
            assert_eq!(game.board.get(point), Cell::Empty);
        }

        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();
        replay.validate().unwrap();
        let round = replay
            .events
            .iter()
            .find(|event| matches!(event.kind, ReplayEventKind::RoundResolved { .. }))
            .unwrap();
        assert_eq!(round.frame.capture_totals, Score { black: 2, white: 2 });
        assert_eq!(
            round.frame.current_score(&replay.match_settings),
            Score { black: 5, white: 5 }
        );
        assert_eq!(
            replay
                .audio_cues_between(0.0, replay.duration_seconds)
                .into_iter()
                .filter(|cue| *cue == GameCue::BothCapture)
                .count(),
            1
        );
    }

    #[test]
    fn self_removal_capture_points_control_bonus_and_point_target() {
        let mut enabled_settings = settings_with(MatchSettingField::PointTarget, 5);
        enabled_settings.set_capture_points(true);
        enabled_settings.set_value(MatchSettingField::RoundTime, 1);
        enabled_settings.set_enabled(MatchSettingField::RoundTime, true);
        let mut enabled = GameState::with_match_rules(MatchRules::snapshot(&enabled_settings));
        set_black_corner_suicide(&mut enabled.board);
        begin_active_match(&mut enabled);
        enabled.cursors[Player::One.index()] = Point::new(1, 0);
        enabled.elapsed = MatchInstant::from_secs_f64(0.5);
        enabled.round_elapsed = MatchDuration::from_secs_f64(0.5);
        try_lock(&mut enabled, Player::One, MatchInstant::from_secs_f64(0.5));
        advance_match_state_by(&mut enabled, secs(0.5), false);

        assert_eq!(enabled.capture_totals, Score { black: 0, white: 2 });
        assert_eq!(enabled.phase, MatchPhase::Ended);
        assert_eq!(enabled.end_reasons, vec![EndReason::PointTarget]);
        let replay = enabled.pending_replay.as_ref().unwrap();
        replay.validate().unwrap();
        assert!(replay.match_settings.capture_points());
        assert_eq!(
            replay
                .events
                .iter()
                .find(|event| matches!(event.kind, ReplayEventKind::RoundTimedOut { .. }))
                .unwrap()
                .frame
                .capture_totals,
            Score { black: 0, white: 2 }
        );

        let mut disabled_settings = settings_with(MatchSettingField::PointTarget, 5);
        disabled_settings.set_value(MatchSettingField::RoundTime, 1);
        disabled_settings.set_enabled(MatchSettingField::RoundTime, true);
        let mut disabled = GameState::with_match_rules(MatchRules::snapshot(&disabled_settings));
        set_black_corner_suicide(&mut disabled.board);
        begin_active_match(&mut disabled);
        disabled.cursors[Player::One.index()] = Point::new(1, 0);
        disabled.elapsed = MatchInstant::from_secs_f64(0.5);
        disabled.round_elapsed = MatchDuration::from_secs_f64(0.5);
        try_lock(&mut disabled, Player::One, MatchInstant::from_secs_f64(0.5));
        advance_match_state_by(&mut disabled, secs(0.5), false);

        assert_eq!(disabled.capture_totals, Score { black: 0, white: 2 });
        assert_eq!(disabled.current_score(), disabled.board.score());
        assert_eq!(disabled.current_score().white, 3);
        assert_eq!(disabled.phase, MatchPhase::Playing);
        assert!(disabled.end_reasons.is_empty());
    }

    #[test]
    fn repetition_rolls_back_self_removal_credits_and_uses_gray_only() {
        let mut game = GameState::default();
        set_black_corner_suicide(&mut game.board);
        begin_active_match(&mut game);
        let attempt = Point::new(1, 0);
        let repeated = game.board.apply_move(Player::One, attempt).unwrap().board;
        let mut unrelated = Board::default();
        unrelated.set(Point::new(4, 4), Cell::Gray);
        game.completed_history.extend([repeated, unrelated]);
        game.capture_totals = Score { black: 4, white: 5 };
        game.cursors[Player::One.index()] = attempt;
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(0.5));

        assert_eq!(
            complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout),
            Some(GameCue::GrayCapture)
        );

        assert_eq!(game.capture_totals, Score { black: 4, white: 5 });
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(game.board.get(attempt), Cell::Gray);
        assert_eq!(game.no_stone_streak, 0);
        assert!(
            game.recorder
                .as_ref()
                .unwrap()
                .events()
                .iter()
                .any(|event| {
                    event.kind
                        == ReplayEventKind::RoundTimedOut {
                            resolution: crate::rules::ResolutionKind::KoGray,
                            submitted: [true, false],
                            captures: [0, 0],
                            cause: crate::replay::TimeoutCause::RoundTime,
                        }
                })
        );
    }

    #[test]
    fn sequential_capture_of_new_stone_records_and_round_trips_exact_delta() {
        let mut game = GameState::default();
        for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
            game.board.set(point, Cell::White);
        }
        begin_active_match(&mut game);
        game.cursors = [Point::new(1, 1), Point::new(1, 2)];
        game.elapsed = MatchInstant::from_secs_f64(1.0);
        game.round_elapsed = MatchDuration::from_secs_f64(1.0);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        game.elapsed = MatchInstant::from_secs_f64(1.2);
        game.round_elapsed = MatchDuration::from_secs_f64(1.2);
        try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.2));

        assert_eq!(game.board.get(Point::new(1, 1)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(1, 2)), Cell::White);
        assert_eq!(game.capture_totals, Score { black: 0, white: 1 });
        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();
        let json = replay.to_storage_json().unwrap();
        let decoded = Replay::from_json(&json).unwrap();
        assert!(decoded.events.iter().any(|event| {
            event.kind
                == (ReplayEventKind::RoundResolved {
                    resolution: crate::rules::ResolutionKind::Sequential,
                    captures: [0, 1],
                })
        }));
    }

    #[test]
    fn repetition_never_adds_tentative_capture_bonus() {
        let mut game = active_game(&MatchSettings::default());
        game.board.set(Point::new(1, 1), Cell::White);
        for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
            game.board.set(point, Cell::Black);
        }
        let repeated = game
            .board
            .apply_move(Player::One, Point::new(1, 2))
            .unwrap()
            .board;
        game.completed_history = HashSet::from([repeated, game.board.clone()]);
        game.capture_totals.black = 4;
        game.locks[0] = Some(LockedMove {
            player: Player::One,
            point: Point::new(1, 2),
            at: MatchInstant::from_secs_f64(1.0),
        });

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);

        assert_eq!(game.capture_totals.black, 4);
        assert_eq!(game.board.get(Point::new(1, 1)), Cell::White);
        assert_eq!(game.board.get(Point::new(1, 2)), Cell::Gray);
        assert_eq!(game.no_stone_streak, 0);
    }

    #[test]
    fn no_stone_streak_increments_only_when_the_board_is_exactly_unchanged() {
        let mut game = active_game(&MatchSettings::default());

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.no_stone_streak, 1);
        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.no_stone_streak, 2);

        game.locks[0] = Some(LockedMove {
            player: Player::One,
            point: Point::new(0, 0),
            at: MatchInstant::from_secs_f64(1.0),
        });
        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(game.no_stone_streak, 0);
    }

    #[test]
    fn no_stone_streak_resets_on_ordinary_capture_and_on_suicide() {
        // A capture changes the board even though the acting stone remains.
        let mut game = active_game(&MatchSettings::default());
        game.no_stone_streak = 3;
        game.board.set(Point::new(1, 1), Cell::White);
        game.board.set(Point::new(0, 1), Cell::Black);
        game.board.set(Point::new(1, 0), Cell::Black);
        game.board.set(Point::new(2, 1), Cell::Black);
        game.cursors[player_index(Player::One)] = Point::new(1, 2);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.board.get(Point::new(1, 1)), Cell::Empty);
        assert_eq!(game.no_stone_streak, 0);

        // Suicide that self-removes a group containing pre-existing friendly
        // stones also changes the board, distinct from repetition rollback
        // (this group is more than just the newly placed stone, so the
        // result matches no earlier board and repetition never triggers).
        let mut game = GameState::with_match_rules(MatchRules::snapshot(&MatchSettings::default()));
        game.board.set(Point::new(0, 0), Cell::Black);
        game.board.set(Point::new(1, 0), Cell::Black);
        game.board.set(Point::new(0, 1), Cell::White);
        game.board.set(Point::new(2, 0), Cell::White);
        game.board.set(Point::new(2, 1), Cell::White);
        game.board.set(Point::new(1, 2), Cell::White);
        begin_active_match(&mut game);
        game.no_stone_streak = 4;
        game.cursors[player_index(Player::One)] = Point::new(1, 1);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(1, 0)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(1, 1)), Cell::Empty);
        assert_eq!(game.no_stone_streak, 0);
    }

    #[test]
    fn no_stone_streak_resets_on_a_cleanup_removal() {
        let mut game = GameState::with_match_rules(MatchRules::snapshot(&MatchSettings::default()));
        game.board.set(Point::new(0, 0), Cell::White);
        game.board.set(Point::new(1, 0), Cell::Black);
        game.board.set(Point::new(0, 1), Cell::Black);
        begin_active_match(&mut game);
        game.no_stone_streak = 5;
        game.cursors[player_index(Player::One)] = Point::new(1, 0);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(1, 0)), Cell::Black);
        assert_eq!(game.no_stone_streak, 0);
    }

    #[test]
    fn no_stone_streak_resets_on_positional_repetition_gray() {
        let mut game = GameState::with_match_rules(MatchRules::snapshot(&MatchSettings::default()));
        begin_active_match(&mut game);
        let mut already_seen = game.board.clone();
        already_seen.set(Point::new(4, 4), Cell::Black);
        game.completed_history.insert(already_seen);
        game.no_stone_streak = 2;
        game.cursors[player_index(Player::One)] = Point::new(4, 4);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.board.get(Point::new(4, 4)), Cell::Gray);
        assert_eq!(game.no_stone_streak, 0);
    }

    #[test]
    fn simultaneous_post_round_limits_have_canonical_order() {
        let mut settings = MatchSettings::default();
        for (field, value) in [
            (MatchSettingField::RoundLimit, 1),
            (MatchSettingField::NoStoneRoundLimit, 1),
            (MatchSettingField::PointTarget, 1),
        ] {
            settings.set_value(field, value);
            settings.set_enabled(field, true);
        }
        let mut game = GameState::default();
        game.board.set(Point::new(0, 0), Cell::Black);
        game.completed_history = HashSet::from([game.board.clone()]);
        game.match_rules = MatchRules::snapshot(&settings);
        begin_active_match(&mut game);

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);

        assert_eq!(
            game.end_reasons,
            vec![
                EndReason::RoundLimit,
                EndReason::NoStoneRoundLimit,
                EndReason::PointTarget
            ]
        );
        assert_eq!(game.phase, MatchPhase::Ended);
    }

    #[test]
    fn round_and_no_stone_limits_trigger_only_at_the_exact_completed_boundary() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundLimit, 2);
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        settings.set_value(MatchSettingField::NoStoneRoundLimit, 2);
        settings.set_enabled(MatchSettingField::NoStoneRoundLimit, true);
        let mut game = active_game(&settings);

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.phase, MatchPhase::Playing);
        assert_eq!(game.round, 2);
        assert_eq!(game.no_stone_streak, 1);

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(
            game.end_reasons,
            vec![EndReason::RoundLimit, EndReason::NoStoneRoundLimit]
        );
    }

    #[test]
    fn capture_points_affect_progressive_target_and_survive_territory_fill() {
        let mut settings = settings_with(MatchSettingField::PointTarget, 5);
        settings.set_capture_points(true);
        settings.set_value(MatchSettingField::RoundTime, 1);
        settings.set_enabled(MatchSettingField::RoundTime, true);
        let mut game = GameState::with_match_rules(MatchRules::snapshot(&settings));
        game.board.set(Point::new(1, 1), Cell::White);
        for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
            game.board.set(point, Cell::Black);
        }
        begin_active_match(&mut game);
        game.cursors[0] = Point::new(1, 2);
        game.elapsed = MatchInstant::from_secs_f64(0.5);
        game.round_elapsed = MatchDuration::from_secs_f64(0.5);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(0.5));
        game.elapsed = MatchInstant::from_secs_f64(1.0);
        game.round_elapsed = MatchDuration::from_secs_f64(1.0);

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);

        assert_eq!(game.capture_totals.black, 1);
        assert!(game.end_reasons.contains(&EndReason::PointTarget));
        assert_eq!(game.phase, MatchPhase::Ended);
        assert_eq!(
            game.current_score(),
            game.board.score_with_captures(game.capture_totals, true)
        );
        let replay = game.pending_replay.as_ref().unwrap();
        assert_eq!(replay.score, game.current_score());
        replay.validate().unwrap();
    }

    #[test]
    fn disabled_capture_bonus_does_not_trigger_point_target() {
        let settings = settings_with(MatchSettingField::PointTarget, 5);
        let mut game = active_game(&settings);
        game.board.set(Point::new(1, 1), Cell::White);
        for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
            game.board.set(point, Cell::Black);
        }
        game.completed_history = HashSet::from([game.board.clone()]);
        game.locks[0] = Some(LockedMove {
            player: Player::One,
            point: Point::new(1, 2),
            at: MatchInstant::from_secs_f64(1.0),
        });

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);

        assert_eq!(game.capture_totals.black, 1);
        assert_eq!(game.current_score().black, 4);
        assert_eq!(game.phase, MatchPhase::Playing);
        assert!(game.end_reasons.is_empty());
    }

    #[test]
    fn disabled_capture_points_preserve_area_only_final_score() {
        let mut game = active_game(&MatchSettings::default());
        for y in 0..crate::rules::BOARD_SIZE {
            game.board.set(Point::new(4, y), Cell::Gray);
        }
        game.board.set(Point::new(0, 0), Cell::Black);
        game.capture_totals = Score { black: 7, white: 4 };

        let replay = finish_match(&mut game, vec![EndReason::Manual]).unwrap();

        assert_eq!(game.current_score(), game.board.score());
        assert_eq!(replay.score, game.current_score());
        assert_eq!(replay.events.last().unwrap().frame.capture_totals.black, 7);
    }

    #[test]
    fn full_position_history_keeps_old_boards_beyond_replay_event_limit() {
        let mut game = active_game(&MatchSettings::default());
        let repeated = game
            .board
            .apply_move(Player::One, Point::new(0, 0))
            .unwrap()
            .board;
        game.completed_history.insert(repeated.clone());
        for mut value in 0..=crate::replay::MAX_REPLAY_EVENTS {
            let mut board = Board::default();
            for cell_index in 0..8 {
                let cell = match value % 4 {
                    0 => Cell::Empty,
                    1 => Cell::Black,
                    2 => Cell::White,
                    _ => Cell::Gray,
                };
                board.set(Point::new(cell_index, 0), cell);
                value /= 4;
            }
            board.set(Point::new(8, 8), Cell::Gray);
            game.completed_history.insert(board);
        }
        assert!(game.completed_history.len() > crate::replay::MAX_REPLAY_EVENTS);
        game.locks[0] = Some(LockedMove {
            player: Player::One,
            point: Point::new(0, 0),
            at: MatchInstant::from_secs_f64(1.0),
        });

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);

        assert!(game.completed_history.contains(&repeated));
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Gray);
    }

    #[test]
    fn repeated_no_placement_boards_do_not_grow_position_history() {
        let mut game = active_game(&MatchSettings::default());
        let baseline_count = game.completed_history.len();

        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);
        complete_ordinary_round(&mut game, OrdinaryCompletion::RoundTimeout);

        assert_eq!(game.completed_history.len(), baseline_count);
    }
}
