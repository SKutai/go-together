//! Shared visual vocabulary: window metrics, live-board geometry and the
//! stone mesh/material handles every screen draws with.
//!
//! # Responsibility
//!
//! Anything that more than one screen needs in order to *look* consistent lives
//! here, so that [`crate::ui`] modules stay focused on layout and behaviour.
//! Colours themselves live in [`crate::presentation`]; this module is about
//! sizes, positions and the GPU handles built once at startup.
//!
//! # Key items
//!
//! * [`StonePalette`] and [`StoneMesh`] are created once by [`setup_app`] (a
//!   `Startup` system) and then cloned per intersection, so the whole board
//!   shares a handful of materials and a single circle mesh.
//! * [`stone_geometry`] is the single answer to how large a cell's disc is
//!   drawn and whether it carries a ring. The live board, the Replay
//!   Viewer's board, the Replay Library's thumbnails and the Viewer's Key
//!   all read it, so a stone cannot be ringed in one place and bare in
//!   another. White and gray stones carry the ring; it is what marks their
//!   edge against the board, which gray's fill alone cannot do and white's
//!   does only barely.
//! * [`board_position`] is the single source of truth for live-board
//!   coordinates. The replay viewer intentionally keeps its own, smaller grid
//!   constants so the two layouts can drift independently.
//! * [`STAR_POINTS`] is the one exception to that split: both boards draw
//!   the same five decorative star points, each spawner placing this shared
//!   table through its own geometry.
//! * [`cursor_visual_style`] and [`cursor_outline_layers`] describe the
//!   two-layer player cursor: a thick contrast outline underneath and a
//!   thinner player-accent outline on top, both drawn at the same size as
//!   corner brackets by [`spawn_outline_segments`].
//!   Player One's cursor is deliberately larger than Player Two's so the two
//!   remain distinguishable when they overlap on the same intersection. The
//!   Replay Viewer's Teleport Marker takes the cursor's two colours as its
//!   outer two strokes and nothing else: it is a closed outline with a
//!   spawner of its own there, because these brackets, at Player Two's
//!   marker's size, read as an X.
//!
//! # Extending
//!
//! Add new shared geometry or handles here; add screen-specific layout to the
//! owning `ui` module instead.

use bevy::prelude::*;

use crate::presentation::{
    BLACK_STONE, BOARD_GRID, EMPTY_INTERSECTION, GRAY_STONE, STONE_RING, WHITE_STONE, player_accent,
};
use crate::rules::{BOARD_SIZE, Cell, Player, Point};

pub(crate) const WINDOW_WIDTH: u32 = 1040;
pub(crate) const WINDOW_HEIGHT: u32 = 900;
pub(crate) const GRID_SPACING: f32 = 72.0;
pub(crate) const GRID_EXTENT: f32 = GRID_SPACING * (BOARD_SIZE as f32 - 1.0);
pub(crate) const BOARD_CENTER_Y: f32 = -22.0;
pub(crate) const STONE_RADIUS: f32 = 28.0;

/// Scale of the small dot that marks an empty intersection, against
/// [`STONE_RADIUS`].
const EMPTY_INTERSECTION_SCALE: f32 = 0.14;

/// Scale of a gray stone's whole footprint, ring included, against a black
/// or white stone's. Gray is drawn smaller on purpose: size is a second,
/// non-colour channel for the one stone that belongs to neither player.
const GRAY_STONE_SCALE: f32 = 0.92;

/// Width, in pixels, of the ink ring around a white or gray stone - the
/// grid lines' own weight, so the ring reads as the lattice closing around
/// the stone rather than as a new kind of mark.
pub(crate) const STONE_RING_WIDTH: f32 = 2.5;

/// Radius of one decorative star point, in pixels. Large enough to read
/// around the smaller empty-intersection dot drawn above it, and well below
/// stone size, so it stays board furniture.
pub(crate) const STAR_POINT_RADIUS: f32 = 8.0;

/// The five traditional 9x9 star points (hoshi): the centre intersection and
/// the four intersections three lines in from each corner. Purely decorative
/// landmarks - both board spawners draw them once as inert furniture, no
/// rule reads them, and a `BOARD_SIZE` change must update this table along
/// with the live and Replay geometry.
pub(crate) const STAR_POINTS: [Point; 5] = [
    Point::new(2, 2),
    Point::new(6, 2),
    Point::new(4, 4),
    Point::new(2, 6),
    Point::new(6, 6),
];

