//! Shared fixtures used across `rules`' own submodule test suites.
//!
//! The `scoring` and `resolution` test suites import from here with
//! `use crate::rules::test_support::*;` instead of redefining fixtures,
//! mirroring the same pattern in `crate::replay::test_support`. `board`'s
//! example tests and `properties` build their own boards and need no shared
//! fixture.

use crate::clock::MatchInstant;

use super::board::Board;
use super::resolution::LockedMove;
use super::values::{Cell, Player, Point};

pub(super) fn lock(player: Player, x: usize, y: usize, at_seconds: f64) -> LockedMove {
    LockedMove {
        player,
        point: Point::new(x, y),
        at: MatchInstant::from_secs_f64(at_seconds),
    }
}

pub(super) fn at(seconds: f64) -> MatchInstant {
    MatchInstant::from_secs_f64(seconds)
}

pub(super) fn board_where_hidden_black_move_makes_white_suicidal() -> Board {
    let mut board = Board::default();
    board.set(Point::new(1, 0), Cell::White);
    for point in [Point::new(1, 1), Point::new(2, 1), Point::new(3, 0)] {
        board.set(point, Cell::Gray);
    }
    board
}

pub(super) fn merge_created_zero_liberty_group_board() -> Board {
    let mut board = Board::default();
    board.set(Point::new(0, 0), Cell::Black);
    board.set(Point::new(0, 2), Cell::Gray);
    board.set(Point::new(1, 1), Cell::Gray);
    board
}
