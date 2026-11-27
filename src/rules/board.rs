//! [`Board`] representation, topology, and the individual placement,
//! suicide, capture, and active-cleanup effects one action has on it.
//!
//! Everything here operates on a single `Board` value with no notion of
//! timing, hidden selections, or round resolution - see
//! [`super::resolution`] for how two players' selections combine into a
//! completed round, and [`super::scoring`] for territory and final scoring.

use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::values::{BOARD_SIZE, CELL_COUNT, Cell, Player, Point};

/// Fixed-size board value.
///
/// Custom serde keeps the JSON representation stable while rejecting files
/// whose cell count does not match [`CELL_COUNT`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Board {
    pub(super) cells: [Cell; CELL_COUNT],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BoardFile {
    cells: Vec<Cell>,
}

impl Serialize for Board {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        BoardFile {
            cells: self.cells.to_vec(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Board {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let file = BoardFile::deserialize(deserializer)?;
        let cells: [Cell; CELL_COUNT] = file.cells.try_into().map_err(|cells: Vec<Cell>| {
            D::Error::custom(format!(
                "expected {CELL_COUNT} board cells, found {}",
                cells.len()
            ))
        })?;
        Ok(Self { cells })
    }
}

impl Default for Board {
    fn default() -> Self {
        Self {
            cells: [Cell::Empty; CELL_COUNT],
        }
    }
}

/// Authoritative result of one structurally valid ordinary placement.
///
/// Opponent groups are removed before the acting group is evaluated. A
/// no-liberty acting group is then removed in full, including friendly stones
/// that existed before the placement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveOutcome {
    pub board: Board,
    pub opponent_stones_captured: Vec<Point>,
    pub acting_stones_self_removed: Vec<Point>,
    pub placed_stone_remains: bool,
}

/// Why an ordinary placement could not be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveError {
    OutOfBounds,
    Occupied,
}

/// Result of successfully removing every opposing zero-liberty group
/// orthogonally adjacent to one of the acting player's own stones.
///
/// No new stone is placed and the selected friendly stone is left in place;
/// only opposing stones are ever removed by this action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CleanupOutcome {
    pub board: Board,
    pub captured_stones: Vec<Point>,
}

/// Why an active cleanup action cannot be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupError {
    OutOfBounds,
    NotOwnStone,
    NoCapturableGroup,
}

impl Board {
    /// Reads one cell.
    ///
    /// `point` must be on the board: neither this nor [`Board::set`] checks
    /// bounds, so an `x` at or past [`BOARD_SIZE`] silently aliases a cell on
    /// the next row, and an index past [`CELL_COUNT`] panics. The
    /// rule-applying entry points - [`Board::apply_move`],
    /// [`Board::apply_cleanup`], `apply_selection`, and `validate_lock` -
    /// check bounds first and report `OutOfBounds` instead.
    pub fn get(&self, point: Point) -> Cell {
        self.cells[point.index()]
    }

    /// Writes one cell directly, applying no rule: no capture, no suicide
    /// check, no occupancy check.
    ///
    /// This is how the Round 0 reveal in [`crate::opening`] and every test
    /// fixture builds a position; ordinary play goes through
    /// [`Board::apply_move`] and [`Board::apply_cleanup`]. Same bounds
    /// contract as [`Board::get`].
    pub fn set(&mut self, point: Point, cell: Cell) {
        self.cells[point.index()] = cell;
    }

    /// Applies one ordinary move without mutating the input board.
    ///
    /// The acting stone is placed, adjacent dead opponent groups are removed,
    /// and only then is the acting group evaluated. If that group has no
    /// liberties, the entire group self-removes. Only structural placement
    /// errors fail; suicide is a successful outcome.
    pub fn apply_move(&self, player: Player, point: Point) -> Result<MoveOutcome, MoveError> {
        if point.x >= BOARD_SIZE || point.y >= BOARD_SIZE {
            return Err(MoveError::OutOfBounds);
        }
        if self.get(point) != Cell::Empty {
            return Err(MoveError::Occupied);
        }

        let mut board = self.clone();
        board.set(point, player.cell());

        let mut opponent_stones_captured =
            board.adjacent_zero_liberty_opponent_groups(player, point);

        for captured_point in &opponent_stones_captured {
            board.set(*captured_point, Cell::Empty);
        }

        let acting_group = board.group_at(point);
        let acting_stones_self_removed = if board.liberties(&acting_group).is_empty() {
            for self_removed_point in &acting_group {
                board.set(*self_removed_point, Cell::Empty);
            }
            acting_group
        } else {
            Vec::new()
        };
        debug_assert!(
            opponent_stones_captured.is_empty() || acting_stones_self_removed.is_empty(),
            "capturing an adjacent group creates a liberty for the acting group"
        );

        opponent_stones_captured.sort_by_key(|captured_point| captured_point.index());
        let mut acting_stones_self_removed = acting_stones_self_removed;
        acting_stones_self_removed.sort_by_key(|removed_point| removed_point.index());
        Ok(MoveOutcome {
            placed_stone_remains: board.get(point) == player.cell(),
            board,
            opponent_stones_captured,
            acting_stones_self_removed,
        })
    }

