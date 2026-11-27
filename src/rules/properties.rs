//! Property-based tests for the pure mechanics in [`super::board`],
//! [`super::resolution`] and [`super::scoring`].
//!
//! These generate small, arbitrary (not necessarily rules-consistent)
//! boards and inputs through [`Board`]'s own public API - never by
//! constructing a `Board` from raw cells, since that field is deliberately
//! not exposed even to tests (see [`super::test_support`] for the
//! hand-crafted fixtures used where a *specific* board shape matters more
//! than broad input coverage). Property tests and the hand-written example
//! tests in `board.rs`/`resolution.rs` are complementary, not redundant:
//! examples pin exact documented behavior for specific board shapes,
//! properties check that broad invariants hold across many shapes at once.
//!
//! # Two generation regimes
//!
//! [`arbitrary_board`] spreads its placements over the whole board and is
//! the right input for the robustness properties - "this never panics",
//! "this never invents a Gray" - which want shapes no real match could
//! reach. It is the wrong input for a property *about* captures: over 256
//! cases it produces no enclosed group at all, so a capture property fed
//! only by it passes no matter what it asserts. [`crowded_board`] packs the
//! same kind of placements into a small region so enclosure is ordinary, and
//! the two are drawn from together where both matter. A property that says
//! something about removals should use [`board_and_move_point`] or
//! [`any_board`], not [`arbitrary_board`] alone.
//!
//! # Constructed preconditions
//!
//! Most properties here generate their whole input. Positional repetition
//! cannot be reached that way: a generated history would essentially never
//! contain the exact board a generated round happens to produce, so the
//! repetition branch would go untested while the property still passed.
//! `a_repeated_position_is_reported_and_grays_the_attempted_empty_points`
//! therefore *constructs* its precondition - it resolves the round once
//! against an empty history and seeds the resulting board back in - and
//! generates only the round itself. That is a deliberate shape, not a
//! shortcut: what is generated is still the board and the pair of
//! selections, which is what the invariant quantifies over.
//!
//! Run just this module's properties with
//! `cargo test --locked rules::properties`; the default case count (256) is
//! deliberately modest so the whole suite - property tests included - stays
//! fast enough for routine development. See `docs/TESTING.md` for how a
//! failing property's minimized case becomes a permanent regression test,
//! and for the list of things the rules intentionally permit, which a
//! property here must never assert against.
use std::collections::HashSet;

use proptest::prelude::*;

use crate::clock::MatchInstant;

use super::board::{Board, CleanupError, MoveError};
use super::resolution::{LockedMove, ResolutionKind, apply_selection, resolve_round};
use super::values::{BOARD_SIZE, Cell, Player, Point};

fn arbitrary_player() -> impl Strategy<Value = Player> {
    prop_oneof![Just(Player::One), Just(Player::Two)]
}

fn arbitrary_cell() -> impl Strategy<Value = Cell> {
    prop_oneof![
        Just(Cell::Empty),
        Just(Cell::Black),
        Just(Cell::White),
        Just(Cell::Gray),
    ]
}

/// A point somewhere on the board.
fn on_board_point() -> impl Strategy<Value = Point> {
    (0..BOARD_SIZE, 0..BOARD_SIZE).prop_map(|(x, y)| Point::new(x, y))
}

/// A point that may be on or beyond either edge, to exercise bounds
/// rejection without ever panicking.
fn any_point() -> impl Strategy<Value = Point> {
    (0..BOARD_SIZE * 3, 0..BOARD_SIZE * 3).prop_map(|(x, y)| Point::new(x, y))
}

/// Every point on the board, in a fixed order, for the properties that
/// have to inspect the whole board rather than one point.
fn all_points() -> impl Iterator<Item = Point> {
    (0..BOARD_SIZE).flat_map(|y| (0..BOARD_SIZE).map(move |x| Point::new(x, y)))
}

