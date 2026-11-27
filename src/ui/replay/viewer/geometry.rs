//! Where everything sits in the viewer's fixed window.
//!
//! The viewer keeps its own, tighter grid constants rather than reusing the
//! live board's, because it must also fit a header, transport controls and the
//! key and Match Settings modals in the same fixed window. Those measurements
//! live here, rather than beside the code that spawns the furniture, because
//! spawning is not their only reader: [`super::layout`] places entities with
//! them once and [`super::display`] moves the same entities with them whenever
//! playback crosses a recorded event boundary.
//!
//! Nothing here touches the ECS. Every item is a constant or a pure
//! calculation over a [`Point`], a player or a recorded frame, which is what
//! lets the tests assert that the whole vertical layout balances and fits
//! inside the window, and that the board's marks fit their cell and one
//! another, without building an app.
//!
//! Two rules here are more than arithmetic. [`score_badge_position`]: badges
//! follow each cursor, clamp to the viewport and separate when the cursors
//! overlap, so both stay legible - Player One's always on the left, since
//! that order is what tells the two apart there, and the Key says so. And
//! [`viewer_selection_position`]: two hidden selections of one point are
//! drawn side by side, not one over the other. [`viewer_marker_layout`]
//! says what decided the sizes of the two Replay-only marks.

use bevy::prelude::*;

use super::player_index;
use crate::app::visuals::{WINDOW_HEIGHT, WINDOW_WIDTH, cursor_visual_style};
use crate::replay::ReplayFrame;
use crate::rules::{BOARD_SIZE, Player, Point};

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
const SCORE_BADGE_EDGE_INSET: f32 = 4.0;
const SCORE_BADGE_OVERLAP_GAP: f32 = 5.0;
pub(super) const VIEWER_GRID_SPACING: f32 = 67.0;
pub(super) const VIEWER_GRID_EXTENT: f32 = VIEWER_GRID_SPACING * (BOARD_SIZE as f32 - 1.0);
pub(super) const VIEWER_BOARD_OUTER_SIZE: f32 = VIEWER_GRID_EXTENT + 78.0;
pub(super) const VIEWER_BOARD_CENTER_Y: f32 = -45.0;
pub(super) const VIEWER_HEADER_TOP: f32 = 12.0;
pub(super) const VIEWER_TIMER_TOP: f32 = 80.0;
pub(super) const VIEWER_ROUND_TOP: f32 = 104.0;
pub(super) const VIEWER_RESULT_TOP: f32 = 128.0;
pub(super) const VIEWER_RESULT_HEIGHT: f32 = 17.0;
pub(super) const VIEWER_CONTROL_BOTTOM: f32 = 22.0;
pub(super) const VIEWER_CONTROL_HEIGHT: f32 = 38.0;
pub(super) const VIEWER_MATCH_SETTINGS_MODAL_WIDTH: f32 = 620.0;
pub(super) const VIEWER_MATCH_SETTINGS_MODAL_MAX_HEIGHT: f32 = 760.0;
pub(super) const VIEWER_HEADER_BADGE_MIN_GAP: f32 = 16.0;
pub(super) const VIEWER_HEADER_BOARD_MAX_GAP: f32 = 48.0;
pub(super) const VIEWER_BOARD_CONTROL_MIN_GAP: f32 = 32.0;
pub(super) const VIEWER_VERTICAL_BALANCE_TOLERANCE: f32 = 8.0;

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

pub(super) fn viewer_header_board_gap() -> f32 {
    viewer_board_top_from_window() - viewer_result_bottom()
}

pub(super) fn viewer_header_badge_gap() -> f32 {
    let top_row_cursor = viewer_board_position(Point::new(0, BOARD_SIZE - 1));
    let top_badge = score_badge_position(top_row_cursor, Player::Two, true);
    let badge_top_from_window =
        WINDOW_HEIGHT as f32 * 0.5 - (top_badge.y + SCORE_BADGE_SIZE.y * 0.5);
    badge_top_from_window - viewer_result_bottom()
}

pub(super) fn viewer_board_controls_gap() -> f32 {
    viewer_controls_top_from_window() - viewer_board_bottom_from_window()
}

const fn viewer_result_bottom() -> f32 {
    VIEWER_RESULT_TOP + VIEWER_RESULT_HEIGHT
}

/// How far down the window the board's centre is drawn, in logical pixels:
/// what the Round 0 countdown card is placed over.
pub(super) const fn viewer_board_center_from_window() -> f32 {
    WINDOW_HEIGHT as f32 * 0.5 - VIEWER_BOARD_CENTER_Y
}

