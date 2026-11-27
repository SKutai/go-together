//! The authoritative state itself - [`MatchState`] and the [`MatchProgress`]
//! it separates out for rollback - and the command methods that move it.
//!
//! This is where a player's intention becomes an authoritative change:
//! [`MatchState::begin_round_zero`], [`MatchState::move_cursor`],
//! [`MatchState::set_teleport_marker`],
//! [`MatchState::jump_to_teleport_marker`],
//! [`MatchState::start_opening_countdown`],
//! [`MatchState::try_opening_select`], [`MatchState::reveal_round_zero`],
//! [`MatchState::try_lock`], [`MatchState::resolve_ordinary_round`], and
//! [`MatchState::finish`], each answering in [`super::outcomes`]'
//! vocabulary. Time is the one command that does not live here: it is
//! [`super::timing`], which adds [`MatchState::advance_clock`] to the same
//! type.
//!
//! [`super`]'s module docs carry the design these methods implement - the
//! commands-and-outcomes contract, the caller-supplied identity and time a
//! future networked host would build on, the rollback contract
//! [`MatchProgress`] is the mechanism for, and why
//! [`MatchProgress::locks`] is `pub(crate)`.

use std::collections::HashSet;

use crate::clock::{MatchDuration, MatchInstant};
use crate::match_settings::{MatchSettingField, MatchSettings};
use crate::opening::OpeningSelections;
use crate::replay::EndReason;
use crate::rules::{Board, LockedMove, Player, Point, Score, resolve_round};

use super::outcomes::{CursorMove, LockOutcome, OpeningSelectOutcome, RoundOutcome, TeleportJump};
use super::values::{
    MatchPhase, MatchRules, OpeningAttempt, OpeningAttemptOutcome, OrdinaryCompletion,
    PLAYER_STARTS, RoundRemovals, player_index,
};

/// The part of [`MatchState`] that changes only as the result of moves and
/// round resolution: the board, hidden selections, positional-repetition
/// history, round number, capture totals, the last resolution's removals,
/// the no-board-change streak, and official end reasons. The Round Time
/// clock is here for a related reason:
/// the Match clock advances it, but every resolution resets it, so it belongs
/// to the round a rollback restores and is restored with it.
///
/// The score is deliberately *not* stored here. It is derived on demand by
/// [`MatchState::current_score`] from the board and the capture totals, both
/// of which live in this struct, so there is exactly one way to obtain it and
/// no way for a stored copy to drift from the position it describes.
///
/// This is exactly what the Bevy client's *completed* end-gesture hold
/// rolls back - a hold that is released early instead *discards* its
/// snapshot, so the incidental presses that happened during it stand. The
/// snapshot clones one `MatchProgress`, so restoring it is a single
/// assignment that can never omit a newly added field the way a
/// hand-written list of fields to restore could. Cursors, Teleport Markers,
/// Round 0 selections, the immutable [`MatchRules`] snapshot, the phase, and
/// the Match and closing-countdown clocks live outside this struct precisely
/// because they must *not* roll back: real time keeps passing - and a
/// player's cursor stays wherever they left it - for as long as the hold
/// actually takes.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchProgress {
    /// The Match's current position.
    pub board: Board,
    /// Each player's hidden selection. Deliberately restricted: a general
    /// public accessor exposing both players' hidden choices would defeat
    /// simultaneous play. [`crate::view`] releases each entry's point only
    /// to its owner, as `PlayerMatchView::own_selected_point` - whose doc
    /// explains why even the owner must not see it surfaced while the
    /// round is unresolved.
    pub(crate) locks: [Option<LockedMove>; 2],
    /// Every distinct completed board in this Match, used for full positional
    /// repetition without a rule-changing age cutoff.
    pub completed_history: HashSet<Board>,
    /// `0` throughout Round 0, `1` from the reveal, then one more for every
    /// ordinary round that resolves.
    pub round: usize,
    /// Capture credits earned by Black and White. These include ordinary
    /// opponent captures and stones self-removed by the other player.
    pub capture_totals: Score,
    /// Consecutive completed ordinary rounds that left the board exactly as
    /// it was. The field and its persisted Replay name predate this rule and
    /// are kept for schema compatibility; the rule itself now tracks board
    /// changes, not just new placements.
    pub no_stone_streak: usize,
    /// Time spent in the current ordinary round, advanced by
    /// [`MatchState::advance_clock`] and reset by the reveal and by every
    /// resolution. The one clock inside this struct; see above for why.
    pub round_elapsed: MatchDuration,
    /// Empty until [`MatchState::finish`] records the official reasons this
    /// Match ended.
    pub end_reasons: Vec<EndReason>,
    /// What the most recent resolution took off the board - the reveal
    /// sweep's or the last ordinary round's - for the renderer and for any
    /// reader to announce. Public; see [`RoundRemovals`].
    pub last_removals: RoundRemovals,
}

