//! Where everything in the Replay Viewer stands, for a window of any size,
//! and where everything stands on its board.
//!
//! Nothing here touches the ECS. Every item is a constant or a pure
//! calculation over the window's size, the room a Replay's label needs, a
//! [`Point`], a player or a recorded frame. That is what lets the tests run [`viewer_layout`] at every size
//! from the minimum to 2560x1440 in steps of 20 px, at the named sizes and
//! at a few larger windows, and check that nothing overlaps and nothing
//! leaves the window, and check that the board's marks fit their cell and
//! one another, without building an app. [`super::layout`] spawns the
//! furniture where this says and moves it whenever the window's size
//! changes, and [`super::display`] moves the marks on the board with it
//! whenever playback crosses a recorded event boundary.
//!
//! The viewer keeps its own, tighter grid constants rather than reusing the
//! live board's, because its window also holds a header, transport controls
//! and the Key and Match Settings modals.
//!
//! # Two arrangements
//!
//! Text never changes size. The board is the one thing that grows and
//! shrinks, with every mark on it, from 0.79 of its own size at the
//! smallest window up to twice it, and the camera does that
//! ([`ViewerLayout::framing`]), so the board's own geometry - the grid,
//! [`viewer_board_position`] and the sizes of the marks - is the same at
//! every size. The interface finds its room by moving:
//!
//! * **Tall** is the 1040x900 layout: Back, the Replay's label and Match
//!   Settings across the top, the timer, the round and the result line
//!   under them, the board, and the transport row under the board. Laid
//!   out in that window it is exactly the screen as it has always been.
//! * **Wide** lets the board fill the height above the transport row. Back
//!   and Match Settings stay in the top corners, the label, the timer, the
//!   round and the result line stand in a column under Back, and the
//!   transport row stays under the board.
//!
//! Both centre the board across the window, which is where the Round 0
//! countdown card is centred ([`crate::ui::countdown_card`]). In a window
//! with more room than the board takes - one taller than tall needs, a wide
//! one whose width limits the board, or one past the size where the board
//! reaches twice its own - the whole of it, board and interface together,
//! stands centred down the window as one piece.
//!
//! Which of the two a window gets is the rule the Match screen chooses by
//! too ([`crate::ui::arrangement`]): wide only where it would draw the board
//! more than 4% larger than tall would, and past the cap by the window's
//! shape alone. With this screen's margins ([`viewer_arrangements`]), every
//! window narrower than about 1.20 times its height is tall, and a wider
//! one is tall until wide gains that much. So the opening shape, 1040x900,
//! is tall at every size, and a window 1040 px wide is wide up to 778 px
//! tall and tall from 779. Past the cap a window is wide when it is more
//! than about 1.20 times as wide as it is tall: 2560x1600, 3840x2160 and
//! 5120x2880 are wide, as 2560x1440 is, and a portrait window is tall. A
//! label that needs a third line in tall moves those figures, as the next
//! section says.
//!
//! # Making room
//!
//! A window shorter than the opening one has less room around the board
//! than it seems, because nothing but the board may shrink. What each place
//! holds is sized for the longest text the screen writes there, with
//! [`crate::ui::text_metrics`]:
//!
//! * **Wide's column** holds any label - up to 60 characters, set at 26 px
//!   and wrapped at a word, or within a word too long for a line - over the
//!   longest timer, round and result lines, and stands clear of the
//!   transport row under it at every size. The column is narrow, so all
//!   but the shortest labels wrap: a recorded Replay's default label, about
//!   31 characters, takes three lines in the smallest window.
//! * **Tall's header** gives the label the middle half of its row, as it
//!   always has: 498 px in the narrowest window, where every label the game
//!   writes by itself, and most a player writes, takes two lines at most.
//!   One that may take a third ([`TallLabelRoom`]) is given room for it:
//!   the lines under it, the board and the transport row stand a line
//!   lower, which for such a Replay makes 1040 px wide windows wide up to
//!   809 px tall, and past the cap turns the choice at about 1.17 times as
//!   wide as tall. Until this was made room for, such a label's last line
//!   ran over the timer.
//! * **The score badges** are text drawn on the board, so the camera's zoom
//!   would scale them too, and the floor on text size reads the sizes text
//!   is set at, not the camera. Each is drawn at its own size whatever the
//!   board does, and placed in window pixels: see [`score_badge_position`].
//!
//! Three of the column's texts have no longest once a limit is switched
//! off, so their places are sized for assumptions, as the Match screen's
//! are: a round under 10,000, a Replay under 1,000 minutes and scores under
//! 1,000. Past them a line only grows, wrapping in the column, into room
//! the column keeps under it; a test in `super` lays out a Replay a digit
//! past all three and finds it clear.
//!
//! # The Key
//!
//! The Key ([`KeyColumns`]) is the one panel too tall for a short window: in
//! one column, its ten rows stand under its title and introduction, with
//! Close below them. In a window too short to hold that, it is as wide as
//! the smallest window allows and sets its rows in two columns of five,
//! the cursors, the score badges and the Teleport Markers on the left and
//! the hidden choices and the stones on the right, with the title and the
//! introduction across the top and Close under both.
//!
//! # Two rules that are more than arithmetic
//!
//! [`score_badge_position`]: badges follow each cursor, clamp to the window
//! and separate when the cursors overlap, so both stay legible - Player
//! One's always on the left, since that order is what tells the two apart
//! there, and the Key says so. And [`viewer_selection_position`]: two hidden
//! selections of one point are drawn side by side, not one over the other.
//! [`viewer_marker_layout`] says what decided the sizes of the two
//! Replay-only marks.

use bevy::prelude::*;

use super::player_index;
use crate::app::visuals::{MIN_WINDOW_WIDTH, WorldFraming, cursor_visual_style};
use crate::replay::ReplayFrame;
use crate::rules::{BOARD_SIZE, Player, Point};
use crate::ui::arrangement::{Arrangement, Arrangements, MAX_ZOOM, Margins};
use crate::ui::text_metrics::{line_width, text_height};

/// Side of the square each player's Teleport Marker is drawn on before it
/// is turned on its point, Player One's first; see [`viewer_marker_layout`].
const VIEWER_MARKER_SIDES: [f32; 2] = [40.0, 20.0];
/// Widths of a Teleport Marker's three strokes, outermost first, centred
/// together on the side of its square: seven in all, finer than a cursor's
/// ten, since a marker is secondary to the cursor and two of them have to
/// nest in one cell. The outer stroke is two pixels, as the showing edge of
/// a cursor's border is, because on wood it is all that stands 3:1 clear
/// of Player Two's marker but its innermost stroke; that stroke is two as
/// well, because on a white stone it is all that stands clear at all; and
/// the accent between them is three, wide enough to show its hue.
pub(super) const VIEWER_MARKER_STROKES: [f32; 3] = [2.0, 3.0, 2.0];
/// Outer radius of a hidden-selection mark: a pixel inside the inner edge of
/// Player Two's cursor, the smaller of the two, since a selection is made
/// with the cursor on it; and inside every stone's fill, so that on a stone
/// the mark's outer stroke lies on that stone and not on the wood around it.
const VIEWER_SELECTION_RADIUS: f32 = 20.0;
/// Outer radius of each of the two marks drawn for a point both players
/// selected, and how far each is moved aside: two pixels are left between
/// them, and the pair spans 58 of the cell's 67. That is as far as Player
/// One's cursor reaches in, so the pair lies within it; Player Two's
/// smaller cursor it overlaps, and is drawn over.
const VIEWER_SHARED_SELECTION_RADIUS: f32 = 14.0;
const VIEWER_SHARED_SELECTION_OFFSET: f32 = 15.0;
/// Widths of a mark's three strokes, outermost first, at either radius. The
/// grid's lines are 2.5; these are a little finer, which leaves an opening
/// 28 across in the ordinary mark - wide enough to show what is beneath, a
/// star point included - and 16 across in the smaller one.
pub(super) const VIEWER_SELECTION_STROKES: [f32; 3] = [2.0, 2.0, 2.0];
pub(super) const SCORE_BADGE_SIZE: Vec2 = Vec2::new(42.0, 28.0);
/// How far a score badge keeps from the window's edges.
const SCORE_BADGE_EDGE_INSET: f32 = 4.0;
/// How far a score badge stands off its cursor's corner, as both are
/// drawn.
const SCORE_BADGE_CURSOR_GAP: f32 = 4.0;
/// The gap between two badges side by side on a shared point.
const SCORE_BADGE_OVERLAP_GAP: f32 = 5.0;
pub(super) const VIEWER_GRID_SPACING: f32 = 67.0;
pub(super) const VIEWER_GRID_EXTENT: f32 = VIEWER_GRID_SPACING * (BOARD_SIZE as f32 - 1.0);
pub(super) const VIEWER_BOARD_OUTER_SIZE: f32 = VIEWER_GRID_EXTENT + 78.0;
pub(super) const VIEWER_BOARD_CENTER_Y: f32 = -45.0;
/// The board's centre in world units, which the camera's framing draws at
/// the centre of [`ViewerLayout::board`].
const VIEWER_BOARD_WORLD_CENTER: Vec2 = Vec2::new(0.0, VIEWER_BOARD_CENTER_Y);

/// The sizes the header's four texts are set at: the label, the timer, the
/// round and the result line.
pub(super) const VIEWER_LABEL_SIZE: f32 = 26.0;
pub(super) const VIEWER_TIMER_SIZE: f32 = 18.0;
pub(super) const VIEWER_ROUND_SIZE: f32 = 16.0;
pub(super) const VIEWER_RESULT_SIZE: f32 = 14.0;
/// The row of Back, tall's label and Match Settings: its top, from the top
/// of its piece, and how far in from each side of the window it runs.
pub(super) const VIEWER_HEADER_TOP: f32 = 12.0;
pub(super) const VIEWER_HEADER_INSET: f32 = 22.0;
/// The widths of the two buttons in that row.
pub(super) const VIEWER_BACK_WIDTH: f32 = 86.0;
pub(super) const VIEWER_MATCH_SETTINGS_WIDTH: f32 = 145.0;
/// The result line's least height.
pub(super) const VIEWER_RESULT_HEIGHT: f32 = 17.0;
/// How far the transport row stands above the bottom of its piece, and the
/// height of every button on the screen.
pub(super) const VIEWER_CONTROL_BOTTOM: f32 = 22.0;
pub(super) const VIEWER_CONTROL_HEIGHT: f32 = 38.0;
pub(super) const VIEWER_MATCH_SETTINGS_MODAL_WIDTH: f32 = 620.0;
pub(super) const VIEWER_MATCH_SETTINGS_MODAL_MAX_HEIGHT: f32 = 760.0;

