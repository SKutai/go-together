//! Privacy-scoped, read-only projections of a live [`MatchState`].
//!
//! [`MatchState`] represents the trusted authoritative host: it legitimately
//! holds both players' hidden Round 0 selections and pending ordinary lock,
//! because something has to in order to resolve a round. Nothing else should
//! reach that through a general accessor - see `authority`'s own module docs.
//! This module is the one place that turns "everything the host knows" into
//! "everything one specific audience is *permitted* to know," so a future
//! networked client, spectator, or broadcast built on it structurally cannot
//! read the wrong player's hidden data - not merely by convention, but
//! because the type it receives has no field to read it from.
//!
//! # The three live audiences
//!
//! * [`PublicMatchView`] - what the opponent, a spectator, or a future
//!   network broadcast may see: the board, phase, round, score, capture
//!   totals, no-stone streak, clocks, end reasons, live cursors (only once
//!   they stop previewing a hidden Round 0 choice - see
//!   `crate::ui::match_screen`'s `live_cursor_visibility`), and each
//!   player's Round 0 selection *count* and what their last selection did
//!   to it - never the points. There is
//!   deliberately no separate spectator type: a spectator is permitted to see
//!   exactly what an opponent is, nothing more.
//! * [`PlayerMatchView`] - a specific player's own authorized view: a
//!   [`PublicMatchView`] plus that player's own Round 0 selections, own
//!   pending ordinary selection (carried with a timing rule on surfacing
//!   it - see [`PlayerMatchView::own_selected_point`]), and own Teleport
//!   Marker. It has no field an opponent's data could occupy, so
//!   [`PlayerMatchView::of`] cannot be made to expose it by a future caller
//!   passing the wrong argument the way a shared "give me both players'
//!   locks" accessor could.
//! * Complete authoritative [`MatchState`] - the trusted host's own state,
//!   unchanged by this module, still `pub(crate)` for `locks` exactly as
//!   `authority` documents. Never serialize it as a client-facing payload;
//!   see `docs/SECURITY.md`'s trust-boundary section.
//!
//! # No live caller yet, and the two guards that stand in for one
//!
//! Nothing outside this module constructs either view today. The local
//! shared-screen UI is the trusted host's own renderer and reads
//! [`MatchState`] directly by design - see `authority`'s Privacy docs for
//! why that is correct, and for the rendering discipline that applies to it
//! instead of this type boundary. A type with no consumer can drift
//! unnoticed, so two guards stand in for one:
//!
//! * [`PublicMatchView::of`] and [`PlayerMatchView::of`] destructure
//!   [`MatchState`] and [`crate::authority::MatchProgress`] exhaustively,
//!   with no `..` rest pattern, so a field added to either struct fails to
//!   compile until someone decides here whether it is public, released to
//!   its owner, or withheld. The decision is recorded as the pattern itself,
//!   checked by the compiler at no run-time cost.
//! * That catches an *omitted* field, not a *wrong value* in a field that is
//!   present - which is what actually went wrong once: [`PublicMatchView`]
//!   read `score` from a stored `MatchProgress` field that only `finish`
//!   ever wrote, so it reported 0-0 for the whole of every live Match. This
//!   module's tests therefore also assert every field of both views against
//!   its authoritative source on a fully populated, thoroughly non-default
//!   [`MatchState`], for both players.
//!
//! [`crate::replay::ReplayFrame`] is a fourth, separate thing: a *complete*
//! historical snapshot, including both players' Teleport Markers and
//! resolved selections, meant only for authorized Replay viewing after the
//! fact. Nothing here converts a `ReplayFrame` into a live view or vice
//! versa - a Replay must never be mistaken for, or reused as, a live
//! network payload.
//!
//! # What is preserved, and why the fields stop where they do
//!
//! * A pending ordinary lock (which point, and when) never appears in
//!   [`PublicMatchView`] - not even as a "some player has locked in" flag,
//!   since nothing in the live client renders that today and adding it would
//!   be new exposed information the current game never gives away.
//! * Round 0 cursors and selections are hidden from [`PublicMatchView`]
//!   entirely during `Opening`/`OpeningCountdown`/`Ended`; only the count,
//!   and whether each player's last selection added a point, rolled the
//!   window forward or changed nothing, is ever public - which is what the
//!   live dot rows draw (`crate::ui::match_screen`'s `opening_dots`) and
//!   never a point. `docs/SECURITY.md` argues why the second part is
//!   already on the shared screen.
//! * Teleport Markers appear only in the owning player's [`PlayerMatchView`]:
//!   the live client never renders either player's marker location today
//!   (only the Replay Viewer draws `ReplayFrame::teleport_markers`, an
//!   explicitly authorized historical context).
//! * Even the owner's own pending lock carries a timing rule on top of the
//!   structure: [`PlayerMatchView::own_selected_point`] documents why no
//!   live view may surface it while its round is unresolved.
use serde::Serialize;