impl Default for MatchProgress {
    fn default() -> Self {
        let board = Board::default();
        Self {
            board: board.clone(),
            locks: [None, None],
            completed_history: HashSet::from([board]),
            round: 0,
            capture_totals: Score::default(),
            no_stone_streak: 0,
            round_elapsed: MatchDuration::ZERO,
            end_reasons: Vec::new(),
            last_removals: RoundRemovals::default(),
        }
    }
}

/// The authoritative, deterministic state of a Match in progress.
///
/// See the module documentation for the design this separates from.
#[derive(Clone, Debug)]
pub struct MatchState {
    /// Everything a *completed* end-gesture hold rolls back. See
    /// [`MatchProgress`].
    pub progress: MatchProgress,
    /// Each player's cursor, indexed by [`player_index`] and always on the
    /// board.
    pub cursors: [Point; 2],
    /// Each player's Teleport Marker, indexed by [`player_index`], or `None`
    /// until they set one. Hidden data: [`crate::view`] hands a marker only
    /// to its owner, and the shared-screen renderer draws neither during
    /// live play.
    pub teleport_markers: [Option<Point>; 2],
    /// Ordered private Round 0 choices. Live UI may read only their lengths;
    /// Replay snapshots intentionally expose the points to spectators.
    pub opening_selections: [OpeningSelections; 2],
    /// What each player's most recent Round 0 selection did, indexed by
    /// [`player_index`], or `None` before their first. Written by
    /// [`Self::try_opening_select`] and cleared with the selections at the
    /// reveal; public, and carries no point - see [`OpeningAttempt`].
    pub last_opening_attempts: [Option<OpeningAttempt>; 2],
    /// Where the Match is in its life cycle. The phase-sensitive commands -
    /// [`Self::try_opening_select`], [`Self::start_opening_countdown`],
    /// [`Self::resolve_ordinary_round`], [`Self::finish`], and
    /// [`Self::advance_clock`] - read it first and do nothing outside the
    /// phases they apply to.
    pub phase: MatchPhase,
    /// The immutable rules this Match is played under, copied from the
    /// preset by [`Self::begin_round_zero`].
    pub match_rules: MatchRules,
    /// Time since the closing countdown started, running on through the Go
    /// card. Crate-private on purpose: [`Self::opening_countdown_stage`] and
    /// [`Self::opening_go_remaining`] are its readers, so nothing outside
    /// re-derives the stage thresholds.
    pub(crate) countdown_elapsed: MatchDuration,
    /// The Match clock: time since Round 0 began. [`Self::advance_clock`]
    /// advances it, capped at what a Replay can persist, and it never rolls
    /// back with `progress`.
    pub elapsed: MatchInstant,
}

impl std::ops::Deref for MatchState {
    type Target = MatchProgress;

    fn deref(&self) -> &MatchProgress {
        &self.progress
    }
}

impl std::ops::DerefMut for MatchState {
    fn deref_mut(&mut self) -> &mut MatchProgress {
        &mut self.progress
    }
}

impl Default for MatchState {
    fn default() -> Self {
        Self {
            progress: MatchProgress::default(),
            cursors: PLAYER_STARTS,
            teleport_markers: [None, None],
            opening_selections: Default::default(),
            last_opening_attempts: [None, None],
            phase: MatchPhase::Opening,
            match_rules: MatchRules::default(),
            countdown_elapsed: MatchDuration::ZERO,
            elapsed: MatchInstant::ZERO,
        }
    }
}

impl MatchState {
    /// The live score: the board's own score plus, when the Match's Capture
    /// Points rule is on, [`MatchProgress::capture_totals`]. Derived on every
    /// call - see [`MatchProgress`] for why it is never stored.
    pub fn current_score(&self) -> Score {
        self.progress.board.score_with_captures(
            self.progress.capture_totals,
            self.match_rules.capture_points(),
        )
    }

    /// Whole seconds left in the current ordinary round, rounded up the way
    /// a HUD shows them; `None` unless the Match is `Playing` with Round Time
    /// enabled. A display reader only - the deadline itself is decided in
    /// [`Self::advance_clock`].
    pub fn round_time_remaining_seconds(&self) -> Option<u64> {
        self.round_time_left()
            .map(|(remaining, _)| remaining.as_secs_ceil())
    }

