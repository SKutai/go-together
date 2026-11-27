//! Fundamental value types shared by every other `rules` module: board
//! dimensions, [`Cell`], [`Player`], [`Point`], and [`Score`].
//!
//! Nothing here depends on [`super::board::Board`] or round resolution -
//! these are the values everything else is built from.

use serde::{Deserialize, Serialize};

/// Width and height of the square board.
pub const BOARD_SIZE: usize = 9;
/// Number of intersections on the board, and the exact length of the flat
/// `cells` list a persisted [`crate::rules::Board`] carries - changing it
/// invalidates every saved Replay board.
pub const CELL_COUNT: usize = BOARD_SIZE * BOARD_SIZE;

/// Contents of one board intersection.
///
/// `Gray` is a permanent neutral wall, not a third player color.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cell {
    #[default]
    Empty,
    Black,
    White,
    Gray,
}

/// Stable player identity used by rules, input, UI, and persisted Replays.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Player {
    One,
    Two,
}

impl Player {
    /// Both players in [`Player::index`] order, so
    /// `Player::ALL[player.index()] == player`.
    pub const ALL: [Self; 2] = [Self::One, Self::Two];
    /// Controller assignment priority: the first connected gamepad fills
    /// White/Player Two before Black/Player One.
    pub const PRIORITY_ORDER: [Self; 2] = [Self::Two, Self::One];

    /// This player's stone colour: `One` is Black, `Two` is White.
    pub const fn cell(self) -> Cell {
        match self {
            Self::One => Cell::Black,
            Self::Two => Cell::White,
        }
    }

    pub const fn opponent_cell(self) -> Cell {
        match self {
            Self::One => Cell::White,
            Self::Two => Cell::Black,
        }
    }

    pub const fn opponent(self) -> Self {
        match self {
            Self::One => Self::Two,
            Self::Two => Self::One,
        }
    }

    /// This player's position in every per-player `[T; 2]` in the crate -
    /// capture credits, attempted points, locks, cursors: `One` is 0, `Two`
    /// is 1.
    pub const fn index(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
        }
    }
}

/// Zero-based board coordinate, serialized as `{ "x": ..., "y": ... }`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}

impl Point {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }

    /// This point's position in a flat, row-major list of the board's
    /// cells: the order a [`crate::rules::Board`] is persisted in, and the
    /// order the rules list removed stones in.
    pub const fn index(self) -> usize {
        self.y * BOARD_SIZE + self.x
    }

    /// The orthogonal neighbours of this point that lie on the board, in
    /// the order left, right, up, down. The rules read connectivity and
    /// liberties through it; a renderer ordering a captured group from its
    /// outside in reads the same adjacency, so the two cannot disagree
    /// about what "adjacent" means.
    pub fn neighbors(self) -> impl Iterator<Item = Point> {
        let mut points = [None; 4];
        let mut count = 0;

        if self.x > 0 {
            points[count] = Some(Point::new(self.x - 1, self.y));
            count += 1;
        }
        if self.x + 1 < BOARD_SIZE {
            points[count] = Some(Point::new(self.x + 1, self.y));
            count += 1;
        }
        if self.y > 0 {
            points[count] = Some(Point::new(self.x, self.y - 1));
            count += 1;
        }
        if self.y + 1 < BOARD_SIZE {
            points[count] = Some(Point::new(self.x, self.y + 1));
            count += 1;
        }

        points.into_iter().take(count).flatten()
    }
}

/// One stone a resolution took off the board: where it stood, whose it was,
/// and what took it.
///
/// This is the record of a capture the rules hand upward, so that a
/// renderer can show what a round took and a future screen reader can say
/// it. `owner` is the player whose stone it was, which is also the colour
/// it showed. The rules credit every removal to the *opponent* of its
/// owner - an ordinary capture, a self-removal, an active cleanup and the
/// Round 0 reveal sweep alike - so the owner alone says who was credited.
///
/// `by` is the point of the action that removed it: the stone that was
/// placed, or the stone an active cleanup was made from. For a
/// self-removal it is the acting player's own placement, which is itself
/// among the stones removed. It is `None` only for the Round 0 reveal
/// sweep, which has no acting player. Every such point is public once the
/// round has resolved - it is where a stone now stands, or where one stood
/// for the instant before it died - so publishing it reveals nothing the
/// board does not.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct RemovedStone {
    pub point: Point,
    pub owner: Player,
    pub by: Option<Point>,
}

/// A Black/White count pair.
///
/// `Board::score` fills it with area-scoring intersection counts, where Gray
/// and empty score zero; a Match and a Replay also use it for the running
/// capture totals that `score_with_captures` adds on top when Capture Points
/// is enabled.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Score {
    pub black: usize,
    pub white: usize,
}