use crate::authority::{
    MatchPhase, MatchProgress, MatchState, OpeningAttempt, RoundRemovals, player_index,
};
use crate::opening::OpeningSelections;
use crate::replay::EndReason;
use crate::rules::{Board, Player, Point, Score};

/// What the opponent, a spectator, or a future network broadcast may see of a
/// Match in progress. Contains no field either player's hidden Round 0
/// selections or pending ordinary lock could occupy.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PublicMatchView {
    pub board: Board,
    pub phase: MatchPhase,
    pub round: usize,
    /// The score as it stands right now, in every phase - not only once the
    /// Match has ended. Derived from the board and `capture_totals` by
    /// [`MatchState::current_score`], so it tracks play live the same way
    /// `capture_totals` does.
    pub score: Score,
    pub capture_totals: Score,
    pub no_stone_streak: usize,
    pub elapsed_seconds: f64,
    pub round_elapsed_seconds: f64,
    pub end_reasons: Vec<EndReason>,
    /// `None` through Round 0 and after the Match ends; `Some` only while
    /// `phase` is `Playing`, matching `crate::ui::match_screen`'s
    /// `live_cursor_visibility`.
    pub cursors: Option<[Point; 2]>,
    /// How many of each player's three Round 0 slots are filled, indexed by
    /// [`player_index`] - never the points themselves.
    pub opening_selection_counts: [usize; 2],
    /// What each player's last Round 0 selection did, indexed by
    /// [`player_index`] - added, rolled forward, or changed nothing - and
    /// how many they have made; `None` before a player's first and after
    /// the reveal. No point, and no hint of which held point a duplicate
    /// matched. Public because the shared screen already shows all of it:
    /// the count changes for an addition, the clack and the refusal mark
    /// say a selection changed nothing, and the dot row's roll-forward
    /// says one was accepted with three held. See [`OpeningAttempt`] and
    /// `docs/SECURITY.md`.
    pub last_opening_attempts: [Option<OpeningAttempt>; 2],
    /// What the most recent resolution took off the board. Public because
    /// a resolved selection is not secret: the rules hide only a pending
    /// one, and every Replay records the resolved ones. That is the rule
    /// to lean on rather than "the board already showed it", which is
    /// false for some of these stones - one placed and removed within a
    /// round, or swept at the Round 0 reveal, stood on no board a player
    /// ever saw, and this record is the only trace it leaves. This is what
    /// a screen reader reads a capture from, and what the live board marks.
    pub last_removals: RoundRemovals,
}