    /// The share of the current ordinary round's Round Time still to run:
    /// `1.0` as a round starts, `0.0` at its deadline. `Some` exactly when
    /// [`Self::round_time_remaining_seconds`] is, because both read the same
    /// subtraction, so a shape drawn from this can never show a clock the
    /// numeric readout would deny. Also a display reader only - it sizes the
    /// round-time bar the Bevy client draws below the board - which is why
    /// it may answer in `f32`: no authoritative decision reads it.
    pub fn round_time_remaining_fraction(&self) -> Option<f32> {
        self.round_time_left().map(|(remaining, limit)| {
            (remaining.as_secs_f64() / limit.as_secs_f64()).clamp(0.0, 1.0) as f32
        })
    }

    /// `(remaining, limit)` for a running round clock, or `None` unless the
    /// Match is `Playing` with Round Time enabled. The display-side
    /// subtraction behind the two readers above; `limit` is never zero
    /// because Round Time's domain starts at one second.
    fn round_time_left(&self) -> Option<(MatchDuration, MatchDuration)> {
        (self.phase == MatchPhase::Playing
            && self.match_rules.enabled(MatchSettingField::RoundTime))
        .then(|| {
            let limit = MatchDuration::from_secs(u64::from(
                self.match_rules.value(MatchSettingField::RoundTime),
            ));
            (limit.saturating_sub(self.progress.round_elapsed), limit)
        })
    }

    /// Starts the interactive opening at elapsed zero. Recorder creation and
    /// the `OpeningStarted` Replay event are the caller's responsibility.
    ///
    /// This resets the rules snapshot, the phase, the round number, all three
    /// clocks, the capture totals, the last resolution's removals, the
    /// streak, and the end reasons - and nothing else. The board, cursors,
    /// Teleport Markers, Round 0 selections and the record of the last of
    /// them, hidden selections, and repetition history are left exactly as
    /// they were, so a second Match
    /// starts from `MatchState::default()` rather than from this method on a
    /// used state. The Bevy client's `gameplay::reset_match` does precisely
    /// that.
    pub fn begin_round_zero(&mut self, settings: &MatchSettings) {
        self.match_rules = MatchRules::snapshot(settings);
        self.phase = MatchPhase::Opening;
        self.progress.round = 0;
        self.elapsed = MatchInstant::ZERO;
        self.progress.round_elapsed = MatchDuration::ZERO;
        self.progress.capture_totals = Score::default();
        self.progress.last_removals = RoundRemovals::default();
        self.progress.no_stone_streak = 0;
        self.progress.end_reasons.clear();
        self.countdown_elapsed = MatchDuration::ZERO;
    }

    /// Moves a player's cursor, clamped to the board.
    pub fn move_cursor(&mut self, player: Player, dx: i32, dy: i32) -> Option<CursorMove> {
        if dx == 0 && dy == 0 {
            return None;
        }
        let index = player_index(player);
        let from = self.cursors[index];
        let cursor = &mut self.cursors[index];
        cursor.x = (cursor.x as i32 + dx).clamp(0, crate::rules::BOARD_SIZE as i32 - 1) as usize;
        cursor.y = (cursor.y as i32 + dy).clamp(0, crate::rules::BOARD_SIZE as i32 - 1) as usize;
        let to = *cursor;
        (from != to).then_some(CursorMove { player, from, to })
    }

    /// Marks the player's current cursor point as their Teleport Marker,
    /// replacing any earlier one, and returns it. Recording the event is the
    /// caller's responsibility.
    pub fn set_teleport_marker(&mut self, player: Player) -> Point {
        let index = player_index(player);
        let point = self.cursors[index];
        self.teleport_markers[index] = Some(point);
        point
    }

    /// Moves the player's cursor to their Teleport Marker. `None` when no
    /// marker is set, and when the cursor is already on it.
    pub fn jump_to_teleport_marker(&mut self, player: Player) -> Option<TeleportJump> {
        let index = player_index(player);
        let marker = self.teleport_markers[index]?;
        let from = self.cursors[index];
        if from == marker {
            return None;
        }
        self.cursors[index] = marker;
        Some(TeleportJump {
            player,
            from,
            to: marker,
        })
    }

