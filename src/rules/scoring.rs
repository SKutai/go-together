//! Territory filling, final scoring, and the Round 0 reveal's one-time
//! capture sweep - the parts of [`super::board::Board`] that evaluate a
//! finished or revealed position rather than one player's action.

use std::collections::HashSet;

use super::board::{Board, neighbors};
use super::values::{BOARD_SIZE, CELL_COUNT, Cell, Player, Point, RemovedStone, Score};

/// What the Round 0 reveal sweep did to the raw revealed board.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpeningSweep {
    /// The revealed board with every zero-liberty group removed.
    pub board: Board,
    /// Credits indexed like [`Player::index`]: each player is credited for
    /// every opposing stone the sweep removed.
    pub capture_credits: [usize; 2],
    /// Every stone the sweep removed, with its owner, sorted by cell index.
    pub removed: Vec<RemovedStone>,
}

impl Board {
    /// Fills empty regions bordered by exactly one player color.
    ///
    /// Gray blocks flood-fill traversal but is deliberately excluded from the
    /// owner set, so it acts as a neutral wall rather than territory.
    pub fn fill_territory(&mut self) {
        let mut visited = [false; CELL_COUNT];

        for index in 0..CELL_COUNT {
            let start = Point::new(index % BOARD_SIZE, index / BOARD_SIZE);
            if visited[index] || self.get(start) != Cell::Empty {
                continue;
            }

            let mut region = Vec::new();
            let mut boundary = HashSet::new();
            let mut stack = vec![start];
            visited[index] = true;

            while let Some(point) = stack.pop() {
                region.push(point);
                for neighbor in neighbors(point) {
                    match self.get(neighbor) {
                        Cell::Empty => {
                            if !visited[neighbor.index()] {
                                visited[neighbor.index()] = true;
                                stack.push(neighbor);
                            }
                        }
                        Cell::Black | Cell::White => {
                            boundary.insert(self.get(neighbor));
                        }
                        Cell::Gray => {}
                    }
                }
            }

            // The `len() == 1` guard is load-bearing for determinism, not
            // just a readable way to say "bordered by one colour". `boundary`
            // is a `HashSet`, and Rust's default hasher is randomly seeded
            // per process, so iterating a set holding more than one element
            // yields a process-dependent order. Taking the first element is
            // only well-defined because the guard has already established
            // there is exactly one.
            //
            // A multi-element boundary must therefore never be resolved by
            // iteration order - not even to "pick the majority colour" or
            // "prefer whichever comes first". Relaxing this condition without
            // introducing an explicit, order-independent rule would make
            // territory ownership differ between runs of the same binary: it
            // would compile, pass on the machine that wrote it, and fail
            // intermittently everywhere.
            let owner = if boundary.len() == 1 {
                boundary.iter().next().copied()
            } else {
                None
            };
            if let Some(owner) = owner {
                for point in region {
                    self.set(point, owner);
                }
            }
        }
    }

    /// Counts Black and White cells without prisoners, komi, or Gray points.
    pub fn score(&self) -> Score {
        self.cells.iter().fold(Score::default(), |mut score, cell| {
            match cell {
                Cell::Black => score.black += 1,
                Cell::White => score.white += 1,
                Cell::Empty | Cell::Gray => {}
            }
            score
        })
    }

    /// Current board-area score plus optional captured-stone bonuses.
    pub fn score_with_captures(&self, capture_credits: Score, capture_points: bool) -> Score {
        let mut score = self.score();
        if capture_points {
            score.black = score.black.saturating_add(capture_credits.black);
            score.white = score.white.saturating_add(capture_credits.white);
        }
        score
    }

