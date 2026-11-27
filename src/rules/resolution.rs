//! Sequential and simultaneous round resolution: combining zero, one, or two
//! hidden selections - each an ordinary placement or an active cleanup, see
//! [`super::board`] - into one completed round using the timing, Gray, and
//! positional-repetition rules.
//!
//! # Simultaneous moves
//!
//! Both players normally select in secret; a round can also be completed by a
//! timeout with zero or one selections. [`SIMULTANEOUS_WINDOW`] is the entire
//! timing model when both players selected, compared as exact
//! [`crate::clock::MatchInstant`] microseconds rather than `f64` seconds, so
//! the boundary never depends on floating-point rounding:
//!
//! * more than the window apart - the moves are *sequential*. The earlier move
//!   is played first, including any self-removal, and the later move is applied
//!   to the resulting board;
//! * within the window - the moves are *simultaneous*. Both are evaluated from
//!   the same starting board and merged, so neither player is punished for
//!   being milliseconds slower.
//!
//! The two timestamps are the moments each player *chose* the point they
//! hold, not the moments they last pressed Select. Moving and selecting a
//! different point is a new choice and re-times the selection; re-selecting
//! the point already held is not, and keeps the original timestamp
//! (`MatchState::try_lock` in `crate::authority` decides this). Pressing
//! Select repeatedly therefore cannot hold a player's commit time at "now",
//! which would otherwise let a round that should resolve sequentially fall
//! inside the window - taking any same-point reservation with it, so that
//! the opponent's later attempt on the reserved point would be accepted into
//! a tie instead of rejected.
//!
//! # Gray stones
//!
//! Gray is how the rules resolve a genuine tie without favouring either player.
//! It appears when both players pick the *same* point simultaneously and when
//! the ko rule rejects a round. Gray stones are permanent neutral walls: they
//! belong to nobody, never connect to a player's group, cannot be captured and
//! own no territory. They count as occupied when liberties are evaluated and
//! provide no liberties to an adjacent group, exactly like the edge of the
//! board.
//!
//! # Suicide, capture and ko
//!
//! [`crate::rules::Board::apply_move`] removes the opponent's dead groups
//! first, then removes the acting player's entire connected group if it is
//! left without liberties. This ordering lets a capturing placement gain
//! liberties and survive. Positional repetition is enforced against every
//! completed board in the current Match, including boards produced by
//! self-removal.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::clock::{MatchDuration, MatchInstant};

use super::board::{Board, CleanupOutcome, MoveError, MoveOutcome};
use super::values::{BOARD_SIZE, Cell, Player, Point, RemovedStone};

/// Maximum timestamp difference that still resolves from one shared board.
pub const SIMULTANEOUS_WINDOW: MatchDuration = MatchDuration::from_millis(100);

/// One player's hidden point and the Match-clock time it was selected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LockedMove {
    pub player: Player,
    pub point: Point,
    pub at: MatchInstant,
}

/// Why a hidden selection cannot currently be accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockError {
    /// The point is off the board, holds a Gray or opposing stone, or holds
    /// the selecting player's own stone that currently borders no opposing
    /// zero-liberty group, so it is not an eligible cleanup either.
    Occupied,
    /// The other player locked this same point more than
    /// [`SIMULTANEOUS_WINDOW`] earlier, so it is sequentially reserved for
    /// them.
    Reserved,
}

/// Persisted classification of how a completed round resolved.
///
/// Serialised by `snake_case` name in every Replay round event, so renaming
/// a variant changes the on-disk format.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionKind {
    /// No selection changed the board: none was locked, or the only one was
    /// illegal by the time it resolved.
    NoPlacement,
    /// One selection - a placement or a cleanup - applied alone.
    Single,
    /// Both players selected the same empty point within
    /// [`SIMULTANEOUS_WINDOW`]; it became Gray.
    SamePointGray,
    /// Two selections more than [`SIMULTANEOUS_WINDOW`] apart: the earlier
    /// applied first, the later against its result.
    Sequential,
    /// Two selections within [`SIMULTANEOUS_WINDOW`], evaluated from the
    /// same pre-round board and merged.
    Simultaneous,
    /// The candidate board repeated a completed one, so the pre-round board
    /// was restored with the attempted empty points turned Gray. This
    /// replaces whichever kind the round would otherwise have had, for one
    /// or two selections alike.
    KoGray,
}

/// Semantic capture summary used to choose one authoritative sound cue.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RoundSound {
    pub black_capture: bool,
    pub white_capture: bool,
    pub gray_capture: bool,
    pub same_point_gray: bool,
}

impl RoundSound {
    fn record_capture(&mut self, player: Player, captured: bool) {
        if !captured {
            return;
        }
        match player {
            Player::One => self.black_capture = true,
            Player::Two => self.white_capture = true,
        }
    }
}

/// Board, classification, and sound summary produced by a completed round.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundResolution {
    pub board: Board,
    pub kind: ResolutionKind,
    pub sound: RoundSound,
    /// Stones credited as captures to Black and White respectively. This
    /// includes opposing stones captured normally and acting stones
    /// self-removed by the opposing player.
    pub capture_credits: [usize; 2],
    /// Legal attempted points, indexed by player. Repetition can contain the
    /// same point twice; callers must not infer placement count from this.
    pub attempted_valid_points: [Option<Point>; 2],
    /// True when a Black, White, or Gray stone was newly placed during the
    /// completed round, even if that stone was captured later in the round.
    pub placed_stone: bool,
    pub repetition: bool,
    /// Every stone the round took off the board, with its owner, sorted by
    /// cell index: ordinary captures, self-removed acting stones and
    /// cleanup removals alike, including a stone placed and removed within
    /// the same round, which never appeared on any displayed board. Empty
    /// for a repetition round, which restores the pre-round board, and for
    /// the Gray outcomes, which remove nothing.
    ///
    /// One entry per point. A sequential round can empty a point twice:
    /// the later player's selection, a cleanup of their own stone, can find
    /// the earlier action has captured that stone and so become a placement
    /// on the emptied point, and die there as a self-removal. Both stones
    /// were the later player's - only they could have selected a point
    /// holding their stone - so one entry, the earlier removal's, carries
    /// both. The counts in `capture_credits` are not affected: both stones
    /// are credited.
    pub removed: Vec<RemovedStone>,
}

/// A pair of selections that cannot be resolved as a round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionError {
    SamePlayer,
    Reserved,
    IllegalMove(Player, MoveError),
}

/// Validates one hidden selection against the currently visible board.
///
/// A selection is legal onto an empty point (ordinary placement) or onto one
/// of the selecting player's own stones that currently borders at least one
/// opposing zero-liberty group (active cleanup) - plus the existing
/// sequential same-point reservation. Which of the two a legal selection
/// actually is, and whether a cleanup selection stays valid, is decided only
/// when the round resolves: a sequential earlier action can change or
/// invalidate it before then. Suicide, including suicide caused by an
/// earlier hidden move, is likewise decided only at resolution.
///
/// `other_lock`'s timestamp is the moment the opponent *chose* that point,
/// not the moment they last pressed Select, so a reservation ages from the
/// choice and pressing again cannot reset that clock. This function is passed
/// only the opponent's selection; whether the caller is re-selecting a point
/// they already hold - and so keeps its timestamp rather than taking `now` -
/// is `MatchState::try_lock`'s business in `crate::authority`, not this
/// check's.
pub fn validate_lock(
    board: &Board,
    player: Player,
    point: Point,
    other_lock: Option<LockedMove>,
    now: MatchInstant,
) -> Result<(), LockError> {
    if !board.point_is_empty(point) && board.apply_cleanup(player, point).is_err() {
        return Err(LockError::Occupied);
    }

    if let Some(other_lock) = other_lock {
        let sequential = timing_is_sequential(other_lock.at, now);
        if other_lock.point == point && sequential {
            return Err(LockError::Reserved);
        }
    }

    Ok(())
}