    /// Starts the closing countdown once both players hold three Round 0
    /// selections. Recording the countdown event is the caller's
    /// responsibility.
    pub fn start_opening_countdown(&mut self) -> bool {
        if self.phase != MatchPhase::Opening
            || !self
                .opening_selections
                .iter()
                .all(OpeningSelections::is_full)
        {
            return false;
        }
        self.phase = MatchPhase::OpeningCountdown;
        self.countdown_elapsed = MatchDuration::ZERO;
        true
    }

    /// Adds the player's cursor point to their Round 0 rolling window,
    /// evicting the oldest of three. Filling the second window starts the
    /// closing countdown, which the outcome's `countdown_started` reports, so
    /// the caller need not check for it. `Inactive` outside `Opening` and
    /// `OpeningCountdown`; recording the event is the caller's
    /// responsibility.
    ///
    /// An addition or a duplicate also replaces the player's entry in
    /// [`Self::last_opening_attempts`], which is how the live dot row and
    /// any reader learn what the selection did without being told the
    /// point. An inactive or out-of-bounds attempt never reached the window
    /// and leaves it alone.
    pub fn try_opening_select(&mut self, player: Player) -> OpeningSelectOutcome {
        if !matches!(
            self.phase,
            MatchPhase::Opening | MatchPhase::OpeningCountdown
        ) {
            return OpeningSelectOutcome::Inactive;
        }
        let index = player_index(player);
        let point = self.cursors[index];
        let (outcome, attempt) = match self.opening_selections[index].add(point) {
            crate::opening::OpeningSelectionOutcome::Added { evicted } => (
                OpeningSelectOutcome::Added {
                    point,
                    evicted,
                    countdown_started: self.start_opening_countdown(),
                },
                if evicted.is_some() {
                    OpeningAttemptOutcome::RolledForward
                } else {
                    OpeningAttemptOutcome::Added
                },
            ),
            crate::opening::OpeningSelectionOutcome::Duplicate => (
                OpeningSelectOutcome::Duplicate { point },
                OpeningAttemptOutcome::Duplicate,
            ),
            crate::opening::OpeningSelectionOutcome::OutOfBounds => {
                return OpeningSelectOutcome::OutOfBounds;
            }
        };
        let number =
            self.last_opening_attempts[index].map_or(1, |last| last.number.wrapping_add(1));
        self.last_opening_attempts[index] = Some(OpeningAttempt {
            number,
            outcome: attempt,
        });
        outcome
    }

    /// Reveals the six Round 0 slots and sweeps the completed board for
    /// zero-liberty groups. Recording the reveal event is the caller's
    /// responsibility.
    pub fn reveal_round_zero(&mut self) {
        let revealed = crate::opening::reveal_opening(&self.opening_selections);
        let sweep = revealed.resolve_opening_captures();
        self.progress.board = sweep.board;
        self.opening_selections = Default::default();
        self.last_opening_attempts = [None, None];
        self.progress.completed_history = HashSet::from([self.progress.board.clone()]);
        self.progress.locks = [None, None];
        // Recorded as Round 0's, before the round counter moves on: the
        // swept stones were on no board a player ever saw, and this is
        // the only record that they stood there at all.
        self.progress.last_removals = RoundRemovals {
            round: self.progress.round,
            stones: sweep.removed,
        };
        self.progress.round = 1;
        self.progress.round_elapsed = MatchDuration::ZERO;
        self.progress.capture_totals = Score {
            black: sweep.capture_credits[Player::One.index()],
            white: sweep.capture_credits[Player::Two.index()],
        };
        self.progress.no_stone_streak = 0;
        self.phase = MatchPhase::Playing;
    }