/// The room tall keeps around the board, which is the 1040x900 layout's.
/// Above it, the row of Back, the label and Match Settings, the three lines
/// under that row, and the room the top row's score badges rise into under
/// them; below it, a gap, the transport row and its margin; and at each
/// side, what is left of 1040 px. Tall keeps exactly this room around a
/// board of any size, and a line more above it for a label that needs one
/// ([`TallLabelRoom`]).
const TALL_TOP: f32 = 188.0;
const TALL_BOTTOM: f32 = 98.0;
const TALL_SIDE: f32 = 213.0;
/// The tops of tall's three lines under the label - the timer, the round
/// and the result line - from the top of its piece.
const TALL_LINE_TOPS: [f32; 3] = [80.0, 104.0, 128.0];

/// The room wide keeps above the board, for the top row's score badges: a
/// badge there stands 32 px less 5 px a zoom above the board's edge (see
/// [`score_badge_position`]), so this keeps it 8 px or more inside the
/// window.
const WIDE_TOP: f32 = 40.0;
/// The gap wide keeps between the board and the transport row.
const WIDE_CONTROL_GAP: f32 = 16.0;
/// The room wide keeps under the board: that gap, the transport row and its
/// margin.
const WIDE_BOTTOM: f32 = WIDE_CONTROL_GAP + VIEWER_CONTROL_HEIGHT + VIEWER_CONTROL_BOTTOM;
/// The least room wide keeps at each side of the board. On the left it
/// holds the column under Back. On the right, Match Settings stands in the
/// top corner, beside the room above the board that the top row's score
/// badges rise into, and a pair of badges on the board's top right point
/// reaches 93 px less 5 px a zoom past the board's edge: this keeps that
/// pair 8 px clear of the button in the narrowest wide window.
const WIDE_SIDE: f32 = 264.0;
/// Where wide's column under Back starts, down from the top of its piece,
/// and how far it keeps from the window's side and from the board.
const WIDE_COLUMN_TOP: f32 = 64.0;
const WIDE_COLUMN_INSET: f32 = 22.0;
/// The gaps between the texts in wide's column: under the label, under the
/// timer and under the round.
pub(super) const WIDE_COLUMN_GAPS: [f32; 3] = [34.0, 4.0, 6.0];

/// The least gap the Key keeps from the window's edges.
const KEY_EDGE: f32 = 12.0;
/// The Key's panel, as wide as it has always been, in one column.
pub(super) const KEY_ONE_COLUMN_WIDTH: f32 = 720.0;
/// The Key's panel in two columns: the smallest window's width, less the
/// Key's edge either side.
pub(super) const KEY_TWO_COLUMN_WIDTH: f32 = MIN_WINDOW_WIDTH as f32 - 2.0 * KEY_EDGE;
/// The gap between the Key's two columns.
pub(super) const KEY_COLUMN_GAP: f32 = 24.0;
/// What the Key's panel holds inside its border and padding: its width,
/// less 2 px of border and 24 px of padding either side.
pub(super) const KEY_PANEL_FRAME: f32 = 2.0 * (2.0 + 24.0);
/// The height of the Key's panel in one column, as `bevy_ui` lays it out
/// at a scale factor of 1: 698 px, measured on 2026-11-28 with
/// `the_key_stands_inside_the_window_in_either_form` in `super`, which
/// holds it there. It is the tallest at any scale factor, because text is
/// rounded up to whole physical pixels, and 1 is the coarsest: on a
/// display at twice the scale it is 695 px.
pub(super) const KEY_ONE_COLUMN_HEIGHT: f32 = 698.0;
/// The height of the Key's panel in two columns, measured the same way
/// and held by the same test: 494 px, which leaves the smallest window 53
/// px above and below it.
#[cfg(test)]
pub(super) const KEY_TWO_COLUMN_HEIGHT: f32 = 494.0;

/// The most characters a Replay's label holds: the label rule that
/// `crate::replay::validate_label` holds every Replay to.
#[cfg(test)]
const LABEL_MAX_CHARS: usize = 60;
/// The longest timer, round and result lines that wide's column makes room
/// for, as the display pass writes them: a Replay under 1,000 minutes, a
/// round under 10,000 with an hour of Round Time left, and scores under
/// 1,000 with a four-digit year. `super`'s tests hold each to the text the
/// screen writes there.
#[cfg(test)]
pub(super) const LONGEST_TIMER: &str = "999:59 / 999:59";
#[cfg(test)]
pub(super) const LONGEST_ROUND: &str = "Round 9999 - 60:00 remaining";
#[cfg(test)]
pub(super) const LONGEST_RESULT: &str =
    "White | Black 999 - White 999 | completed 2026-11-27 23:59";

pub(super) fn viewer_cursor_position(frame: &ReplayFrame, player: Player) -> Vec2 {
    viewer_board_position(frame.cursors[player_index(player)])
}

pub(super) fn viewer_board_position(point: Point) -> Vec2 {
    Vec2::new(
        point.x as f32 * VIEWER_GRID_SPACING - VIEWER_GRID_EXTENT / 2.0,
        VIEWER_BOARD_CENTER_Y + point.y as f32 * VIEWER_GRID_SPACING - VIEWER_GRID_EXTENT / 2.0,
    )
}

/// Where one player's Teleport Marker is drawn, before [`super::layout`]
/// turns it 45 degrees to stand on its point: a square outline with closed
/// sides, in three strokes that lie together on the sides of a `size`
/// square. The turn is what tells a marker from a cursor or a stone; size
/// and the opposed outer strokes are what tell the two players' markers
/// apart, as they do the cursors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ViewerMarkerLayout {
    /// Side of the square the strokes are centred on.
    pub(super) size: f32,
    /// Distance from the centre, square to a side, to the outer edge of
    /// each stroke, outermost first.
    pub(super) outer_edges: [f32; 3],
    /// Distance from the centre, square to a side, to the inner edge of the
    /// innermost stroke. Every stroke is drawn in to here and the next one
    /// painted over it, as the strokes of a hidden-selection mark are, so
    /// that no two abut and no seam of board shows between them.
    pub(super) inner_edge: f32,
}

impl ViewerMarkerLayout {
    /// How far along a side, from its middle, a stroke's four bars run: to
    /// the corners of its own outer square, but never past the corners of
    /// the `size` square. That notches each tip by half the marker's
    /// stroke - a V, half the stroke deep, whose faces show the accent for
    /// a pixel and a half before the outer stroke - which is what lets
    /// Player One's fit its cell: with whole tips it would be 66.5 across
    /// in a cell of 67.
    pub(super) fn reach(self, outer_edge: f32) -> f32 {
        outer_edge.min(self.size / 2.0)
    }

    /// How far the marker reaches from its centre along a grid line once
    /// it is turned: the notched tip's two corners lie this far out.
    #[cfg(test)]
    pub(super) fn tip(self) -> f32 {
        let [outer, ..] = self.outer_edges;
        (self.reach(outer) + outer) / std::f32::consts::SQRT_2
    }
}

/// The layout of one player's Teleport Marker, whose sizes are what the
/// Viewer's 67-pixel cell leaves room for, and the tests below hold each
/// of these.
///
/// `size` is the side of the square *before* the turn, and that is the
/// trap in choosing one: a square on its point is the square root of two
/// times as wide, corner to corner, as it is along a side, and the stroke
/// widens it further. Player One's 40 spans 61.5 of the cell, tip to tip -
/// a cursor's 68, turned, would span 103.
///
/// Player One's diamond clears the cell and the stones beside it. Player
/// Two's nests inside it with three pixels of board between their strokes,
/// so that two markers on one point are both seen. Each lies inside its
/// own player's cursor, since a marker is set where the cursor stands.
/// That fixes Player Two's side at half of Player One's, where the cursors
/// are three quarters: with a stroke that still shows on wood, nothing
/// larger leaves those three pixels.
///
/// What the cell cannot do is keep both diamonds clear of a hidden
/// selection's mark as well - and a player who marks the point they then
/// select puts both there. So the mark is drawn over the marker, and no
/// marker covers any of it. Under it, Player Two's diamond keeps its
/// innermost stroke whole and its accent along most of each side, inside
/// the mark's opening, and loses its tips to the mark's strokes; Player
/// One's stands outside the mark, the middle of each side running under
/// the mark's outer and accent strokes and its tips well clear.
pub(super) fn viewer_marker_layout(player: Player) -> ViewerMarkerLayout {
    let size = VIEWER_MARKER_SIDES[player_index(player)];
    let thickness: f32 = VIEWER_MARKER_STROKES.iter().sum();
    let mut edge = (size + thickness) / 2.0;
    let outer_edges = VIEWER_MARKER_STROKES.map(|width| {
        let outer = edge;
        edge -= width;
        outer
    });
    ViewerMarkerLayout {
        size,
        outer_edges,
        inner_edge: edge,
    }
}

/// Outer radius of one hidden-selection mark; `shared` is the smaller one
/// drawn when the other player selected the same point.
pub(super) const fn viewer_selection_radius(shared: bool) -> f32 {
    if shared {
        VIEWER_SHARED_SELECTION_RADIUS
    } else {
        VIEWER_SELECTION_RADIUS
    }
}

/// Where one player's hidden-selection mark is drawn for a selection of
/// `point`. `shared` says the other player selected the same point - which
/// a recorded Match shows only in Round 0, since an ordinary round resolves
/// in the instant its second selection is made. The two marks are then
/// drawn side by side, Player One's on the left, rather than one inside the
/// other: nested, the light inner stroke of one would lie against the light
/// outer stroke of the other, and neither edge would be seen.
pub(super) fn viewer_selection_position(point: Point, player: Player, shared: bool) -> Vec2 {
    let position = viewer_board_position(point);
    if !shared {
        return position;
    }
    let offset = match player {
        Player::One => -VIEWER_SHARED_SELECTION_OFFSET,
        Player::Two => VIEWER_SHARED_SELECTION_OFFSET,
    };
    position + Vec2::new(offset, 0.0)
}

/// How much room tall gives the Replay's label in the row between Back and
/// Match Settings, which is half the window's width less the row's insets:
/// 498 px, 31 characters at 26 px, in the narrowest window. Every label the
/// game writes by itself takes two lines there at most, as do most a player
/// writes, and two lines are what the row has always had room for. A label
/// that may take a third gets a line's more room: tall's three lines under
/// it, the board and the transport row stand that much lower.
///
/// It is decided once, for the narrowest window, so that a Replay keeps it
/// at every size; a wider window can only wrap the label onto fewer lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TallLabelRoom {
    TwoLines,
    ThreeLines,
}