#[derive(Resource)]
/// Materials shared by every intersection on the live board and the Replay
/// Viewer's board, plus the one their shared star-point furniture is drawn
/// with.
///
/// Built once at startup and cloned per entity, so the board costs a handful
/// of materials rather than one per intersection. It holds neither the
/// meshes nor the materials of the mark a hidden selection is revealed with,
/// on purpose. Only the Replay Viewer draws that mark, once the Match is over
/// and nothing is secret any more, and it makes them itself on entry
/// (`spawn_viewer_selections` in `ui::replay::viewer`), so the mark is not
/// lying in a shared resource for the live board to pick up. That is a
/// convenience removed, not a wall: a stone's own material could still be
/// misused to draw one. What holds the rule is a test in `ui::match_screen`
/// that the live board draws nothing but its cells while both players hold
/// hidden selections.
pub(crate) struct StonePalette {
    pub(crate) empty: Handle<ColorMaterial>,
    pub(crate) black: Handle<ColorMaterial>,
    pub(crate) white: Handle<ColorMaterial>,
    pub(crate) gray: Handle<ColorMaterial>,
    /// Grid-line ink for the five decorative star points; see
    /// [`STAR_POINTS`].
    pub(crate) star: Handle<ColorMaterial>,
    /// The ring behind a white or gray stone; see [`stone_geometry`].
    pub(crate) ring: Handle<ColorMaterial>,
}

impl StonePalette {
    /// The material a cell's own disc is drawn with.
    pub(crate) fn fill(&self, cell: Cell) -> &Handle<ColorMaterial> {
        match cell {
            Cell::Empty => &self.empty,
            Cell::Black => &self.black,
            Cell::White => &self.white,
            Cell::Gray => &self.gray,
        }
    }
}

#[derive(Resource)]
/// The single circle mesh every stone and intersection is drawn with, scaled
/// per cell rather than re-meshed.
pub(crate) struct StoneMesh(pub(crate) Handle<Mesh>);

/// The two scales one board cell is drawn at, both against [`STONE_RADIUS`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StoneGeometry {
    /// Scale of the cell's own disc: the stone's fill, or the empty dot.
    pub(crate) fill_scale: f32,
    /// Scale of the ring disc drawn behind the fill, for the stones that
    /// carry one.
    pub(crate) ring_scale: Option<f32>,
}