    /// Attempts to lock in one hidden ordinary selection at the player's
    /// current cursor.
    ///
    /// # When `at` is written
    ///
    /// The stored timestamp records when the player *chose* that point, not
    /// when they last pressed Select. Selecting a point different from the
    /// one this player already holds is a new decision and takes `now`;
    /// re-selecting the point they already hold is not, so the existing
    /// [`LockedMove`] is left exactly as it is - `at` included - and the
    /// answer is [`LockOutcome::Unchanged`]. Revising a selection stays
    /// completely free; the clock is the one thing a repeated press cannot
    /// move.
    ///
    /// This matters because `at` is what both timing rules measure from:
    /// [`crate::rules::validate_lock`] ages a same-point reservation against
    /// it, and [`crate::rules::SIMULTANEOUS_WINDOW`] decides the
    /// sequential/simultaneous split by it. Were a re-press to refresh it,
    /// pressing Select repeatedly would hold a player's commit time at
    /// "now": a round they would have led would land inside the window and
    /// resolve simultaneously instead, and their own selection would never
    /// age past the window, so an opponent's attempt on the point they hold
    /// would be taken as a same-point tie rather than rejected as reserved.
    /// The press would then be a lever on the resolution rules rather than a
    /// statement of intent.
    ///
    /// # Silence
    ///
    /// Every outcome here is deliberately indistinguishable to the player,
    /// `Unchanged` included. A rejection produces no message, sound, or
    /// marker precisely so that a player cannot tell an accepted selection
    /// from one the opponent had already reserved; give any single outcome
    /// its own feedback and the *absence* of that feedback becomes a readout
    /// of the opponent's hidden point.
    ///
    /// Recording the selection event for a [`LockOutcome::Locked`], and
    /// completing the round once both selections are filled, are the
    /// caller's responsibility (see [`Self::locks_filled`] and
    /// [`Self::resolve_ordinary_round`]).
    pub fn try_lock(&mut self, player: Player, now: MatchInstant) -> LockOutcome {
        let index = player_index(player);
        let other_index = 1 - index;
        if self.progress.locks[index].is_some() && self.progress.locks[other_index].is_some() {
            return LockOutcome::Rejected;
        }
        let point = self.cursors[index];
        match crate::rules::validate_lock(
            &self.progress.board,
            player,
            point,
            self.progress.locks[other_index],
            now,
        ) {
            Ok(()) => match self.progress.locks[index] {
                // Already holding this exact point: leave the stored
                // LockedMove alone rather than rewriting it with the same
                // player and point and a fresher `at`.
                Some(held) if held.point == point => LockOutcome::Unchanged { point },
                _ => {
                    self.progress.locks[index] = Some(LockedMove {
                        player,
                        point,
                        at: now,
                    });
                    LockOutcome::Locked { point }
                }
            },
            Err(_) => LockOutcome::Rejected,
        }
    }

    /// Whether both players hold a hidden ordinary selection - the caller's
    /// cue to call [`Self::resolve_ordinary_round`].
    pub fn locks_filled(&self) -> bool {
        self.progress.locks.iter().all(Option::is_some)
    }

    /// Resolves the pending hidden selections, updates every counter, and
    /// evaluates automatic limits. `None` means nothing completed - either
    /// the Match is not `Playing`, or `completion` was `Normal` and the
    /// selections could not be resolved (locks are still cleared).
    ///
    /// Recording the round event, queuing the cue, and finishing the Match
    /// when `end_reasons` is non-empty are the caller's responsibility.
    pub fn resolve_ordinary_round(
        &mut self,
        completion: OrdinaryCompletion,
    ) -> Option<RoundOutcome> {
        if self.phase != MatchPhase::Playing {
            return None;
        }
        let submitted = self.progress.locks.map(|selection| selection.is_some());
        let resolved = match resolve_round(
            &self.progress.board,
            self.progress.locks,
            &self.progress.completed_history,
        ) {
            Ok(resolved) => resolved,
            Err(_) if completion != OrdinaryCompletion::Normal => resolve_round(
                &self.progress.board,
                [None, None],
                &self.progress.completed_history,
            )
            .expect("an empty round always resolves"),
            Err(_) => {
                self.progress.locks = [None, None];
                return None;
            }
        };
        self.apply_round_resolution(&resolved);
        self.progress.round = self.progress.round.saturating_add(1);
        self.progress.round_elapsed = MatchDuration::ZERO;
        self.progress.locks = [None, None];

        let mut end_reasons = Vec::new();
        if completion == OrdinaryCompletion::MatchTimeTimeout {
            end_reasons.push(EndReason::MatchTimeLimit);
        }
        end_reasons.extend(self.evaluate_post_round_limits());

        Some(RoundOutcome {
            kind: resolved.kind,
            capture_credits: resolved.capture_credits,
            submitted,
            sound: resolved.sound,
            end_reasons,
            removed: resolved.removed,
        })
    }