impl TallLabelRoom {
    /// The room `label` needs: two lines unless it may take three, judged
    /// by [`label_fits_two_lines`], which never says it fits when it may
    /// not.
    pub(super) fn for_label(label: &str) -> Self {
        let width = (MIN_WINDOW_WIDTH as f32 - 2.0 * VIEWER_HEADER_INSET) / 2.0;
        if label_fits_two_lines(label, chars_per_line(width, VIEWER_LABEL_SIZE)) {
            Self::TwoLines
        } else {
            Self::ThreeLines
        }
    }

    /// How much lower than the opening window's places tall stands what is
    /// under the label.
    const fn extra(self) -> f32 {
        match self {
            Self::TwoLines => 0.0,
            Self::ThreeLines => text_height(1, VIEWER_LABEL_SIZE),
        }
    }
}

/// How many characters of the game's monospaced font, at `font_px`, a line
/// `width` logical pixels wide holds - a hundredth of a pixel short, so
/// that a line that only just fits is not counted on.
fn chars_per_line(width: f32, font_px: f32) -> usize {
    ((width - 0.01) / line_width(1, font_px)).floor().max(1.0) as usize
}

/// Whether `label` is sure to take at most two lines `chars` characters
/// wide, as `bevy_ui` wraps a label: at the last place a line may break
/// before it is full, or within a word too long for a line.
///
/// It answers yes only for a label that fits on one line, or that has a
/// break between two words where each part fits on a line of its own; then
/// the first line can break there or later, and either way what is left
/// fits on the second. Not every space is a place a line may break: in the
/// Unicode line-breaking rules a line does not break before a closing
/// bracket, `!`, `?`, `,`, `.`, `:`, `;` or `/`, even after a space, nor
/// after an opening bracket. So only a space before a letter or a digit,
/// and not after an opening bracket, is counted, which can only make the
/// answer no where a looser count would say yes. A label is printable
/// ASCII, which `crate::replay::validate_label` holds every Replay to, so a
/// byte is a character.
fn label_fits_two_lines(label: &str, chars: usize) -> bool {
    let label = label.trim_end_matches(' ').as_bytes();
    if label.len() <= chars {
        return true;
    }
    let mut index = 0;
    while index < label.len() {
        if label[index] != b' ' {
            index += 1;
            continue;
        }
        let first_end = index;
        while index < label.len() && label[index] == b' ' {
            index += 1;
        }
        let breaks = first_end > 0
            && index < label.len()
            && label[index].is_ascii_alphanumeric()
            && !b"([{".contains(&label[first_end - 1]);
        if breaks && first_end <= chars && label.len() - index <= chars {
            return true;
        }
    }
    false
}

/// The most lines any label takes `chars` characters wide. Each line but
/// the last ends where what follows would not fit on it, so a line and the
/// one after it hold more than `chars` characters between them, counting
/// what the break between them takes; a label of up to
/// [`LABEL_MAX_CHARS`] therefore takes at most twice that over `chars`,
/// rounded down, and one line more.
#[cfg(test)]
fn longest_label_lines(chars: usize) -> usize {
    (2 * (LABEL_MAX_CHARS / chars) + 1).min(LABEL_MAX_CHARS)
}

/// The lines `text` takes `chars` characters wide, breaking only at a space
/// before a letter or a digit, as [`label_fits_two_lines`] counts a break,
/// for the screen's own timer, round and result lines: single spaces, no
/// brackets, and no run of words between two such breaks longer than a
/// line. A line may break in more places than these, which can only make
/// each line end later, and so take fewer lines.
#[cfg(test)]
fn wrapped_lines(text: &str, chars: usize) -> usize {
    // Runs of words that a line cannot break between, by their lengths.
    let mut runs: Vec<usize> = Vec::new();
    for word in text.split(' ') {
        let breaks_before = word.as_bytes()[0].is_ascii_alphanumeric();
        match runs.last_mut() {
            Some(run) if !breaks_before => *run += 1 + word.len(),
            _ => runs.push(word.len()),
        }
    }
    let mut lines = 1;
    let mut line = 0;
    for run in runs {
        assert!(
            run <= chars,
            "{text:?} has more than a line between two breaks"
        );
        if line == 0 {
            line = run;
        } else if line + 1 + run <= chars {
            line += 1 + run;
        } else {
            lines += 1;
            line = run;
        }
    }
    lines
}

/// The two arrangements' margins and the board's cap, for a Replay whose
/// label needs `label` in tall.
pub(super) const fn viewer_arrangements(label: TallLabelRoom) -> Arrangements {
    Arrangements {
        tall: Margins {
            top: TALL_TOP + label.extra(),
            bottom: TALL_BOTTOM,
            side: TALL_SIDE,
        },
        wide: Margins {
            top: WIDE_TOP,
            bottom: WIDE_BOTTOM,
            side: WIDE_SIDE,
        },
        cap: VIEWER_BOARD_OUTER_SIZE * MAX_ZOOM,
    }
}

/// How the Key sets its ten rows: in one column, as it always has, or in
/// two side by side, in a window too short for one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyColumns {
    One,
    Two,
}

/// How the Key is set in a window of `window`: in one column wherever the
/// window holds that panel with the Key's edge above and below it.
fn key_columns(window: Vec2) -> KeyColumns {
    if window.y >= KEY_ONE_COLUMN_HEIGHT + 2.0 * KEY_EDGE {
        KeyColumns::One
    } else {
        KeyColumns::Two
    }
}

/// One of tall's three lines under the label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HeaderLine {
    Timer,
    Round,
    Result,
}

/// Where everything in the Viewer stands in one window. Every length is in
/// logical pixels from the window's top-left; the module docs give the
/// rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ViewerLayout {
    /// The size laid out for: the window's, raised to the minimum.
    pub(super) window: Vec2,
    pub(super) arrangement: Arrangement,
    /// The board's wood, edge to edge, where it is drawn.
    pub(super) board: Rect,
    /// How far down the window the piece - board and interface together -
    /// stands: half the height the window has beyond what the piece needs,
    /// and zero wherever the board fills the height.
    drop: f32,
    /// How much lower than the opening window's places tall stands what is
    /// under the label: a line, for a label that needs a third.
    label_extra: f32,
    pub(super) key: KeyColumns,
}

/// Lays the Viewer out for a window of `window` logical pixels, which the
/// caller has already raised to the minimum with `WindowSize::layout`, for
/// a Replay whose label needs `label` in tall: in the arrangement
/// [`viewer_arrangements`] chooses, with the room it leaves the board,
/// capped at twice the board's size.
pub(super) fn viewer_layout(window: Vec2, label: TallLabelRoom) -> ViewerLayout {
    let arrangements = viewer_arrangements(label);
    let arrangement = arrangements.choose(window);
    let size = arrangements.board(window, arrangement);
    let margins = arrangements.margins(arrangement);
    let drop = (window.y - (margins.top + size + margins.bottom)) / 2.0;
    let left = (window.x - size) / 2.0;
    let top = drop + margins.top;
    ViewerLayout {
        window,
        arrangement,
        board: Rect::new(left, top, left + size, top + size),
        drop,
        label_extra: match arrangement {
            Arrangement::Tall => label.extra(),
            Arrangement::Wide => 0.0,
        },
        key: key_columns(window),
    }
}

impl ViewerLayout {
    /// How large the board is drawn against its own size.
    pub(super) fn zoom(&self) -> f32 {
        self.board.width() / VIEWER_BOARD_OUTER_SIZE
    }

    /// The main camera's framing that draws the board where this layout
    /// puts it: the board's centre at the centre of [`Self::board`], at
    /// [`Self::zoom`].
    pub(super) fn framing(&self) -> WorldFraming {
        WorldFraming::placing(
            VIEWER_BOARD_WORLD_CENTER,
            self.board.center(),
            self.window,
            self.zoom(),
        )
    }

    /// Where the world point `world` on the board is drawn, in logical
    /// pixels from the window's top-left. World coordinates run up and
    /// window coordinates down.
    pub(super) fn window_point(&self, world: Vec2) -> Vec2 {
        let from_center = (world - VIEWER_BOARD_WORLD_CENTER) * self.zoom();
        let center = self.board.center();
        Vec2::new(center.x + from_center.x, center.y - from_center.y)
    }

    /// The world point drawn at `window_point`: the inverse of
    /// [`Self::window_point`].
    fn world_point(&self, window_point: Vec2) -> Vec2 {
        let from_center = (window_point - self.board.center()) / self.zoom();
        VIEWER_BOARD_WORLD_CENTER + Vec2::new(from_center.x, -from_center.y)
    }

    /// Whether the label, the timer, the round and the result line stand in
    /// the column under Back, as they do in wide, rather than across the
    /// top.
    pub(super) fn has_column(&self) -> bool {
        self.arrangement == Arrangement::Wide
    }

    /// The top of the row of Back, tall's label and Match Settings.
    pub(super) fn header_top(&self) -> f32 {
        self.drop + VIEWER_HEADER_TOP
    }

    /// The top of one of tall's three lines under the label.
    pub(super) fn tall_line_top(&self, line: HeaderLine) -> f32 {
        let opening = match line {
            HeaderLine::Timer => TALL_LINE_TOPS[0],
            HeaderLine::Round => TALL_LINE_TOPS[1],
            HeaderLine::Result => TALL_LINE_TOPS[2],
        };
        self.drop + self.label_extra + opening
    }

    /// The left, top and width of wide's column under Back: from the
    /// column's inset off the window's side to its inset off the board.
    pub(super) fn column(&self) -> (f32, f32, f32) {
        (
            WIDE_COLUMN_INSET,
            self.drop + WIDE_COLUMN_TOP,
            self.board.min.x - 2.0 * WIDE_COLUMN_INSET,
        )
    }

    /// How far the transport row's bottom stands above the window's.
    pub(super) fn controls_bottom(&self) -> f32 {
        self.drop + VIEWER_CONTROL_BOTTOM
    }

    /// The width of the Key's panel.
    pub(super) fn key_width(&self) -> f32 {
        match self.key {
            KeyColumns::One => KEY_ONE_COLUMN_WIDTH,
            KeyColumns::Two => KEY_TWO_COLUMN_WIDTH,
        }
    }
}

/// How large a score badge is drawn against how the board around it is: the
/// inverse of the board's zoom, so that a badge, and the 15 px score in it,
/// are drawn at their own size in logical pixels whatever the board does.
/// Only across the screen; depth is left alone.
pub(super) fn score_badge_scale(layout: &ViewerLayout) -> Vec3 {
    let scale = 1.0 / layout.zoom();
    Vec3::new(scale, scale, 1.0)
}