/// A small, arbitrary board built only through [`Board::set`] - not
/// necessarily rules-consistent (a placed group may already have zero
/// liberties, mirroring a mid-round snapshot), which is exactly the kind of
/// input `apply_move`/`apply_cleanup` must handle without panicking.
fn arbitrary_board() -> impl Strategy<Value = Board> {
    prop::collection::vec((on_board_point(), arbitrary_cell()), 0..40).prop_map(|placements| {
        let mut board = Board::default();
        for (point, cell) in placements {
            board.set(point, cell);
        }
        board
    })
}

/// Width of the crowded region used by [`crowded_board`].
///
/// Nine intersections - every point with `x < 3` and `y < 3` - have every
/// neighbour they possess inside a 4-wide region anchored at the origin (the
/// board edge walls off the rest), so a group there can actually be
/// enclosed. A stone at the region's far edge still borders the empty rest
/// of the board and keeps a liberty forever, which is fine: it is the
/// enclosable interior that the crowded generator exists to reach.
const CROWDED_REGION: usize = 4;

/// A point inside the crowded region.
fn crowded_point() -> impl Strategy<Value = Point> {
    (0..CROWDED_REGION, 0..CROWDED_REGION).prop_map(|(x, y)| Point::new(x, y))
}

/// A board whose stones are packed into one small region, so neighbouring
/// groups actually enclose one another.
///
/// [`arbitrary_board`] is the right generator for the robustness
/// properties, since it deliberately reaches shapes no real match could,
/// but it spreads a few dozen placements over all 81 intersections, which
/// leaves an enclosed group so rare that a property about captures observes
/// none at all. That is measured, not assumed: with `arbitrary_board`
/// alone, negating the capture-accounting property's assertions still
/// leaves it passing over 256 cases, because no generated move ever removes
/// a stone. Concentrating the same kind of placements into
/// [`CROWDED_REGION`] makes captures and self-removals ordinary. It still
/// builds only through [`Board::set`].
fn crowded_board() -> impl Strategy<Value = Board> {
    prop::collection::vec((crowded_point(), arbitrary_cell()), 12..48).prop_map(|placements| {
        let mut board = Board::default();
        for (point, cell) in placements {
            board.set(point, cell);
        }
        board
    })
}

/// Both board regimes together: sparse-and-arbitrary for breadth, crowded
/// for the enclosures the sparse one never reaches.
fn any_board() -> impl Strategy<Value = Board> {
    prop_oneof![arbitrary_board(), crowded_board()]
}