/// One resolved hidden selection: an ordinary placement, or an active
/// cleanup that removed adjacent opposing zero-liberty groups without
/// placing or moving a stone.
pub enum SelectionOutcome {
    Placement(MoveOutcome),
    Cleanup(CleanupOutcome),
}

impl SelectionOutcome {
    /// The board after this selection alone was applied - what a sequential
    /// later selection is evaluated against.
    pub fn board(&self) -> &Board {
        match self {
            Self::Placement(outcome) => &outcome.board,
            Self::Cleanup(outcome) => &outcome.board,
        }
    }

    fn into_board(self) -> Board {
        match self {
            Self::Placement(outcome) => outcome.board,
            Self::Cleanup(outcome) => outcome.board,
        }
    }

    fn is_placement(&self) -> bool {
        matches!(self, Self::Placement(_))
    }

    /// Every stone this selection alone removed, with its owner: the
    /// opponent's stones a placement or a cleanup captured, and the
    /// `player`'s own stones a placement self-removed. `at` is the point
    /// the selection acted from, recorded on each stone as what took it.
    fn removed_stones(&self, player: Player, at: Point) -> Vec<RemovedStone> {
        let opponent = player.opponent();
        let stone = |point: &Point, owner| RemovedStone {
            point: *point,
            owner,
            by: Some(at),
        };
        match self {
            Self::Placement(outcome) => outcome
                .opponent_stones_captured
                .iter()
                .map(|point| stone(point, opponent))
                .chain(
                    outcome
                        .acting_stones_self_removed
                        .iter()
                        .map(|point| stone(point, player)),
                )
                .collect(),
            Self::Cleanup(outcome) => outcome
                .captured_stones
                .iter()
                .map(|point| stone(point, opponent))
                .collect(),
        }
    }
}

/// Appends `more` to `removed`, keeping the first entry listed for any point
/// that appears again, then sorts by cell index so the order never depends
/// on how the removals were collected. A point listed twice always held the
/// same colour both times (see [`RoundResolution::removed`]), which the
/// debug assertion pins.
fn merge_removals(removed: &mut Vec<RemovedStone>, more: impl IntoIterator<Item = RemovedStone>) {
    for stone in more {
        match removed.iter().find(|listed| listed.point == stone.point) {
            Some(listed) => debug_assert_eq!(
                listed.owner, stone.owner,
                "a point emptied twice in one round held the same colour both times"
            ),
            None => removed.push(stone),
        }
    }
    removed.sort_by_key(|stone| stone.point.index());
}

/// Applies one hidden selection against `board`: an ordinary placement onto
/// an empty point, or an active cleanup targeting the player's own stone.
/// Which one applies is decided only now, against whichever board is passed
/// in, mirroring [`validate_lock`]'s deferred-legality design.
///
/// Bounds are checked before the target cell is read, exactly as
/// [`Board::apply_move`] and [`Board::apply_cleanup`] do: an off-board point
/// is reported as `MoveError::OutOfBounds` and never reaches either branch.
/// That check has to live here rather than being left to the two of them,
/// because the branch itself is chosen from the cell's contents - an
/// off-board point read through [`Board::get`] would otherwise panic or
/// alias a live cell and pick the wrong action. In-crate callers pass only
/// points [`validate_lock`] or Replay frame validation has already accepted,
/// so the check is a public-API guarantee rather than a path they rely on.
/// A cleanup that cannot apply is reported as `MoveError::Occupied` whatever
/// the underlying `CleanupError`.
pub fn apply_selection(
    board: &Board,
    player: Player,
    point: Point,
) -> Result<SelectionOutcome, MoveError> {
    if point.x >= BOARD_SIZE || point.y >= BOARD_SIZE {
        return Err(MoveError::OutOfBounds);
    }
    if board.get(point) == player.cell() {
        board
            .apply_cleanup(player, point)
            .map(SelectionOutcome::Cleanup)
            .map_err(|_| MoveError::Occupied)
    } else {
        board
            .apply_move(player, point)
            .map(SelectionOutcome::Placement)
    }
}