impl PublicMatchView {
    /// Projects the public audience's view of `state`.
    ///
    /// The destructuring below is exhaustive on purpose - no `..` rest
    /// pattern - so it reads as a manifest of what this boundary publishes
    /// and what it deliberately withholds, checked by the compiler. Adding a
    /// field to [`MatchState`] or [`MatchProgress`] breaks this function
    /// until someone decides which of the two it is; the check is entirely a
    /// compile-time pattern and costs nothing at run time.
    pub fn of(state: &MatchState) -> Self {
        let MatchState {
            progress,
            cursors,
            opening_selections,
            last_opening_attempts,
            phase,
            elapsed,
            // Not withheld: published in derived form as `score` below.
            // `MatchState::current_score` is the single place that combines
            // the rules' Capture Points setting with the board and capture
            // totals, so this projection calls it rather than re-deriving a
            // second score that could drift from the authority's.
            match_rules: _,
            // Owner-only; the owning player's `PlayerMatchView` carries it.
            teleport_markers: _,
            // Host-only Round 0 closing-countdown timing.
            countdown_elapsed: _,
        } = state;
        let MatchProgress {
            board,
            round,
            capture_totals,
            no_stone_streak,
            round_elapsed,
            end_reasons,
            last_removals,
            // Each player's pending hidden selection - the entire point of
            // this boundary. Owner-only, via `PlayerMatchView`.
            locks: _,
            // Host-only positional-repetition bookkeeping.
            completed_history: _,
        } = progress;

        Self {
            board: board.clone(),
            phase: *phase,
            round: *round,
            score: state.current_score(),
            capture_totals: *capture_totals,
            no_stone_streak: *no_stone_streak,
            elapsed_seconds: elapsed.as_secs_f64(),
            round_elapsed_seconds: round_elapsed.as_secs_f64(),
            end_reasons: end_reasons.clone(),
            cursors: (*phase == MatchPhase::Playing).then_some(*cursors),
            opening_selection_counts: [opening_selections[0].len(), opening_selections[1].len()],
            last_opening_attempts: *last_opening_attempts,
            last_removals: last_removals.clone(),
        }
    }
}

/// One specific player's authorized view: everything [`PublicMatchView`]
/// carries, plus that player's own hidden Round 0 selections, own pending
/// ordinary selection, and own Teleport Marker. Never the opponent's.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlayerMatchView {
    pub public: PublicMatchView,
    pub own_opening_selections: OpeningSelections,
    /// The owner's own pending ordinary selection - populated, and read by
    /// no production code, deliberately. No live view may surface this
    /// field, in any form a player could observe, while the round it
    /// belongs to is unresolved.
    ///
    /// When a player presses Select on a point they can see is empty, two
    /// worlds leave the round unresolved, and they produce identical
    /// observations (every world in which the press completes the round
    /// announces itself through the resolution instead). Either
    /// the press was accepted, this field holds the point, and the round is
    /// waiting on the opponent; or the opponent locked that same point more
    /// than [`crate::rules::SIMULTANEOUS_WINDOW`] earlier, the press was
    /// rejected ([`crate::rules::LockError::Reserved`], reaching the client
    /// only as the undifferentiated
    /// [`crate::authority::LockOutcome::Rejected`]), this field is `None`,
    /// and the round is waiting on *this* player. That ambiguity is
    /// load-bearing: a player who cannot confirm their own lock cannot read
    /// a failure to confirm as proof that the opponent holds that exact
    /// point, so an ordinary selection attempt cannot work as a probe.
    /// [`MatchState::try_lock`] keeps every outcome silent for exactly this
    /// reason - see its "Silence" section. Surface this field during an
    /// unresolved round and the worlds separate: a marker appears in one
    /// and not the other, and the marker's *absence* becomes a precise
    /// readout of the opponent's hidden move, in a game whose whole premise
    /// is that the move is hidden. The `Some`/`None` state itself is the
    /// leak, not the pixels: handing the live value to anything a player
    /// can read mid-round gives it away as surely as drawing a marker.
    ///
    /// Probing is currently self-defeating for a second reason: a second,
    /// accepted press resolves the round instantly in the rejected world
    /// and silently revises this player's held selection in the accepted
    /// one, so the answer only arrives after the player has already
    /// committed - one move too late to use.
    ///
    /// The field is not wrong to exist. It is the owning player's own data,
    /// and a non-Bevy consumer may legitimately need it; once its round
    /// resolves, the lock clears and this field is `None` again, so there
    /// is nothing left to keep quiet. The constraint is narrower than
    /// "never expose", and it is exactly the one above: not while the round
    /// is unresolved.
    pub own_selected_point: Option<Point>,
    pub own_teleport_marker: Option<Point>,
}