/// How large a cell's fill and ring are drawn.
///
/// White and gray stones carry a ring in [`STONE_RING`]; black stones and
/// empty intersections do not. A black stone's fill is already far from the
/// board's wood. Gray's is close to it, unavoidably - no gray can stand
/// clear of black, white and the board at once - and white's clears it by
/// a tenth of a ratio point, on a board darkened for the purpose. So the
/// ring is what draws their edge.
///
/// The ring is drawn *inside* the stone's footprint - the ring disc takes
/// the scale the stone always had and the fill gives up
/// [`STONE_RING_WIDTH`] to it - so no stone grows: neighbours keep their
/// gap on both boards, which matters most on the Replay Viewer's tighter
/// grid; a white stone stays the size of a black one; and gray keeps its
/// smaller footprint.
pub(crate) fn stone_geometry(cell: Cell) -> StoneGeometry {
    let ring = STONE_RING_WIDTH / STONE_RADIUS;
    match cell {
        Cell::Empty => StoneGeometry {
            fill_scale: EMPTY_INTERSECTION_SCALE,
            ring_scale: None,
        },
        Cell::Black => StoneGeometry {
            fill_scale: 1.0,
            ring_scale: None,
        },
        Cell::White => StoneGeometry {
            fill_scale: 1.0 - ring,
            ring_scale: Some(1.0),
        },
        Cell::Gray => StoneGeometry {
            fill_scale: GRAY_STONE_SCALE - ring,
            ring_scale: Some(GRAY_STONE_SCALE),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// How one player's cursor is drawn.
///
/// The two players get different sizes on purpose: when both cursors sit on
/// the same intersection the smaller one stays visible inside the larger.
pub(crate) struct CursorVisualStyle {
    pub(crate) color: Color,
    pub(crate) border_color: Color,
    pub(crate) size: f32,
    pub(crate) thickness: f32,
    pub(crate) border_thickness: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorOutlineLayer {
    pub(crate) color: Color,
    pub(crate) thickness: f32,
    pub(crate) z: f32,
}

pub(crate) fn cursor_visual_style(player: Player) -> CursorVisualStyle {
    match player {
        Player::One => CursorVisualStyle {
            color: player_accent(player),
            border_color: BLACK_STONE,
            size: 68.0,
            thickness: 6.0,
            border_thickness: 10.0,
        },
        Player::Two => CursorVisualStyle {
            color: player_accent(player),
            border_color: WHITE_STONE,
            size: 52.0,
            thickness: 6.0,
            border_thickness: 10.0,
        },
    }
}

/// The two layers a cursor is drawn from: a thick contrast outline underneath
/// and the thinner player accent on top, so the cursor reads against both the
/// wooden board and a placed stone.
pub(crate) const fn cursor_outline_layers(style: CursorVisualStyle) -> [CursorOutlineLayer; 2] {
    [
        CursorOutlineLayer {
            color: style.border_color,
            thickness: style.border_thickness,
            z: 0.0,
        },
        CursorOutlineLayer {
            color: style.color,
            thickness: style.thickness,
            z: 0.1,
        },
    ]
}

/// Creates the camera and the shared stone mesh and materials.
pub(super) fn setup_app(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn(Camera2d);

    let palette = StonePalette {
        empty: materials.add(EMPTY_INTERSECTION),
        black: materials.add(BLACK_STONE),
        white: materials.add(WHITE_STONE),
        gray: materials.add(GRAY_STONE),
        star: materials.add(BOARD_GRID),
        ring: materials.add(STONE_RING),
    };
    commands.insert_resource(palette);
    commands.insert_resource(StoneMesh(meshes.add(Circle::new(STONE_RADIUS))));
}

/// Length of each arm of an outline's corner brackets, as a share of a side.
/// Two arms meet at each corner, so the middle 16% of every side is open.
pub(crate) const OUTLINE_ARM: f32 = 0.42;

/// Spawns one outline layer of a cursor as eight sprites: a horizontal and
/// a vertical arm at each corner of a `size`-by-`size` square, each arm
/// [`OUTLINE_ARM`] of a side long and `thickness` wide, so the four corner
/// brackets leave the middle of every side open and the stone underneath
/// stays visible. The arms are centred on the square's sides, so the
/// outline is `size + thickness` across and each outer corner is notched by
/// half a `thickness`.
pub(crate) fn spawn_outline_segments(
    parent: &mut ChildSpawnerCommands,
    color: Color,
    size: f32,
    thickness: f32,
    z: f32,
) {
    let edge_length = size * OUTLINE_ARM;
    for x_sign in [-1.0, 1.0] {
        for y_sign in [-1.0, 1.0] {
            parent.spawn((
                Sprite::from_color(color, Vec2::new(edge_length, thickness)),
                Transform::from_xyz(x_sign * (size - edge_length) / 2.0, y_sign * size / 2.0, z),
            ));
            parent.spawn((
                Sprite::from_color(color, Vec2::new(thickness, edge_length)),
                Transform::from_xyz(x_sign * size / 2.0, y_sign * (size - edge_length) / 2.0, z),
            ));
        }
    }
}

/// World position of a live-board intersection. The single source of truth
/// for stones, cursors and anything else placed on the board.
pub(crate) fn board_position(point: Point) -> Vec2 {
    Vec2::new(
        point.x as f32 * GRID_SPACING - GRID_EXTENT / 2.0,
        BOARD_CENTER_Y + point.y as f32 * GRID_SPACING - GRID_EXTENT / 2.0,
    )
}

/// Formats a duration as `MM:SS`, shared by the match timer, the Replay
/// Library's card durations and the Replay Viewer so all three read the same
/// way. Minutes are not wrapped at 60.
pub(crate) fn format_elapsed_time(elapsed_seconds: f64) -> String {
    let total_seconds = elapsed_seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

/// A [`StonePalette`] built from placeholder materials, for tests that need
/// the resource to exist but never inspect which handle is which.
#[cfg(test)]
pub(crate) fn test_stone_palette(materials: &mut Assets<ColorMaterial>) -> StonePalette {
    StonePalette {
        empty: materials.add(ColorMaterial::default()),
        black: materials.add(ColorMaterial::default()),
        white: materials.add(ColorMaterial::default()),
        gray: materials.add(ColorMaterial::default()),
        star: materials.add(ColorMaterial::default()),
        ring: materials.add(ColorMaterial::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_and_gray_stones_carry_a_ring_inside_their_old_footprint() {
        let ring = STONE_RING_WIDTH / STONE_RADIUS;
        assert_eq!(stone_geometry(Cell::Empty).ring_scale, None);
        assert_eq!(stone_geometry(Cell::Black).ring_scale, None);
        assert_eq!(stone_geometry(Cell::Black).fill_scale, 1.0);

        // The ring disc takes the scale the stone had before it carried a
        // ring, and the fill gives up exactly the ring's width to it.
        let white = stone_geometry(Cell::White);
        assert_eq!(white.ring_scale, Some(1.0));
        assert_eq!(white.fill_scale, 1.0 - ring);
        let gray = stone_geometry(Cell::Gray);
        assert_eq!(gray.ring_scale, Some(0.92));
        assert_eq!(gray.fill_scale, 0.92 - ring);

        // Size stays a channel of its own: gray's whole footprint is smaller
        // than a black or white stone's, and its fill smaller than white's.
        assert!(gray.ring_scale < white.ring_scale);
        assert!(gray.fill_scale < white.fill_scale);
    }

    #[test]
    fn no_stone_outgrows_the_footprint_the_two_boards_were_laid_out_for() {
        // Both boards space their intersections for a stone of STONE_RADIUS;
        // the Replay Viewer's tighter grid leaves the smaller gap. A ring
        // that widened a stone would spend that gap, so none may.
        for cell in [Cell::Empty, Cell::Black, Cell::White, Cell::Gray] {
            let geometry = stone_geometry(cell);
            let footprint = geometry.ring_scale.unwrap_or(geometry.fill_scale);
            assert!(footprint <= 1.0, "{cell:?} is drawn at {footprint}");
            assert!(geometry.fill_scale > 0.0);
        }
    }

    #[test]
    fn elapsed_time_format_supports_long_matches() {
        assert_eq!(format_elapsed_time(0.0), "00:00");
        assert_eq!(format_elapsed_time(59.999), "00:59");
        assert_eq!(format_elapsed_time(60.0), "01:00");
        assert_eq!(format_elapsed_time(3_600.0), "60:00");
        assert_eq!(format_elapsed_time(7_507.9), "125:07");
    }
}
