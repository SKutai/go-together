//! The input-release gate plus the translation of semantic actions into
//! cursor moves, teleport markers and hidden selections.
//!
//! # The release gate
//!
//! [`InputGate`] exists so that an input which was *already held* when a match
//! (re)starts cannot immediately act. [`update_input_gate_state`] gates
//! actions on its own only behind a modal - screen entry arms the gate through
//! [`super::initialize_match_state`], and a completed restart through
//! `holds::advance_restart_hold` - and it releases each action separately,
//! the frame that action is no longer held.
//!
//! Gating one action at a time is the whole point: the gate is shared by both
//! players, and a single flag meant one player resting a finger on Select
//! froze the other player as well. It also removes every special case the flag
//! needed. `RestartMatch` is one action OR'd across the keyboard and both
//! assigned controllers, so it releases on its own whatever else is held;
//! after the Match has ended, the actions that can no longer act gate nobody,
//! because nothing reads them.
//!
//! # Selection and round resolution
//!
//! Round 0 collects a rolling window of three private points per player. Its
//! closing countdown remains interactive, and duplicate-own selections emit a
//! distinct cue without changing the window. After reveal, an ordinary
//! selection is *hidden*: it can be freely replaced until the opponent also
//! selects. Replacing it with a *different* point is a new decision and takes
//! a new timestamp; pressing Select again on the point already held is not, so
//! it keeps the original timestamp and records no Replay event, there being no
//! change to record.
//! A selection targets either an empty point (an ordinary placement)
//! or one of the selecting player's own stones eligible for active cleanup
//! (see [`crate::rules::Board::apply_cleanup`]); [`crate::rules::validate_lock`]
//! accepts both, and which one a selection actually is - and whether a
//! cleanup stays valid - is decided only when the round resolves, exactly
//! like suicide.
//! When both players hold an ordinary selection the round is resolved by
//! [`crate::rules::resolve_round`], whose result decides ordinary, sequential, simultaneous
//! or gray outcomes; see [`crate::rules`] for that logic. Occupied or reserved
//! selections are silently ignored - the live board deliberately shows no
//! error text, since any message could leak the opponent's hidden move.

use bevy::prelude::*;

use super::{
    GameState, GameplayPause, InputGate, MatchPhase, OrdinaryCompletion, complete_ordinary_round,
    player_index, record_game_event,
};
use crate::audio::{AudioCueQueue, GameCue};
use crate::authority::{LockOutcome, OpeningSelectOutcome};
use crate::clock::MatchInstant;
use crate::input::{GameAction, SemanticInput};
use crate::opening::OpeningCountdownStage;
use crate::replay::ReplayEventKind;
use crate::rules::Player;

pub(super) fn update_input_gate(
    semantic: Res<SemanticInput>,
    pause: Res<GameplayPause>,
    mut input_gate: ResMut<InputGate>,
) {
    update_input_gate_state(&semantic, pause.input_blocked, &mut input_gate);
}

/// Gates and releases one action at a time, which is the whole of the gate's
/// per-frame logic.
///
/// While a modal is open every held action is gated, so the press that
/// dismisses the modal cannot also act on the board underneath, and an action
/// pressed behind the modal is gated the frame it goes down. Otherwise each
/// action is released the frame it stops being held, independently of every
/// other - which is what keeps one player's held key off the other player's
/// actions. Neither branch needs [`super::GameState`]: the gate reads no Match
/// phase and no hold progress, so it is the one system in the `MatchUpdate`
/// chain that depends on nothing the Match clock writes.
pub(super) fn update_input_gate_state(
    semantic: &SemanticInput,
    input_blocked: bool,
    input_gate: &mut InputGate,
) {
    if input_blocked {
        input_gate.gate_held(semantic);
    } else {
        input_gate.release_unheld(semantic);
    }
}

pub(super) fn handle_player_input(
    semantic: Res<SemanticInput>,
    pause: Res<GameplayPause>,
    input_gate: Res<InputGate>,
    mut game: ResMut<GameState>,
    mut audio: ResMut<AudioCueQueue>,
) {
    apply_player_input(&semantic, &pause, &input_gate, &mut game, &mut audio);
}

pub(super) fn apply_player_input(
    semantic: &SemanticInput,
    pause: &GameplayPause,
    input_gate: &InputGate,
    game: &mut GameState,
    audio: &mut AudioCueQueue,
) {
    // A modal is a real pause and still stops everything; the gate is not,
    // and is consulted action by action below so that one player's held input
    // never suppresses the other player's.
    if !match_takes_play(game) || game.input_frozen_this_frame || pause.input_blocked {
        return;
    }

    // Gameplay only ever consumes the aggregated semantic booleans, which
    // combine simultaneous keyboard input with whichever controller is
    // assigned to each player. `counts` folds in the gate: a press that is
    // still waiting to be released is not a press.
    let counts = |action| action_just_pressed(semantic, input_gate, action);

    let dx = i32::from(counts(GameAction::PlayerOneRight))
        - i32::from(counts(GameAction::PlayerOneLeft));
    let dy =
        i32::from(counts(GameAction::PlayerOneUp)) - i32::from(counts(GameAction::PlayerOneDown));
    move_cursor(game, Player::One, dx, dy);

    let dx = i32::from(counts(GameAction::PlayerTwoRight))
        - i32::from(counts(GameAction::PlayerTwoLeft));
    let dy =
        i32::from(counts(GameAction::PlayerTwoUp)) - i32::from(counts(GameAction::PlayerTwoDown));
    move_cursor(game, Player::Two, dx, dy);

    if counts(GameAction::PlayerOneSetMarker) {
        store_teleport_marker(game, Player::One);
    }
    if counts(GameAction::PlayerOneJumpToMarker) && jump_to_teleport_marker(game, Player::One) {
        audio.game(GameCue::BlackTeleport);
    }
    if counts(GameAction::PlayerTwoSetMarker) {
        store_teleport_marker(game, Player::Two);
    }
    if counts(GameAction::PlayerTwoJumpToMarker) && jump_to_teleport_marker(game, Player::Two) {
        audio.game(GameCue::WhiteTeleport);
    }

    let now = game.elapsed;
    for (player, action) in [
        (Player::One, GameAction::PlayerOneSelect),
        (Player::Two, GameAction::PlayerTwoSelect),
    ] {
        // Asked again for each Select, not only on entry: the first can
        // resolve a round that ends the Match or parks its end, and the
        // second must then meet the freeze it would meet a frame later.
        if !counts(action) || !match_takes_play(game) {
            continue;
        }
        let cue = match game.phase {
            MatchPhase::Opening | MatchPhase::OpeningCountdown => try_opening_select(game, player),
            MatchPhase::Playing => try_lock(game, player, now),
            MatchPhase::Ended => None,
        };
        if let Some(cue) = cue {
            audio.game(cue);
        }
    }
}