    fn apply_round_resolution(&mut self, resolved: &crate::rules::RoundResolution) {
        // The streak measures rounds that leave the board exactly as it was:
        // a placement, an ordinary capture, a suicide removal, a cleanup
        // removal, and a repetition Gray conversion are all just different
        // ways the board can change, so one direct comparison covers all of
        // them.
        let board_unchanged = self.progress.board == resolved.board;
        self.progress.board = resolved.board.clone();
        // Written on every resolution, removals or none, so the record is
        // always the last round's and never an older round's left standing.
        // `round` is still the round that just resolved here; the caller
        // advances it afterwards.
        self.progress.last_removals = RoundRemovals {
            round: self.progress.round,
            stones: resolved.removed.clone(),
        };
        self.progress.capture_totals.black = self
            .progress
            .capture_totals
            .black
            .saturating_add(resolved.capture_credits[0]);
        self.progress.capture_totals.white = self
            .progress
            .capture_totals
            .white
            .saturating_add(resolved.capture_credits[1]);
        if board_unchanged {
            self.progress.no_stone_streak = self.progress.no_stone_streak.saturating_add(1);
        } else {
            self.progress.no_stone_streak = 0;
        }
        self.progress
            .completed_history
            .insert(self.progress.board.clone());
    }

    fn evaluate_post_round_limits(&self) -> Vec<EndReason> {
        let mut reasons = Vec::new();
        let completed_rounds = self.progress.round.saturating_sub(1);
        if self.match_rules.enabled(MatchSettingField::RoundLimit)
            && completed_rounds
                >= usize::from(self.match_rules.value(MatchSettingField::RoundLimit))
        {
            reasons.push(EndReason::RoundLimit);
        }
        if self
            .match_rules
            .enabled(MatchSettingField::NoStoneRoundLimit)
            && self.progress.no_stone_streak
                >= usize::from(self.match_rules.value(MatchSettingField::NoStoneRoundLimit))
        {
            reasons.push(EndReason::NoStoneRoundLimit);
        }
        if self.match_rules.enabled(MatchSettingField::PointTarget) {
            let score = self.current_score();
            let target = usize::from(self.match_rules.value(MatchSettingField::PointTarget));
            if score.black >= target || score.white >= target {
                reasons.push(EndReason::PointTarget);
            }
        }
        reasons
    }

