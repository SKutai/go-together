//! Pure Round 0 selection and reveal rules.
//!
//! Each player owns an ordered rolling window of at most three unique board
//! points. Adding a fourth distinct point evicts the oldest; selecting a point
//! already in the window is an explicit no-op. The reveal combines both
//! players' final windows on an empty board: a point exactly one player chose
//! becomes that player's stone, and a point both chose becomes a permanent
//! Gray stone.
//!
//! [`reveal_opening`] itself invokes no ordinary move, capture, suicide, or ko
//! logic, so a zero-liberty stone survives it. That is not the Match rule: the
//! authoritative reveal, [`crate::authority::MatchState::reveal_round_zero`],
//! immediately sweeps the returned board with
//! [`crate::rules::Board::resolve_opening_captures`], so no zero-liberty group
//! reaches Round 1 in a real Match. This module stops at the raw combination so
//! that sweep has one owner, in [`crate::rules`].

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::clock::MatchDuration;
use crate::rules::{BOARD_SIZE, Board, Cell, Player, Point};

/// How many distinct points each player's Round 0 window holds; a fourth
/// pick evicts the oldest.
pub const OPENING_SELECTION_LIMIT: usize = 3;
/// How long each spoken number of the closing 3-2-1 countdown is shown:
/// "3" from the moment the countdown starts, "2" one stage later, "1" one
/// stage after that.
pub const OPENING_COUNTDOWN_STAGE: MatchDuration = MatchDuration::from_secs(1);
/// When, measured from the start of the closing countdown, the six Round 0
/// slots are revealed atomically and ordinary Round 1 begins: the end of the
/// third and last numbered stage.
pub const OPENING_REVEAL: MatchDuration = MatchDuration::from_secs(3);
/// How long the "Go!" card stays up after the closing countdown reveals
/// Round 0, while ordinary play is already active underneath it.
pub const OPENING_GO_DISPLAY: MatchDuration = MatchDuration::from_millis(750);
/// The whole closing sequence, from the countdown starting to the Go card
/// clearing: [`OPENING_REVEAL`] plus [`OPENING_GO_DISPLAY`].
pub const OPENING_COUNTDOWN_TOTAL: MatchDuration =
    OPENING_REVEAL.saturating_add(OPENING_GO_DISPLAY);

/// The cards of the closing 3-2-1-Go countdown, in order. `Go` is the stage
/// during which the reveal has already happened and the Go card is still
/// showing over ordinary play.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningCountdownStage {
    Three,
    Two,
    One,
    Go,
}

impl OpeningCountdownStage {
    /// The text the countdown card shows for this stage.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Three => "3",
            Self::Two => "2",
            Self::One => "1",
            Self::Go => "Go!",
        }
    }
}

/// What one [`OpeningSelections::add`] did: `Added` (carrying the point it
/// evicted, if the window was already full), `Duplicate` (the point was
/// already in the window, so nothing changed), or `OutOfBounds`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpeningSelectionOutcome {
    Added { evicted: Option<Point> },
    Duplicate,
    OutOfBounds,
}

/// One player's ordered Round 0 selections.
///
/// The transparent JSON representation is a compact point array. Deserialization
/// enforces the same maximum, uniqueness, and board bounds as live input so
/// persisted Replay data cannot construct an invalid opening state.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OpeningSelections {
    points: Vec<Point>,
}

impl<'de> Deserialize<'de> for OpeningSelections {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let points = Vec::<Point>::deserialize(deserializer)?;
        let selections = Self { points };
        selections.validate().map_err(D::Error::custom)?;
        Ok(selections)
    }
}

impl OpeningSelections {
    /// Appends `point` to the window, evicting the oldest of three to make
    /// room. Re-selecting a point already held is an explicit no-op.
    pub fn add(&mut self, point: Point) -> OpeningSelectionOutcome {
        if !point_is_on_board(point) {
            return OpeningSelectionOutcome::OutOfBounds;
        }
        if self.points.contains(&point) {
            return OpeningSelectionOutcome::Duplicate;
        }
        let evicted = (self.points.len() == OPENING_SELECTION_LIMIT).then(|| self.points.remove(0));
        self.points.push(point);
        OpeningSelectionOutcome::Added { evicted }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.points.len() == OPENING_SELECTION_LIMIT
    }

    pub fn contains(&self, point: Point) -> bool {
        self.points.contains(&point)
    }

    pub fn as_slice(&self) -> &[Point] {
        &self.points
    }

    /// The invariant the window keeps and the `Deserialize` impl re-checks on
    /// persisted data: at most [`OPENING_SELECTION_LIMIT`] points, every one
    /// on the board, and none repeated.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.points.len() > OPENING_SELECTION_LIMIT {
            return Err("opening selection has more than three points");
        }
        if self.points.iter().any(|point| !point_is_on_board(*point)) {
            return Err("opening selection point is outside the board");
        }
        if self
            .points
            .iter()
            .enumerate()
            .any(|(index, point)| self.points[..index].contains(point))
        {
            return Err("opening selection contains duplicate points");
        }
        Ok(())
    }
}