/// A board paired with a point to act on, keeping a crowded board paired
/// with a point inside its crowded region - a crowded board acted on at some
/// far-away empty intersection would be no denser than a sparse one.
fn board_and_move_point() -> impl Strategy<Value = (Board, Point)> {
    prop_oneof![
        (arbitrary_board(), on_board_point()),
        (crowded_board(), crowded_point()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `apply_move` is a pure function of its inputs: the same board, player
    /// and point always produce the same outcome, never observing hidden
    /// state or prior calls.
    #[test]
    fn apply_move_is_deterministic(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in on_board_point(),
    ) {
        let first = board.apply_move(player, point);
        let second = board.apply_move(player, point);
        prop_assert_eq!(first, second);
    }

    /// A rejected move (out of bounds or occupied) must never mutate the
    /// board it was rejected against - `apply_move` takes `&self`, but the
    /// documented contract is that the *caller's* board is untouched on
    /// `Err`, which this checks by comparing before and after.
    #[test]
    fn a_rejected_move_leaves_the_board_unchanged(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in any_point(),
    ) {
        let before = board.clone();
        if board.apply_move(player, point).is_err() {
            prop_assert_eq!(board, before);
        }
    }

    /// Out-of-bounds points are rejected cleanly, never panicking and never
    /// silently wrapping/clamping onto the board.
    ///
    /// `apply_selection` is checked alongside the two `Board` entry points it
    /// dispatches to, because it reads the target cell itself to choose
    /// between them and so needs its own bounds check rather than inheriting
    /// theirs. Leaving it out of this property is exactly how it came to be
    /// missing one. Random points make the panicking half of that likely and
    /// the aliasing half (off-board but in-range index) rare, so
    /// `resolution.rs` pins the latter by example as well.
    #[test]
    fn out_of_bounds_points_are_rejected_not_panicked_on(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in any_point(),
    ) {
        let out_of_bounds = point.x >= BOARD_SIZE || point.y >= BOARD_SIZE;
        if out_of_bounds {
            prop_assert_eq!(board.apply_move(player, point), Err(MoveError::OutOfBounds));
            prop_assert_eq!(board.apply_cleanup(player, point), Err(CleanupError::OutOfBounds));
            prop_assert_eq!(
                apply_selection(&board, player, point).err(),
                Some(MoveError::OutOfBounds),
            );
        }
    }

    /// A successful move places the acting player's color at the target
    /// point unless the placement fully self-removed (suicide) - one of the
    /// two documented outcomes, never a silent third possibility.
    #[test]
    fn a_successful_move_either_places_the_stone_or_fully_self_removes(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in on_board_point(),
    ) {
        if board.get(point) != Cell::Empty {
            return Ok(());
        }
        if let Ok(outcome) = board.apply_move(player, point) {
            if outcome.placed_stone_remains {
                prop_assert_eq!(outcome.board.get(point), player.cell());
            } else {
                prop_assert_eq!(outcome.board.get(point), Cell::Empty);
                prop_assert!(outcome.acting_stones_self_removed.contains(&point));
            }
        }
    }

    /// `apply_move` never introduces a `Gray` cell: within `rules`, Gray is
    /// produced only by `resolution`'s same-point/repetition handling, a
    /// level above pure board mechanics - the Round 0 reveal in
    /// `crate::opening` is the one producer outside `rules`. See
    /// `resolution.rs`'s module doc.
    #[test]
    fn ordinary_moves_never_produce_gray(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in on_board_point(),
    ) {
        if let Ok(outcome) = board.apply_move(player, point) {
            for y in 0..BOARD_SIZE {
                for x in 0..BOARD_SIZE {
                    let cell = outcome.board.get(Point::new(x, y));
                    let was_gray = board.get(Point::new(x, y)) == Cell::Gray;
                    prop_assert!(was_gray || cell != Cell::Gray);
                }
            }
        }
    }

    /// A successful cleanup never removes the acting player's own stone at
    /// the point it acted from, and every removed stone was the opponent's
    /// color on the original board - cleanup targets only adjacent
    /// zero-liberty *opponent* groups, by construction.
    #[test]
    fn cleanup_never_removes_the_acting_players_own_point_or_color(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in on_board_point(),
    ) {
        if let Ok(outcome) = board.apply_cleanup(player, point) {
            prop_assert!(!outcome.captured_stones.contains(&point));
            for captured in &outcome.captured_stones {
                prop_assert_eq!(board.get(*captured), player.opponent_cell());
            }
            // The acting player's own stone survives the cleanup untouched.
            prop_assert_eq!(outcome.board.get(point), player.cell());
        }
    }

    /// Cleanup is deterministic, exactly like `apply_move`.
    #[test]
    fn apply_cleanup_is_deterministic(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in on_board_point(),
    ) {
        let first = board.apply_cleanup(player, point);
        let second = board.apply_cleanup(player, point);
        prop_assert_eq!(first, second);
    }

    /// A `Gray` cell is a permanent neutral wall: no board operation this
    /// module can reach - an ordinary move or a cleanup, successful or not -
    /// ever converts an existing `Gray` cell into anything else, or removes
    /// it as a captured stone.
    #[test]
    fn gray_is_never_converted_or_captured_by_board_operations(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in on_board_point(),
    ) {
        let gray_points: Vec<Point> = (0..BOARD_SIZE)
            .flat_map(|y| (0..BOARD_SIZE).map(move |x| Point::new(x, y)))
            .filter(|&candidate| board.get(candidate) == Cell::Gray)
            .collect();

        if let Ok(outcome) = board.apply_move(player, point) {
            for gray_point in &gray_points {
                prop_assert_eq!(outcome.board.get(*gray_point), Cell::Gray);
            }
        }
        if let Ok(outcome) = board.apply_cleanup(player, point) {
            for gray_point in &gray_points {
                prop_assert_eq!(outcome.board.get(*gray_point), Cell::Gray);
            }
        }
    }

    /// Reported captures account for every stone the move removed, and for
    /// nothing else.
    ///
    /// These two vectors are not decoration: they become
    /// [`super::resolution::RoundResolution::capture_credits`], which become
    /// the running `capture_totals` a Replay persists, which feed both the
    /// final score and the Point Target end condition. A miscount there is a
    /// wrong score rather than a crash - the kind of defect nobody notices
    /// until a player disputes one - so this checks the counts against what
    /// actually happened to the board rather than against themselves.
    ///
    /// The conservation clause is the half that makes it worth having: an
    /// unreported removal, a stone that quietly vanished, would satisfy every
    /// per-stone assertion above it.
    #[test]
    fn reported_removals_match_the_stones_the_move_actually_removed(
        (board, point) in board_and_move_point(),
        player in arbitrary_player(),
    ) {
        let Ok(outcome) = board.apply_move(player, point) else {
            return Ok(());
        };

        for captured in &outcome.opponent_stones_captured {
            prop_assert_eq!(board.get(*captured), player.opponent_cell());
            prop_assert_eq!(outcome.board.get(*captured), Cell::Empty);
        }
        for self_removed in &outcome.acting_stones_self_removed {
            prop_assert_eq!(outcome.board.get(*self_removed), Cell::Empty);
        }

        let captured: HashSet<Point> = outcome.opponent_stones_captured.iter().copied().collect();
        let self_removed: HashSet<Point> =
            outcome.acting_stones_self_removed.iter().copied().collect();
        prop_assert!(
            captured.is_disjoint(&self_removed),
            "a stone cannot be both captured from the opponent and self-removed"
        );

        // Conservation: the only cells that went from occupied to empty are
        // the reported ones. Note the acting player's *placed* stone can
        // appear in `acting_stones_self_removed` without appearing here - it
        // was empty before the move - which is why this is stated as "every
        // removal is reported", not "every report is a removal".
        for candidate in all_points() {
            let vanished =
                board.get(candidate) != Cell::Empty && outcome.board.get(candidate) == Cell::Empty;
            prop_assert!(
                !vanished || captured.contains(&candidate) || self_removed.contains(&candidate),
                "{:?} was removed without being reported in either outcome list",
                candidate
            );
        }
    }

    /// Filling territory twice equals filling it once, and never disturbs a
    /// cell that already held a stone.
    ///
    /// `authority::MatchState::finish` is the only production caller of
    /// [`Board::fill_territory`], and the board it fills is what a Replay's
    /// final frame stores. Everything else that fills - Replay fixtures, the
    /// holds pipeline's tests, playback and validation tests - starts from a
    /// board it did not build, so a second pass reaching a different answer
    /// than the first would make a score depend on how many times it
    /// happened to be computed.
    #[test]
    fn fill_territory_is_idempotent_and_leaves_occupied_cells_alone(board in any_board()) {
        let mut once = board.clone();
        once.fill_territory();
        let mut twice = once.clone();
        twice.fill_territory();
        prop_assert_eq!(&once, &twice);

        for point in all_points() {
            if board.get(point) != Cell::Empty {
                prop_assert_eq!(once.get(point), board.get(point));
            }
        }
    }

    /// [`Board`]'s hand-written [`serde`] implementations round-trip any
    /// board exactly.
    ///
    /// Every event frame in a saved Replay carries a board, so this is a
    /// persisted-compatibility surface, not just an internal convenience: a
    /// serializer that dropped, reordered, or defaulted a cell would corrupt
    /// stored matches silently. The rejection paths - a wrong cell count, an
    /// unknown field - are enforced by `BoardFile`'s `deny_unknown_fields`
    /// and the `try_into` length check in `board.rs`, but have no example
    /// test of their own; this covers the happy path across arbitrary
    /// contents rather than one hand-picked shape.
    #[test]
    fn a_board_survives_a_serde_round_trip_unchanged(board in arbitrary_board()) {
        let json = serde_json::to_string(&board).expect("a board always serializes");
        let restored: Board =
            serde_json::from_str(&json).expect("a board this crate serialized always parses");
        prop_assert_eq!(restored, board);
    }
}

/// Two distinct on-board points, for genuinely simultaneous (not
/// same-point) resolution properties - same-point simultaneous selections
/// have their own documented, already example-tested tie-break rule
/// (`same_point_within_window_becomes_gray` and
/// `identical_timestamps_resolve_simultaneously_with_argument_order_independent_tie_breaking`
/// in `resolution.rs`), which this deliberately does not re-derive.
fn two_distinct_on_board_points() -> impl Strategy<Value = (Point, Point)> {
    (on_board_point(), on_board_point()).prop_filter("points must differ", |(a, b)| a != b)
}

fn valid_instant() -> impl Strategy<Value = MatchInstant> {
    (0u64..1_000_000).prop_map(MatchInstant::from_micros)
}

/// Zero, one, or two locked selections for a single round - every shape
/// [`resolve_round`] accepts - always on distinct points, so the same-point
/// tie-break stays out of scope for the same reason as above.
fn arbitrary_pending_round() -> impl Strategy<Value = [Option<LockedMove>; 2]> {
    (
        two_distinct_on_board_points(),
        valid_instant(),
        valid_instant(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |((point_one, point_two), at_one, at_two, black_locked, white_locked)| {
                [
                    black_locked.then_some(LockedMove {
                        player: Player::One,
                        point: point_one,
                        at: at_one,
                    }),
                    white_locked.then_some(LockedMove {
                        player: Player::Two,
                        point: point_two,
                        at: at_two,
                    }),
                ]
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Simultaneous resolution of two *different* points must not depend on
    /// which position in the `[Option<LockedMove>; 2]` array each player's
    /// lock occupies - `LockedMove` already carries its own `player` field,
    /// so array order is bookkeeping, not meaning, and swapping it must
    /// produce the identical resulting board.
    #[test]
    fn simultaneous_different_points_resolve_the_same_regardless_of_argument_order(
        board in arbitrary_board(),
        (point_a, point_b) in two_distinct_on_board_points(),
        at in valid_instant(),
    ) {
        let lock_black = LockedMove { player: Player::One, point: point_a, at };
        let lock_white = LockedMove { player: Player::Two, point: point_b, at };
        let history = HashSet::new();

        let forward = resolve_round(&board, [Some(lock_black), Some(lock_white)], &history);
        let backward = resolve_round(&board, [Some(lock_white), Some(lock_black)], &history);

        prop_assert_eq!(
            forward.map(|resolution| resolution.board),
            backward.map(|resolution| resolution.board)
        );
    }

    /// A single locked move (the other player passed/has no pending
    /// selection) resolves without panicking for any on-board point and any
    /// arbitrary starting board, and always reports a `NoPlacement` or
    /// `Single` resolution kind - never `Simultaneous`/`Sequential`, which
    /// require two locks.
    #[test]
    fn a_single_lock_never_panics_and_never_reports_a_two_player_resolution_kind(
        board in arbitrary_board(),
        player in arbitrary_player(),
        point in on_board_point(),
        at in valid_instant(),
    ) {
        let lock = LockedMove { player, point, at };
        let pending = match player {
            Player::One => [Some(lock), None],
            Player::Two => [None, Some(lock)],
        };
        if let Ok(resolution) = resolve_round(&board, pending, &HashSet::new()) {
            prop_assert!(matches!(
                resolution.kind,
                ResolutionKind::NoPlacement | ResolutionKind::Single
            ));
        }
    }

    /// Positional repetition converts the round's attempted points to Gray
    /// instead of replaying a board the Match has already held.
    ///
    /// Unlike every other property here, this one cannot *generate* its
    /// precondition: a randomly built history would essentially never
    /// contain the exact board a randomly generated round produces. So it
    /// constructs the precondition instead - resolve the round once against
    /// an empty history, then resolve the identical round again with the
    /// board that produced seeded into the history. `resolve_round` is pure,
    /// so the second call reaches the same candidate board and must now see
    /// it as a repetition.
    ///
    /// This asserts the *mechanism* and deliberately not a prohibition:
    /// repetition-to-Gray is documented as intentionally exploitable (see
    /// `docs/TESTING.md`), so nothing here claims repetition is prevented,
    /// discouraged, or rare - only that when it is detected, the round
    /// reports it and Grays exactly the points that were attempted onto
    /// empty intersections.
    #[test]
    fn a_repeated_position_is_reported_and_grays_the_attempted_empty_points(
        board in arbitrary_board(),
        pending in arbitrary_pending_round(),
    ) {
        let Ok(first) = resolve_round(&board, pending, &HashSet::new()) else {
            return Ok(());
        };
        // A round that resolved to no placement at all - no locks, or a
        // single lock that turned out to be illegal - never consults the
        // history, so there is no repetition to observe.
        if first.kind == ResolutionKind::NoPlacement {
            return Ok(());
        }
        prop_assert!(!first.repetition, "an empty history can never repeat");

        let history = HashSet::from([first.board.clone()]);
        let second = resolve_round(&board, pending, &history);
        prop_assert!(
            second.is_ok(),
            "resolution is pure: a round that resolved once must resolve again"
        );
        let second = second.unwrap();

        prop_assert!(second.repetition);
        prop_assert_eq!(second.kind, ResolutionKind::KoGray);
        prop_assert_eq!(second.capture_credits, [0, 0]);
        prop_assert!(second.removed.is_empty(), "a rolled-back round removed nothing");
        for attempted in second.attempted_valid_points.into_iter().flatten() {
            if board.get(attempted) == Cell::Empty {
                prop_assert_eq!(second.board.get(attempted), Cell::Gray);
            }
        }
    }

    /// `RoundResolution::removed` is the record the live board marks and a
    /// screen reader will read, so it has to be complete, honest and
    /// deterministic: every stone that was on the board and is not any
    /// more is listed with the owner it had; every entry names a stone
    /// that was there or was placed this round by that owner; and the list
    /// is in cell order with no point twice. The sequential case where one
    /// point empties twice is pinned by example in `resolution.rs`.
    #[test]
    fn a_round_lists_every_stone_it_removed_once_with_its_owner(
        board in any_board(),
        pending in arbitrary_pending_round(),
    ) {
        let Ok(resolved) = resolve_round(&board, pending, &HashSet::new()) else {
            return Ok(());
        };

        let indices: Vec<usize> = resolved
            .removed
            .iter()
            .map(|stone| stone.point.y * BOARD_SIZE + stone.point.x)
            .collect();
        let mut expected = indices.clone();
        expected.sort_unstable();
        expected.dedup();
        prop_assert_eq!(&indices, &expected, "removals are in cell order, one per point");

        for stone in &resolved.removed {
            let placed_here = pending
                .iter()
                .flatten()
                .any(|locked| locked.player == stone.owner && locked.point == stone.point);
            prop_assert!(
                board.get(stone.point) == stone.owner.cell() || placed_here,
                "{:?} names a stone that was never on the board",
                stone
            );
            // What took it is one of the round's own selections, and an
            // ordinary round always has one: only the reveal sweep has none.
            prop_assert!(
                pending
                    .iter()
                    .flatten()
                    .any(|locked| Some(locked.point) == stone.by),
                "{:?} was taken by a point nobody selected",
                stone
            );
        }

        for candidate in all_points() {
            let owner = match board.get(candidate) {
                Cell::Black => Player::One,
                Cell::White => Player::Two,
                Cell::Empty | Cell::Gray => continue,
            };
            if resolved.board.get(candidate) == Cell::Empty {
                prop_assert!(
                    resolved
                        .removed
                        .iter()
                        .any(|stone| stone.point == candidate && stone.owner == owner),
                    "{:?} left the board without being listed",
                    candidate
                );
            }
        }
    }
}