impl PlayerMatchView {
    /// Projects `player`'s own authorized view of `state`.
    ///
    /// Exhaustive for the same reason as [`PublicMatchView::of`], from the
    /// owning player's side: the three fields bound here are the whole of
    /// what one player may see beyond the public projection, so a new
    /// authoritative field has to be handled in both places - published
    /// publicly, released to its owner here, or withheld from both - before
    /// this compiles again.
    pub fn of(state: &MatchState, player: Player) -> Self {
        let index = player_index(player);
        let MatchState {
            progress,
            opening_selections,
            teleport_markers,
            // Everything else is the public projection's decision; see
            // `PublicMatchView::of` for what each of these publishes.
            cursors: _,
            last_opening_attempts: _,
            phase: _,
            elapsed: _,
            match_rules: _,
            countdown_elapsed: _,
        } = state;
        let MatchProgress {
            locks,
            board: _,
            round: _,
            capture_totals: _,
            no_stone_streak: _,
            round_elapsed: _,
            end_reasons: _,
            last_removals: _,
            completed_history: _,
        } = progress;

        Self {
            public: PublicMatchView::of(state),
            // Indexed by `player` alone: there is no field the opponent's
            // equivalent data could be written into, even by mistake.
            own_opening_selections: opening_selections[index].clone(),
            own_selected_point: locks[index].map(|locked| locked.point),
            own_teleport_marker: teleport_markers[index],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{MatchRules, OpeningAttemptOutcome, PLAYER_STARTS};
    use crate::clock::{MatchDuration, MatchInstant};
    use crate::match_settings::MatchSettings;
    use crate::rules::{Cell, LockedMove, RemovedStone};

    fn state_with_both_locked() -> MatchState {
        let mut state = MatchState {
            phase: MatchPhase::Playing,
            ..MatchState::default()
        };
        state.progress.locks = [
            Some(LockedMove {
                player: Player::One,
                point: Point::new(1, 1),
                at: MatchInstant::from_secs(1),
            }),
            Some(LockedMove {
                player: Player::Two,
                point: Point::new(8, 8),
                at: MatchInstant::from_secs(1),
            }),
        ];
        state
    }

    /// A `MatchState` in which every field a view can read differs from its
    /// default, so a field-agreement assertion cannot pass by both sides
    /// happening to be empty, zero, or a starting value.
    fn fully_populated_state() -> MatchState {
        let mut settings = MatchSettings::default();
        // Capture Points on, so `capture_totals` actually reaches the
        // derived score instead of being folded away.
        settings.set_capture_points(true);

        let mut state = state_with_both_locked();
        state.match_rules = MatchRules::snapshot(&settings);
        state.progress.board.set(Point::new(0, 0), Cell::Black);
        state.progress.board.set(Point::new(4, 4), Cell::Black);
        state.progress.board.set(Point::new(7, 7), Cell::White);
        state.progress.round = 7;
        state.progress.capture_totals = Score { black: 5, white: 3 };
        state.progress.no_stone_streak = 2;
        state.progress.round_elapsed = MatchDuration::from_millis(4_250);
        state.progress.end_reasons = vec![EndReason::RoundLimit, EndReason::PointTarget];
        state.progress.last_removals = RoundRemovals {
            round: 6,
            stones: vec![RemovedStone {
                point: Point::new(5, 5),
                owner: Player::Two,
                by: Some(Point::new(5, 6)),
            }],
        };
        let board = state.progress.board.clone();
        state.progress.completed_history.insert(board);
        state.cursors = [Point::new(2, 6), Point::new(5, 1)];
        state.teleport_markers = [Some(Point::new(3, 3)), Some(Point::new(6, 6))];
        state.opening_selections[0].add(Point::new(2, 3));
        state.opening_selections[0].add(Point::new(4, 5));
        state.opening_selections[1].add(Point::new(6, 7));
        state.last_opening_attempts = [
            Some(OpeningAttempt {
                number: 3,
                outcome: OpeningAttemptOutcome::Duplicate,
            }),
            Some(OpeningAttempt {
                number: 1,
                outcome: OpeningAttemptOutcome::Added,
            }),
        ];
        state.elapsed = MatchInstant::from_secs(93);
        state.countdown_elapsed = MatchDuration::from_millis(1_500);
        state
    }

    /// How a `Point` appears inside a serialized view, for asserting that a
    /// specific hidden point is nowhere in one.
    fn point_json(point: Point) -> String {
        format!("\"x\":{},\"y\":{}", point.x, point.y)
    }

    #[test]
    fn every_public_view_field_agrees_with_its_authoritative_source() {
        let state = fully_populated_state();

        // Destructured rather than accessed field by field so that adding a
        // field to `PublicMatchView` fails to compile here too: exhaustive
        // destructuring in `of` catches an authoritative field that never
        // reaches the view, and this catches a view field nothing checks.
        let PublicMatchView {
            board,
            phase,
            round,
            score,
            capture_totals,
            no_stone_streak,
            elapsed_seconds,
            round_elapsed_seconds,
            end_reasons,
            cursors,
            opening_selection_counts,
            last_opening_attempts,
            last_removals,
        } = PublicMatchView::of(&state);

        assert_eq!(board, state.progress.board);
        assert_eq!(last_removals, state.progress.last_removals);
        assert_eq!(phase, state.phase);
        assert_eq!(round, state.progress.round);
        assert_eq!(score, state.current_score());
        assert_eq!(capture_totals, state.progress.capture_totals);
        assert_eq!(no_stone_streak, state.progress.no_stone_streak);
        assert_eq!(elapsed_seconds, state.elapsed.as_secs_f64());
        assert_eq!(
            round_elapsed_seconds,
            state.progress.round_elapsed.as_secs_f64()
        );
        assert_eq!(end_reasons, state.progress.end_reasons);
        assert_eq!(cursors, Some(state.cursors));
        assert_eq!(
            opening_selection_counts,
            [
                state.opening_selections[0].len(),
                state.opening_selections[1].len(),
            ]
        );
        assert_eq!(last_opening_attempts, state.last_opening_attempts);

        // Every source above is thoroughly non-default, so none of those
        // assertions can be satisfied by two empty or zero values agreeing.
        assert_ne!(board, Board::default());
        assert_ne!(phase, MatchState::default().phase);
        assert_ne!(round, 0);
        assert_ne!(score, Score::default());
        assert_ne!(capture_totals, Score::default());
        assert_ne!(no_stone_streak, 0);
        assert!(elapsed_seconds > 0.0);
        assert!(round_elapsed_seconds > 0.0);
        assert!(!end_reasons.is_empty());
        assert_ne!(cursors, Some(PLAYER_STARTS));
        assert_eq!(opening_selection_counts, [2, 1]);
        assert!(last_opening_attempts.iter().all(Option::is_some));
        assert_ne!(last_opening_attempts[0], last_opening_attempts[1]);
        assert_ne!(last_removals, RoundRemovals::default());

        // The score is the live derived one, folding in capture totals -
        // not the bare board count, and not the permanently 0-0 value the
        // removed `MatchProgress::score` field used to report during play.
        assert_ne!(score, state.progress.board.score());
    }

    #[test]
    fn every_player_view_field_agrees_with_that_players_own_source() {
        let state = fully_populated_state();

        for (player, own, opponent) in [(Player::One, 0, 1), (Player::Two, 1, 0)] {
            // The opponent's data must actually differ from this player's,
            // or "the opponent's data appears nowhere" would prove nothing.
            assert_ne!(
                state.opening_selections[own],
                state.opening_selections[opponent]
            );
            assert_ne!(state.locks[own], state.locks[opponent]);
            assert_ne!(
                state.teleport_markers[own],
                state.teleport_markers[opponent]
            );

            let view = PlayerMatchView::of(&state, player);
            let PlayerMatchView {
                public,
                own_opening_selections,
                own_selected_point,
                own_teleport_marker,
            } = &view;

            assert_eq!(*public, PublicMatchView::of(&state));
            assert_eq!(*own_opening_selections, state.opening_selections[own]);
            assert_eq!(
                *own_selected_point,
                state.locks[own].map(|locked| locked.point)
            );
            assert_eq!(*own_teleport_marker, state.teleport_markers[own]);

            assert!(!own_opening_selections.is_empty());
            assert!(own_selected_point.is_some());
            assert!(own_teleport_marker.is_some());

            // None of the opponent's private points appears anywhere in the
            // serialized view - neither in its own three fields nor in the
            // embedded public projection.
            let json = serde_json::to_string(&view).unwrap();
            let hidden = state.opening_selections[opponent]
                .as_slice()
                .iter()
                .copied()
                .chain(state.locks[opponent].map(|locked| locked.point))
                .chain(state.teleport_markers[opponent]);
            for point in hidden {
                assert!(
                    !json.contains(&point_json(point)),
                    "{player:?}'s view leaked the opponent's {point:?}: {json}"
                );
            }
        }
    }

    #[test]
    fn public_view_reports_a_live_score_before_the_match_ends() {
        let mut state = state_with_both_locked();
        state.progress.board.set(Point::new(0, 0), Cell::Black);
        state.progress.board.set(Point::new(4, 4), Cell::Black);
        state.progress.board.set(Point::new(8, 8), Cell::White);

        // Still `Playing`. This is the case the removed `MatchProgress::score`
        // field got wrong: it stayed 0-0 until `finish` wrote it, so the view
        // reported no score at all for the whole of a live Match.
        assert_eq!(state.phase, MatchPhase::Playing);
        assert_ne!(state.current_score(), Score::default());
        assert_eq!(PublicMatchView::of(&state).score, state.current_score());
    }

    #[test]
    fn public_view_has_no_field_for_either_players_pending_selection() {
        let state = state_with_both_locked();
        let public = PublicMatchView::of(&state);
        let json = serde_json::to_string(&public).unwrap();

        // Structural: `PublicMatchView` has no `locks`-shaped field to
        // populate, so there is nothing "point (1, 1)" or "(8, 8)" could ever
        // be assigned from - this asserts the serialized form agrees.
        assert!(!json.contains("\"x\":1,\"y\":1"));
        assert!(!json.contains("\"x\":8,\"y\":8"));
    }

    #[test]
    fn blacks_view_exposes_only_blacks_own_pending_selection() {
        let state = state_with_both_locked();
        let black = PlayerMatchView::of(&state, Player::One);

        assert_eq!(black.own_selected_point, Some(Point::new(1, 1)));
        let json = serde_json::to_string(&black).unwrap();
        assert!(
            !json.contains("\"x\":8,\"y\":8"),
            "Black's view leaked White's pending point: {json}"
        );
    }

    #[test]
    fn whites_view_exposes_only_whites_own_pending_selection() {
        let state = state_with_both_locked();
        let white = PlayerMatchView::of(&state, Player::Two);

        assert_eq!(white.own_selected_point, Some(Point::new(8, 8)));
        let json = serde_json::to_string(&white).unwrap();
        assert!(
            !json.contains("\"x\":1,\"y\":1"),
            "White's view leaked Black's pending point: {json}"
        );
    }

    #[test]
    fn round_zero_hides_points_and_cursors_but_exposes_permitted_counts() {
        let mut state = MatchState::default();
        state.opening_selections[0].add(Point::new(2, 3));
        state.opening_selections[0].add(Point::new(4, 5));
        state.opening_selections[1].add(Point::new(6, 7));
        state.cursors = [Point::new(2, 3), Point::new(6, 7)];

        let public = PublicMatchView::of(&state);
        assert_eq!(public.opening_selection_counts, [2, 1]);
        assert_eq!(
            public.cursors, None,
            "cursors preview a hidden Round 0 choice and must stay hidden"
        );
        let json = serde_json::to_string(&public).unwrap();
        assert!(!json.contains("\"x\":2,\"y\":3"));
        assert!(!json.contains("\"x\":4,\"y\":5"));
        assert!(!json.contains("\"x\":6,\"y\":7"));

        let black = PlayerMatchView::of(&state, Player::One);
        assert_eq!(
            black.own_opening_selections.as_slice(),
            &[Point::new(2, 3), Point::new(4, 5)]
        );
    }

    /// What a Round 0 selection did is public; the point it named is not,
    /// and nor is which held point a duplicate matched - re-selecting the
    /// oldest and re-selecting the newest publish identical views.
    #[test]
    fn round_zero_attempts_publish_what_a_selection_did_and_nothing_of_where() {
        fn select(state: &mut MatchState, x: usize, y: usize) -> Option<OpeningAttempt> {
            state.cursors[0] = Point::new(x, y);
            state.try_opening_select(Player::One);
            PublicMatchView::of(state).last_opening_attempts[0]
        }
        let attempt = |number, outcome| Some(OpeningAttempt { number, outcome });
        let mut state = MatchState::default();
        assert_eq!(
            PublicMatchView::of(&state).last_opening_attempts,
            [None, None]
        );

        assert_eq!(
            select(&mut state, 2, 3),
            attempt(1, OpeningAttemptOutcome::Added)
        );
        select(&mut state, 4, 5);
        assert_eq!(
            select(&mut state, 6, 7),
            attempt(3, OpeningAttemptOutcome::Added)
        );

        let mut oldest = state.clone();
        let mut newest = state.clone();
        assert_eq!(
            select(&mut oldest, 2, 3),
            attempt(4, OpeningAttemptOutcome::Duplicate)
        );
        assert_eq!(
            select(&mut newest, 6, 7),
            attempt(4, OpeningAttemptOutcome::Duplicate)
        );
        assert_eq!(PublicMatchView::of(&oldest), PublicMatchView::of(&newest));

        assert_eq!(
            select(&mut state, 8, 8),
            attempt(4, OpeningAttemptOutcome::RolledForward)
        );
        let public = PublicMatchView::of(&state);
        assert_eq!(public.last_opening_attempts[1], None);
        let json = serde_json::to_string(&public).unwrap();
        for point in [(2, 3), (4, 5), (6, 7), (8, 8)] {
            assert!(
                !json.contains(&point_json(Point::new(point.0, point.1))),
                "{point:?} leaked: {json}"
            );
        }

        state.reveal_round_zero();
        assert_eq!(
            PublicMatchView::of(&state).last_opening_attempts,
            [None, None],
            "the record belongs to Round 0 and goes with its selections"
        );
    }

    #[test]
    fn cursors_are_public_only_while_playing() {
        let mut state = MatchState::default();
        for phase in [
            MatchPhase::Opening,
            MatchPhase::OpeningCountdown,
            MatchPhase::Ended,
        ] {
            state.phase = phase;
            assert_eq!(PublicMatchView::of(&state).cursors, None);
        }
        state.phase = MatchPhase::Playing;
        assert_eq!(PublicMatchView::of(&state).cursors, Some(PLAYER_STARTS));
    }

    #[test]
    fn resolved_moves_become_public_through_the_shared_board_after_resolution() {
        let mut state = MatchState {
            phase: MatchPhase::Playing,
            progress: crate::authority::MatchProgress {
                round: 1,
                ..Default::default()
            },
            ..MatchState::default()
        };
        state.cursors = [Point::new(0, 0), Point::new(8, 8)];
        state.try_lock(Player::One, MatchInstant::from_secs(1));
        state.try_lock(Player::Two, MatchInstant::from_secs(1));

        assert_eq!(
            PublicMatchView::of(&state).board.get(Point::new(0, 0)),
            crate::rules::Cell::Empty,
            "an unresolved lock must not yet appear on the public board"
        );

        state.resolve_ordinary_round(crate::authority::OrdinaryCompletion::Normal);

        assert_eq!(
            PublicMatchView::of(&state).board.get(Point::new(0, 0)),
            crate::rules::Cell::Black,
            "a resolved move becomes public through the shared board"
        );
    }

    #[test]
    fn teleport_markers_stay_private_to_their_own_player() {
        let state = MatchState {
            teleport_markers: [Some(Point::new(3, 3)), Some(Point::new(6, 6))],
            ..MatchState::default()
        };

        let black = PlayerMatchView::of(&state, Player::One);
        assert_eq!(black.own_teleport_marker, Some(Point::new(3, 3)));
        let json = serde_json::to_string(&black).unwrap();
        assert!(!json.contains("\"x\":6,\"y\":6"));

        let public_json = serde_json::to_string(&PublicMatchView::of(&state)).unwrap();
        assert!(!public_json.contains("\"x\":3,\"y\":3"));
        assert!(!public_json.contains("\"x\":6,\"y\":6"));
    }
}