/// Whether the Match takes ordinary play at all: not once it has `Ended`,
/// and not while an automatic end is parked for the end gesture to settle.
///
/// A parked end leaves the Match's outcome genuinely undecided - an automatic
/// end has been reached, but completing the end hold would roll back the
/// round that reached it - so a move made now could belong to a Match that
/// is already over. Both clocks stop for the same reason (see
/// [`super::countdown::advance_match_state_by`]); this stops the players to
/// match, so that nothing at all happens to the Match while it waits.
///
/// Only ordinary play asks this. [`super::holds::handle_hold_gestures`] must
/// not, because the holds are what settle a parked end - a release commits
/// it, a completed end hold rolls it back, a completed Restart drops it - so
/// freezing them as well would leave it parked for good, with both clocks
/// stopped and nothing short of leaving the Match able to end it. That is
/// also why this is a condition of its own rather than
/// `input_frozen_this_frame`, which the holds read too.
fn match_takes_play(game: &GameState) -> bool {
    game.phase != MatchPhase::Ended && !game.provisional_end_pending()
}

/// Whether this action was pressed *this frame* and is not gated.
///
/// An action already held when the gate armed never reads as freshly pressed
/// until it has been released and pressed again, which is what stops a held
/// key acting the instant a modal closes.
pub(crate) fn action_just_pressed(
    semantic: &SemanticInput,
    input_gate: &InputGate,
    action: GameAction,
) -> bool {
    semantic.just_pressed(action) && !input_gate.is_gated(action)
}

/// Whether this action is held *and* ungated - the level-state counterpart of
/// [`action_just_pressed`], used by the hold gestures.
pub(super) fn action_pressed(
    semantic: &SemanticInput,
    input_gate: &InputGate,
    action: GameAction,
) -> bool {
    semantic.pressed(action) && !input_gate.is_gated(action)
}

pub(super) fn move_cursor(game: &mut GameState, player: Player, dx: i32, dy: i32) {
    let Some(moved) = game.state.move_cursor(player, dx, dy) else {
        return;
    };
    game.control_hints_visible[player_index(player)] = false;
    record_game_event(
        game,
        ReplayEventKind::CursorMoved {
            player: moved.player,
            from: moved.from,
            to: moved.to,
        },
    );
}

pub(super) fn store_teleport_marker(game: &mut GameState, player: Player) {
    let point = game.state.set_teleport_marker(player);
    game.control_hints_visible[player_index(player)] = false;
    record_game_event(game, ReplayEventKind::MarkerChanged { player, point });
}

pub(super) fn jump_to_teleport_marker(game: &mut GameState, player: Player) -> bool {
    let Some(jump) = game.state.jump_to_teleport_marker(player) else {
        return false;
    };
    game.control_hints_visible[player_index(player)] = false;
    record_game_event(
        game,
        ReplayEventKind::JumpToMarker {
            player: jump.player,
            from: jump.from,
            to: jump.to,
        },
    );
    record_game_event(
        game,
        ReplayEventKind::MatchSound {
            cue: GameCue::teleport(player),
        },
    );
    true
}

pub(super) fn try_opening_select(game: &mut GameState, player: Player) -> Option<GameCue> {
    match game.state.try_opening_select(player) {
        OpeningSelectOutcome::Inactive | OpeningSelectOutcome::OutOfBounds => None,
        OpeningSelectOutcome::Added {
            point,
            evicted,
            countdown_started,
        } => {
            game.control_hints_visible[player_index(player)] = false;
            record_game_event(
                game,
                ReplayEventKind::OpeningSelectionChanged {
                    player,
                    point,
                    evicted,
                },
            );
            if countdown_started {
                record_game_event(
                    game,
                    ReplayEventKind::OpeningCountdown {
                        stage: OpeningCountdownStage::Three,
                    },
                );
                record_game_event(
                    game,
                    ReplayEventKind::MatchSound {
                        cue: GameCue::Countdown3,
                    },
                );
            }
            countdown_started.then_some(GameCue::Countdown3)
        }
        OpeningSelectOutcome::Duplicate { point } => {
            record_game_event(
                game,
                ReplayEventKind::OpeningSelectionDuplicate { player, point },
            );
            record_game_event(
                game,
                ReplayEventKind::MatchSound {
                    cue: GameCue::OpeningDuplicate,
                },
            );
            Some(GameCue::OpeningDuplicate)
        }
    }
}