    /// Fills territory and marks the Match `Ended`. Filling first is what
    /// makes the board [`MatchState::current_score`] reads from the final
    /// one, and the same board later recorded and replayed. Returns `false`
    /// when there was nothing to finish (not `Playing`); completing the
    /// Replay recording is the caller's responsibility.
    pub fn finish(&mut self, end_reasons: Vec<EndReason>) -> bool {
        if self.phase != MatchPhase::Playing {
            return false;
        }
        self.progress.board.fill_territory();
        self.phase = MatchPhase::Ended;
        self.progress.locks = [None, None];
        self.progress.end_reasons = end_reasons;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::test_support::*;
    use crate::rules::{Cell, ResolutionKind};

    #[test]
    fn match_state_is_usable_without_a_bevy_app() {
        let mut state = MatchState::default();
        assert_eq!(state.phase, MatchPhase::Opening);

        let start = state.cursors[player_index(Player::One)];
        let moved = state.move_cursor(Player::One, 1, 0).unwrap();
        assert_eq!(moved.from, start);
        assert_ne!(moved.from, moved.to);

        let marker = state.set_teleport_marker(Player::Two);
        assert_eq!(marker, state.cursors[player_index(Player::Two)]);
        state.move_cursor(Player::Two, 1, 0);
        let jump = state.jump_to_teleport_marker(Player::Two).unwrap();
        assert_eq!(jump.to, marker);
    }

    /// Each player's last Round 0 selection is recorded on its own, with
    /// what it did; a selection that never reached the window is not one.
    #[test]
    fn round_zero_records_what_each_players_last_selection_did() {
        let attempt = |number, outcome| Some(OpeningAttempt { number, outcome });
        let mut state = MatchState::default();
        let mut select = |player: Player, x: usize, y: usize| {
            state.cursors[player_index(player)] = Point::new(x, y);
            state.try_opening_select(player);
            state.last_opening_attempts
        };

        assert_eq!(
            select(Player::One, 0, 0),
            [attempt(1, OpeningAttemptOutcome::Added), None]
        );
        assert_eq!(
            select(Player::One, 0, 0),
            [attempt(2, OpeningAttemptOutcome::Duplicate), None]
        );
        select(Player::One, 1, 0);
        select(Player::One, 2, 0);
        assert_eq!(
            select(Player::One, 3, 0)[0],
            attempt(5, OpeningAttemptOutcome::RolledForward)
        );
        assert_eq!(
            select(Player::Two, 3, 0),
            [
                attempt(5, OpeningAttemptOutcome::RolledForward),
                attempt(1, OpeningAttemptOutcome::Added),
            ],
            "the other player's record is their own, whatever point it names"
        );

        state.last_opening_attempts[0] = attempt(u32::MAX, OpeningAttemptOutcome::Added);
        state.cursors[0] = Point::new(3, 0);
        state.try_opening_select(Player::One);
        assert_eq!(
            state.last_opening_attempts[0],
            attempt(0, OpeningAttemptOutcome::Duplicate),
            "the count wraps rather than overflowing"
        );

        state.phase = MatchPhase::Playing;
        let before = state.last_opening_attempts;
        assert!(matches!(
            state.try_opening_select(Player::One),
            OpeningSelectOutcome::Inactive
        ));
        assert_eq!(state.last_opening_attempts, before);
    }

    #[test]
    fn try_lock_and_resolve_ordinary_round_produce_a_round_outcome() {
        let mut state = active_state();
        state.cursors = [Point::new(0, 0), Point::new(8, 8)];

        assert!(matches!(
            state.try_lock(Player::One, MatchInstant::from_secs_f64(1.0)),
            LockOutcome::Locked { .. }
        ));
        assert!(matches!(
            state.try_lock(Player::Two, MatchInstant::from_secs_f64(1.1)),
            LockOutcome::Locked { .. }
        ));
        assert!(state.locks_filled());

        let outcome = state
            .resolve_ordinary_round(OrdinaryCompletion::Normal)
            .expect("both selections resolve");
        assert_eq!(outcome.submitted, [true, true]);
        assert_eq!(state.round, 2);
        assert_eq!(state.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(state.board.get(Point::new(8, 8)), Cell::White);
    }

    /// A press is a statement of intent, not a lever on the clock. Pressing
    /// Select on the point a player already holds keeps the timestamp their
    /// first press wrote; moving and selecting elsewhere is a genuine new
    /// decision and takes the new one.
    #[test]
    fn try_lock_retimes_a_new_point_and_leaves_an_unchanged_one_alone() {
        let mut state = active_state();
        let chosen = state.cursors[player_index(Player::One)];

        assert!(matches!(
            state.try_lock(Player::One, MatchInstant::from_secs_f64(1.0)),
            LockOutcome::Locked { point } if point == chosen
        ));
        assert!(matches!(
            state.try_lock(Player::One, MatchInstant::from_secs_f64(5.0)),
            LockOutcome::Unchanged { point } if point == chosen
        ));
        assert_eq!(
            state.progress.locks[player_index(Player::One)],
            Some(LockedMove {
                player: Player::One,
                point: chosen,
                at: MatchInstant::from_secs_f64(1.0),
            })
        );

        state.move_cursor(Player::One, 1, 0);
        let revised = state.cursors[player_index(Player::One)];
        assert_ne!(revised, chosen);
        assert!(matches!(
            state.try_lock(Player::One, MatchInstant::from_secs_f64(9.0)),
            LockOutcome::Locked { point } if point == revised
        ));
        assert_eq!(
            state.progress.locks[player_index(Player::One)],
            Some(LockedMove {
                player: Player::One,
                point: revised,
                at: MatchInstant::from_secs_f64(9.0),
            })
        );
    }

    /// The difference the preserved timestamp makes, stated as the two
    /// outcomes it chooses between. Black chooses at 1.0 and presses Select
    /// again on the same point at 5.0; White chooses at 5.05. Measured from
    /// the press that actually chose the point the round is sequential;
    /// measured from a timestamp the repeated press had refreshed it falls
    /// inside [`crate::rules::SIMULTANEOUS_WINDOW`], so the press alone would
    /// have decided how the round resolves.
    #[test]
    fn a_repeated_press_cannot_pull_a_sequential_round_into_the_window() {
        let mut preserved = active_state();
        preserved.cursors = [Point::new(0, 0), Point::new(8, 8)];

        assert!(matches!(
            preserved.try_lock(Player::One, MatchInstant::from_secs_f64(1.0)),
            LockOutcome::Locked { .. }
        ));
        assert!(matches!(
            preserved.try_lock(Player::One, MatchInstant::from_secs_f64(5.0)),
            LockOutcome::Unchanged { .. }
        ));

        // The same round, differing only in the timestamp a repeated press
        // used to write.
        let mut refreshed = preserved.clone();
        refreshed.progress.locks[player_index(Player::One)] = Some(LockedMove {
            player: Player::One,
            point: Point::new(0, 0),
            at: MatchInstant::from_secs_f64(5.0),
        });

        for (state, expected) in [
            (&mut preserved, ResolutionKind::Sequential),
            (&mut refreshed, ResolutionKind::Simultaneous),
        ] {
            assert!(matches!(
                state.try_lock(Player::Two, MatchInstant::from_secs_f64(5.05)),
                LockOutcome::Locked { .. }
            ));
            let outcome = state
                .resolve_ordinary_round(OrdinaryCompletion::Normal)
                .expect("both selections resolve");
            assert_eq!(outcome.kind, expected);
        }
    }

    /// The same rule from the opponent's side: Black's reservation ages from
    /// the press that made it, so Black cannot press their way back inside
    /// the window and turn White's later attempt on that point from a silent
    /// rejection into a same-point tie.
    #[test]
    fn a_repeated_press_does_not_reopen_a_same_point_reservation() {
        let mut state = active_state();
        state.cursors = [Point::new(4, 4), Point::new(4, 4)];

        assert!(matches!(
            state.try_lock(Player::One, MatchInstant::from_secs_f64(1.0)),
            LockOutcome::Locked { .. }
        ));
        assert!(matches!(
            state.try_lock(Player::One, MatchInstant::from_secs_f64(5.0)),
            LockOutcome::Unchanged { .. }
        ));
        assert!(matches!(
            state.try_lock(Player::Two, MatchInstant::from_secs_f64(5.05)),
            LockOutcome::Rejected
        ));
        assert!(state.progress.locks[player_index(Player::Two)].is_none());
    }

    /// Locks in the rollback contract described on [`MatchProgress`]: cloning
    /// and restoring it undoes a round's moves exactly, while the clocks it
    /// deliberately excludes are left alone by the same operation.
    #[test]
    fn match_progress_clone_restores_exactly_what_moves_changed() {
        let mut state = active_state();
        state.cursors = [Point::new(1, 1), Point::new(8, 8)];
        let snapshot = state.progress.clone();

        state.try_lock(Player::One, MatchInstant::from_secs_f64(1.0));
        state.try_lock(Player::Two, MatchInstant::from_secs_f64(1.1));
        state.resolve_ordinary_round(OrdinaryCompletion::Normal);
        assert_ne!(state.progress, snapshot);
        assert_eq!(state.round, 2);

        state.elapsed = MatchInstant::from_secs(5);
        state.progress = snapshot.clone();

        assert_eq!(state.progress, snapshot);
        assert_eq!(state.round, 1);
        assert_eq!(
            state.elapsed,
            MatchInstant::from_secs(5),
            "clocks live outside MatchProgress and must not roll back with it"
        );
    }

    /// The two display readers of the round clock answer together: the
    /// fraction is `Some` exactly when the whole-second countdown is, and it
    /// runs from full to empty over the round rather than stepping with the
    /// rounded-up seconds.
    #[test]
    fn round_time_fraction_and_seconds_agree_on_when_a_round_clock_is_running() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundTime, 60);
        settings.set_enabled(MatchSettingField::RoundTime, true);

        let mut state = MatchState::default();
        state.begin_round_zero(&settings);
        assert_eq!(state.round_time_remaining_fraction(), None);
        assert_eq!(state.round_time_remaining_seconds(), None);

        state.phase = MatchPhase::Playing;
        assert_eq!(state.round_time_remaining_fraction(), Some(1.0));
        assert_eq!(state.round_time_remaining_seconds(), Some(60));

        state.progress.round_elapsed = MatchDuration::from_secs_f64(45.0);
        assert_eq!(state.round_time_remaining_fraction(), Some(0.25));
        assert_eq!(state.round_time_remaining_seconds(), Some(15));

        state.progress.round_elapsed = MatchDuration::from_secs_f64(59.5);
        let fraction = state.round_time_remaining_fraction().unwrap();
        assert!((fraction - 0.5 / 60.0).abs() < 1e-6);
        assert_eq!(state.round_time_remaining_seconds(), Some(1));

        state.progress.round_elapsed = MatchDuration::from_secs_f64(75.0);
        assert_eq!(
            state.round_time_remaining_fraction(),
            Some(0.0),
            "past the deadline the subtraction saturates rather than going negative"
        );
        assert_eq!(state.round_time_remaining_seconds(), Some(0));

        state.phase = MatchPhase::Ended;
        assert_eq!(state.round_time_remaining_fraction(), None);
        assert_eq!(state.round_time_remaining_seconds(), None);

        let mut untimed = MatchState::default();
        untimed.begin_round_zero(&MatchSettings::default());
        untimed.phase = MatchPhase::Playing;
        assert_eq!(untimed.round_time_remaining_fraction(), None);
        assert_eq!(untimed.round_time_remaining_seconds(), None);
    }
}