    /// Sweeps a freshly revealed Round 0 board for every zero-liberty Black
    /// or White group and removes them all at once.
    ///
    /// Every group's liberties are read from this same starting board, so
    /// neither color is evaluated first and no removal can create a liberty
    /// that saves the other color's group - mirroring the no-priority
    /// semantics simultaneous ordinary-round resolution already uses. Gray
    /// stones are permanent neutral walls: they count as occupied for
    /// liberties and are never candidates for removal. Returns the cleaned
    /// board, capture credits (indexed like [`Player::index`]) crediting
    /// each player for every opposing stone removed, and the removed stones
    /// themselves.
    pub fn resolve_opening_captures(&self) -> OpeningSweep {
        let mut visited = [false; CELL_COUNT];
        let mut removed = [HashSet::new(), HashSet::new()];

        for index in 0..CELL_COUNT {
            if visited[index] {
                continue;
            }
            let start = Point::new(index % BOARD_SIZE, index / BOARD_SIZE);
            let player = match self.get(start) {
                Cell::Black => Player::One,
                Cell::White => Player::Two,
                Cell::Empty | Cell::Gray => {
                    visited[index] = true;
                    continue;
                }
            };

            let group = self.group_at(start);
            for point in &group {
                visited[point.index()] = true;
            }
            if self.liberties(&group).is_empty() {
                removed[player.index()].extend(group);
            }
        }

        let mut cleaned = self.clone();
        for group in &removed {
            for point in group {
                cleaned.set(*point, Cell::Empty);
            }
        }

        let capture_credits = [
            removed[Player::Two.index()].len(),
            removed[Player::One.index()].len(),
        ];
        let mut removed_stones: Vec<RemovedStone> = Player::ALL
            .into_iter()
            .flat_map(|owner| {
                removed[owner.index()]
                    .iter()
                    .map(move |point| RemovedStone {
                        point: *point,
                        owner,
                        by: None,
                    })
            })
            .collect();
        removed_stones.sort_by_key(|stone| stone.point.index());
        OpeningSweep {
            board: cleaned,
            capture_credits,
            removed: removed_stones,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::resolution::resolve_round;
    use crate::rules::test_support::*;

    #[test]
    fn surviving_zero_liberty_stones_score_normally_at_match_end() {
        let board = merge_created_zero_liberty_group_board();
        let final_board = resolve_round(
            &board,
            [
                Some(lock(Player::One, 0, 1, 1.0)),
                Some(lock(Player::Two, 1, 0, 1.05)),
            ],
            &HashSet::from([board.clone()]),
        )
        .unwrap()
        .board;

        // No cleanup was spent before the Match ended: the zero-liberty
        // Black group is still on the board and still scores like any other
        // Black stones, exactly as if it had liberties. `score` counts
        // placed stones directly and has no notion of liberties at all, so
        // it cannot special-case a stone for lacking them.
        let score = final_board.score();

        assert_eq!(score.black, 2);
        assert_eq!(score.white, 1);
    }

    #[test]
    fn territory_regions_bounded_by_gray_and_one_color_belong_to_that_color() {
        // Two Gray walls split the board into three column strips: a Black
        // strip, an entirely empty strip walled in by Gray on both sides,
        // and a White strip.
        let mut board = Board::default();
        for y in 0..BOARD_SIZE {
            board.set(Point::new(3, y), Cell::Gray);
            board.set(Point::new(6, y), Cell::Gray);
        }
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(8, 8), Cell::White);

        board.fill_territory();

        assert_eq!(board.get(Point::new(2, 8)), Cell::Black);
        assert_eq!(board.get(Point::new(7, 0)), Cell::White);
        // Bounded only by Gray (and the board edge) on every side: neither
        // player owns it, so it stays empty rather than being claimed.
        assert_eq!(board.get(Point::new(4, 4)), Cell::Empty);
        assert_eq!(board.get(Point::new(5, 4)), Cell::Empty);
    }

    #[test]
    fn territory_touching_both_colors_has_no_owner() {
        // The contract the `len() == 1` guard in `fill_territory` provides:
        // a region whose boundary holds more than one colour is owned by
        // nobody. Pinning it here keeps that guard from being relaxed into
        // an iteration-order-dependent "pick a winner" rule, which would
        // make scoring differ between runs of the same binary.
        let mut board = Board::default();
        for y in 0..BOARD_SIZE {
            board.set(Point::new(2, y), Cell::Black);
            board.set(Point::new(6, y), Cell::White);
        }

        board.fill_territory();

        // The middle strip is walled by Black on one side and White on the
        // other, so neither owns it.
        for x in 3..6 {
            for y in 0..BOARD_SIZE {
                assert_eq!(
                    board.get(Point::new(x, y)),
                    Cell::Empty,
                    "({x}, {y}) borders both colours and must stay unowned"
                );
            }
        }
        // The single-colour strips on the outside still fill normally, so the
        // test is pinning the two-colour case rather than a board on which
        // nothing fills at all.
        assert_eq!(board.get(Point::new(0, 4)), Cell::Black);
        assert_eq!(board.get(Point::new(8, 4)), Cell::White);
    }

    #[test]
    fn gray_wall_separates_territory_regions() {
        let mut board = Board::default();
        for y in 0..BOARD_SIZE {
            board.set(Point::new(4, y), Cell::Gray);
        }
        board.set(Point::new(0, 0), Cell::Black);

        board.fill_territory();

        assert_eq!(board.get(Point::new(3, 8)), Cell::Black);
        assert_eq!(board.get(Point::new(5, 8)), Cell::Empty);
        assert_eq!(board.get(Point::new(4, 4)), Cell::Gray);
    }

    #[test]
    fn score_counts_only_player_stones_after_territory_fill() {
        let mut board = Board::default();
        for y in 0..BOARD_SIZE {
            board.set(Point::new(4, y), Cell::Gray);
        }
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(8, 8), Cell::White);

        board.fill_territory();
        let score = board.score();

        assert_eq!(score.black, 36);
        assert_eq!(score.white, 36);
    }