/// Combines both players' final windows onto an empty board: a point exactly
/// one player chose becomes that player's stone, a point both chose becomes
/// Gray. See the module documentation for the zero-liberty sweep that follows
/// this in an authoritative Match.
pub fn reveal_opening(selections: &[OpeningSelections; 2]) -> Board {
    let mut board = Board::default();
    for point in selections[Player::One.index()].as_slice() {
        let cell = if selections[Player::Two.index()].contains(*point) {
            Cell::Gray
        } else {
            Cell::Black
        };
        board.set(*point, cell);
    }
    for point in selections[Player::Two.index()].as_slice() {
        if board.get(*point) == Cell::Empty {
            board.set(*point, Cell::White);
        }
    }
    board
}

const fn point_is_on_board(point: Point) -> bool {
    point.x < BOARD_SIZE && point.y < BOARD_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: usize, y: usize) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn selection_window_adds_three_then_evicts_fifo_repeatedly() {
        let mut selections = OpeningSelections::default();
        for expected in [point(0, 0), point(1, 0), point(2, 0)] {
            assert_eq!(
                selections.add(expected),
                OpeningSelectionOutcome::Added { evicted: None }
            );
        }
        assert_eq!(
            selections.add(point(3, 0)),
            OpeningSelectionOutcome::Added {
                evicted: Some(point(0, 0))
            }
        );
        assert_eq!(
            selections.as_slice(),
            &[point(1, 0), point(2, 0), point(3, 0)]
        );
        assert_eq!(
            selections.add(point(4, 0)),
            OpeningSelectionOutcome::Added {
                evicted: Some(point(1, 0))
            }
        );
        assert_eq!(
            selections.as_slice(),
            &[point(2, 0), point(3, 0), point(4, 0)]
        );
    }

    #[test]
    fn duplicate_and_out_of_bounds_points_leave_order_unchanged() {
        let mut selections = OpeningSelections::default();
        for point in [point(0, 0), point(1, 0), point(2, 0)] {
            selections.add(point);
        }
        let expected = selections.clone();

        assert_eq!(
            selections.add(point(1, 0)),
            OpeningSelectionOutcome::Duplicate
        );
        assert_eq!(selections, expected);
        assert_eq!(
            selections.add(point(BOARD_SIZE, 0)),
            OpeningSelectionOutcome::OutOfBounds
        );
        assert_eq!(selections, expected);
    }

    #[test]
    fn serialization_round_trips_and_rejects_untrusted_invalid_arrays() {
        let mut selections = OpeningSelections::default();
        selections.add(point(1, 2));
        selections.add(point(3, 4));
        let json = serde_json::to_string(&selections).unwrap();
        assert_eq!(
            serde_json::from_str::<OpeningSelections>(&json).unwrap(),
            selections
        );

        for invalid in [
            r#"[{"x":0,"y":0},{"x":1,"y":0},{"x":2,"y":0},{"x":3,"y":0}]"#,
            r#"[{"x":0,"y":0},{"x":0,"y":0}]"#,
            r#"[{"x":9,"y":0}]"#,
        ] {
            assert!(serde_json::from_str::<OpeningSelections>(invalid).is_err());
        }
    }

    #[test]
    fn reveal_places_unique_stones_and_every_overlap_as_gray() {
        let mut selections = [OpeningSelections::default(), OpeningSelections::default()];
        for point in [point(0, 0), point(1, 1), point(2, 2)] {
            selections[Player::One.index()].add(point);
        }
        for point in [point(1, 1), point(2, 2), point(8, 8)] {
            selections[Player::Two.index()].add(point);
        }

        let board = reveal_opening(&selections);

        assert_eq!(board.get(point(0, 0)), Cell::Black);
        assert_eq!(board.get(point(1, 1)), Cell::Gray);
        assert_eq!(board.get(point(2, 2)), Cell::Gray);
        assert_eq!(board.get(point(8, 8)), Cell::White);
    }

    #[test]
    fn reveal_keeps_zero_liberty_stones_without_capture_or_suicide_checks() {
        let mut selections = [OpeningSelections::default(), OpeningSelections::default()];
        for point in [point(0, 0), point(7, 7), point(8, 8)] {
            selections[Player::One.index()].add(point);
        }
        for point in [point(1, 0), point(0, 1), point(6, 6)] {
            selections[Player::Two.index()].add(point);
        }

        let board = reveal_opening(&selections);

        assert_eq!(board.get(point(0, 0)), Cell::Black);
        assert_eq!(board.get(point(1, 0)), Cell::White);
        assert_eq!(board.get(point(0, 1)), Cell::White);
    }
}