    /// Every orthogonally adjacent opposing group with zero liberties on
    /// this board, evaluated once per group regardless of how many of its
    /// stones border `point`, and with no group given priority over another.
    fn adjacent_zero_liberty_opponent_groups(&self, player: Player, point: Point) -> Vec<Point> {
        let mut captured = Vec::new();
        let mut checked = HashSet::new();
        for neighbor in neighbors(point) {
            if self.get(neighbor) != player.opponent_cell() || checked.contains(&neighbor) {
                continue;
            }

            let group = self.group_at(neighbor);
            checked.extend(group.iter().copied());
            if self.liberties(&group).is_empty() {
                captured.extend(group);
            }
        }
        captured
    }

    /// Actively removes every opposing zero-liberty group orthogonally
    /// adjacent to one of the player's own stones, without placing a new
    /// stone or moving the selected one. Only structural preconditions fail:
    /// the point must hold the acting player's own stone, and at least one
    /// adjacent opposing group must currently have zero liberties.
    pub fn apply_cleanup(
        &self,
        player: Player,
        point: Point,
    ) -> Result<CleanupOutcome, CleanupError> {
        if point.x >= BOARD_SIZE || point.y >= BOARD_SIZE {
            return Err(CleanupError::OutOfBounds);
        }
        if self.get(point) != player.cell() {
            return Err(CleanupError::NotOwnStone);
        }

        let mut captured_stones = self.adjacent_zero_liberty_opponent_groups(player, point);
        if captured_stones.is_empty() {
            return Err(CleanupError::NoCapturableGroup);
        }

        let mut board = self.clone();
        for captured_point in &captured_stones {
            board.set(*captured_point, Cell::Empty);
        }

        captured_stones.sort_by_key(|captured_point| captured_point.index());
        Ok(CleanupOutcome {
            board,
            captured_stones,
        })
    }

    pub(super) fn point_is_empty(&self, point: Point) -> bool {
        point.x < BOARD_SIZE && point.y < BOARD_SIZE && self.get(point) == Cell::Empty
    }

    pub(super) fn validate_placement(&self, point: Point) -> Result<(), MoveError> {
        if point.x >= BOARD_SIZE || point.y >= BOARD_SIZE {
            return Err(MoveError::OutOfBounds);
        }
        if self.get(point) != Cell::Empty {
            return Err(MoveError::Occupied);
        }
        Ok(())
    }

    pub(super) fn group_at(&self, start: Point) -> Vec<Point> {
        let color = self.get(start);
        let mut visited = HashSet::from([start]);
        let mut stack = vec![start];
        let mut group = Vec::new();

        while let Some(point) = stack.pop() {
            group.push(point);
            for neighbor in neighbors(point) {
                if self.get(neighbor) == color && visited.insert(neighbor) {
                    stack.push(neighbor);
                }
            }
        }

        group
    }

    /// The union of empty intersections orthogonally adjacent to `group`.
    ///
    /// Only `Cell::Empty` counts: Black, White, and Gray all block a liberty
    /// equally, which is what makes a Gray stone act exactly like the edge of
    /// the board here.
    pub(super) fn liberties(&self, group: &[Point]) -> HashSet<Point> {
        group
            .iter()
            .flat_map(|point| neighbors(*point))
            .filter(|point| self.get(*point) == Cell::Empty)
            .collect()
    }
}