const fn viewer_board_top_from_window() -> f32 {
    WINDOW_HEIGHT as f32 * 0.5 - (VIEWER_BOARD_CENTER_Y + VIEWER_BOARD_OUTER_SIZE * 0.5)
}

const fn viewer_board_bottom_from_window() -> f32 {
    WINDOW_HEIGHT as f32 * 0.5 - (VIEWER_BOARD_CENTER_Y - VIEWER_BOARD_OUTER_SIZE * 0.5)
}

const fn viewer_controls_top_from_window() -> f32 {
    WINDOW_HEIGHT as f32 - VIEWER_CONTROL_BOTTOM - VIEWER_CONTROL_HEIGHT
}

/// Where `player`'s score badge sits: up and to the right of their cursor,
/// inside the window.
///
/// When both cursors share a point the two badges sit side by side, and
/// they are identical but for a frame in each player's accent - 1.54:1
/// apart, one colour to a player who cannot see hue. Their order is then
/// the channel that says whose is whose, so it never changes: Player One's
/// is the left one, the order the Viewer's result line names the players
/// in (`Black 3 - White 5`), at the edge of the window as anywhere else.
/// That was the owner's choice on 2026-10-03; `src/contrast.rs` cites it.
///
/// It is not the order the game places players in - the Match screen puts
/// Black / Player 1 on the right - and it was weighed against that on
/// 2026-10-06 and kept. The badges are a score readout, and every score
/// readout in the game that puts two numbers in a row names Black first:
/// the result line, the Replay Library's summary and a recorded Replay's
/// default name. (The Match screen's end scores are not a row: each stands
/// beside its own player's name, in that player's corner.) Read left to right, the pair agrees with the line
/// above the board that gives the same two numbers. What the order lacked
/// was being said, and the Key now says it, in a sentence and a swatch that
/// `the_keys_badge_row_names_the_order_the_board_draws` in
/// [`super::modals`] holds to this function, so the two cannot drift.
pub(super) fn score_badge_position(cursor: Vec2, player: Player, cursors_overlap: bool) -> Vec2 {
    let half_viewport = Vec2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32) * 0.5;
    let half_badge = SCORE_BADGE_SIZE * 0.5;
    let minimum = -half_viewport + half_badge + Vec2::splat(SCORE_BADGE_EDGE_INSET);
    let maximum = half_viewport - half_badge - Vec2::splat(SCORE_BADGE_EDGE_INSET);
    let position = if cursors_overlap {
        let cursor_size = cursor_visual_style(Player::One)
            .size
            .max(cursor_visual_style(Player::Two).size);
        let first_x = cursor.x + cursor_size * 0.5 + half_badge.x + 4.0;
        let second_x = first_x + SCORE_BADGE_SIZE.x + SCORE_BADGE_OVERLAP_GAP;
        let pair_left = first_x - half_badge.x;
        let pair_right = second_x + half_badge.x;
        let horizontal_shift = if pair_right > half_viewport.x - SCORE_BADGE_EDGE_INSET {
            half_viewport.x - SCORE_BADGE_EDGE_INSET - pair_right
        } else if pair_left < -half_viewport.x + SCORE_BADGE_EDGE_INSET {
            -half_viewport.x + SCORE_BADGE_EDGE_INSET - pair_left
        } else {
            0.0
        };
        Vec2::new(
            match player {
                Player::One => first_x,
                Player::Two => second_x,
            } + horizontal_shift,
            cursor.y + cursor_size * 0.5 + half_badge.y + 4.0,
        )
    } else {
        let cursor_size = cursor_visual_style(player).size;
        cursor
            + Vec2::new(
                cursor_size * 0.5 + half_badge.x + 4.0,
                cursor_size * 0.5 + half_badge.y + 4.0,
            )
    };
    position.clamp(minimum, maximum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{
        CursorVisualStyle, OUTLINE_ARM, STAR_POINT_RADIUS, STONE_RADIUS, stone_geometry,
    };
    use crate::rules::Cell;

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

    #[test]
    fn replay_score_badges_follow_cursors_above_their_top_right_corner() {
        let mut frame = ReplayFrame {
            cursors: [Point::new(4, 4), Point::new(1, 1)],
            ..default()
        };
        let cursor = viewer_cursor_position(&frame, Player::One);
        let badge = score_badge_position(cursor, Player::One, false);
        let cursor_style = cursor_visual_style(Player::One);

        assert!(badge.x - SCORE_BADGE_SIZE.x * 0.5 > cursor.x + cursor_style.size * 0.5);
        assert!(badge.y - SCORE_BADGE_SIZE.y * 0.5 > cursor.y + cursor_style.size * 0.5);

        frame.cursors[0] = Point::new(5, 4);
        let moved_cursor = viewer_cursor_position(&frame, Player::One);
        let moved_badge = score_badge_position(moved_cursor, Player::One, false);
        assert_eq!(moved_badge - badge, moved_cursor - cursor);
        assert_eq!(moved_cursor.x - cursor.x, VIEWER_GRID_SPACING);
    }

    /// Side by side, Player One's badge is the left one wherever the shared
    /// point is - the order is what tells the two badges apart, so the
    /// clamp at the window's edge shifts the pair without swapping it.
    #[test]
    fn replay_score_badges_clamp_to_viewport_and_separate_overlap() {
        let viewport_max = Vec2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32) * 0.5
            - SCORE_BADGE_SIZE * 0.5
            - Vec2::splat(SCORE_BADGE_EDGE_INSET);
        let clamped = score_badge_position(Vec2::new(10_000.0, 10_000.0), Player::One, false);
        assert_eq!(clamped, viewport_max);

        let cursor = Vec2::ZERO;
        let black = score_badge_position(cursor, Player::One, true);
        let white = score_badge_position(cursor, Player::Two, true);
        assert!(
            white.x - SCORE_BADGE_SIZE.x * 0.5
                >= black.x + SCORE_BADGE_SIZE.x * 0.5 + SCORE_BADGE_OVERLAP_GAP
        );
        assert_eq!(black.y, white.y);
        let cursor_top = cursor_visual_style(Player::One)
            .size
            .max(cursor_visual_style(Player::Two).size)
            * 0.5;
        assert!(black.y - SCORE_BADGE_SIZE.y * 0.5 > cursor_top);

        let edge_black = score_badge_position(Vec2::new(10_000.0, 0.0), Player::One, true);
        let edge_white = score_badge_position(Vec2::new(10_000.0, 0.0), Player::Two, true);
        assert_eq!(edge_white.x, viewport_max.x);
        assert!(
            edge_white.x - SCORE_BADGE_SIZE.x * 0.5
                >= edge_black.x + SCORE_BADGE_SIZE.x * 0.5 + SCORE_BADGE_OVERLAP_GAP
        );

        // And on every point both cursors can really share.
        for x in 0..BOARD_SIZE {
            for y in 0..BOARD_SIZE {
                let cursor = viewer_board_position(Point::new(x, y));
                let black = score_badge_position(cursor, Player::One, true);
                let white = score_badge_position(cursor, Player::Two, true);
                assert!(
                    white.x - SCORE_BADGE_SIZE.x * 0.5
                        >= black.x + SCORE_BADGE_SIZE.x * 0.5 + SCORE_BADGE_OVERLAP_GAP,
                    "at ({x}, {y}) Player One's badge is not the left one"
                );
                assert_eq!(black.y, white.y, "at ({x}, {y})");
            }
        }
    }

    #[test]
    fn replay_viewer_vertical_layout_is_compact_safe_balanced_and_fits() {
        let header_board_gap = viewer_header_board_gap();
        let board_controls_gap = viewer_board_controls_gap();

        assert!(header_board_gap <= VIEWER_HEADER_BOARD_MAX_GAP);
        assert!(viewer_header_badge_gap() >= VIEWER_HEADER_BADGE_MIN_GAP);
        assert!(board_controls_gap >= VIEWER_BOARD_CONTROL_MIN_GAP);
        assert!((header_board_gap - board_controls_gap).abs() <= VIEWER_VERTICAL_BALANCE_TOLERANCE);
        const { assert!(VIEWER_HEADER_TOP >= 0.0) };
        assert!(viewer_result_bottom() < viewer_board_top_from_window());
        assert!(viewer_board_bottom_from_window() < viewer_controls_top_from_window());
        assert!(VIEWER_CONTROL_BOTTOM + VIEWER_CONTROL_HEIGHT <= WINDOW_HEIGHT as f32);
        assert!(VIEWER_MATCH_SETTINGS_MODAL_WIDTH < WINDOW_WIDTH as f32);
        assert!(VIEWER_MATCH_SETTINGS_MODAL_MAX_HEIGHT < WINDOW_HEIGHT as f32);
    }
}