/// Resolves zero, one, or two hidden selections using the timing, Gray, and
/// positional-repetition rules.
///
/// A selection may be an ordinary placement or an active cleanup; see
/// [`apply_selection`]. Simultaneous actions preview independently from the
/// same starting board; sequential actions apply the earlier one first and
/// evaluate the later one against its result, so a sequential cleanup can be
/// invalidated by the action played before it - it then simply contributes
/// nothing, and the round still advances on the earlier action alone.
/// Repetition is checked after the candidate board exists and converts every
/// unique legal attempted point that was empty before the round to Gray.
pub fn resolve_round(
    board: &Board,
    pending: [Option<LockedMove>; 2],
    completed_history: &HashSet<Board>,
) -> Result<RoundResolution, ResolutionError> {
    let moves = pending.into_iter().flatten().collect::<Vec<_>>();
    if moves.len() == 2 && moves[0].player == moves[1].player {
        return Err(ResolutionError::SamePlayer);
    }

    let mut capture_credits = [0; 2];
    let mut attempted_valid_points = [None; 2];
    let mut removed = Vec::new();
    let (candidate, kind, sound, resolved, stone_placed) = match moves.as_slice() {
        [] => (
            board.clone(),
            ResolutionKind::NoPlacement,
            RoundSound::default(),
            false,
            false,
        ),
        [selection] => match apply_selection(board, selection.player, selection.point) {
            Ok(outcome) => {
                let mut sound = RoundSound::default();
                record_selection_credit(
                    &mut capture_credits,
                    &mut sound,
                    selection.player,
                    &outcome,
                );
                attempted_valid_points[selection.player.index()] = Some(selection.point);
                let stone_placed = outcome.is_placement();
                merge_removals(
                    &mut removed,
                    outcome.removed_stones(selection.player, selection.point),
                );
                (
                    outcome.into_board(),
                    ResolutionKind::Single,
                    sound,
                    true,
                    stone_placed,
                )
            }
            Err(_) => (
                board.clone(),
                ResolutionKind::NoPlacement,
                RoundSound::default(),
                false,
                false,
            ),
        },
        [first, second] => {
            let (early, late) = ordered_moves(*first, *second);
            let sequential = timing_is_sequential(early.at, late.at);
            if early.point == late.point && sequential {
                return Err(ResolutionError::Reserved);
            }

            attempted_valid_points[early.player.index()] = Some(early.point);
            attempted_valid_points[late.player.index()] = Some(late.point);
            if early.point == late.point {
                // Both legal moves reached the same empty point before
                // reservation took effect. A cleanup selection can never
                // collide here: it targets the selecting player's own
                // stone, which the other player could never have also
                // legally selected on the same frozen pre-round board.
                board
                    .validate_placement(early.point)
                    .map_err(|error| ResolutionError::IllegalMove(early.player, error))?;
                board
                    .validate_placement(late.point)
                    .map_err(|error| ResolutionError::IllegalMove(late.player, error))?;
                let mut candidate = board.clone();
                candidate.set(early.point, Cell::Gray);
                (
                    candidate,
                    ResolutionKind::SamePointGray,
                    RoundSound {
                        same_point_gray: true,
                        ..Default::default()
                    },
                    true,
                    true,
                )
            } else if sequential {
                let early_outcome = apply_selection(board, early.player, early.point)
                    .map_err(|error| ResolutionError::IllegalMove(early.player, error))?;
                let mut sound = RoundSound::default();
                record_selection_credit(
                    &mut capture_credits,
                    &mut sound,
                    early.player,
                    &early_outcome,
                );
                let mut stone_placed = early_outcome.is_placement();
                merge_removals(
                    &mut removed,
                    early_outcome.removed_stones(early.player, early.point),
                );

                // The later action is evaluated against the board the
                // earlier one produced, so it may target a different point
                // than intended (nothing here relies on that) or - only for
                // a cleanup - no longer be valid at all. An invalidated
                // cleanup fails silently and contributes nothing; the round
                // still advances on the earlier action alone.
                let candidate =
                    match apply_selection(early_outcome.board(), late.player, late.point) {
                        Ok(late_outcome) => {
                            record_selection_credit(
                                &mut capture_credits,
                                &mut sound,
                                late.player,
                                &late_outcome,
                            );
                            stone_placed |= late_outcome.is_placement();
                            merge_removals(
                                &mut removed,
                                late_outcome.removed_stones(late.player, late.point),
                            );
                            late_outcome.into_board()
                        }
                        Err(_) => early_outcome.into_board(),
                    };

                (
                    candidate,
                    ResolutionKind::Sequential,
                    sound,
                    true,
                    stone_placed,
                )
            } else {
                let simultaneous = resolve_simultaneous(board, early, late)?;
                capture_credits = simultaneous.capture_credits;
                merge_removals(&mut removed, simultaneous.removed);
                (
                    simultaneous.board,
                    ResolutionKind::Simultaneous,
                    simultaneous.sound,
                    true,
                    simultaneous.placed_stone,
                )
            }
        }
        _ => unreachable!("a round has at most two players"),
    };

    if resolved && completed_history.contains(&candidate) {
        let mut repetition_board = board.clone();
        let mut grayed = HashSet::new();
        for point in attempted_valid_points.into_iter().flatten() {
            if board.get(point) == Cell::Empty && grayed.insert(point) {
                repetition_board.set(point, Cell::Gray);
            }
        }
        return Ok(RoundResolution {
            board: repetition_board,
            kind: ResolutionKind::KoGray,
            sound: RoundSound {
                gray_capture: true,
                ..Default::default()
            },
            capture_credits: [0, 0],
            attempted_valid_points,
            placed_stone: !grayed.is_empty(),
            repetition: true,
            removed: Vec::new(),
        });
    }

    Ok(RoundResolution {
        board: candidate,
        kind,
        sound,
        capture_credits,
        attempted_valid_points,
        placed_stone: stone_placed,
        repetition: false,
        removed,
    })
}

/// What two simultaneous selections did between them; see
/// [`resolve_simultaneous`].
struct SimultaneousResolution {
    board: Board,
    capture_credits: [usize; 2],
    sound: RoundSound,
    placed_stone: bool,
    /// The merged removal set with each stone's owner, one entry per
    /// point. Both snapshots see the same pre-round board, so a point they
    /// both remove held the same stone in each, and keeping either entry
    /// is the same answer.
    removed: Vec<RemovedStone>,
}

/// Evaluates both simultaneous selections independently against the same
/// pre-round board and merges their effects with no post-merge liberty
/// reevaluation, so neither action gains priority over the other.
fn resolve_simultaneous(
    board: &Board,
    first: LockedMove,
    second: LockedMove,
) -> Result<SimultaneousResolution, ResolutionError> {
    let first_outcome = apply_selection(board, first.player, first.point)
        .map_err(|error| ResolutionError::IllegalMove(first.player, error))?;
    let second_outcome = apply_selection(board, second.player, second.point)
        .map_err(|error| ResolutionError::IllegalMove(second.player, error))?;

    let mut credited_points = [HashSet::new(), HashSet::new()];
    record_simultaneous_selection_credit(&mut credited_points, first.player, &first_outcome);
    record_simultaneous_selection_credit(&mut credited_points, second.player, &second_outcome);

    let stone_placed = first_outcome.is_placement() || second_outcome.is_placement();

    // Every removal is evaluated from the shared pre-round board. The union
    // deliberately receives no post-merge liberty reevaluation: simultaneous
    // moves remain snapshots rather than becoming an implicit sequence. The
    // two stones are (re-)placed below before this removal loop runs, so a
    // cleanup stone that this same removal set also captures still ends up
    // empty: its own valid cleanup effect - the opposing group going empty -
    // still lands, exactly like the other player's independent capture of it.
    let mut removed_stones = Vec::new();
    merge_removals(
        &mut removed_stones,
        first_outcome.removed_stones(first.player, first.point),
    );
    merge_removals(
        &mut removed_stones,
        second_outcome.removed_stones(second.player, second.point),
    );

    let mut sound = RoundSound::default();
    for player in Player::ALL {
        sound.record_capture(player, !credited_points[player.index()].is_empty());
    }
    let capture_credits = [
        credited_points[Player::One.index()].len(),
        credited_points[Player::Two.index()].len(),
    ];
    let mut candidate = board.clone();
    candidate.set(first.point, first.player.cell());
    candidate.set(second.point, second.player.cell());

    for removed in &removed_stones {
        candidate.set(removed.point, Cell::Empty);
    }

    Ok(SimultaneousResolution {
        board: candidate,
        capture_credits,
        sound,
        placed_stone: stone_placed,
        removed: removed_stones,
    })
}

fn record_selection_credit(
    capture_credits: &mut [usize; 2],
    sound: &mut RoundSound,
    player: Player,
    outcome: &SelectionOutcome,
) {
    match outcome {
        SelectionOutcome::Placement(outcome) => {
            record_move_credits(capture_credits, sound, player, outcome);
        }
        SelectionOutcome::Cleanup(outcome) => {
            record_cleanup_credit(capture_credits, sound, player, outcome);
        }
    }
}

fn record_move_credits(
    capture_credits: &mut [usize; 2],
    sound: &mut RoundSound,
    player: Player,
    outcome: &MoveOutcome,
) {
    debug_assert_eq!(
        outcome.placed_stone_remains,
        outcome.acting_stones_self_removed.is_empty()
    );
    let opponent_captures = outcome.opponent_stones_captured.len();
    let self_removals = outcome.acting_stones_self_removed.len();
    capture_credits[player.index()] =
        capture_credits[player.index()].saturating_add(opponent_captures);
    capture_credits[player.opponent().index()] =
        capture_credits[player.opponent().index()].saturating_add(self_removals);
    sound.record_capture(player, opponent_captures != 0);
    sound.record_capture(player.opponent(), self_removals != 0);
}