/// [`Point::neighbors`], under the name the rules have always read it by.
pub(super) fn neighbors(point: Point) -> impl Iterator<Item = Point> {
    point.neighbors()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_move_captures_surrounded_group() {
        let mut board = Board::default();
        board.set(Point::new(1, 1), Cell::White);
        board.set(Point::new(0, 1), Cell::Black);
        board.set(Point::new(1, 0), Cell::Black);
        board.set(Point::new(2, 1), Cell::Black);

        let outcome = board.apply_move(Player::One, Point::new(1, 2)).unwrap();

        assert_eq!(outcome.opponent_stones_captured, vec![Point::new(1, 1)]);
        assert!(outcome.acting_stones_self_removed.is_empty());
        assert!(outcome.placed_stone_remains);
        assert_eq!(outcome.board.get(Point::new(1, 1)), Cell::Empty);
        assert_eq!(outcome.board.get(Point::new(1, 2)), Cell::Black);
    }

    #[test]
    fn cleanup_removes_a_single_adjacent_zero_liberty_group() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::White);
        board.set(Point::new(1, 0), Cell::Black);
        board.set(Point::new(0, 1), Cell::Black);

        let outcome = board.apply_cleanup(Player::One, Point::new(1, 0)).unwrap();

        assert_eq!(outcome.captured_stones, vec![Point::new(0, 0)]);
        assert_eq!(outcome.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(outcome.board.get(Point::new(1, 0)), Cell::Black);
        assert_eq!(outcome.board.get(Point::new(0, 1)), Cell::Black);
    }

    #[test]
    fn cleanup_removes_multiple_adjacent_zero_liberty_groups_simultaneously() {
        let mut board = Board::default();
        board.set(Point::new(4, 4), Cell::Black);
        board.set(Point::new(4, 3), Cell::White);
        for point in [Point::new(3, 3), Point::new(5, 3), Point::new(4, 2)] {
            board.set(point, Cell::Black);
        }
        board.set(Point::new(4, 5), Cell::White);
        for point in [Point::new(3, 5), Point::new(5, 5), Point::new(4, 6)] {
            board.set(point, Cell::Black);
        }

        let outcome = board.apply_cleanup(Player::One, Point::new(4, 4)).unwrap();

        assert_eq!(
            outcome.captured_stones,
            vec![Point::new(4, 3), Point::new(4, 5)]
        );
        assert_eq!(outcome.board.get(Point::new(4, 3)), Cell::Empty);
        assert_eq!(outcome.board.get(Point::new(4, 5)), Cell::Empty);
        assert_eq!(outcome.board.get(Point::new(4, 4)), Cell::Black);
    }

    #[test]
    fn cleanup_removes_a_group_kept_at_zero_liberties_by_gray_walls() {
        let mut board = Board::default();
        board.set(Point::new(4, 4), Cell::Black);
        board.set(Point::new(4, 3), Cell::White);
        for point in [Point::new(3, 3), Point::new(5, 3), Point::new(4, 2)] {
            board.set(point, Cell::Gray);
        }

        let outcome = board.apply_cleanup(Player::One, Point::new(4, 4)).unwrap();

        assert_eq!(outcome.captured_stones, vec![Point::new(4, 3)]);
        assert_eq!(outcome.board.get(Point::new(4, 3)), Cell::Empty);
        assert_eq!(outcome.board.get(Point::new(4, 4)), Cell::Black);
        for point in [Point::new(3, 3), Point::new(5, 3), Point::new(4, 2)] {
            assert_eq!(outcome.board.get(point), Cell::Gray);
        }
    }

    #[test]
    fn cleanup_is_rejected_without_change_when_no_group_is_capturable() {
        let mut board = Board::default();
        board.set(Point::new(4, 4), Cell::Black);
        board.set(Point::new(4, 3), Cell::White);
        let before = board.clone();

        assert_eq!(
            board.apply_cleanup(Player::One, Point::new(4, 4)),
            Err(CleanupError::NoCapturableGroup)
        );
        assert_eq!(board, before);

        // The selected point must also hold the acting player's own stone.
        assert_eq!(
            board.apply_cleanup(Player::One, Point::new(4, 3)),
            Err(CleanupError::NotOwnStone)
        );
        assert_eq!(board, before);
    }

    #[test]
    fn cleanup_counts_a_group_once_when_it_borders_the_stone_on_multiple_edges() {
        let mut board = Board::default();
        board.set(Point::new(4, 4), Cell::Black);
        // An L-shaped White group bordering the Black stone through both its
        // north (4,3) and west (3,4) edges - one connected group, not two.
        for point in [Point::new(4, 3), Point::new(3, 3), Point::new(3, 4)] {
            board.set(point, Cell::White);
        }
        for point in [
            Point::new(5, 3),
            Point::new(4, 2),
            Point::new(2, 3),
            Point::new(3, 2),
            Point::new(2, 4),
            Point::new(3, 5),
        ] {
            board.set(point, Cell::Black);
        }

        let outcome = board.apply_cleanup(Player::One, Point::new(4, 4)).unwrap();

        assert_eq!(
            outcome.captured_stones,
            vec![Point::new(3, 3), Point::new(4, 3), Point::new(3, 4)]
        );
        assert_eq!(outcome.board.get(Point::new(4, 4)), Cell::Black);
        for point in [Point::new(4, 3), Point::new(3, 3), Point::new(3, 4)] {
            assert_eq!(outcome.board.get(point), Cell::Empty);
        }
    }

    #[test]
    fn isolated_suicide_is_accepted_and_self_removes() {
        let mut board = Board::default();
        board.set(Point::new(0, 1), Cell::White);
        board.set(Point::new(1, 0), Cell::White);
        board.set(Point::new(2, 1), Cell::White);
        board.set(Point::new(1, 2), Cell::White);
        let outcome = board.apply_move(Player::One, Point::new(1, 1)).unwrap();

        assert!(outcome.opponent_stones_captured.is_empty());
        assert_eq!(outcome.acting_stones_self_removed, vec![Point::new(1, 1)]);
        assert!(!outcome.placed_stone_remains);
        assert_eq!(outcome.board, board);
    }

    #[test]
    fn suicide_removes_the_entire_connected_acting_group() {
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        for point in [Point::new(0, 1), Point::new(1, 1), Point::new(2, 0)] {
            board.set(point, Cell::White);
        }

        let outcome = board.apply_move(Player::One, Point::new(1, 0)).unwrap();

        assert!(outcome.opponent_stones_captured.is_empty());
        assert_eq!(
            outcome.acting_stones_self_removed,
            vec![Point::new(0, 0), Point::new(1, 0)]
        );
        assert_eq!(outcome.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(outcome.board.get(Point::new(1, 0)), Cell::Empty);
        assert!(!outcome.placed_stone_remains);
    }

    #[test]
    fn capturing_move_gains_liberties_and_does_not_self_remove() {
        let mut board = Board::default();
        board.set(Point::new(1, 1), Cell::White);
        for point in [
            Point::new(0, 1),
            Point::new(1, 0),
            Point::new(2, 1),
            Point::new(0, 2),
            Point::new(2, 2),
            Point::new(1, 3),
        ] {
            board.set(point, Cell::Black);
        }

        let outcome = board.apply_move(Player::One, Point::new(1, 2)).unwrap();

        assert_eq!(outcome.opponent_stones_captured, vec![Point::new(1, 1)]);
        assert!(outcome.acting_stones_self_removed.is_empty());
        assert!(outcome.placed_stone_remains);
        assert_eq!(outcome.board.get(Point::new(1, 2)), Cell::Black);
    }

    #[test]
    fn gray_stones_are_neither_connected_nor_captured() {
        let mut board = Board::default();
        board.set(Point::new(1, 1), Cell::Gray);
        board.set(Point::new(0, 1), Cell::Black);
        board.set(Point::new(1, 0), Cell::Black);
        board.set(Point::new(2, 1), Cell::Black);

        let outcome = board.apply_move(Player::One, Point::new(1, 2)).unwrap();

        assert!(outcome.opponent_stones_captured.is_empty());
        assert_eq!(outcome.board.get(Point::new(1, 1)), Cell::Gray);
    }

    #[test]
    fn gray_provides_no_liberty_and_helps_capture_like_the_board_edge() {
        // A Black corner stone has only two neighbors, exactly like the
        // classic corner capture that relies solely on the board edge. Here
        // one of those two neighbors is an explicit Gray wall instead of
        // being off-board, and it works exactly the same way: Gray counts
        // as occupied, never as a liberty.
        let mut board = Board::default();
        board.set(Point::new(0, 0), Cell::Black);
        board.set(Point::new(1, 0), Cell::Gray);

        let outcome = board.apply_move(Player::Two, Point::new(0, 1)).unwrap();

        assert_eq!(outcome.opponent_stones_captured, vec![Point::new(0, 0)]);
        assert_eq!(outcome.board.get(Point::new(0, 0)), Cell::Empty);
        assert_eq!(outcome.board.get(Point::new(1, 0)), Cell::Gray);
        assert_eq!(outcome.board.get(Point::new(0, 1)), Cell::White);
    }
}