    #[test]
    fn opening_reveal_capture_removes_a_zero_liberty_corner_group() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(1, 0), Cell::White);
        board.set(Point::new(0, 1), Cell::White);

        let OpeningSweep {
            board: cleaned,
            capture_credits,
            removed,
        } = board.resolve_opening_captures();

        assert_eq!(cleaned.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(cleaned.get(Point::new(1, 0)), Cell::White);
        assert_eq!(cleaned.get(Point::new(0, 1)), Cell::White);
        assert_eq!(
            capture_credits,
            [0, 1],
            "White removed one Black stone, so only White is credited"
        );
        assert_eq!(
            removed,
            vec![RemovedStone {
                point: Point::new(0, 0),
                owner: Player::One,
                by: None,
            }],
            "the sweep has no acting player, so nothing took the stone"
        );
    }

    #[test]
    fn opening_reveal_capture_counts_gray_as_a_wall_but_never_removes_it() {
        let mut board = Board::default();
        board.set(Point::new(4, 4), Cell::Black);
        board.set(Point::new(3, 4), Cell::Gray);
        board.set(Point::new(5, 4), Cell::Gray);
        board.set(Point::new(4, 3), Cell::White);
        board.set(Point::new(4, 5), Cell::White);

        let OpeningSweep {
            board: cleaned,
            capture_credits,
            removed,
        } = board.resolve_opening_captures();

        assert_eq!(cleaned.get(Point::new(4, 4)), Cell::Empty);
        assert_eq!(cleaned.get(Point::new(3, 4)), Cell::Gray);
        assert_eq!(cleaned.get(Point::new(5, 4)), Cell::Gray);
        assert_eq!(cleaned.get(Point::new(4, 3)), Cell::White);
        assert_eq!(cleaned.get(Point::new(4, 5)), Cell::White);
        assert_eq!(capture_credits, [0, 1]);
        assert_eq!(
            removed,
            vec![RemovedStone {
                point: Point::new(4, 4),
                owner: Player::One,
                by: None,
            }],
            "the Gray walls are never listed as removed"
        );
    }

    #[test]
    fn opening_reveal_capture_removes_both_colors_with_no_priority() {
        // The corner Black stone's only two neighbors are White, and those
        // two White stones' only non-Black neighbor is that same Black
        // corner - so all three are zero-liberty only when evaluated
        // together. Removing White first would hand the Black corner a
        // liberty and save it; removing Black first would hand both White
        // stones a liberty and save them. Only simultaneous, same-snapshot
        // removal takes all three at once.
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(1, 0), Cell::White);
        board.set(Point::new(0, 1), Cell::White);
        board.set(Point::new(2, 0), Cell::Black);
        board.set(Point::new(1, 1), Cell::Black);
        board.set(Point::new(0, 2), Cell::Black);

        let OpeningSweep {
            board: cleaned,
            capture_credits,
            removed,
        } = board.resolve_opening_captures();

        assert_eq!(cleaned.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(cleaned.get(Point::new(1, 0)), Cell::Empty);
        assert_eq!(cleaned.get(Point::new(0, 1)), Cell::Empty);
        assert_eq!(cleaned.get(Point::new(2, 0)), Cell::Black);
        assert_eq!(cleaned.get(Point::new(1, 1)), Cell::Black);
        assert_eq!(cleaned.get(Point::new(0, 2)), Cell::Black);
        assert_eq!(
            capture_credits,
            [2, 1],
            "Black is credited the 2 removed White stones, White the 1 removed Black stone"
        );
        // Listed by cell index with each stone's own colour, so the two
        // removed groups of different colours are told apart by owner.
        let stone = |x, y, owner| RemovedStone {
            point: Point::new(x, y),
            owner,
            by: None,
        };
        assert_eq!(
            removed,
            vec![
                stone(0, 0, Player::One),
                stone(1, 0, Player::Two),
                stone(0, 1, Player::Two),
            ]
        );
    }

    #[test]
    fn capture_score_helper_preserves_disabled_area_scoring() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(8, 8), Cell::White);
        let captures = Score { black: 3, white: 5 };

        assert_eq!(board.score_with_captures(captures, false), board.score());
        assert_eq!(
            board.score_with_captures(captures, true),
            Score { black: 4, white: 6 }
        );
    }
}