pub(super) fn try_lock(game: &mut GameState, player: Player, now: MatchInstant) -> Option<GameCue> {
    match game.state.try_lock(player, now) {
        LockOutcome::Rejected => None,
        // Nothing changed, so there is nothing to record: the selection this
        // Replay already carries is still the current one, and its frame is
        // still the moment the player committed to it. The visible effects
        // are the ones a `Locked` has, so that a re-press stays as silent as
        // every other outcome - the hint is already dismissed by the press
        // that took the lock, which makes this assignment a no-op in
        // practice and never a way to tell the two apart.
        LockOutcome::Unchanged { .. } => {
            game.control_hints_visible[player_index(player)] = false;
            None
        }
        LockOutcome::Locked { point } => {
            game.control_hints_visible[player_index(player)] = false;
            record_game_event(game, ReplayEventKind::SelectionChanged { player, point });
            if game.state.locks_filled() {
                complete_ordinary_round(game, OrdinaryCompletion::Normal)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::clock::MatchDuration;
    use crate::gameplay::begin_round_zero;
    use crate::gameplay::countdown::{advance_match_state_by, match_timer_running};
    use crate::gameplay::test_support::{cleanup_restart_test_root, opening_match_input_app};
    use crate::gameplay::{OpeningAttemptOutcome, PLAYER_STARTS};
    use crate::input::controller::{ControllerAssignments, DeviceFamily};
    use crate::input::keyboard::{KeyboardBindings, KeyboardKey};
    use crate::rules::{BOARD_SIZE, Board, Cell, LockedMove, Point};

    use crate::test_support::advance_test_app;
    use crate::ui::help::keyboard_end_gesture_label;
    use crate::ui::hud::{HudAction, HudState, apply_hud_action, modal_pause_state};
    use crate::ui::match_screen::player_label;
    use bevy::input::ButtonInput;

    #[test]
    fn round_zero_selects_roll_forward_and_countdown_never_restarts() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &crate::match_settings::MatchSettings::default());
        for x in 0..3 {
            game.cursors[0] = Point::new(x, 0);
            assert_eq!(try_opening_select(&mut game, Player::One), None);
        }
        for x in 0..2 {
            game.cursors[1] = Point::new(x, 1);
            assert_eq!(try_opening_select(&mut game, Player::Two), None);
        }
        game.cursors[1] = Point::new(2, 1);
        assert_eq!(
            try_opening_select(&mut game, Player::Two),
            Some(GameCue::Countdown3)
        );
        assert_eq!(game.phase, MatchPhase::OpeningCountdown);

        advance_match_state_by(&mut game, MatchDuration::from_secs_f64(0.4), false);
        game.cursors[0] = Point::new(8, 8);
        assert_eq!(try_opening_select(&mut game, Player::One), None);
        assert_eq!(game.phase, MatchPhase::OpeningCountdown);
        assert_eq!(game.countdown_elapsed, MatchDuration::from_secs_f64(0.4));
        assert_eq!(
            game.opening_selections[0].as_slice(),
            &[Point::new(1, 0), Point::new(2, 0), Point::new(8, 8)]
        );
    }

    #[test]
    fn cross_player_duplicate_is_accepted_but_own_duplicate_clacks_without_change() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &crate::match_settings::MatchSettings::default());
        game.cursors = [Point::new(4, 4), Point::new(4, 4)];

        assert_eq!(try_opening_select(&mut game, Player::One), None);
        assert_eq!(try_opening_select(&mut game, Player::Two), None);
        let before = game.opening_selections[0].clone();
        assert_eq!(
            try_opening_select(&mut game, Player::One),
            Some(GameCue::OpeningDuplicate)
        );
        assert_eq!(game.opening_selections[0], before);
        assert_eq!(game.opening_selections[1].as_slice(), &[Point::new(4, 4)]);
        // The press that queues the clack is the one the dot row's cross
        // is drawn from, in the same frame: the record says so too.
        assert_eq!(
            game.last_opening_attempts
                .map(|attempt| attempt.map(|a| a.outcome)),
            [
                Some(OpeningAttemptOutcome::Duplicate),
                Some(OpeningAttemptOutcome::Added)
            ]
        );
    }

    #[test]
    fn held_semantic_select_emits_one_duplicate_clack_per_press() {
        let keyboard = KeyboardBindings::default();
        let select_key = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let (mut app, root) = opening_match_input_app(keyboard);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(select_key);
        advance_test_app(&mut app, 0.01);
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        advance_test_app(&mut app, 0.01);
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(select_key);
        advance_test_app(&mut app, 0.01);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(select_key);
        advance_test_app(&mut app, 0.01);
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![crate::audio::AudioCue::Game(GameCue::OpeningDuplicate)]
        );
        advance_test_app(&mut app, 0.01);
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
        let game = app.world().resource::<GameState>();
        let mut recorder = game.recorder.clone().unwrap();
        let replay = recorder
            .complete(
                1,
                game.elapsed.as_secs_f64(),
                crate::gameplay::replay_frame(game),
            )
            .unwrap();
        let duplicate_events = replay
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    ReplayEventKind::OpeningSelectionDuplicate { .. }
                )
            })
            .count();
        assert_eq!(duplicate_events, 1);
        cleanup_restart_test_root(root);
    }

    #[test]
    fn keyboard_and_assigned_controller_select_round_zero_through_semantic_input() {
        let keyboard = KeyboardBindings::default();
        let black_select = keyboard.key(GameAction::PlayerOneSelect).key_code();
        let (mut app, root) = opening_match_input_app(keyboard);
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::South);
        let entity = app.world_mut().spawn(gamepad).id();
        app.world_mut()
            .resource_mut::<ControllerAssignments>()
            .connect(entity, DeviceFamily::Unknown, "Opening controller".into());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(black_select);

        advance_test_app(&mut app, 0.01);

        let game = app.world().resource::<GameState>();
        assert_eq!(game.opening_selections[Player::One.index()].len(), 1);
        assert_eq!(game.opening_selections[Player::Two.index()].len(), 1);
        cleanup_restart_test_root(root);
    }

    #[test]
    fn selected_player_can_move_and_replace_hidden_selection() {
        let mut game = GameState::default();
        let first_point = game.cursors[player_index(Player::One)];

        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        move_cursor(&mut game, Player::One, -1, 0);
        let revised_point = game.cursors[player_index(Player::One)];

        assert_ne!(revised_point, first_point);
        assert_eq!(
            game.locks[player_index(Player::One)],
            Some(LockedMove {
                player: Player::One,
                point: first_point,
                at: MatchInstant::from_secs_f64(1.0),
            })
        );

        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.25));

        assert_eq!(
            game.locks[player_index(Player::One)],
            Some(LockedMove {
                player: Player::One,
                point: revised_point,
                at: MatchInstant::from_secs_f64(1.25),
            })
        );
        assert_eq!(player_label(Player::One), "BLACK");
        assert_eq!(game.control_hints_visible, [false, true]);
    }

    /// A re-press on the point already held changes nothing, so it records
    /// nothing: the `SelectionChanged` already in the Replay still names the
    /// current selection, and its frame is still the moment that selection
    /// was made. It also leaves everything the player can see exactly as it
    /// was, which is what keeps it indistinguishable from a rejection.
    #[test]
    fn reselecting_the_held_point_records_no_event_and_changes_nothing_visible() {
        let mut game = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut game);
        let chosen = game.cursors[player_index(Player::One)];

        assert!(try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)).is_none());
        let visible = game.control_hints_visible;

        assert!(try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(2.0)).is_none());

        assert_eq!(
            game.control_hints_visible, visible,
            "a re-press must be invisible to the player"
        );
        assert_eq!(
            game.locks[player_index(Player::One)],
            Some(LockedMove {
                player: Player::One,
                point: chosen,
                at: MatchInstant::from_secs_f64(1.0),
            })
        );

        game.elapsed = MatchInstant::from_secs_f64(3.0);
        let replay =
            crate::gameplay::finish_match(&mut game, vec![crate::replay::EndReason::Manual])
                .expect("an active recording completes");
        assert_eq!(
            replay
                .events
                .iter()
                .filter(|event| matches!(event.kind, ReplayEventKind::SelectionChanged { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn moving_after_selection_does_not_change_locked_point() {
        let mut game = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut game);
        let selected_point = game.cursors[player_index(Player::One)];

        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        move_cursor(&mut game, Player::One, -1, 0);
        let cursor_point = game.cursors[player_index(Player::One)];
        try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.05));

        assert_ne!(cursor_point, selected_point);
        assert_eq!(game.board.get(selected_point), Cell::Black);
        assert_eq!(game.board.get(cursor_point), Cell::Empty);
    }

    #[test]
    fn opponent_selection_resolves_against_latest_reselection() {
        let mut game = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut game);
        let abandoned_point = game.cursors[player_index(Player::One)];

        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        move_cursor(&mut game, Player::One, -1, 0);
        let authoritative_point = game.cursors[player_index(Player::One)];
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.2));
        try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.25));

        assert_eq!(game.board.get(abandoned_point), Cell::Empty);
        assert_eq!(game.board.get(authoritative_point), Cell::Black);
        assert_eq!(game.board.get(PLAYER_STARTS[1]), Cell::White);
        assert_eq!(game.round, 2);
        assert_eq!(game.locks, [None, None]);
    }

    #[test]
    fn try_lock_accepts_and_resolves_a_valid_cleanup_selection() {
        let mut game = GameState::default();
        game.board.set(Point::new(0, 0), Cell::White);
        game.board.set(Point::new(1, 0), Cell::Black);
        game.board.set(Point::new(0, 1), Cell::Black);
        game.cursors[player_index(Player::One)] = Point::new(1, 0);
        crate::gameplay::countdown::begin_active_match(&mut game);

        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        assert_eq!(
            game.locks[player_index(Player::One)],
            Some(LockedMove {
                player: Player::One,
                point: Point::new(1, 0),
                at: MatchInstant::from_secs_f64(1.0),
            })
        );

        assert_eq!(
            try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.3)),
            Some(GameCue::BlackCapture)
        );

        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(1, 0)), Cell::Black);
        assert_eq!(game.board.get(Point::new(0, 1)), Cell::Black);
        assert_eq!(game.round, 2);
        assert_eq!(game.locks, [None, None]);
    }

    #[test]
    fn try_lock_rejects_a_players_own_stone_without_a_capturable_group() {
        let mut game = GameState::default();
        let own_point = game.cursors[player_index(Player::One)];
        game.board.set(own_point, Cell::Black);
        crate::gameplay::countdown::begin_active_match(&mut game);

        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        assert_eq!(game.locks[player_index(Player::One)], None);
        assert_eq!(game.round, 1);
    }

    #[test]
    fn sequential_same_point_guess_is_silently_rejected_and_retryable() {
        let mut game = GameState::with_cursors([Point::new(4, 4), Point::new(4, 4)]);
        crate::gameplay::countdown::begin_active_match(&mut game);

        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        let reserved = Some(LockedMove {
            player: Player::One,
            point: Point::new(4, 4),
            at: MatchInstant::from_secs_f64(1.0),
        });
        assert_eq!(game.locks[player_index(Player::One)], reserved);

        // Two's cursor is already sitting on One's hidden point. Guessing it
        // more than 0.1s later must fail with no observable trace: no cue,
        // no board reveal, and One's own lock untouched.
        assert_eq!(
            try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.2)),
            None
        );
        assert_eq!(game.locks[player_index(Player::Two)], None);
        assert_eq!(game.locks[player_index(Player::One)], reserved);
        assert_eq!(game.board.get(Point::new(4, 4)), Cell::Empty);
        assert_eq!(game.round, 1);

        // The guessing player can only infer the miss because nothing
        // registered; they remain free to move and try another point, which
        // resolves the round normally and clears both locks as usual.
        move_cursor(&mut game, Player::Two, 1, 0);
        assert_eq!(
            try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.3)),
            None
        );
        assert_eq!(game.locks, [None, None]);
        assert_eq!(game.round, 2);
        assert_eq!(game.board.get(Point::new(4, 4)), Cell::Black);
        assert_eq!(game.board.get(Point::new(5, 4)), Cell::White);
    }

    #[test]
    fn teleport_then_reselection_does_not_reveal_the_change() {
        let mut game = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut game);
        let one = player_index(Player::One);
        let two = player_index(Player::Two);
        let original_point = game.cursors[one];

        // Plant a Teleport Marker at a distinct point before locking in the
        // original selection, so jumping to it later actually lands on a
        // different hidden point rather than the one already selected.
        move_cursor(&mut game, Player::One, -1, 0);
        let marker_point = game.cursors[one];
        store_teleport_marker(&mut game, Player::One);
        move_cursor(&mut game, Player::One, 1, 0);
        assert_eq!(game.cursors[one], original_point);

        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        let original_lock = Some(LockedMove {
            player: Player::One,
            point: original_point,
            at: MatchInstant::from_secs_f64(1.0),
        });
        assert_eq!(game.locks[one], original_lock);

        // Opponent state before One does anything else, used below to prove
        // none of it moves as a side effect of One's private replacement.
        let two_snapshot = (
            game.locks[two],
            game.control_hints_visible[two],
            game.cursors[two],
        );

        move_cursor(&mut game, Player::One, 0, 1);
        assert_eq!(game.locks[one], original_lock);
        assert!(jump_to_teleport_marker(&mut game, Player::One));
        assert_eq!(game.cursors[one], marker_point);
        assert_ne!(marker_point, original_point);
        assert_eq!(game.locks[one], original_lock);

        // Replacing the hidden selection with the new, teleported-to point is
        // exactly as silent as any other reselection: no cue, and the
        // opponent's own state is untouched.
        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(2.0)),
            None
        );
        assert_eq!(
            game.locks[one],
            Some(LockedMove {
                player: Player::One,
                point: marker_point,
                at: MatchInstant::from_secs_f64(2.0),
            })
        );
        assert_eq!(
            (
                game.locks[two],
                game.control_hints_visible[two],
                game.cursors[two],
            ),
            two_snapshot
        );
    }

    #[test]
    fn teleport_markers_can_be_stored_replaced_and_jumped_to() {
        let mut game = GameState::default();

        for player in [Player::One, Player::Two] {
            let index = player_index(player);
            let first_marker = game.cursors[index];
            store_teleport_marker(&mut game, player);
            move_cursor(
                &mut game,
                player,
                if player == Player::One { -1 } else { 1 },
                0,
            );
            let replacement = game.cursors[index];
            store_teleport_marker(&mut game, player);
            move_cursor(
                &mut game,
                player,
                0,
                if player == Player::One { -1 } else { 1 },
            );

            assert_ne!(replacement, first_marker);
            assert_ne!(game.cursors[index], replacement);

            jump_to_teleport_marker(&mut game, player);

            assert_eq!(game.teleport_markers[index], Some(replacement));
            assert_eq!(game.cursors[index], replacement);
        }
    }

    #[test]
    fn round_zero_marker_changes_and_jumps_are_recorded() {
        let mut game = GameState::default();
        begin_round_zero(&mut game, &crate::match_settings::MatchSettings::default());
        store_teleport_marker(&mut game, Player::One);
        move_cursor(&mut game, Player::One, -1, 0);
        assert!(jump_to_teleport_marker(&mut game, Player::One));

        let mut recorder = game.recorder.clone().unwrap();
        let replay = recorder
            .complete(
                2,
                game.elapsed.as_secs_f64(),
                crate::gameplay::replay_frame(&game),
            )
            .unwrap();
        assert!(replay.events.iter().any(|event| matches!(
            event.kind,
            ReplayEventKind::MarkerChanged {
                player: Player::One,
                ..
            }
        )));
        assert!(replay.events.iter().any(|event| matches!(
            event.kind,
            ReplayEventKind::JumpToMarker {
                player: Player::One,
                ..
            }
        )));
    }

    #[test]
    fn teleporting_does_not_modify_hidden_selection() {
        let mut game = GameState::default();
        let selected_point = game.cursors[player_index(Player::One)];
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));

        move_cursor(&mut game, Player::One, -1, 0);
        store_teleport_marker(&mut game, Player::One);
        move_cursor(&mut game, Player::One, 0, -1);
        jump_to_teleport_marker(&mut game, Player::One);

        assert_eq!(
            game.locks[player_index(Player::One)],
            Some(LockedMove {
                player: Player::One,
                point: selected_point,
                at: MatchInstant::from_secs_f64(1.0),
            })
        );
    }

    #[test]
    fn teleport_marker_can_be_stored_on_an_occupied_intersection() {
        let mut game = GameState::default();
        let point = game.cursors[player_index(Player::One)];
        game.board.set(point, Cell::White);

        store_teleport_marker(&mut game, Player::One);
        move_cursor(&mut game, Player::One, -1, 0);
        jump_to_teleport_marker(&mut game, Player::One);

        assert_eq!(game.cursors[player_index(Player::One)], point);
        assert_eq!(game.board.get(point), Cell::White);
    }

    #[test]
    fn jumping_without_a_marker_does_not_move_the_cursor() {
        let mut game = GameState::default();
        let original = game.cursors[player_index(Player::Two)];

        jump_to_teleport_marker(&mut game, Player::Two);

        assert_eq!(game.cursors[player_index(Player::Two)], original);
        assert!(game.control_hints_visible[player_index(Player::Two)]);
    }

    #[test]
    fn invalid_selection_is_rejected_and_changes_nothing_visible() {
        let mut game = GameState::default();
        let point = game.cursors[player_index(Player::One)];
        game.board.set(point, Cell::White);

        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));

        assert_eq!(game.locks[player_index(Player::One)], None);
        assert_eq!(game.board.get(point), Cell::White);
        assert!(game.control_hints_visible[player_index(Player::One)]);
    }

    #[test]
    fn immediately_suicidal_empty_selection_is_stored_without_a_cue() {
        let mut game = GameState::default();
        let point = Point::new(4, 4);
        game.cursors[Player::One.index()] = point;
        for neighbor in [
            Point::new(3, 4),
            Point::new(4, 3),
            Point::new(5, 4),
            Point::new(4, 5),
        ] {
            game.board.set(neighbor, Cell::White);
        }
        crate::gameplay::countdown::begin_active_match(&mut game);

        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );

        assert_eq!(
            game.locks[Player::One.index()],
            Some(LockedMove {
                player: Player::One,
                point,
                at: MatchInstant::from_secs_f64(1.0),
            })
        );
    }

    #[test]
    fn hidden_earlier_move_can_make_the_later_accepted_selection_suicidal() {
        let mut game = GameState::default();
        game.board.set(Point::new(1, 0), Cell::White);
        for point in [Point::new(1, 1), Point::new(2, 1), Point::new(3, 0)] {
            game.board.set(point, Cell::Gray);
        }
        game.cursors = [Point::new(0, 0), Point::new(2, 0)];
        crate::gameplay::countdown::begin_active_match(&mut game);

        assert_eq!(
            try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        assert_eq!(
            try_lock(&mut game, Player::Two, MatchInstant::from_secs_f64(1.2)),
            Some(GameCue::BlackCapture)
        );

        assert_eq!(game.round, 2);
        assert_eq!(game.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(game.board.get(Point::new(1, 0)), Cell::Empty);
        assert_eq!(game.board.get(Point::new(2, 0)), Cell::Empty);
        assert_eq!(
            game.capture_totals,
            crate::rules::Score { black: 2, white: 0 }
        );
        assert_eq!(game.locks, [None, None]);
    }

    #[test]
    fn first_meaningful_action_hides_only_that_players_control_hint() {
        let mut moved = GameState::default();
        move_cursor(&mut moved, Player::Two, 1, 0);
        assert_eq!(moved.control_hints_visible, [true, false]);
        moved.cursors[player_index(Player::One)] = Point::new(BOARD_SIZE - 1, 4);
        move_cursor(&mut moved, Player::One, 1, 0);
        assert_eq!(moved.control_hints_visible, [true, false]);
        move_cursor(&mut moved, Player::One, -1, 0);
        assert_eq!(moved.control_hints_visible, [false, false]);

        let mut selected = GameState::default();
        assert_eq!(
            try_lock(&mut selected, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        assert_eq!(selected.control_hints_visible, [false, true]);

        let mut marked = GameState::default();
        store_teleport_marker(&mut marked, Player::Two);
        assert_eq!(marked.control_hints_visible, [true, false]);
    }

    #[test]
    fn non_trigger_actions_leave_both_control_hints_visible() {
        let mut game = GameState::default();
        let pause = GameplayPause::default();
        let gate = InputGate::default();
        let mut audio = AudioCueQueue::default();

        let teleport_without_marker =
            SemanticInput::from_pressed(&[GameAction::PlayerOneJumpToMarker]);
        apply_player_input(
            &teleport_without_marker,
            &pause,
            &gate,
            &mut game,
            &mut audio,
        );

        assert_eq!(game.control_hints_visible, [true, true]);
        assert!(audio.drain().is_empty());
    }

    #[test]
    fn action_wrappers_delegate_to_semantic_input_regardless_of_source() {
        let semantic = SemanticInput::from_pressed(&[GameAction::PlayerOneSelect]);
        let open = InputGate::default();
        // The wrappers cannot distinguish whether the underlying press came
        // from a keyboard key or an assigned controller input - both are
        // folded into the same semantic action before gameplay ever sees it.
        assert!(action_just_pressed(
            &semantic,
            &open,
            GameAction::PlayerOneSelect
        ));
        assert!(action_pressed(
            &semantic,
            &open,
            GameAction::PlayerOneSelect
        ));
        assert!(!action_just_pressed(
            &semantic,
            &open,
            GameAction::PlayerTwoSelect
        ));
        assert!(!action_pressed(
            &semantic,
            &open,
            GameAction::PlayerTwoSelect
        ));

        // Gating is what the wrappers add on top of the snapshot, and it is
        // the only thing that distinguishes them from `SemanticInput`.
        let mut gated = InputGate::default();
        gated.arm();
        assert!(semantic.just_pressed(GameAction::PlayerOneSelect));
        assert!(!action_just_pressed(
            &semantic,
            &gated,
            GameAction::PlayerOneSelect
        ));
        assert!(!action_pressed(
            &semantic,
            &gated,
            GameAction::PlayerOneSelect
        ));
    }

    #[test]
    fn semantic_replay_actions_are_source_independent_for_every_game_action() {
        // Regardless of what combination of keyboard/controller produced a
        // given semantic snapshot, every GameAction consumer only ever reads
        // the aggregated booleans - so replaying the same actions always
        // drives gameplay identically.
        let open = InputGate::default();
        for action in GameAction::ALL {
            let semantic = SemanticInput::from_pressed(&[action]);
            assert!(action_just_pressed(&semantic, &open, action));
            assert!(action_pressed(&semantic, &open, action));
            for other in GameAction::ALL {
                if other != action {
                    assert!(!action_pressed(&semantic, &open, other));
                }
            }
        }
    }

    #[test]
    fn a_modal_pauses_gameplay_and_its_close_gates_only_the_held_inputs() {
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();
        let select_held = SemanticInput::from_pressed(&[GameAction::PlayerOneSelect]);

        apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenHelp);
        let pause = modal_pause_state(&hud);
        assert!(pause.input_blocked);
        assert!(!match_timer_running(
            MatchPhase::Playing,
            pause.match_paused
        ));

        apply_hud_action(&mut hud, &mut input_gate, HudAction::CloseHelp);
        assert!(!hud.help_open);
        let pause = modal_pause_state(&hud);
        // The modal pause is gone, but every action is still gated until the
        // first gate update sees what is actually held - which is what stops
        // the press that closed the modal reaching the board.
        assert!(!pause.input_blocked);
        assert!(input_gate.any_gated());
        // The clock, unlike the board, is shared, so it restarts the moment
        // the modal does - it does not wait on the gate. Anything else would
        // let one held key stop Match time for both players.
        assert!(match_timer_running(MatchPhase::Playing, pause.match_paused));

        // One player is still holding Select. That action alone stays gated;
        // everything else, both players', is released immediately.
        update_input_gate_state(&select_held, pause.input_blocked, &mut input_gate);
        assert!(input_gate.is_gated(GameAction::PlayerOneSelect));
        for action in GameAction::ALL {
            assert_eq!(
                input_gate.is_gated(action),
                action == GameAction::PlayerOneSelect,
                "{action:?} must not be gated by another input being held"
            );
        }

        update_input_gate_state(
            &SemanticInput::default(),
            pause.input_blocked,
            &mut input_gate,
        );
        assert!(!input_gate.any_gated());
        assert!(match_timer_running(MatchPhase::Playing, pause.match_paused));
    }

    #[test]
    fn a_held_action_gates_only_itself_and_only_until_it_is_released() {
        let mut input_gate = InputGate::default();
        let both_held =
            SemanticInput::from_pressed(&[GameAction::PlayerOneSelect, GameAction::RestartMatch]);

        // Screen entry arms every action; the first update keeps only the two
        // that are really down.
        input_gate.arm();
        update_input_gate_state(&both_held, false, &mut input_gate);
        for action in GameAction::ALL {
            assert_eq!(
                input_gate.is_gated(action),
                matches!(
                    action,
                    GameAction::PlayerOneSelect | GameAction::RestartMatch
                )
            );
        }

        // Releasing one releases only that one. `RestartMatch` in particular
        // is shared across the keyboard and both assigned controllers, so it
        // is neither player's to hold open.
        let restart_only = SemanticInput::from_pressed(&[GameAction::RestartMatch]);
        update_input_gate_state(&restart_only, false, &mut input_gate);
        assert!(!input_gate.is_gated(GameAction::PlayerOneSelect));
        assert!(input_gate.is_gated(GameAction::RestartMatch));
        assert!(input_gate.any_gated());

        update_input_gate_state(&SemanticInput::default(), false, &mut input_gate);
        assert!(!input_gate.any_gated());
    }

    #[test]
    fn a_modal_gates_whatever_is_held_while_it_is_open() {
        let mut input_gate = InputGate::default();
        let pressed_behind_the_modal =
            SemanticInput::from_pressed(&[GameAction::PlayerTwoJumpToMarker]);

        // Nothing was held when the modal opened, so nothing is gated yet.
        update_input_gate_state(&SemanticInput::default(), true, &mut input_gate);
        assert!(!input_gate.any_gated());

        // An action pressed behind an open modal is gated where it is, so it
        // cannot act on the board the frame the modal closes.
        update_input_gate_state(&pressed_behind_the_modal, true, &mut input_gate);
        assert!(input_gate.is_gated(GameAction::PlayerTwoJumpToMarker));
        assert!(!input_gate.is_gated(GameAction::PlayerOneJumpToMarker));
    }

    #[test]
    fn one_players_gated_action_leaves_the_other_player_free_to_act() {
        // The bug this gate shape exists to prevent: Player One rests a finger
        // on Select as the Match screen opens, and Player Two cannot move.
        let mut game = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut game);
        let mut audio = AudioCueQueue::default();
        let pause = GameplayPause::default();
        let mut input_gate = InputGate::default();
        let one_select = SemanticInput::from_pressed(&[GameAction::PlayerOneSelect]);
        let both_pressed =
            SemanticInput::from_pressed(&[GameAction::PlayerOneSelect, GameAction::PlayerTwoRight]);

        // Screen entry arms the gate with Player One's Select already down.
        input_gate.arm();
        update_input_gate_state(&one_select, false, &mut input_gate);
        assert!(input_gate.is_gated(GameAction::PlayerOneSelect));

        // Player Two presses Right afterwards. A press made after the gate
        // armed is never gated - outside a modal the gate only ever releases.
        update_input_gate_state(&both_pressed, false, &mut input_gate);
        assert!(!input_gate.is_gated(GameAction::PlayerTwoRight));

        let before = game.cursors;
        apply_player_input(&both_pressed, &pause, &input_gate, &mut game, &mut audio);

        assert_eq!(
            game.cursors[player_index(Player::Two)],
            Point::new(
                before[player_index(Player::Two)].x + 1,
                before[player_index(Player::Two)].y
            ),
            "Player Two's cursor must move while Player One's Select is gated"
        );
        assert_eq!(
            game.cursors[player_index(Player::One)],
            before[player_index(Player::One)]
        );
        assert_eq!(
            game.locks[player_index(Player::One)],
            None,
            "the gated Select must not submit a selection"
        );
    }

    #[test]
    fn round_advances_once_for_sequential_same_point_and_ko_resolutions() {
        let mut sequential = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut sequential);
        try_lock(
            &mut sequential,
            Player::One,
            MatchInstant::from_secs_f64(1.0),
        );
        try_lock(
            &mut sequential,
            Player::Two,
            MatchInstant::from_secs_f64(1.2),
        );
        assert_eq!(sequential.round, 2);

        let mut same_point = GameState::with_cursors([Point::new(4, 4), Point::new(4, 4)]);
        crate::gameplay::countdown::begin_active_match(&mut same_point);
        try_lock(
            &mut same_point,
            Player::One,
            MatchInstant::from_secs_f64(1.0),
        );
        try_lock(
            &mut same_point,
            Player::Two,
            MatchInstant::from_secs_f64(1.05),
        );
        assert_eq!(same_point.round, 2);
        assert_eq!(same_point.board.get(Point::new(4, 4)), Cell::Gray);

        let mut ko = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut ko);
        ko.board.set(Point::new(8, 8), Cell::Black);
        let repeated = ko
            .board
            .apply_move(Player::One, Point::new(0, 0))
            .unwrap()
            .board
            .apply_move(Player::Two, Point::new(8, 0))
            .unwrap()
            .board;
        ko.completed_history = std::collections::HashSet::from([repeated, ko.board.clone()]);
        ko.cursors = [Point::new(0, 0), Point::new(8, 0)];
        try_lock(&mut ko, Player::One, MatchInstant::from_secs_f64(1.0));
        try_lock(&mut ko, Player::Two, MatchInstant::from_secs_f64(1.2));
        assert_eq!(ko.round, 2);
        assert_eq!(ko.board.get(Point::new(0, 0)), Cell::Gray);
        assert_eq!(ko.board.get(Point::new(8, 0)), Cell::Gray);
    }

    #[test]
    fn invalid_reselection_and_marker_actions_do_not_advance_round() {
        let mut game = GameState::default();
        crate::gameplay::countdown::begin_active_match(&mut game);
        let cursor = game.cursors[0];
        game.board.set(cursor, Cell::White);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        assert_eq!(game.round, 1);

        game.board = Board::default();
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.0));
        move_cursor(&mut game, Player::One, -1, 0);
        try_lock(&mut game, Player::One, MatchInstant::from_secs_f64(1.1));
        store_teleport_marker(&mut game, Player::One);
        jump_to_teleport_marker(&mut game, Player::One);
        assert_eq!(game.round, 1);
    }

    #[test]
    fn round_resolution_emits_only_authoritative_capture_and_gray_cues() {
        let mut ordinary = GameState::with_cursors([Point::new(0, 0), Point::new(8, 8)]);
        crate::gameplay::countdown::begin_active_match(&mut ordinary);
        assert_eq!(
            try_lock(&mut ordinary, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        assert_eq!(
            try_lock(&mut ordinary, Player::Two, MatchInstant::from_secs_f64(1.2)),
            None
        );

        let mut both = GameState::default();
        both.board.set(Point::new(1, 1), Cell::White);
        for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
            both.board.set(point, Cell::Black);
        }
        both.board.set(Point::new(7, 7), Cell::Black);
        for point in [Point::new(6, 7), Point::new(7, 6), Point::new(8, 7)] {
            both.board.set(point, Cell::White);
        }
        both.cursors = [Point::new(1, 2), Point::new(7, 8)];
        crate::gameplay::countdown::begin_active_match(&mut both);
        assert_eq!(
            try_lock(&mut both, Player::One, MatchInstant::from_secs_f64(1.0)),
            None
        );
        assert_eq!(
            try_lock(&mut both, Player::Two, MatchInstant::from_secs_f64(1.2)),
            Some(GameCue::BothCapture)
        );

        let mut same_point = GameState::with_cursors([Point::new(4, 4), Point::new(4, 4)]);
        crate::gameplay::countdown::begin_active_match(&mut same_point);
        assert_eq!(
            try_lock(
                &mut same_point,
                Player::One,
                MatchInstant::from_secs_f64(1.0)
            ),
            None
        );
        assert_eq!(
            try_lock(
                &mut same_point,
                Player::Two,
                MatchInstant::from_secs_f64(1.05)
            ),
            Some(GameCue::SamePointGray)
        );
        assert_eq!(
            GameCue::from_round_sound(crate::rules::RoundSound {
                black_capture: true,
                white_capture: true,
                gray_capture: true,
                same_point_gray: false,
            }),
            Some(GameCue::GrayCapture)
        );
    }

    #[test]
    fn player_specific_teleport_cues_require_an_actual_jump() {
        let mut game = GameState::default();
        assert!(!jump_to_teleport_marker(&mut game, Player::One));
        store_teleport_marker(&mut game, Player::One);
        assert!(!jump_to_teleport_marker(&mut game, Player::One));
        move_cursor(&mut game, Player::One, -1, 0);
        assert!(jump_to_teleport_marker(&mut game, Player::One));
        assert_eq!(GameCue::teleport(Player::One), GameCue::BlackTeleport);
        assert_eq!(GameCue::teleport(Player::Two), GameCue::WhiteTeleport);
    }

    #[test]
    fn action_consumption_and_keyboard_end_gesture_follow_current_bindings() {
        let mut bindings = KeyboardBindings::default();
        bindings.assign(GameAction::PlayerOneRight, KeyboardKey::F3);
        bindings.assign(GameAction::PlayerOneSelect, KeyboardKey::F4);
        bindings.assign(GameAction::PlayerTwoSelect, KeyboardKey::F5);
        let semantic = SemanticInput::from_pressed(&[GameAction::PlayerOneRight]);

        let open = InputGate::default();

        assert!(action_just_pressed(
            &semantic,
            &open,
            GameAction::PlayerOneRight
        ));
        assert!(!action_just_pressed(
            &semantic,
            &open,
            GameAction::PlayerOneLeft
        ));
        assert_eq!(bindings.key(GameAction::PlayerOneSelect), KeyboardKey::F4);
        assert_eq!(bindings.key(GameAction::PlayerTwoSelect), KeyboardKey::F5);
        assert_eq!(keyboard_end_gesture_label(&bindings), "F4 + F5");
    }

    /// An `Ended` Match takes no more play. Nothing else would refuse it:
    /// the authoritative cursor and Teleport Marker moves carry no phase
    /// guard of their own, so this is `match_takes_play`'s other half, pinned
    /// here because no test failed when it was removed.
    #[test]
    fn an_ended_match_takes_no_more_play() {
        let mut game = GameState::default();
        let before = (game.cursors, game.teleport_markers);
        game.phase = MatchPhase::Ended;
        let semantic = SemanticInput::from_pressed(&[
            GameAction::PlayerOneRight,
            GameAction::PlayerTwoSetMarker,
            GameAction::PlayerOneSelect,
        ]);
        let mut audio = AudioCueQueue::default();

        apply_player_input(
            &semantic,
            &GameplayPause::default(),
            &InputGate::default(),
            &mut game,
            &mut audio,
        );

        assert_eq!((game.cursors, game.teleport_markers), before);
        assert_eq!(game.locks, [None, None]);
        assert!(audio.drain().is_empty());
    }
}