/// A cleanup never self-removes its own (unmoved, unplaced) stone, so unlike
/// [`record_move_credits`] only the acting player is ever credited.
fn record_cleanup_credit(
    capture_credits: &mut [usize; 2],
    sound: &mut RoundSound,
    player: Player,
    outcome: &CleanupOutcome,
) {
    let opponent_captures = outcome.captured_stones.len();
    capture_credits[player.index()] =
        capture_credits[player.index()].saturating_add(opponent_captures);
    sound.record_capture(player, opponent_captures != 0);
}

fn record_simultaneous_selection_credit(
    credited_points: &mut [HashSet<Point>; 2],
    player: Player,
    outcome: &SelectionOutcome,
) {
    match outcome {
        SelectionOutcome::Placement(outcome) => {
            record_simultaneous_credits(credited_points, player, outcome);
        }
        SelectionOutcome::Cleanup(outcome) => {
            credited_points[player.index()].extend(outcome.captured_stones.iter().copied());
        }
    }
}

fn record_simultaneous_credits(
    credited_points: &mut [HashSet<Point>; 2],
    player: Player,
    outcome: &MoveOutcome,
) {
    debug_assert_eq!(
        outcome.placed_stone_remains,
        outcome.acting_stones_self_removed.is_empty()
    );
    credited_points[player.index()].extend(outcome.opponent_stones_captured.iter().copied());
    credited_points[player.opponent().index()]
        .extend(outcome.acting_stones_self_removed.iter().copied());
}

/// Orders two locks by Match time, treating Black ([`Player::One`]) as
/// earlier when the timestamps are identical.
///
/// The tie-break depends only on `player`, never on argument position, so
/// `(a, b)` and `(b, a)` always order the same way - otherwise identical
/// Match commands could resolve differently by call order. Identical
/// timestamps are always within [`SIMULTANEOUS_WINDOW`], so the order only
/// decides which lock the simultaneous merge calls `first`.
fn ordered_moves(first: LockedMove, second: LockedMove) -> (LockedMove, LockedMove) {
    if first.at < second.at || (first.at == second.at && first.player == Player::One) {
        (first, second)
    } else {
        (second, first)
    }
}