/// Where `player`'s score badge sits, as a world point on the board: up and
/// to the right of their cursor, inside the window. Every length here is in
/// the window's logical pixels, as the badge is drawn
/// ([`score_badge_scale`]): it stands [`SCORE_BADGE_CURSOR_GAP`] off its
/// cursor's corner, its cursor being drawn at the board's zoom, and stays
/// [`SCORE_BADGE_EDGE_INSET`] inside the window, which is the part of the
/// world the camera shows.
///
/// When both cursors share a point the two badges sit side by side,
/// [`SCORE_BADGE_OVERLAP_GAP`] apart, and they are identical but for a
/// frame in each player's accent - 1.54:1 apart, one colour to a player who
/// cannot see hue. Their order is then the channel that says whose is whose,
/// so it never changes: Player One's is the left one, the order the
/// Viewer's result line names the players in (`Black 3 - White 5`), at the
/// edge of the window as anywhere else. That was the owner's choice on
/// 2026-10-03; `src/contrast.rs` cites it.
///
/// It is not the order the game places players in - the Match screen puts
/// Black / Player 1 on the right - and it was weighed against that on
/// 2026-10-06 and kept. The badges are a score readout, and every score
/// readout in the game that puts two numbers in a row names Black first:
/// the result line, the Replay Library's summary and a recorded Replay's
/// default name. (The Match screen's end scores are not a row: each stands
/// beside its own player's name, in that player's corner.) Read left to
/// right, the pair agrees with the line that gives the same two numbers
/// over the board. What the order lacked was being said, and the Key now
/// says it, in a sentence and a swatch that
/// `the_keys_badge_row_names_the_order_the_board_draws` in
/// [`super::modals`] holds to this function, so the two cannot drift.
pub(super) fn score_badge_position(
    cursor: Vec2,
    player: Player,
    cursors_overlap: bool,
    layout: &ViewerLayout,
) -> Vec2 {
    let zoom = layout.zoom();
    let cursor = layout.window_point(cursor);
    let half_badge = SCORE_BADGE_SIZE * 0.5;
    let inset = SCORE_BADGE_EDGE_INSET;
    let minimum = half_badge + Vec2::splat(inset);
    let maximum = layout.window - half_badge - Vec2::splat(inset);
    let half_cursor = |player: Player| cursor_visual_style(player).size * zoom * 0.5;
    let position = if cursors_overlap {
        let half_cursor = half_cursor(Player::One).max(half_cursor(Player::Two));
        let first_x = cursor.x + half_cursor + half_badge.x + SCORE_BADGE_CURSOR_GAP;
        let second_x = first_x + SCORE_BADGE_SIZE.x + SCORE_BADGE_OVERLAP_GAP;
        let pair_left = first_x - half_badge.x;
        let pair_right = second_x + half_badge.x;
        let horizontal_shift = if pair_right > layout.window.x - inset {
            layout.window.x - inset - pair_right
        } else if pair_left < inset {
            inset - pair_left
        } else {
            0.0
        };
        Vec2::new(
            match player {
                Player::One => first_x,
                Player::Two => second_x,
            } + horizontal_shift,
            cursor.y - (half_cursor + half_badge.y + SCORE_BADGE_CURSOR_GAP),
        )
    } else {
        let half_cursor = half_cursor(player);
        cursor
            + Vec2::new(
                half_cursor + half_badge.x + SCORE_BADGE_CURSOR_GAP,
                -(half_cursor + half_badge.y + SCORE_BADGE_CURSOR_GAP),
            )
    };
    layout.world_point(position.clamp(minimum, maximum))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{
        CursorVisualStyle, NAMED_WINDOW_SIZES, OUTLINE_ARM, STAR_POINT_RADIUS, STONE_RADIUS,
        WindowSize, layout_sweep, stone_geometry,
    };
    use crate::rules::Cell;
    use crate::ui::arrangement::WIDE_GAIN;
    use crate::ui::text_metrics::half_leading;

    /// Pins the decorative star-point table: exactly the five traditional
    /// 9x9 hoshi, in bounds, and symmetric as a *set* - mirroring every
    /// point through the board centre yields the same five points, which is
    /// what makes the marking read the same from either player's side.
    ///
    /// The set property is the one worth asserting. Testing that each
    /// mirrored *pair* lands symmetrically under a geometry function proves
    /// nothing about the table: both geometries are affine, so
    /// `pos(p) + pos(mirror(p))` equals `2 * pos(centre)` for every point on
    /// the board whatever the table holds.
    #[test]
    fn star_points_are_the_five_hoshi_and_mirror_onto_themselves() {
        use crate::app::visuals::STAR_POINTS;

        let mut sorted: Vec<(usize, usize)> =
            STAR_POINTS.iter().map(|point| (point.x, point.y)).collect();
        sorted.sort_unstable();
        assert_eq!(sorted, [(2, 2), (2, 6), (4, 4), (6, 2), (6, 6)]);

        for point in STAR_POINTS {
            assert!(point.x < BOARD_SIZE && point.y < BOARD_SIZE);
        }

        let mirrored =
            |point: Point| Point::new(BOARD_SIZE - 1 - point.x, BOARD_SIZE - 1 - point.y);
        let mut mirrored_set: Vec<(usize, usize)> = STAR_POINTS
            .iter()
            .map(|point| {
                let point = mirrored(*point);
                (point.x, point.y)
            })
            .collect();
        mirrored_set.sort_unstable();
        assert_eq!(mirrored_set, sorted);
    }

    /// Distance from the centre to the inner and outer edges of a cursor
    /// outline's stroke, measured square to a side.
    fn stroke_band(style: CursorVisualStyle) -> (f32, f32) {
        (
            (style.size - style.border_thickness) / 2.0,
            (style.size + style.border_thickness) / 2.0,
        )
    }

    /// How far along a side, from its middle, a cursor outline's arms begin.
    fn mid_side_gap(style: CursorVisualStyle) -> f32 {
        style.size / 2.0 - style.size * OUTLINE_ARM
    }

    /// The farthest a marker's ink lies from its centre: the outer corner
    /// of its outer stroke, where the bars stop at the notched tip.
    fn farthest_ink(layout: ViewerMarkerLayout) -> f32 {
        let [outer, ..] = layout.outer_edges;
        outer.hypot(layout.reach(outer))
    }

    /// The inner edge of each of a hidden-selection mark's three strokes,
    /// outermost first; the last is the edge of its opening.
    fn stroke_inner_edges(shared: bool) -> [f32; 3] {
        let mut edge = viewer_selection_radius(shared);
        VIEWER_SELECTION_STROKES.map(|width| {
            edge -= width;
            edge
        })
    }

    /// A marker's three strokes lie together on the side of its square,
    /// outermost first, each drawn in to the one inner edge; and they are
    /// finer than a cursor's, but each still two pixels or more - the outer
    /// and innermost are what carry the marker on wood and on a stone of
    /// the outer one's colour, and the accent is wide enough to show its
    /// hue. The diamond keeps an open middle: it is an outline, not a blob.
    #[test]
    fn a_teleport_marker_is_three_strokes_on_the_side_of_its_square() {
        assert!(VIEWER_MARKER_STROKES.iter().all(|width| *width >= 2.0));
        let thickness: f32 = VIEWER_MARKER_STROKES.iter().sum();
        let [outer_width, accent_width, inner_width] = VIEWER_MARKER_STROKES;
        assert!(
            accent_width >= outer_width.max(inner_width),
            "the accent is the widest"
        );
        for player in Player::ALL {
            let cursor = cursor_visual_style(player);
            let marker = viewer_marker_layout(player);
            assert!(marker.size < cursor.size);
            assert!(thickness < cursor.border_thickness);
            let [outer, accent, inner] = marker.outer_edges;
            assert_eq!(
                (marker.inner_edge, outer),
                (
                    (marker.size - thickness) / 2.0,
                    (marker.size + thickness) / 2.0
                ),
                "{player:?}'s strokes are not centred on the side"
            );
            assert_eq!(outer - accent, outer_width, "{player:?}'s outer stroke");
            assert_eq!(accent - inner, accent_width, "{player:?}'s accent stroke");
            assert_eq!(
                inner - marker.inner_edge,
                inner_width,
                "{player:?}'s inner stroke"
            );
            assert!(
                2.0 * marker.inner_edge >= 10.0,
                "{player:?}'s diamond closes up"
            );
        }
    }

    /// Every stroke's bars run to the corners of that stroke's own square,
    /// except where that would pass the corners of the marker's `size`
    /// square: the two strokes outside it stop there, which notches the
    /// tip by half the marker's stroke, and the innermost, which lies
    /// inside it, keeps whole corners.
    #[test]
    fn a_teleport_markers_tips_are_notched_at_its_squares_corners() {
        for player in Player::ALL {
            let marker = viewer_marker_layout(player);
            let [outer, accent, inner] = marker.outer_edges;
            assert_eq!(marker.reach(outer), marker.size / 2.0);
            assert_eq!(marker.reach(accent), marker.size / 2.0);
            assert_eq!(marker.reach(inner), inner, "{player:?}");
            assert!(marker.reach(outer) < outer);
            // The notch is small against the tip, so the tip still reads.
            let whole_tip = outer * std::f32::consts::SQRT_2;
            assert!(whole_tip - marker.tip() < 3.0, "{player:?}");
        }
    }

    /// Player One's diamond, the larger, fits its cell on the Viewer's
    /// tighter grid with room to spare, and stops short of a stone on the
    /// next point.
    #[test]
    fn a_teleport_marker_fits_its_cell_on_its_point() {
        let one = viewer_marker_layout(Player::One);
        let two = viewer_marker_layout(Player::Two);
        assert!(two.tip() < one.tip(), "Player Two's marker is the smaller");
        let footprint = 2.0 * one.tip();
        assert!(
            VIEWER_GRID_SPACING - footprint >= 4.0,
            "a {footprint}px diamond leaves no visible clearance in a {VIEWER_GRID_SPACING}px cell"
        );
        let neighbouring_stone = VIEWER_GRID_SPACING - STONE_RADIUS;
        assert!(neighbouring_stone - one.tip() >= 6.0);
        // Two markers on adjacent points keep their tips apart too.
        assert!(VIEWER_GRID_SPACING - one.tip() - two.tip() >= 6.0);
    }

    /// With both markers on one point, the board shows between the two
    /// diamonds: Player Two's whole stroke lies inside Player One's.
    #[test]
    fn two_teleport_markers_on_one_point_nest_with_board_between_them() {
        let one = viewer_marker_layout(Player::One);
        let two = viewer_marker_layout(Player::Two);
        let [two_outer_edge, ..] = two.outer_edges;
        assert!(
            one.inner_edge - two_outer_edge >= 3.0,
            "only {}px of board shows between two nested markers",
            one.inner_edge - two_outer_edge
        );
        // Along a grid line too, where Player Two's tips come nearest
        // Player One's inner corners.
        let one_inner_corner = one.inner_edge * std::f32::consts::SQRT_2;
        assert!(one_inner_corner - farthest_ink(two) >= 3.0);
    }

    /// A marker is set where its player's cursor stands, so the two share
    /// a point whenever one has just been set or jumped to: the diamond
    /// lies inside the cursor's brackets, its tips reaching only into the
    /// gaps they leave mid-side.
    #[test]
    fn a_teleport_marker_sits_inside_its_own_players_cursor() {
        for player in Player::ALL {
            let cursor = cursor_visual_style(player);
            let marker = viewer_marker_layout(player);
            let (cursor_inner_edge, _) = stroke_band(cursor);
            assert!(mid_side_gap(cursor) > 0.0, "a cursor has no gap mid-side");
            // Where the cursor's arms begin, the diamond's outer edge has
            // come in to here.
            let [marker_outer_edge, ..] = marker.outer_edges;
            let reach = marker_outer_edge * std::f32::consts::SQRT_2 - mid_side_gap(cursor);
            assert!(
                cursor_inner_edge - reach >= 1.0,
                "{player:?}'s marker reaches {reach} under a cursor whose arms begin at \
                 {cursor_inner_edge}"
            );
        }
    }

    /// The ordinary hidden-selection mark lies inside both cursors - a
    /// selection is made with the cursor on it - and inside the fill of
    /// every stone, so that on a stone its strokes are seen against that
    /// stone alone. Its opening shows what is beneath: wider than a star
    /// point, and so than the dot of an empty one.
    #[test]
    fn a_hidden_selection_mark_lies_inside_both_cursors_and_every_stone() {
        let radius = viewer_selection_radius(false);
        for player in Player::ALL {
            let (cursor_inner_edge, _) = stroke_band(cursor_visual_style(player));
            assert!(cursor_inner_edge - radius >= 1.0, "{player:?}");
        }
        for cell in [Cell::Black, Cell::White, Cell::Gray] {
            let fill = stone_geometry(cell).fill_scale * STONE_RADIUS;
            assert!(fill - radius >= 1.0, "{cell:?}");
        }
        let [.., opening] = stroke_inner_edges(false);
        assert!(opening > STAR_POINT_RADIUS);
        assert!(STAR_POINT_RADIUS > stone_geometry(Cell::Empty).fill_scale * STONE_RADIUS);
    }

    /// Both forms of the mark keep three strokes of two pixels or more -
    /// the smaller one is a mark of its own, not the larger one scaled
    /// down - around a bare middle wider than the strokes around it are
    /// deep, so that each still reads as a ring and not as a disc.
    #[test]
    fn a_hidden_selection_mark_keeps_its_strokes_and_its_opening_at_both_sizes() {
        assert!(VIEWER_SELECTION_STROKES.iter().all(|width| *width >= 2.0));
        let band: f32 = VIEWER_SELECTION_STROKES.iter().sum();
        for shared in [false, true] {
            let [.., opening] = stroke_inner_edges(shared);
            assert!(2.0 * opening > band, "shared: {shared}");
        }
        assert!(viewer_selection_radius(true) < viewer_selection_radius(false));
    }

    /// What a marker and a selection on one point do to each other. The
    /// mark is drawn over the marker, so no marker covers it; this holds
    /// how much of each diamond the mark leaves. Player Two's keeps its
    /// innermost stroke whole and its accent along most of each side
    /// inside the opening, and its tips stop short of the mark's outer
    /// stroke. Player One's begins outside the innermost stroke, and its
    /// tips stand well clear of the whole mark.
    #[test]
    fn a_marker_and_a_selection_on_one_point_leave_each_other_readable() {
        let radius = viewer_selection_radius(false);
        let [outer_stroke, accent_stroke, opening] = stroke_inner_edges(false);

        let two = viewer_marker_layout(Player::Two);
        let [_, accent_outer_edge, inner_outer_edge] = two.outer_edges;
        assert!(
            inner_outer_edge.hypot(two.reach(inner_outer_edge)) < opening,
            "Player Two's innermost stroke reaches under the mark"
        );
        // A side runs from its middle out to a notched tip. The outer edge
        // of the accent leaves the opening this far along it.
        assert!(accent_outer_edge < opening);
        let leaves_at = (opening.powi(2) - accent_outer_edge.powi(2)).sqrt();
        let share_inside = leaves_at / two.reach(accent_outer_edge);
        assert!(
            share_inside >= 0.7,
            "only {share_inside} of each side of Player Two's accent shows inside the opening"
        );
        assert!(
            farthest_ink(two) < outer_stroke,
            "Player Two's tips reach the outer stroke"
        );

        let one = viewer_marker_layout(Player::One);
        assert!(
            one.inner_edge > accent_stroke,
            "Player One's sides reach the innermost stroke"
        );
        assert!(
            one.tip() - radius >= 8.0,
            "Player One's tips barely clear the mark"
        );
    }

    /// Two selections of one point are drawn side by side: apart from each
    /// other, inside the cell, each still on its own side of the point,
    /// and reaching no further than the inner edge of Player One's cursor.
    #[test]
    fn two_hidden_selections_of_one_point_sit_side_by_side_inside_the_cell() {
        let point = Point::new(4, 4);
        let centre = viewer_board_position(point);
        for player in Player::ALL {
            assert_eq!(viewer_selection_position(point, player, false), centre);
        }

        let one = viewer_selection_position(point, Player::One, true);
        let two = viewer_selection_position(point, Player::Two, true);
        assert_eq!((one.y, two.y), (centre.y, centre.y));
        assert!(
            one.x < centre.x && centre.x < two.x,
            "Player One's is on the left"
        );
        assert_eq!(centre.x - one.x, two.x - centre.x);
        let radius = viewer_selection_radius(true);
        assert!(two.x - one.x - 2.0 * radius >= 1.0, "the two marks touch");
        let reach = two.x + radius - centre.x;
        assert!(
            reach <= VIEWER_GRID_SPACING / 2.0 - 2.0,
            "the pair leaves no clearance in its cell"
        );
        let (cursor_inner_edge, _) = stroke_band(cursor_visual_style(Player::One));
        assert!(
            reach <= cursor_inner_edge,
            "the pair runs under Player One's cursor"
        );
    }

    fn opening() -> ViewerLayout {
        viewer_layout(WindowSize::OPENING.0, TallLabelRoom::TwoLines)
    }

    const LABEL_ROOMS: [TallLabelRoom; 2] = [TallLabelRoom::TwoLines, TallLabelRoom::ThreeLines];
    const LINES: [HeaderLine; 3] = [HeaderLine::Timer, HeaderLine::Round, HeaderLine::Result];

    /// In the opening window the layout is the one the screen has always
    /// had, number for number: the board where it was, drawn at its own
    /// size by a camera at a scale of 1 over the origin, and every piece of
    /// interface where it was spawned before the layout adapted. The
    /// numbers are written out, not this module's constants.
    #[test]
    fn the_opening_window_is_laid_out_exactly_as_it_always_was() {
        let opening = opening();
        assert_eq!(opening.arrangement, Arrangement::Tall);
        assert_eq!(opening.board, Rect::new(213.0, 188.0, 827.0, 802.0));
        assert_eq!(opening.zoom(), 1.0);
        assert_eq!(opening.framing(), WorldFraming::IDENTITY);
        assert_eq!(
            opening.window_point(Vec2::new(0.0, VIEWER_BOARD_CENTER_Y)),
            Vec2::new(520.0, 495.0),
            "the board's centre, 495 px down the window"
        );
        assert_eq!(opening.header_top(), 12.0);
        assert_eq!(
            LINES.map(|line| opening.tall_line_top(line)),
            [80.0, 104.0, 128.0]
        );
        assert_eq!(opening.controls_bottom(), 22.0);
        assert!(!opening.has_column());
        assert_eq!(opening.key, KeyColumns::One);
        assert_eq!(opening.key_width(), 720.0);
        assert_eq!(score_badge_scale(&opening), Vec3::ONE);
    }

    /// The score badges in the opening window are where they always were,
    /// number for number: up and to the right of a cursor by its half size
    /// and the badge's, 4 px off its corner; a pair on a shared point side
    /// by side 5 px apart; and a badge kept 4 px inside the window.
    #[test]
    fn the_opening_window_places_the_badges_where_it_always_did() {
        let opening = opening();
        let centre = Vec2::new(0.0, VIEWER_BOARD_CENTER_Y);
        let place =
            |cursor, player, overlap| score_badge_position(cursor, player, overlap, &opening);
        assert_eq!(place(centre, Player::One, false), Vec2::new(59.0, 7.0));
        assert_eq!(place(centre, Player::Two, false), Vec2::new(51.0, -1.0));
        assert_eq!(place(centre, Player::One, true), Vec2::new(59.0, 7.0));
        assert_eq!(place(centre, Player::Two, true), Vec2::new(106.0, 7.0));
        let top_right = Vec2::new(268.0, 223.0);
        assert_eq!(place(top_right, Player::One, true), Vec2::new(327.0, 275.0));
        assert_eq!(place(top_right, Player::Two, true), Vec2::new(374.0, 275.0));
        let far = Vec2::new(10_000.0, 10_000.0);
        assert_eq!(place(far, Player::One, false), Vec2::new(495.0, 432.0));
        assert_eq!(place(far, Player::Two, true), Vec2::new(495.0, 432.0));
        assert_eq!(place(far, Player::One, true), Vec2::new(448.0, 432.0));
    }

    /// The two rooms a window of `size` leaves the board, tall's and
    /// wide's, before the cap, worked out here from the margins written
    /// out as numbers rather than by the functions under test: tall keeps
    /// 188 px above the board, or a line of 26 px text more for a label
    /// that needs it, 98 below and 213 a side; wide 40 above, 76 below and
    /// 264 a side.
    fn rooms(size: Vec2, label: TallLabelRoom) -> (f32, f32) {
        let above = match label {
            TallLabelRoom::TwoLines => 188.0,
            TallLabelRoom::ThreeLines => 188.0 + 31.2,
        };
        (
            (size.x - 426.0).min(size.y - above - 98.0),
            (size.x - 528.0).min(size.y - 116.0),
        )
    }

    /// The board's cap: twice its own 614 px.
    const CAP: f32 = 1228.0;

    /// Whether `size` should be wide, decided here as the arrangement
    /// module says: by the two rooms below the cap; past it, by the rooms
    /// of the same shape scaled down until the larger is at the cap, which
    /// this finds by halving rather than by the closed form the rule uses.
    fn expected_wide(size: Vec2, label: TallLabelRoom) -> bool {
        let larger = |size: Vec2| {
            let (tall, wide) = rooms(size, label);
            tall.max(wide)
        };
        let mut at = size;
        if larger(size) > CAP {
            let (mut low, mut high) = (0.0_f32, 1.0_f32);
            for _ in 0..48 {
                let middle = (low + high) / 2.0;
                if larger(size * middle) < CAP {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            at = size * high;
        }
        let (tall, wide) = rooms(at, label);
        wide > tall * WIDE_GAIN
    }

    /// Every window gets a board between the smallest window's and twice
    /// its own size: inside the window, centred across it, square, and
    /// within its arrangement's margins. The arrangement is the one the
    /// shared rule gives these margins, worked out here for itself, and the
    /// board is its room, capped, so a larger window never gets a board
    /// smaller by more than the rule's 4%. Every window narrower than 1.19
    /// times its height is tall, or 1.16 for a label that needs a third
    /// line, as the module docs say. All of it for either room the label
    /// can need.
    #[test]
    fn the_board_grows_with_the_window_from_the_smallest_up_to_twice_its_size() {
        let smallest = viewer_layout(WindowSize::MINIMUM.0, TallLabelRoom::TwoLines).zoom();
        assert!(
            (0.785..0.795).contains(&smallest),
            "the smallest window's board is 0.79 of its own size: {smallest}"
        );
        for label in LABEL_ROOMS {
            let mut both = [false, false];
            for size in layout_sweep() {
                let layout = viewer_layout(size, label);
                let (board, zoom) = (layout.board, layout.zoom());
                let context = format!("{size} {label:?}");
                assert!(
                    zoom >= smallest - 1e-5 && zoom <= 2.0 + 1e-5,
                    "{context}: {zoom}"
                );
                assert!((board.width() - board.height()).abs() < 1e-3, "{context}");
                assert!((board.center().x - size.x / 2.0).abs() < 1e-3, "{context}");
                let wide = expected_wide(size, label);
                assert_eq!(layout.arrangement == Arrangement::Wide, wide, "{context}");
                let (tall_room, wide_room) = rooms(size, label);
                let room = if wide { wide_room } else { tall_room };
                assert!(
                    (board.width() - room.min(CAP)).abs() < 1e-3,
                    "{context}: {board:?} for a room of {room}"
                );
                let margins = viewer_arrangements(label).margins(layout.arrangement);
                assert!(
                    board.min.y >= margins.top - 1e-3
                        && board.max.y <= size.y - margins.bottom + 1e-3
                        && board.min.x >= margins.side - 1e-3,
                    "{context}: {board:?}"
                );
                let narrow = match label {
                    TallLabelRoom::TwoLines => 1.19,
                    TallLabelRoom::ThreeLines => 1.16,
                };
                if size.x / size.y < narrow {
                    assert_eq!(layout.arrangement, Arrangement::Tall, "{context}");
                }
                both[usize::from(wide)] = true;
                for larger in [size + Vec2::new(20.0, 0.0), size + Vec2::new(0.0, 20.0)] {
                    assert!(
                        viewer_layout(larger, label).zoom() * WIDE_GAIN >= zoom - 1e-5,
                        "{context} -> {larger}"
                    );
                }
            }
            assert_eq!(both, [true, true], "{label:?}: the sweep reaches both");
        }
    }

    /// The figures this layout gives the named windows: its arrangement and
    /// the board's zoom, which the layout holds to within 0.01 - tall at its
    /// own size in the opening window, and wide everywhere else, from 0.79
    /// at the minimum to twice its size at 2560x1440.
    #[test]
    fn each_named_window_gets_its_arrangement_and_zoom() {
        let expected = [
            (Arrangement::Tall, 1.00),
            (Arrangement::Wide, 0.79),
            (Arrangement::Wide, 0.86),
            (Arrangement::Wide, 0.98),
            (Arrangement::Wide, 0.93),
            (Arrangement::Wide, 1.06),
            (Arrangement::Wide, 1.19),
            (Arrangement::Wide, 1.57),
            (Arrangement::Wide, 2.00),
        ];
        for (size, (arrangement, zoom)) in NAMED_WINDOW_SIZES.into_iter().zip(expected) {
            let layout = viewer_layout(size, TallLabelRoom::TwoLines);
            assert_eq!(layout.arrangement, arrangement, "{size}");
            assert!(
                (layout.zoom() - zoom).abs() <= 0.01,
                "{size}: {} against {zoom}",
                layout.zoom()
            );
        }
    }

    /// Where the arrangement turns, pinned on both sides: a window 1040 px
    /// wide is wide up to 778 px tall and tall from 779, or for a label
    /// that needs a third line in tall, wide up to 809 and tall from 810.
    /// The opening shape is tall at every size. Past the cap the shape
    /// alone decides, at about 1.20 times as wide as tall, or 1.17 for such
    /// a label: a little narrower is tall, a little wider wide, however
    /// large the window.
    #[test]
    fn the_arrangement_turns_where_the_module_docs_say() {
        for (label, last_wide, first_tall, boundary) in [
            (TallLabelRoom::TwoLines, 778.0, 779.0, 1.197),
            (TallLabelRoom::ThreeLines, 809.0, 810.0, 1.172),
        ] {
            let at = |width: f32, height: f32| {
                viewer_layout(Vec2::new(width, height), label).arrangement
            };
            assert_eq!(at(1040.0, last_wide), Arrangement::Wide, "{label:?}");
            assert_eq!(at(1040.0, first_tall), Arrangement::Tall, "{label:?}");
            for scale in [1.0, 1.5, 2.0, 3.0, 5.0] {
                let size = WindowSize::OPENING.0 * scale;
                assert_eq!(at(size.x, size.y), Arrangement::Tall, "{label:?} {size}");
            }
            for height in [2_000.0, 3_000.0, 5_000.0, 10_000.0] {
                let (tall, wide) = rooms(Vec2::new(height * boundary, height), label);
                assert!(tall.max(wide) > CAP, "{height} is past the cap");
                assert_eq!(at(height * (boundary - 0.005), height), Arrangement::Tall);
                assert_eq!(at(height * (boundary + 0.005), height), Arrangement::Wide);
            }
        }
        for (size, arrangement) in [
            (Vec2::new(2560.0, 1440.0), Arrangement::Wide),
            (Vec2::new(2560.0, 1600.0), Arrangement::Wide),
            (Vec2::new(3840.0, 2160.0), Arrangement::Wide),
            (Vec2::new(5120.0, 2880.0), Arrangement::Wide),
            (Vec2::new(1440.0, 2560.0), Arrangement::Tall),
            (Vec2::new(2160.0, 3840.0), Arrangement::Tall),
        ] {
            let layout = viewer_layout(size, TallLabelRoom::TwoLines);
            assert_eq!(layout.arrangement, arrangement, "{size}");
        }
    }

    /// A window smaller than the minimum in either direction is laid out as
    /// the minimum would be in that direction.
    #[test]
    fn a_window_below_the_minimum_is_laid_out_at_the_minimum() {
        let layout = |width, height| {
            viewer_layout(
                WindowSize(Vec2::new(width, height)).layout(),
                TallLabelRoom::TwoLines,
            )
        };
        assert_eq!(layout(800.0, 500.0), layout(1040.0, 600.0));
        assert_eq!(layout(1600.0, 400.0), layout(1600.0, 600.0));
        assert_eq!(layout(900.0, 900.0), layout(1040.0, 900.0));
    }

    /// The tops of the four things a moved piece carries: the header row,
    /// tall's lines, wide's column and the board, and the transport row's
    /// bottom.
    fn tops(layout: &ViewerLayout) -> Vec<f32> {
        let mut tops = vec![layout.header_top(), layout.column().1, layout.board.min.y];
        tops.extend(LINES.map(|line| layout.tall_line_top(line)));
        tops.push(-layout.controls_bottom());
        tops
    }

    /// A window with more room than the piece needs keeps the piece whole
    /// and centres it, in either arrangement: the header, the board and the
    /// transport row move down together, by half the height added. In
    /// tall, a window taller than the opening one; in wide, one past the
    /// size where the board reaches its cap, and one whose width limits the
    /// board, where the piece stands centred from the first.
    #[test]
    fn a_window_with_room_to_spare_centres_the_piece_in_either_arrangement() {
        let moved_down = |from: Vec2, to: Vec2, by: f32| {
            let (from, to) = (
                viewer_layout(from, TallLabelRoom::TwoLines),
                viewer_layout(to, TallLabelRoom::TwoLines),
            );
            assert_eq!(from.arrangement, to.arrangement);
            assert_eq!(from.board.size(), to.board.size());
            let moves: Vec<f32> = tops(&to)
                .iter()
                .zip(tops(&from))
                .map(|(to, from)| to - from)
                .collect();
            let mut expected = vec![by; moves.len()];
            *expected.last_mut().unwrap() = -by;
            assert!(
                moves
                    .iter()
                    .zip(&expected)
                    .all(|(moved, by)| (moved - by).abs() < 1e-3),
                "{} -> {}: {moves:?}",
                from.window,
                to.window
            );
        };
        moved_down(Vec2::new(1040.0, 900.0), Vec2::new(1040.0, 1100.0), 100.0);
        // 2560x1344 is the window whose height the capped board and its
        // margins fill exactly.
        let filled = viewer_layout(Vec2::new(2560.0, 1344.0), TallLabelRoom::TwoLines);
        assert_eq!(filled.arrangement, Arrangement::Wide);
        assert!((filled.board.min.y - 40.0).abs() < 1e-3);
        moved_down(Vec2::new(2560.0, 1344.0), Vec2::new(2560.0, 1544.0), 100.0);
        // At 1040x700 the width limits a wide board to 512 px, which leaves
        // 36 px above the piece and below it.
        let narrow = viewer_layout(Vec2::new(1040.0, 700.0), TallLabelRoom::TwoLines);
        assert_eq!(narrow.arrangement, Arrangement::Wide);
        assert_eq!(narrow.board.width(), 512.0);
        assert!((narrow.header_top() - 12.0 - 36.0).abs() < 1e-3);
        assert!((narrow.controls_bottom() - 22.0 - 36.0).abs() < 1e-3);
    }

    /// A label that needs a third line in tall gets one: the lines under it,
    /// the board and the transport row stand a 26 px line lower than they
    /// would, and the header row where it was; in wide nothing moves.
    #[test]
    fn a_label_that_needs_a_third_line_moves_tall_down_a_line() {
        for size in [Vec2::new(1040.0, 1100.0), Vec2::new(1300.0, 1300.0)] {
            let two = viewer_layout(size, TallLabelRoom::TwoLines);
            let three = viewer_layout(size, TallLabelRoom::ThreeLines);
            assert_eq!(two.arrangement, Arrangement::Tall, "{size}");
            assert_eq!(three.arrangement, Arrangement::Tall, "{size}");
            let line = 31.2;
            let moved = three.board.min.y - two.board.min.y;
            // A board with room to spare stays its size and the piece moves
            // half a line; a board that fills the height shrinks by a line.
            for line_name in LINES {
                assert!(
                    (three.tall_line_top(line_name)
                        - three.board.min.y
                        - (two.tall_line_top(line_name) - two.board.min.y))
                        .abs()
                        < 1e-3,
                    "{size}: {line_name:?} keeps its place above the board"
                );
            }
            assert!(
                ((three.board.min.y - three.header_top())
                    - (two.board.min.y - two.header_top())
                    - line)
                    .abs()
                    < 1e-3,
                "{size}: a line more between the header row and the board"
            );
            assert!(moved > 0.0, "{size}");
        }
        let wide = Vec2::new(1280.0, 641.0);
        assert_eq!(
            viewer_layout(wide, TallLabelRoom::ThreeLines),
            viewer_layout(wide, TallLabelRoom::TwoLines)
        );
        let board = viewer_layout(WindowSize::OPENING.0, TallLabelRoom::ThreeLines).board;
        let expected = Rect::new(228.6, 219.2, 811.4, 802.0);
        assert!(
            (board.min - expected.min).abs().max_element() < 1e-3
                && (board.max - expected.max).abs().max_element() < 1e-3,
            "the opening window's board, a line shorter and a line lower: {board:?}"
        );
    }

    /// A box from its left, top, width and height.
    fn rect(left: f32, top: f32, width: f32, height: f32) -> Rect {
        Rect::new(left, top, left + width, top + height)
    }

    /// The box of `chars` characters at `font_px`, on `lines` lines, centred
    /// across a window `width` wide with its top at `top`: the em box of
    /// its lines, which is its line box less the leading.
    fn centred_text(width: f32, top: f32, chars: usize, lines: usize, font_px: f32) -> Rect {
        let leading = half_leading(font_px);
        let line = line_width(chars, font_px);
        Rect::new(
            (width - line) / 2.0,
            top + leading,
            (width + line) / 2.0,
            top + text_height(lines, font_px) - leading,
        )
    }

    /// The tallest wide's column grows: the longest label it can hold, the
    /// longest timer, round and result lines, and the gaps between them, at
    /// the column's width, with a pixel for each text that `bevy_ui` rounds
    /// up to whole pixels.
    fn column_height(width: f32) -> f32 {
        let lines = |text: &str, font_px: f32| wrapped_lines(text, chars_per_line(width, font_px));
        let label = longest_label_lines(chars_per_line(width, VIEWER_LABEL_SIZE));
        text_height(label, VIEWER_LABEL_SIZE)
            + text_height(lines(LONGEST_TIMER, VIEWER_TIMER_SIZE), VIEWER_TIMER_SIZE)
            + text_height(lines(LONGEST_ROUND, VIEWER_ROUND_SIZE), VIEWER_ROUND_SIZE)
            + text_height(
                lines(LONGEST_RESULT, VIEWER_RESULT_SIZE),
                VIEWER_RESULT_SIZE,
            )
            .max(VIEWER_RESULT_HEIGHT)
            + WIDE_COLUMN_GAPS.iter().sum::<f32>()
            + 4.0
    }

    /// Every box `layout` makes room for, at its largest, labelled: the row
    /// of Back, Match Settings and, in tall, the label at as many lines as
    /// the label needs; tall's three lines at their longest, or wide's
    /// column at its tallest; the board; the countdown card; and the
    /// transport row.
    fn boxes(layout: &ViewerLayout, label: TallLabelRoom) -> Vec<(String, Rect)> {
        use super::super::layout::{VIEWER_COUNTDOWN_CARD, viewer_controls_width};

        let window = layout.window;
        let label_lines = match (layout.has_column(), label) {
            (true, _) => 0,
            (false, TallLabelRoom::TwoLines) => 2,
            (false, TallLabelRoom::ThreeLines) => 3,
        };
        let row = VIEWER_CONTROL_HEIGHT.max(text_height(label_lines, VIEWER_LABEL_SIZE));
        let button_top = layout.header_top() + (row - VIEWER_CONTROL_HEIGHT) / 2.0;
        let mut boxes = vec![
            (
                "Back".to_string(),
                rect(
                    VIEWER_HEADER_INSET,
                    button_top,
                    VIEWER_BACK_WIDTH,
                    VIEWER_CONTROL_HEIGHT,
                ),
            ),
            (
                "Match Settings".into(),
                rect(
                    window.x - VIEWER_HEADER_INSET - VIEWER_MATCH_SETTINGS_WIDTH,
                    button_top,
                    VIEWER_MATCH_SETTINGS_WIDTH,
                    VIEWER_CONTROL_HEIGHT,
                ),
            ),
            ("board".into(), layout.board),
        ];
        if layout.has_column() {
            let (left, top, width) = layout.column();
            boxes.push((
                "column".into(),
                rect(left, top, width, column_height(width)),
            ));
        } else {
            let quarter = (window.x - 2.0 * VIEWER_HEADER_INSET) / 4.0;
            let height = text_height(label_lines, VIEWER_LABEL_SIZE);
            boxes.push((
                "label".into(),
                rect(
                    VIEWER_HEADER_INSET + quarter,
                    layout.header_top() + (row - height) / 2.0,
                    2.0 * quarter,
                    height,
                ),
            ));
            for (line, text, font_px) in [
                (HeaderLine::Timer, LONGEST_TIMER, VIEWER_TIMER_SIZE),
                (HeaderLine::Round, LONGEST_ROUND, VIEWER_ROUND_SIZE),
                (HeaderLine::Result, LONGEST_RESULT, VIEWER_RESULT_SIZE),
            ] {
                let top = layout.tall_line_top(line);
                boxes.push((
                    format!("{line:?}"),
                    centred_text(window.x, top, text.len(), 1, font_px),
                ));
            }
        }
        let card = VIEWER_COUNTDOWN_CARD.size;
        let card_top = VIEWER_COUNTDOWN_CARD.overlay_top(layout.board.center().y, layout.zoom());
        boxes.push((
            "countdown card".into(),
            rect((window.x - card.x) / 2.0, card_top, card.x, card.y),
        ));
        let controls = viewer_controls_width();
        boxes.push((
            "transport row".into(),
            rect(
                (window.x - controls) / 2.0,
                window.y - layout.controls_bottom() - VIEWER_CONTROL_HEIGHT,
                controls,
                VIEWER_CONTROL_HEIGHT,
            ),
        ));
        boxes
    }

    /// The score badges at their highest - where the top row's cursors put
    /// them - at either end of the row: each player's alone, and the pair
    /// that a shared point shows. Each is drawn as a box the badge's own
    /// size in window pixels, around where [`score_badge_position`] puts it.
    fn top_row_badges(layout: &ViewerLayout) -> Vec<(String, Rect)> {
        let mut badges = Vec::new();
        for x in [0, BOARD_SIZE - 1] {
            let point = viewer_board_position(Point::new(x, BOARD_SIZE - 1));
            for (shared, kind) in [(false, "alone"), (true, "paired")] {
                for player in Player::ALL {
                    let world = score_badge_position(point, player, shared, layout);
                    badges.push((
                        format!("{player:?}'s badge {kind} at {x}"),
                        Rect::from_center_size(layout.window_point(world), SCORE_BADGE_SIZE),
                    ));
                }
            }
        }
        badges
    }

    /// The heart of it: at every size, everything the screen can show at
    /// its largest stands inside the window and at least a pixel clear of
    /// everything else - for either room a label can need in tall. The
    /// board is the one thing meant to have things over it: the countdown
    /// card, and the score badges, which stand up and to the right of their
    /// cursors and so reach past its edge into the room kept around it.
    /// Two badges are held apart only where they are shown together as a
    /// pair; one cursor's badge may meet another's, as it always could when
    /// the two stand on neighbouring points. The positive half is the
    /// count: every size lays out every box.
    #[test]
    fn nothing_overlaps_and_nothing_leaves_the_window_at_any_size() {
        let meets = |a: &Rect, b: &Rect| !a.inflate(0.5).intersect(b.inflate(0.5)).is_empty();
        for label in LABEL_ROOMS {
            for size in layout_sweep() {
                let layout = viewer_layout(size, label);
                let context = format!("{size} ({:?}, {label:?})", layout.arrangement);
                let boxes = boxes(&layout, label);
                let badges = top_row_badges(&layout);
                let expected = if layout.has_column() { 6 } else { 9 };
                assert_eq!(boxes.len(), expected, "{context}: {boxes:?}");
                assert_eq!(badges.len(), 8, "{context}");
                let window = Rect::from_corners(Vec2::ZERO, size);
                for (name, rect) in boxes.iter().chain(&badges) {
                    assert!(
                        window.contains(rect.min) && window.contains(rect.max),
                        "{context}: {name} {rect:?} leaves the window"
                    );
                }
                for (index, (name, rect)) in boxes.iter().enumerate() {
                    for (other, other_rect) in &boxes[index + 1..] {
                        let names = [name.as_str(), other.as_str()];
                        if names.contains(&"board") && names.contains(&"countdown card") {
                            // Held over the board below.
                            continue;
                        }
                        assert!(
                            !meets(rect, other_rect),
                            "{context}: {name} {rect:?} meets {other} {other_rect:?}"
                        );
                    }
                    if name == "board" {
                        continue;
                    }
                    for (badge, badge_rect) in &badges {
                        assert!(
                            !meets(rect, badge_rect),
                            "{context}: {name} {rect:?} meets {badge} {badge_rect:?}"
                        );
                    }
                }
                for pair in badges.chunks(4) {
                    let (one, two) = (&pair[2], &pair[3]);
                    assert!(one.0.contains("paired") && two.0.contains("paired"));
                    assert!(
                        !meets(&one.1, &two.1),
                        "{context}: {} {:?} meets {} {:?}",
                        one.0,
                        one.1,
                        two.0,
                        two.1
                    );
                }
                let card = boxes
                    .iter()
                    .find(|(name, _)| name == "countdown card")
                    .unwrap()
                    .1;
                assert!(
                    layout.board.contains(card.min) && layout.board.contains(card.max),
                    "{context}: the card {card:?} stands over the board"
                );
            }
        }
    }

    /// Tall keeps the room the 1040x900 layout keeps between its parts at
    /// every size: 43 px from the result line to the board and 38 from the
    /// board to the transport row, within 8 of each other, so the board
    /// stands balanced between them; and the top row's score badges, which
    /// rise above the board by 32 px less 5 px a zoom, at least 15 px under
    /// the result line - 16 at the board's own size.
    #[test]
    fn tall_keeps_its_gaps_balanced_and_the_badges_clear_of_the_header() {
        let mut seen = 0;
        for label in LABEL_ROOMS {
            for size in layout_sweep() {
                let layout = viewer_layout(size, label);
                if layout.has_column() {
                    continue;
                }
                seen += 1;
                let result_bottom = layout.tall_line_top(HeaderLine::Result) + VIEWER_RESULT_HEIGHT;
                let above = layout.board.min.y - result_bottom;
                let controls_top = size.y - layout.controls_bottom() - VIEWER_CONTROL_HEIGHT;
                let below = controls_top - layout.board.max.y;
                assert!((above - 43.0).abs() < 1e-3, "{size}: {above}");
                assert!((below - 38.0).abs() < 1e-3, "{size}: {below}");
                let highest = top_row_badges(&layout)
                    .iter()
                    .map(|(_, badge)| badge.min.y)
                    .fold(f32::MAX, f32::min);
                let gap = highest - result_bottom;
                assert!(gap >= 15.0, "{size}: {gap}");
                assert!(
                    (gap - (11.0 + 5.0 * layout.zoom())).abs() < 1e-3,
                    "{size}: {gap}"
                );
            }
        }
        assert!(seen > 100, "the sweep reaches tall: {seen}");
        let opening = opening();
        let result_bottom = opening.tall_line_top(HeaderLine::Result) + VIEWER_RESULT_HEIGHT;
        let highest = opening.window_point(score_badge_position(
            viewer_board_position(Point::new(0, BOARD_SIZE - 1)),
            Player::One,
            false,
            &opening,
        ));
        assert_eq!(highest.y - SCORE_BADGE_SIZE.y / 2.0 - result_bottom, 16.0);
    }

    /// Wide's column holds its longest texts with room to spare above the
    /// transport row, at every wide size: [`column_height`] is what the
    /// overlap test lays out, and this says how much it has left, and that
    /// the label takes the most lines in the narrowest column.
    #[test]
    fn wides_column_holds_the_longest_label_at_every_size() {
        let mut least = f32::MAX;
        let mut narrowest = (f32::MAX, 0);
        for size in layout_sweep() {
            let layout = viewer_layout(size, TallLabelRoom::TwoLines);
            if !layout.has_column() {
                continue;
            }
            let (_, top, width) = layout.column();
            let controls_top = size.y - layout.controls_bottom() - VIEWER_CONTROL_HEIGHT;
            least = least.min(controls_top - (top + column_height(width)));
            if width < narrowest.0 {
                narrowest = (
                    width,
                    longest_label_lines(chars_per_line(width, VIEWER_LABEL_SIZE)),
                );
            }
        }
        assert!(least >= 1.0, "{least}");
        assert_eq!(narrowest, (220.0, 9), "the narrowest column and its lines");
        let smallest = viewer_layout(WindowSize::MINIMUM.0, TallLabelRoom::TwoLines);
        assert_eq!(smallest.column().2, 234.0, "the column at 1040x600");
    }

    /// The model of wrapping that the column's budget is reckoned with, on
    /// the screen's own longest lines, against counts worked out by hand:
    /// a break is taken only at a space before a letter or a digit.
    #[test]
    fn the_budget_wraps_the_screens_lines_where_a_line_can_break() {
        // 234 px is 15 characters at 26 px exactly, and a line that only
        // just fits is not counted on.
        assert_eq!(chars_per_line(234.0, VIEWER_LABEL_SIZE), 14);
        assert_eq!(chars_per_line(234.5, VIEWER_LABEL_SIZE), 15);
        assert_eq!(chars_per_line(220.0, VIEWER_ROUND_SIZE), 22);
        assert_eq!(chars_per_line(498.0, VIEWER_LABEL_SIZE), 31);
        assert_eq!(chars_per_line(line_width(31, 26.0), VIEWER_LABEL_SIZE), 30);
        // "Round 9999 - 60:00" and "remaining".
        assert_eq!(wrapped_lines(LONGEST_ROUND, 22), 2);
        assert_eq!(wrapped_lines(LONGEST_ROUND, 28), 1);
        // "White | Black 999 -", "White 999 | completed",
        // "2026-11-27 23:59": no break before "|" or "-" is counted.
        assert_eq!(wrapped_lines(LONGEST_RESULT, 24), 3);
        assert_eq!(wrapped_lines(LONGEST_RESULT, 58), 1);
        assert_eq!(wrapped_lines(LONGEST_TIMER, 15), 1);
        assert_eq!(wrapped_lines(LONGEST_TIMER, 14), 2);
        assert_eq!(longest_label_lines(15), 9);
        assert_eq!(longest_label_lines(31), 3);
        assert_eq!(longest_label_lines(61), 1);
    }

    /// Which labels tall gives a third line to. Every label the game writes
    /// by itself fits two lines of the narrowest header, its longest - a
    /// winner and three-digit scores - included, and so does the label of
    /// the Replay the Viewer's captures use; a label of 60 characters whose
    /// words fall badly, or that has no space to break at, or whose only
    /// spaces come before a character a line may not start with, gets a
    /// third.
    #[test]
    fn tall_gives_a_third_line_only_to_a_label_that_may_need_it() {
        use crate::replay::validate_label;

        for label in [
            "Round zero",
            "Black - 42-38 - 2024-01-01 00:00",
            "White - 999-998 - 2026-11-27 23:59",
            "Draw - 999-999 - 2026-11-27 23:59",
            "Saturday rematch with my sister best of three, second game",
        ] {
            assert!(validate_label(label).is_ok(), "{label:?}");
            assert_eq!(
                TallLabelRoom::for_label(label),
                TallLabelRoom::TwoLines,
                "{label:?}"
            );
        }
        for label in [
            "Saturday afternoon rematch against my brother, best of three",
            &format!("A{}", "b".repeat(59)),
            &format!("A{} !{}", "a".repeat(28), "b".repeat(28)),
            &format!("A{} ( {}", "a".repeat(28), "b".repeat(28)),
        ] {
            assert!(validate_label(label).is_ok(), "{label:?}");
            assert_eq!(
                TallLabelRoom::for_label(label),
                TallLabelRoom::ThreeLines,
                "{label:?}"
            );
        }
        assert!(validate_label(&format!("A{}", "b".repeat(60))).is_err());
        assert_eq!(LABEL_MAX_CHARS, 60);
    }

    /// The Key is set in one column wherever the window holds the panel
    /// with 12 px above and below it - from 722 px tall, pinned on both
    /// sides - and otherwise in two; either way it stands inside the window
    /// with that edge around it, at every size.
    #[test]
    fn the_key_takes_two_columns_only_in_a_window_too_short_for_one() {
        assert_eq!(layout(1040.0, 721.0).key, KeyColumns::Two);
        assert_eq!(layout(1040.0, 722.0).key, KeyColumns::One);
        assert_eq!(layout(2560.0, 721.0).key, KeyColumns::Two);
        assert_eq!(KEY_ONE_COLUMN_HEIGHT + 24.0, 722.0);
        assert_eq!(KEY_TWO_COLUMN_WIDTH, 1016.0);
        let mut both = [false, false];
        for size in layout_sweep() {
            let layout = viewer_layout(size, TallLabelRoom::TwoLines);
            let height = match layout.key {
                KeyColumns::One => KEY_ONE_COLUMN_HEIGHT,
                KeyColumns::Two => KEY_TWO_COLUMN_HEIGHT,
            };
            let panel = Rect::from_center_size(size / 2.0, Vec2::new(layout.key_width(), height));
            let inside = Rect::from_corners(Vec2::splat(12.0), size - Vec2::splat(12.0));
            assert!(
                inside.contains(panel.min) && inside.contains(panel.max),
                "{size}: {panel:?}"
            );
            both[usize::from(layout.key == KeyColumns::Two)] = true;
        }
        assert_eq!(both, [true, true]);
    }

    fn layout(width: f32, height: f32) -> ViewerLayout {
        viewer_layout(Vec2::new(width, height), TallLabelRoom::TwoLines)
    }

    /// A badge is drawn at its own size whatever the board's zoom: its scale
    /// undoes the zoom exactly, across the screen, and leaves depth alone.
    #[test]
    fn a_badge_is_drawn_at_its_own_size_at_every_zoom() {
        for size in layout_sweep() {
            let layout = viewer_layout(size, TallLabelRoom::TwoLines);
            let scale = score_badge_scale(&layout);
            assert!((scale.x * layout.zoom() - 1.0).abs() < 1e-6, "{size}");
            assert_eq!(scale.x, scale.y);
            assert_eq!(scale.z, 1.0);
        }
    }

    /// At every named size and every point of the board, the badges as
    /// drawn, in window pixels: up and to the right of their cursor, 4 px
    /// off its drawn corner; a pair on a shared point side by side, 5 px
    /// apart, Player One's the left one; and every badge 4 px inside the
    /// window, a pair at the window's edge shifted together and never
    /// swapped.
    #[test]
    fn the_badges_keep_their_gaps_and_order_in_window_pixels_at_every_size() {
        for size in NAMED_WINDOW_SIZES {
            let layout = viewer_layout(size, TallLabelRoom::TwoLines);
            let drawn = |world: Vec2| layout.window_point(world);
            for x in 0..BOARD_SIZE {
                for y in 0..BOARD_SIZE {
                    let point = viewer_board_position(Point::new(x, y));
                    let cursor = drawn(point);
                    for player in Player::ALL {
                        let badge = drawn(score_badge_position(point, player, false, &layout));
                        let half_cursor = cursor_visual_style(player).size * layout.zoom() / 2.0;
                        let corner = cursor + Vec2::new(half_cursor, -half_cursor);
                        let expected = corner + Vec2::new(4.0 + 21.0, -(4.0 + 14.0));
                        assert!(
                            (badge - expected).abs().max_element() < 1e-3,
                            "{size} ({x}, {y}) {player:?}: {badge} for {expected}"
                        );
                    }
                    let one = drawn(score_badge_position(point, Player::One, true, &layout));
                    let two = drawn(score_badge_position(point, Player::Two, true, &layout));
                    assert!((two.x - one.x - 47.0).abs() < 1e-3, "{size} ({x}, {y})");
                    assert!((two.y - one.y).abs() < 1e-3, "{size} ({x}, {y})");
                }
            }
            // At the window's edge the pair shifts together and the one
            // nearest the edge keeps 4 px from it.
            let far = layout.world_point(Vec2::new(size.x + 500.0, size.y / 2.0));
            let one = drawn(score_badge_position(far, Player::One, true, &layout));
            let two = drawn(score_badge_position(far, Player::Two, true, &layout));
            assert!((two.x + 21.0 - (size.x - 4.0)).abs() < 1e-3, "{size}");
            assert!((two.x - one.x - 47.0).abs() < 1e-3, "{size}");
            let near = layout.world_point(Vec2::new(-500.0, -500.0));
            let lone = drawn(score_badge_position(near, Player::One, false, &layout));
            assert!(
                (lone - Vec2::new(25.0, 18.0)).abs().max_element() < 1e-3,
                "{size}"
            );
        }
    }
}