/// `late` must not be earlier than `early`; a reversed pair (which should
/// never happen for an authoritative clock) saturates to a zero gap rather
/// than reporting a nonsensical negative one, so it reads as simultaneous.
fn timing_is_sequential(early: MatchInstant, late: MatchInstant) -> bool {
    late.saturating_duration_since(early) > SIMULTANEOUS_WINDOW
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::test_support::*;
    use crate::rules::{BOARD_SIZE, CELL_COUNT};

    #[test]
    fn same_point_within_window_becomes_gray() {
        let board = Board::default();
        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 4, 4, 2.0)),
                Some(lock(Player::Two, 4, 4, 2.08)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::SamePointGray);
        assert_eq!(resolved.board.get(Point::new(4, 4)), Cell::Gray);
        assert_eq!(
            resolved.sound,
            RoundSound {
                same_point_gray: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn identical_timestamps_resolve_simultaneously_with_argument_order_independent_tie_breaking() {
        let board = Board::default();
        let same_instant = at(1.5);
        let one = LockedMove {
            player: Player::One,
            point: Point::new(3, 3),
            at: same_instant,
        };
        let two = LockedMove {
            player: Player::Two,
            point: Point::new(5, 5),
            at: same_instant,
        };

        // The tie-break must depend only on `player`, not on which argument
        // position each move happened to arrive in - a symmetric function
        // that produced a different winner for `(one, two)` than for
        // `(two, one)` would make otherwise-identical Match commands resolve
        // differently depending on incidental call order.
        assert_eq!(ordered_moves(one, two), ordered_moves(two, one));
        let (early, late) = ordered_moves(one, two);
        assert_eq!(early.player, Player::One);
        assert_eq!(late.player, Player::Two);

        let resolved = resolve_round(
            &board,
            [Some(one), Some(two)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();
        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
    }

    #[test]
    fn same_point_at_exact_window_boundary_becomes_gray() {
        let board = Board::default();
        let first = lock(Player::One, 4, 4, 1.0);
        let exact_boundary = 1.0 + SIMULTANEOUS_WINDOW.as_secs_f64();

        assert_eq!(
            validate_lock(
                &board,
                Player::Two,
                first.point,
                Some(first),
                at(exact_boundary)
            ),
            Ok(())
        );
        let resolved = resolve_round(
            &board,
            [Some(first), Some(lock(Player::Two, 4, 4, exact_boundary))],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::SamePointGray);
        assert_eq!(resolved.board.get(first.point), Cell::Gray);
    }

    #[test]
    fn different_points_at_exact_window_boundary_are_simultaneous() {
        let board = Board::default();
        let first = lock(Player::One, 3, 3, 1.0);
        let exact_boundary = 1.0 + SIMULTANEOUS_WINDOW.as_secs_f64();

        assert_eq!(
            validate_lock(
                &board,
                Player::Two,
                Point::new(5, 5),
                Some(first),
                at(exact_boundary)
            ),
            Ok(())
        );
        let resolved = resolve_round(
            &board,
            [Some(first), Some(lock(Player::Two, 5, 5, exact_boundary))],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
    }

    #[test]
    fn timestamps_clearly_above_window_are_sequential() {
        let board = Board::default();
        let first = lock(Player::One, 3, 3, 1.0);
        // One microsecond past the window is enough to be unambiguously
        // sequential now that the boundary is an exact integer comparison
        // rather than an `f64` difference needing an epsilon fudge.
        let just_above =
            MatchInstant::from_secs(1) + SIMULTANEOUS_WINDOW + MatchDuration::from_micros(1);

        assert_eq!(
            validate_lock(&board, Player::Two, first.point, Some(first), just_above),
            Err(LockError::Reserved)
        );
        let resolved = resolve_round(
            &board,
            [
                Some(first),
                Some(LockedMove {
                    player: Player::Two,
                    point: Point::new(5, 5),
                    at: just_above,
                }),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Sequential);
    }

    #[test]
    fn reservation_blocks_same_point_after_window() {
        let board = Board::default();
        let first = lock(Player::One, 3, 3, 1.0);

        assert_eq!(
            validate_lock(
                &board,
                Player::Two,
                Point::new(3, 3),
                Some(first),
                at(1.101)
            ),
            Err(LockError::Reserved)
        );
        assert_eq!(
            validate_lock(&board, Player::Two, Point::new(3, 3), Some(first), at(1.09)),
            Ok(())
        );
    }

    #[test]
    fn selection_accepts_immediate_suicide_but_still_rejects_occupied_points() {
        let mut board = Board::default();
        for point in [
            Point::new(0, 1),
            Point::new(1, 0),
            Point::new(2, 1),
            Point::new(1, 2),
        ] {
            board.set(point, Cell::White);
        }

        assert_eq!(
            validate_lock(&board, Player::One, Point::new(1, 1), None, at(1.0)),
            Ok(())
        );
        assert_eq!(
            validate_lock(&board, Player::One, Point::new(0, 1), None, at(1.0)),
            Err(LockError::Occupied)
        );
    }

    #[test]
    fn hidden_earlier_move_can_make_an_accepted_later_move_suicidal() {
        let board = board_where_hidden_black_move_makes_white_suicidal();
        let early = lock(Player::One, 0, 0, 1.0);
        let late = lock(Player::Two, 2, 0, 1.2);
        assert!(
            board
                .apply_move(late.player, late.point)
                .unwrap()
                .placed_stone_remains
        );
        assert_eq!(
            validate_lock(&board, late.player, late.point, Some(early), late.at),
            Ok(())
        );

        let resolved = resolve_round(
            &board,
            [Some(early), Some(late)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Sequential);
        assert_eq!(resolved.board.get(early.point), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(1, 0)), Cell::Empty);
        assert_eq!(resolved.board.get(late.point), Cell::Empty);
        assert_eq!(resolved.capture_credits, [2, 0]);
        assert_eq!(
            resolved.sound,
            RoundSound {
                black_capture: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn sequential_later_move_uses_the_board_after_earlier_self_removal() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(1, 0), Cell::White);
        for point in [
            Point::new(0, 2),
            Point::new(1, 1),
            Point::new(2, 1),
            Point::new(3, 0),
        ] {
            board.set(point, Cell::Gray);
        }
        let early = lock(Player::One, 0, 1, 1.0);
        let late = lock(Player::Two, 2, 0, 1.2);
        let snapshot_late = board.apply_move(late.player, late.point).unwrap();
        assert!(!snapshot_late.placed_stone_remains);
        assert_eq!(snapshot_late.acting_stones_self_removed.len(), 2);

        let resolved = resolve_round(
            &board,
            [Some(early), Some(late)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Sequential);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(resolved.board.get(early.point), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(1, 0)), Cell::White);
        assert_eq!(resolved.board.get(late.point), Cell::White);
        assert_eq!(resolved.capture_credits, [0, 2]);
        assert_eq!(
            resolved.sound,
            RoundSound {
                white_capture: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn simultaneous_moves_keep_snapshot_groups_even_if_the_merge_has_no_liberties() {
        let board = board_where_hidden_black_move_makes_white_suicidal();
        let black = lock(Player::One, 0, 0, 1.0);
        let white = lock(Player::Two, 2, 0, 1.05);

        let resolved = resolve_round(
            &board,
            [Some(black), Some(white)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        assert_eq!(resolved.board.get(black.point), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(1, 0)), Cell::White);
        assert_eq!(resolved.board.get(white.point), Cell::White);
        assert_eq!(resolved.capture_credits, [0, 0]);
    }

    #[test]
    fn simultaneous_snapshot_applies_both_complete_self_removal_sets() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        for point in [Point::new(0, 1), Point::new(1, 1), Point::new(2, 0)] {
            board.set(point, Cell::White);
        }
        board.set(Point::new(8, 8), Cell::White);
        for point in [Point::new(8, 7), Point::new(7, 7), Point::new(6, 8)] {
            board.set(point, Cell::Black);
        }
        let black = lock(Player::One, 1, 0, 1.0);
        let white = lock(Player::Two, 7, 8, 1.05);

        let resolved = resolve_round(
            &board,
            [Some(black), Some(white)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        for point in [Point::new(0, 0), black.point, Point::new(8, 8), white.point] {
            assert_eq!(resolved.board.get(point), Cell::Empty);
        }
        assert_eq!(resolved.capture_credits, [2, 2]);
        assert_eq!(
            resolved.sound,
            RoundSound {
                black_capture: true,
                white_capture: true,
                ..Default::default()
            }
        );
        assert!(resolved.placed_stone);
    }

    #[test]
    fn simultaneous_snapshot_does_not_revive_mutually_self_removed_groups() {
        let mut board = Board::default();
        for point in [
            Point::new(1, 0),
            Point::new(4, 0),
            Point::new(2, 1),
            Point::new(3, 1),
        ] {
            board.set(point, Cell::Black);
        }
        for point in [Point::new(2, 0), Point::new(0, 1), Point::new(1, 1)] {
            board.set(point, Cell::White);
        }
        let black = lock(Player::One, 0, 0, 1.0);
        let white = lock(Player::Two, 3, 0, 1.05);

        let resolved = resolve_round(
            &board,
            [Some(black), Some(white)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        for point in [black.point, Point::new(1, 0), Point::new(2, 0), white.point] {
            assert_eq!(resolved.board.get(point), Cell::Empty);
        }
        assert_eq!(resolved.capture_credits, [2, 2]);
        assert_eq!(
            resolved.sound,
            RoundSound {
                black_capture: true,
                white_capture: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn simultaneous_merge_can_leave_a_zero_liberty_group_on_the_board() {
        let board = merge_created_zero_liberty_group_board();
        let black = lock(Player::One, 0, 1, 1.0);
        let white = lock(Player::Two, 1, 0, 1.05);

        let resolved = resolve_round(
            &board,
            [Some(black), Some(white)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        // Neither snapshot flagged a capture or self-removal, so nothing is
        // credited and no capture cue fires, even though the merged board
        // now holds a zero-liberty group.
        assert_eq!(resolved.capture_credits, [0, 0]);
        assert_eq!(resolved.sound, RoundSound::default());
        // The Black group survives...
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(0, 1)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(1, 0)), Cell::White);
        // ...with genuinely zero liberties: every neighbor of the group is
        // occupied on the final board (White, Gray, or the group itself).
        for point in [Point::new(1, 0), Point::new(0, 2), Point::new(1, 1)] {
            assert_ne!(resolved.board.get(point), Cell::Empty);
        }
    }

    #[test]
    fn merge_survivor_is_removable_by_a_later_cleanup_action() {
        let board = merge_created_zero_liberty_group_board();
        let simultaneous = resolve_round(
            &board,
            [
                Some(lock(Player::One, 0, 1, 1.0)),
                Some(lock(Player::Two, 1, 0, 1.05)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap()
        .board;

        // A later round in which White spends a cleanup action on its own
        // bordering stone removes the whole surviving group at once.
        let cleaned_up = resolve_round(
            &simultaneous,
            [Some(lock(Player::Two, 1, 0, 5.0)), None],
            &HashSet::from([simultaneous.clone()]),
        )
        .unwrap();

        assert_eq!(cleaned_up.kind, ResolutionKind::Single);
        assert_eq!(cleaned_up.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(cleaned_up.board.get(Point::new(0, 1)), Cell::Empty);
        assert_eq!(cleaned_up.board.get(Point::new(1, 0)), Cell::White);
        assert_eq!(cleaned_up.capture_credits, [0, 2]);
    }

    #[test]
    fn mixed_ordinary_capture_and_self_removal_credit_every_removed_stone() {
        let mut board = Board::default();
        board.set(Point::new(1, 1), Cell::White);
        for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
            board.set(point, Cell::Black);
        }
        board.set(Point::new(8, 8), Cell::White);
        for point in [Point::new(8, 7), Point::new(7, 7), Point::new(6, 8)] {
            board.set(point, Cell::Black);
        }

        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 1, 2, 1.0)),
                Some(lock(Player::Two, 7, 8, 1.2)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Sequential);
        assert_eq!(resolved.capture_credits, [3, 0]);
        assert_eq!(resolved.board.get(Point::new(1, 1)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(8, 8)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(7, 8)), Cell::Empty);
        assert_eq!(
            resolved.sound,
            RoundSound {
                black_capture: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn pure_suicide_self_return_is_repetition_and_becomes_gray() {
        let mut board = Board::default();
        for point in [
            Point::new(0, 1),
            Point::new(1, 0),
            Point::new(2, 1),
            Point::new(1, 2),
        ] {
            board.set(point, Cell::White);
        }
        let attempt = lock(Player::One, 1, 1, 1.0);

        let resolved = resolve_round(
            &board,
            [Some(attempt), None],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert_eq!(resolved.board.get(attempt.point), Cell::Gray);
        assert_eq!(resolved.capture_credits, [0, 0]);
        assert_eq!(
            resolved.sound,
            RoundSound {
                gray_capture: true,
                ..Default::default()
            }
        );
        assert!(resolved.placed_stone);
        assert!(resolved.repetition);
    }

    #[test]
    fn ko_repeat_restores_board_and_grays_attempted_points() {
        let mut board = Board::default();
        board.set(Point::new(8, 8), Cell::Black);
        let first = lock(Player::One, 0, 0, 1.0);
        let second = lock(Player::Two, 8, 0, 1.2);
        let repeated = board
            .apply_move(first.player, first.point)
            .unwrap()
            .board
            .apply_move(second.player, second.point)
            .unwrap()
            .board;
        let history = HashSet::from([repeated, board.clone()]);

        let resolved = resolve_round(&board, [Some(first), Some(second)], &history).unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Gray);
        assert_eq!(resolved.board.get(Point::new(8, 0)), Cell::Gray);
        assert_eq!(resolved.board.get(Point::new(8, 8)), Cell::Black);
        assert!(resolved.sound.gray_capture);
        assert_eq!(resolved.capture_credits, [0, 0]);
        assert!(resolved.repetition);
        assert!(resolved.placed_stone);
    }

    #[test]
    fn positional_repetition_checks_every_prior_completed_board() {
        let board = Board::default();
        let first = lock(Player::One, 0, 0, 1.0);
        let second = lock(Player::Two, 8, 8, 1.2);
        let repeated = board
            .apply_move(first.player, first.point)
            .unwrap()
            .board
            .apply_move(second.player, second.point)
            .unwrap()
            .board;
        let mut unrelated = board.clone();
        unrelated.set(Point::new(4, 4), Cell::Gray);
        let history = HashSet::from([
            repeated,
            unrelated.clone(),
            board.clone(),
            unrelated,
            board.clone(),
        ]);

        let resolved = resolve_round(&board, [Some(first), Some(second)], &history).unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert_eq!(resolved.board.get(first.point), Cell::Gray);
        assert_eq!(resolved.board.get(second.point), Cell::Gray);
    }

    #[test]
    fn one_player_repetition_grays_only_its_legal_attempt() {
        let board = Board::default();
        let selection = lock(Player::One, 3, 4, 8.0);
        let repeated = board
            .apply_move(selection.player, selection.point)
            .unwrap()
            .board;

        let resolved = resolve_round(
            &board,
            [Some(selection), None],
            &HashSet::from([repeated, board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert_eq!(
            resolved.attempted_valid_points,
            [Some(selection.point), None]
        );
        assert_eq!(resolved.board.get(selection.point), Cell::Gray);
        assert_eq!(resolved.capture_credits, [0, 0]);
    }

    #[test]
    fn repeated_same_point_attempt_is_deduplicated() {
        let board = Board::default();
        let point = Point::new(4, 4);
        let mut repeated = board.clone();
        repeated.set(point, Cell::Gray);
        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 4, 4, 1.0)),
                Some(lock(Player::Two, 4, 4, 1.05)),
            ],
            &HashSet::from([repeated, board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert_eq!(resolved.attempted_valid_points, [Some(point), Some(point)]);
        assert_eq!(resolved.board.get(point), Cell::Gray);
        assert_eq!(
            (0..CELL_COUNT)
                .filter(|index| {
                    resolved
                        .board
                        .get(Point::new(index % BOARD_SIZE, index / BOARD_SIZE))
                        == Cell::Gray
                })
                .count(),
            1
        );
    }

    #[test]
    fn invalid_or_occupied_single_attempt_advances_without_gray() {
        let mut board = Board::default();
        let point = Point::new(2, 2);
        board.set(point, Cell::White);

        let resolved = resolve_round(
            &board,
            [Some(lock(Player::One, 2, 2, 1.0)), None],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::NoPlacement);
        assert_eq!(resolved.board, board);
        assert_eq!(resolved.attempted_valid_points, [None, None]);
        assert!(!resolved.placed_stone);
        assert!(!resolved.repetition);
    }

    #[test]
    fn repetition_rolls_back_capture_counts_and_capture_sound() {
        let mut board = Board::default();
        board.set(Point::new(1, 1), Cell::White);
        for point in [Point::new(0, 1), Point::new(1, 0), Point::new(2, 1)] {
            board.set(point, Cell::Black);
        }
        let attempt = lock(Player::One, 1, 2, 3.0);
        let repeated = board
            .apply_move(attempt.player, attempt.point)
            .unwrap()
            .board;

        let resolved = resolve_round(
            &board,
            [Some(attempt), None],
            &HashSet::from([repeated, board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert_eq!(resolved.capture_credits, [0, 0]);
        assert!(!resolved.sound.black_capture);
        assert!(resolved.sound.gray_capture);
        assert_eq!(resolved.board.get(Point::new(1, 1)), Cell::White);
        assert_eq!(resolved.board.get(Point::new(1, 2)), Cell::Gray);
    }

    #[test]
    fn deliberately_recreating_a_prior_board_is_a_permitted_strategy() {
        let board = Board::default();
        let point = Point::new(4, 4);
        let mut already_seen = board.clone();
        already_seen.set(point, Cell::Black);

        // Selection legality never consults Match history, so nothing stops
        // a player from knowingly aiming at a position they know will
        // repeat - this is ordinary, permitted play, not something to guard
        // against.
        assert_eq!(
            validate_lock(&board, Player::One, point, None, at(1.0)),
            Ok(())
        );

        let resolved = resolve_round(
            &board,
            [Some(lock(Player::One, 4, 4, 1.0)), None],
            &HashSet::from([already_seen, board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert_eq!(resolved.board.get(point), Cell::Gray);
        assert_eq!(resolved.capture_credits, [0, 0]);
    }

    #[test]
    fn simultaneous_different_moves_use_the_same_starting_board() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::White);
        board.set(Point::new(1, 0), Cell::Black);
        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 0, 1, 3.0)),
                Some(lock(Player::Two, 8, 8, 3.05)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(0, 1)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(8, 8)), Cell::White);
        assert_eq!(
            resolved.sound,
            RoundSound {
                black_capture: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn simultaneous_opposing_captures_remove_disjoint_groups() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::White);
        board.set(Point::new(1, 0), Cell::Black);
        board.set(Point::new(8, 8), Cell::Black);
        board.set(Point::new(7, 8), Cell::White);

        let resolved = resolve_round(
            &board,
            [
                Some(LockedMove {
                    player: Player::One,
                    point: Point::new(0, 1),
                    at: at(1.0),
                }),
                Some(LockedMove {
                    player: Player::Two,
                    point: Point::new(8, 7),
                    at: at(1.05),
                }),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(8, 8)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(0, 1)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(8, 7)), Cell::White);
        assert_eq!(
            resolved.sound,
            RoundSound {
                black_capture: true,
                white_capture: true,
                ..Default::default()
            }
        );
        assert_eq!(resolved.capture_credits, [1, 1]);
    }

    #[test]
    fn lone_cleanup_selection_resolves_without_placing_a_stone() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::White);
        board.set(Point::new(1, 0), Cell::Black);
        board.set(Point::new(0, 1), Cell::Black);

        let resolved = resolve_round(
            &board,
            [Some(lock(Player::One, 1, 0, 1.0)), None],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Single);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(1, 0)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(0, 1)), Cell::Black);
        assert!(!resolved.placed_stone);
        assert_eq!(resolved.capture_credits, [1, 0]);
    }

    #[test]
    fn sequential_cleanup_after_an_unrelated_placement_still_captures() {
        let mut board = Board::default();
        board.set(Point::new(8, 8), Cell::Black);
        board.set(Point::new(7, 8), Cell::White);
        board.set(Point::new(8, 7), Cell::White);

        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 0, 0, 1.0)),
                Some(lock(Player::Two, 7, 8, 1.2)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Sequential);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(8, 8)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(7, 8)), Cell::White);
        assert!(resolved.placed_stone);
        assert_eq!(resolved.capture_credits, [0, 1]);
    }

    #[test]
    fn sequential_cleanup_invalidated_by_the_earlier_action_fails_silently() {
        let mut board = Board::default();
        // G, the Black group Two intends to clean up via its own stone at
        // (4,4), starts zero-liberty: every neighbor is already occupied.
        board.set(Point::new(4, 3), Cell::Black);
        board.set(Point::new(4, 4), Cell::White);
        board.set(Point::new(5, 3), Cell::White);
        board.set(Point::new(4, 2), Cell::White);
        board.set(Point::new(3, 3), Cell::White);
        // One's earlier placement at (3,4) captures White(3,3), which opens
        // a liberty for G at (3,3) before Two's cleanup ever runs.
        board.set(Point::new(2, 3), Cell::Black);
        board.set(Point::new(3, 2), Cell::Black);

        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 3, 4, 1.0)),
                Some(lock(Player::Two, 4, 4, 1.2)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Sequential);
        assert_eq!(resolved.board.get(Point::new(3, 4)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(3, 3)), Cell::Empty);
        // G survives: Two's now-invalid cleanup contributed nothing, but the
        // round still advanced on One's earlier action alone.
        assert_eq!(resolved.board.get(Point::new(4, 3)), Cell::Black);
        assert_eq!(resolved.board.get(Point::new(4, 4)), Cell::White);
        assert!(resolved.placed_stone);
        assert_eq!(resolved.capture_credits, [1, 0]);
    }

    #[test]
    fn simultaneous_cleanup_actions_can_both_capture() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(1, 0), Cell::White);
        board.set(Point::new(0, 1), Cell::White);
        board.set(Point::new(8, 8), Cell::White);
        board.set(Point::new(7, 8), Cell::Black);
        board.set(Point::new(8, 7), Cell::Black);

        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 7, 8, 1.0)),
                Some(lock(Player::Two, 1, 0, 1.05)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(1, 0)), Cell::White);
        assert_eq!(resolved.board.get(Point::new(8, 8)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(7, 8)), Cell::Black);
        assert_eq!(resolved.capture_credits, [1, 1]);
        assert!(!resolved.placed_stone);
    }

    #[test]
    fn simultaneous_capture_of_a_cleanup_stone_still_applies_its_cleanup_effect() {
        let mut board = Board::default();
        // G, the Black group Two's cleanup targets, is zero-liberty via
        // three Gray walls plus Two's own stone at (4,4).
        board.set(Point::new(4, 3), Cell::Black);
        board.set(Point::new(3, 3), Cell::Gray);
        board.set(Point::new(5, 3), Cell::Gray);
        board.set(Point::new(4, 2), Cell::Gray);
        board.set(Point::new(4, 4), Cell::White);
        // One's simultaneous placement at (4,5) independently captures
        // Two's cleanup stone, evaluated against this same starting board.
        board.set(Point::new(3, 4), Cell::Black);
        board.set(Point::new(5, 4), Cell::Black);

        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 4, 5, 1.0)),
                Some(lock(Player::Two, 4, 4, 1.05)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        assert_eq!(resolved.board.get(Point::new(4, 5)), Cell::Black);
        // Two's cleanup stone was itself simultaneously captured...
        assert_eq!(resolved.board.get(Point::new(4, 4)), Cell::Empty);
        // ...yet its valid cleanup effect still removed G.
        assert_eq!(resolved.board.get(Point::new(4, 3)), Cell::Empty);
        assert_eq!(resolved.capture_credits, [1, 1]);
        assert!(resolved.placed_stone);
    }

    #[test]
    fn cleanup_round_is_still_checked_for_positional_repetition() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::White);
        board.set(Point::new(1, 0), Cell::Black);
        board.set(Point::new(0, 1), Cell::Black);
        let mut already_seen = board.clone();
        already_seen.set(Point::new(0, 0), Cell::Empty);

        let resolved = resolve_round(
            &board,
            [Some(lock(Player::One, 1, 0, 1.0)), None],
            &HashSet::from([board.clone(), already_seen]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::KoGray);
        assert!(resolved.repetition);
        // The cleanup point already held a live stone before the round, so
        // rollback cannot Gray it the way it would an ordinary attempted
        // placement onto a previously empty point - it simply restores the
        // pre-round board.
        assert_eq!(resolved.board, board);
        assert!(!resolved.placed_stone);
        assert_eq!(resolved.capture_credits, [0, 0]);
    }

    /// An off-board point whose flat index still lands inside the cell array
    /// is the subtle half of the bounds contract: it cannot panic, so nothing
    /// announces it, and before `apply_selection` bounds-checked it read the
    /// aliased cell and chose its cleanup-or-placement branch from a wholly
    /// unrelated point's contents. `Point::new(15, 2)` has index 33, which is
    /// `Point::new(6, 3)`, so seeding that cell with the acting player's own
    /// stone used to send the call down the cleanup branch and report
    /// `Occupied` - a plausible-looking answer about the wrong point. The
    /// property test in `properties.rs` covers out-of-bounds rejection
    /// broadly; random points rarely produce this in-range-index case, so it
    /// is pinned by example here.
    #[test]
    fn off_board_point_with_an_in_range_index_does_not_alias_another_cell() {
        let off_board = Point::new(15, 2);
        let aliased = Point::new(6, 3);
        assert_eq!(off_board.index(), aliased.index());
        assert!(off_board.index() < CELL_COUNT);
        assert!(off_board.x >= BOARD_SIZE);

        let mut board = Board::default();
        board.set(aliased, Cell::Black);

        // Black owns the aliased cell, which is what used to select the
        // cleanup branch and turn a bounds error into `Occupied`.
        assert_eq!(
            apply_selection(&board, Player::One, off_board).err(),
            Some(MoveError::OutOfBounds)
        );
        // The other player reaches the placement branch through the same
        // aliased read; both report the point they were actually given.
        assert_eq!(
            apply_selection(&board, Player::Two, off_board).err(),
            Some(MoveError::OutOfBounds)
        );
        // The in-bounds point it aliased is untouched and still playable.
        assert_eq!(board.get(aliased), Cell::Black);
        assert!(apply_selection(&board, Player::Two, Point::new(6, 4)).is_ok());
    }

    /// The louder half of the same contract: an index at or past
    /// [`CELL_COUNT`] used to panic inside [`Board::get`] before any branch
    /// was chosen.
    #[test]
    fn off_board_point_past_the_cell_array_is_rejected_not_panicked_on() {
        let board = Board::default();
        let past_the_end = Point::new(0, BOARD_SIZE);
        assert_eq!(past_the_end.index(), CELL_COUNT);

        assert_eq!(
            apply_selection(&board, Player::One, past_the_end).err(),
            Some(MoveError::OutOfBounds)
        );
    }

    // ---- What a round removes, with each stone's owner -----------------

    fn removed(x: usize, y: usize, owner: Player, by: (usize, usize)) -> RemovedStone {
        RemovedStone {
            point: Point::new(x, y),
            owner,
            by: Some(Point::new(by.0, by.1)),
        }
    }

    #[test]
    fn a_capture_lists_the_opponents_stone_and_a_suicide_the_actors_own() {
        // Black's corner stone has one liberty, (0, 1), and White fills it.
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(1, 0), Cell::White);
        let captured = resolve_round(
            &board,
            [None, Some(lock(Player::Two, 0, 1, 1.0))],
            &HashSet::from([board.clone()]),
        )
        .unwrap();
        assert_eq!(captured.removed, vec![removed(0, 0, Player::One, (0, 1))]);
        assert_eq!(captured.capture_credits, [0, 1]);

        // Black plays into a corner White has walled off. The stone is
        // listed under Black's name - it was Black's - and credited to
        // White, so the owner alone says who was credited. The history is
        // empty so the restored board is not read as a repetition.
        let mut board = Board::default();
        board.set(Point::new(1, 0), Cell::White);
        board.set(Point::new(0, 1), Cell::White);
        let suicide = resolve_round(
            &board,
            [Some(lock(Player::One, 0, 0, 1.0)), None],
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(
            suicide.removed,
            vec![removed(0, 0, Player::One, (0, 0))],
            "a self-removed stone was taken by its own placement"
        );
        assert_eq!(suicide.capture_credits, [0, 1]);
        assert_eq!(suicide.board.get(Point::new(0, 0)), Cell::Empty);
    }

    #[test]
    fn a_cleanup_lists_the_opposing_group_it_removed_and_keeps_its_own_stone() {
        // White at (4, 3) is walled by three Grays and Black's stone at
        // (4, 4); Black's cleanup from that stone removes it.
        let mut board = Board::default();
        board.set(Point::new(4, 3), Cell::White);
        for point in [Point::new(3, 3), Point::new(5, 3), Point::new(4, 2)] {
            board.set(point, Cell::Gray);
        }
        board.set(Point::new(4, 4), Cell::Black);

        let resolved = resolve_round(
            &board,
            [Some(lock(Player::One, 4, 4, 1.0)), None],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Single);
        assert_eq!(resolved.removed, vec![removed(4, 3, Player::Two, (4, 4))]);
        assert_eq!(resolved.board.get(Point::new(4, 4)), Cell::Black);
    }

    #[test]
    fn simultaneous_removals_are_listed_together_in_cell_order() {
        // The position of
        // `simultaneous_capture_of_a_cleanup_stone_still_applies_its_cleanup_effect`:
        // Two's cleanup stone at (4, 4) is captured by One's placement
        // while its own cleanup removes Black's (4, 3). Two stones of two
        // colours leave in one round, and both are listed, the lower cell
        // index first.
        let mut board = Board::default();
        board.set(Point::new(4, 3), Cell::Black);
        board.set(Point::new(3, 3), Cell::Gray);
        board.set(Point::new(5, 3), Cell::Gray);
        board.set(Point::new(4, 2), Cell::Gray);
        board.set(Point::new(4, 4), Cell::White);
        board.set(Point::new(3, 4), Cell::Black);
        board.set(Point::new(5, 4), Cell::Black);

        let resolved = resolve_round(
            &board,
            [
                Some(lock(Player::One, 4, 5, 1.0)),
                Some(lock(Player::Two, 4, 4, 1.05)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Simultaneous);
        assert_eq!(
            resolved.removed,
            vec![
                removed(4, 3, Player::One, (4, 4)),
                removed(4, 4, Player::Two, (4, 5)),
            ]
        );
    }

    /// A sequential round can empty one point twice. Black holds the corner
    /// (0, 0) and selects it as a cleanup of the walled-off White group G
    /// beside it; White, earlier, plays (0, 1), which captures the corner
    /// stone and the Black stone at (0, 2) - and that second capture hands
    /// G a liberty. Black's selection then lands on an empty corner, so it
    /// is a placement: G is no longer capturable, the corner has no
    /// liberties, and the new stone self-removes. The corner is listed
    /// once; both of its stones were Black's, as they must be, since only
    /// Black could have selected a point holding a Black stone; and the
    /// credits still count both.
    #[test]
    fn a_point_emptied_twice_in_a_sequential_round_is_listed_once() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(0, 2), Cell::Black);
        for (x, y) in [(1, 0), (2, 0), (2, 1), (2, 2), (1, 2)] {
            board.set(Point::new(x, y), Cell::White);
        }
        for (x, y) in [(1, 1), (3, 0), (3, 1), (3, 2), (2, 3), (1, 3), (0, 3)] {
            board.set(Point::new(x, y), Cell::Gray);
        }
        let early = lock(Player::Two, 0, 1, 1.0);
        let late = lock(Player::One, 0, 0, 1.2);
        assert_eq!(
            validate_lock(&board, late.player, late.point, Some(early), late.at),
            Ok(()),
            "the corner stone borders a zero-liberty White group, so it is a valid cleanup"
        );

        let resolved = resolve_round(
            &board,
            [Some(late), Some(early)],
            &HashSet::from([board.clone()]),
        )
        .unwrap();

        assert_eq!(resolved.kind, ResolutionKind::Sequential);
        assert_eq!(resolved.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(resolved.board.get(Point::new(0, 1)), Cell::White);
        assert_eq!(resolved.board.get(Point::new(0, 2)), Cell::Empty);
        assert_eq!(
            resolved.board.get(Point::new(1, 0)),
            Cell::White,
            "G survives"
        );
        assert_eq!(
            resolved.removed,
            vec![
                removed(0, 0, Player::One, (0, 1)),
                removed(0, 2, Player::One, (0, 1)),
            ],
            "listed once, as White's capture, though Black's suicide emptied it again"
        );
        assert_eq!(resolved.capture_credits, [0, 3]);
    }

    #[test]
    fn gray_outcomes_and_a_repetition_remove_nothing() {
        let board = Board::default();
        let gray = resolve_round(
            &board,
            [
                Some(lock(Player::One, 4, 4, 2.0)),
                Some(lock(Player::Two, 4, 4, 2.08)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap();
        assert_eq!(gray.kind, ResolutionKind::SamePointGray);
        assert!(gray.removed.is_empty());

        // A capture whose board was completed before is rolled back, and
        // its removal goes with it: the captured stone is back, so nothing
        // was taken.
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(1, 0), Cell::White);
        let pending = [None, Some(lock(Player::Two, 0, 1, 1.0))];
        let candidate = resolve_round(&board, pending, &HashSet::new()).unwrap();
        assert_eq!(candidate.removed.len(), 1);
        let repeated = resolve_round(&board, pending, &HashSet::from([candidate.board])).unwrap();
        assert_eq!(repeated.kind, ResolutionKind::KoGray);
        assert!(repeated.removed.is_empty());
        assert_eq!(repeated.board.get(Point::new(0, 0)), Cell::Black);
    }
}
