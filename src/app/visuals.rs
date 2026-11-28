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
//! # The window and the main camera
//!
//! A screen lays itself out from one input, [`WindowSize`]: the primary
//! window's size in logical pixels, which [`track_window_size`] keeps equal
//! to the window. A layout is a pure function of that size, computed for
//! [`WindowSize::layout`], which is the window's own size but never less
//! than [`MIN_WINDOW_WIDTH`] by [`MIN_WINDOW_HEIGHT`]. [`WINDOW_WIDTH`] and
//! [`WINDOW_HEIGHT`] are only the size the window opens at, and the window
//! keeps it: it is not resizable. The Match screen and Home are laid out for
//! any size; the Replay screens, Settings and the Help modal are still laid
//! out for the opening size.
//!
//! Text and controls are `bevy_ui` nodes, which no camera transform touches,
//! so they keep their size whatever the window does. What grows and shrinks
//! is the world-space scene - the Match board with everything drawn on it,
//! and Home's diamond - and it is placed and scaled by the one camera that
//! draws it, [`MainCamera`], rather than by moving the scene: a screen
//! writes [`WorldFraming`], the zoom and the world point at the window's
//! centre, and [`frame_main_camera`] gives the camera that projection scale
//! and translation. So the boards' own geometry - [`board_position`],
//! [`GRID_SPACING`] and the rest - is the same at every size, and every
//! mark on a board keeps its proportions to the cell. A screen with no
//! world-space scene to place writes nothing - so far only the Match and
//! Home have one - and the camera stays at a scale of 1 over the origin,
//! as it always was: [`reset_world_framing_on_screen_change`] puts the
//! framing back on every change of screen, so no screen inherits another's
//! zoom.
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
use bevy::state::state::StateTransitionEvent;
use bevy::window::PrimaryWindow;

use super::AppScreen;
use crate::presentation::{
    BLACK_STONE, BOARD_GRID, EMPTY_INTERSECTION, GRAY_STONE, STONE_RING, WHITE_STONE, player_accent,
};
use crate::rules::{BOARD_SIZE, Cell, Player, Point};

/// The window's size when the game opens, in logical pixels. An adapted
/// layout does not read these: it reads [`WindowSize`], which starts at
/// this size. Of the screens not yet adapted, the Replay Viewer still
/// places itself from them; Settings, the Replay Library and the Help
/// modal are laid out for the opening size with fixed sizes of their own.
pub(crate) const WINDOW_WIDTH: u32 = 1040;
pub(crate) const WINDOW_HEIGHT: u32 = 900;
/// The smallest window the layouts are made for, in logical pixels. A
/// layout handed a smaller size lays itself out at this one; see
/// [`WindowSize::layout`].
///
/// The height is meant to sit below what a maximised window keeps on the
/// smallest common laptop screens - a 1920x1080 screen at Windows' 150%,
/// which is 1280x720 logical pixels, and a 1366x768 one - once Windows'
/// default taskbar and the window's title bar are taken off, with room to
/// spare for a taller taskbar or a Linux desktop's top bar. That was
/// reasoned from those bars' usual sizes, not measured on either screen.
/// The width is the opening width, which the Settings binding columns
/// need without making text smaller.
pub(crate) const MIN_WINDOW_WIDTH: u32 = 1040;
pub(crate) const MIN_WINDOW_HEIGHT: u32 = 600;
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

/// Registers the window's size as the layouts' input and the main camera's
/// framing: [`WindowSize`] and [`track_window_size`], and [`WorldFraming`]
/// with the two systems that apply it to [`MainCamera`] and put it back on
/// a change of screen. Needs the [`AppScreen`] state, which
/// [`super::AppPlugin`] initialises first.
pub(crate) struct WindowLayoutPlugin;

impl Plugin for WindowLayoutPlugin {
    fn build(&self, app: &mut App) {
        use bevy::camera::CameraUpdateSystems;
        use bevy::state::state::StateTransitionSystems;
        use bevy::transform::TransformSystems;

        app.init_resource::<WindowSize>()
            .init_resource::<WorldFraming>()
            .add_systems(PreUpdate, track_window_size)
            .add_systems(
                StateTransition,
                reset_world_framing_on_screen_change
                    .after(StateTransitionSystems::DependentTransitions)
                    .before(StateTransitionSystems::ExitSchedules),
            )
            .add_systems(
                PostUpdate,
                frame_main_camera
                    .before(CameraUpdateSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// The primary window's size in logical pixels: the one input a screen's
/// layout is computed from.
///
/// [`track_window_size`] keeps it equal to the window, writing only when
/// the size really changes, so a layout pass can run on
/// `resource_changed::<WindowSize>` or compare its own output. It starts at
/// the opening size, which is also what it holds in an app with no window.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowSize(pub(crate) Vec2);

impl WindowSize {
    /// The size the window opens at.
    pub(crate) const OPENING: Self = Self(Vec2::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32));
    /// The smallest size the layouts are made for.
    pub(crate) const MINIMUM: Self =
        Self(Vec2::new(MIN_WINDOW_WIDTH as f32, MIN_WINDOW_HEIGHT as f32));

    /// The size a layout is computed for: the window's own, raised to
    /// [`Self::MINIMUM`] in each direction it falls short in. Only a
    /// browser window or a screen smaller than the minimum can be smaller,
    /// and there the layout is the minimum's.
    pub(crate) fn layout(self) -> Vec2 {
        self.0.max(Self::MINIMUM.0)
    }
}

impl Default for WindowSize {
    fn default() -> Self {
        Self::OPENING
    }
}

/// Copies the primary window's logical size into [`WindowSize`] whenever
/// the window changes, writing only when the size itself is different.
/// Runs in `PreUpdate`, so every `Update` layout pass sees this frame's
/// size, and `bevy_ui` lays out against the same size later in the frame.
pub(crate) fn track_window_size(
    windows: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut size: ResMut<WindowSize>,
) {
    for window in &windows {
        size.set_if_neq(WindowSize(window.size()));
    }
}

/// Marks the camera that draws the world-space scene and the interface:
/// the one [`setup_app`] spawns. [`frame_main_camera`] frames it; a second
/// camera, such as a capture probe's, takes its projection and transform
/// from this one to draw the same scene.
#[derive(Component)]
pub(crate) struct MainCamera;

/// How the main camera frames the world-space scene: the zoom it draws the
/// world at, in logical pixels per world unit, and the world point it puts
/// at the centre of the window.
///
/// A screen whose layout places a world-space scene writes it - the Match
/// every frame, for its board, and Home on entry and whenever the window's
/// size changes, for its diamond; every other screen leaves it at
/// [`Self::IDENTITY`], which [`reset_world_framing_on_screen_change`]
/// restores on each change of screen. `bevy_ui` lays out in the window's
/// own pixels whatever this says, so the framing moves and scales the world
/// and nothing else.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct WorldFraming {
    /// Logical pixels per world unit: the main camera's projection scale is
    /// its inverse.
    pub(crate) zoom: f32,
    /// The world point drawn at the centre of the window: the main
    /// camera's translation.
    pub(crate) center: Vec2,
}

impl WorldFraming {
    /// One world unit to a logical pixel, with the world's origin at the
    /// centre of the window: how every board was drawn before the layouts
    /// adapted, and still the framing of a screen with no world-space scene
    /// of its own to place.
    pub(crate) const IDENTITY: Self = Self {
        zoom: 1.0,
        center: Vec2::ZERO,
    };

    /// The framing that draws the world point `world` at `window_point`, in
    /// logical pixels from the top-left of a window of size `window`, at
    /// `zoom`. Window coordinates run down and world coordinates up, which
    /// is the one sign to get right.
    pub(crate) fn placing(world: Vec2, window_point: Vec2, window: Vec2, zoom: f32) -> Self {
        let from_center = window_point - window / 2.0;
        Self {
            zoom,
            center: Vec2::new(
                world.x - from_center.x / zoom,
                world.y + from_center.y / zoom,
            ),
        }
    }

    /// Where the world point `world` is drawn under this framing, in logical
    /// pixels from the top-left of a window of size `window`: the inverse
    /// of [`Self::placing`], for a test that needs to know where something
    /// on a board is drawn.
    #[cfg(test)]
    pub(crate) fn window_point(self, world: Vec2, window: Vec2) -> Vec2 {
        let offset = (world - self.center) * self.zoom;
        Vec2::new(window.x / 2.0 + offset.x, window.y / 2.0 - offset.y)
    }
}

impl Default for WorldFraming {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Gives the main camera the projection scale and translation
/// [`WorldFraming`] asks for, writing only what differs. Runs in
/// `PostUpdate` before the camera's projection is recomputed and transforms
/// are propagated, so a framing written during `Update` is drawn that same
/// frame.
pub(crate) fn frame_main_camera(
    framing: Res<WorldFraming>,
    mut cameras: Query<(&mut Projection, &mut Transform), With<MainCamera>>,
) {
    let scale = 1.0 / framing.zoom;
    for (mut projection, mut transform) in &mut cameras {
        if let Projection::Orthographic(current) = projection.as_ref()
            && current.scale != scale
            && let Projection::Orthographic(orthographic) = projection.as_mut()
        {
            orthographic.scale = scale;
        }
        let translation = framing.center.extend(transform.translation.z);
        if transform.translation != translation {
            transform.translation = translation;
        }
    }
}

/// Puts [`WorldFraming`] back to [`WorldFraming::IDENTITY`] on every change
/// of screen, so a screen with no world-space scene of its own to frame is
/// never drawn at the zoom of the one before it.
///
/// One screen-agnostic system rather than a reset in the `OnExit` of each
/// screen that frames the world, for the reason `ui::hud`'s modal reset
/// gives: a per-screen obligation is the kind a new screen forgets. It runs in
/// `StateTransition` after the transition is applied and before the exit
/// and enter schedules, so a screen's own framing, written on entry or by
/// its layout pass in `Update`, always comes after it; and it fires exactly
/// when a screen's `OnEnter` would.
pub(crate) fn reset_world_framing_on_screen_change(
    mut transitions: MessageReader<StateTransitionEvent<AppScreen>>,
    mut framing: ResMut<WorldFraming>,
) {
    let screen_changed = transitions.read().any(|transition| {
        transition.exited != transition.entered || transition.allow_same_state_transitions
    });
    if screen_changed {
        framing.set_if_neq(WorldFraming::IDENTITY);
    }
}

/// Creates the camera and the shared stone mesh and materials.
pub(super) fn setup_app(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((Camera2d, MainCamera));

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

/// The window sizes that matter by name: the opening window and the
/// smallest; the maximised and the fullscreen windows of a 1920x1080
/// screen at Windows' 150% (1280x720 logical pixels) and of a 1366x768
/// screen; the maximised window of a 1440x900 Mac; a 1920x1080 screen; and
/// a large one. The maximised sizes are estimates, not measurements: the
/// screen less Windows' default taskbar and a title bar for 1280x641 and
/// 1366x689, and less macOS's standard 24 pt menu bar and 28 pt title bar,
/// with the Dock hidden, for 1440x848. Every layout's tests run at these,
/// and a real-layout test that cannot afford the whole [`layout_sweep`]
/// runs at some of them.
#[cfg(test)]
pub(crate) const NAMED_WINDOW_SIZES: [Vec2; 9] = [
    Vec2::new(1040.0, 900.0),
    Vec2::new(1040.0, 600.0),
    Vec2::new(1280.0, 641.0),
    Vec2::new(1280.0, 720.0),
    Vec2::new(1366.0, 689.0),
    Vec2::new(1366.0, 768.0),
    Vec2::new(1440.0, 848.0),
    Vec2::new(1920.0, 1080.0),
    Vec2::new(2560.0, 1440.0),
];

/// Every window size a pure layout is checked at: [`NAMED_WINDOW_SIZES`],
/// every size from the minimum to 2560x1440 in steps of 20, a few that a
/// fractional scale factor gives, which are not whole pixels, and a few
/// larger windows, where a board can reach the most it grows.
#[cfg(test)]
pub(crate) fn layout_sweep() -> Vec<Vec2> {
    let mut sizes = NAMED_WINDOW_SIZES.to_vec();
    for width in (MIN_WINDOW_WIDTH..=2560).step_by(20) {
        for height in (MIN_WINDOW_HEIGHT..=1440).step_by(20) {
            sizes.push(Vec2::new(width as f32, height as f32));
        }
    }
    // 1641x923 and 2049x1153 physical pixels at a scale factor of 1.5, and
    // 1707x961 and 2401x1351 at 1.25.
    sizes.extend([
        Vec2::new(1641.0, 923.0) / 1.5,
        Vec2::new(2049.0, 1153.0) / 1.5,
        Vec2::new(1707.0, 961.0) / 1.25,
        Vec2::new(2401.0, 1351.0) / 1.25,
    ]);
    // A 2560x1600 screen, 3840x2160, 5120x2880 and 7680x4320 ones at a
    // scale factor of 1, and the portrait screens of a 2560x1440 and a
    // 3840x2160 one.
    sizes.extend([
        Vec2::new(2560.0, 1600.0),
        Vec2::new(3840.0, 2160.0),
        Vec2::new(5120.0, 2880.0),
        Vec2::new(7680.0, 4320.0),
        Vec2::new(1440.0, 2560.0),
        Vec2::new(2160.0, 3840.0),
    ]);
    sizes
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

    #[test]
    fn a_layout_is_computed_for_the_window_raised_to_the_minimum() {
        assert_eq!(WindowSize::default(), WindowSize::OPENING);
        assert_eq!(WindowSize::OPENING.layout(), Vec2::new(1040.0, 900.0));
        assert_eq!(WindowSize::MINIMUM.layout(), Vec2::new(1040.0, 600.0));
        assert_eq!(
            WindowSize(Vec2::new(1920.0, 1080.0)).layout(),
            Vec2::new(1920.0, 1080.0)
        );
        // Each direction is raised on its own.
        assert_eq!(
            WindowSize(Vec2::new(800.0, 500.0)).layout(),
            WindowSize::MINIMUM.0
        );
        assert_eq!(
            WindowSize(Vec2::new(1600.0, 480.0)).layout(),
            Vec2::new(1600.0, 600.0)
        );
        assert_eq!(
            WindowSize(Vec2::new(900.0, 1000.0)).layout(),
            Vec2::new(1040.0, 1000.0)
        );
        for size in layout_sweep() {
            assert_eq!(WindowSize(size).layout(), size, "{size}");
        }
    }

    /// The resource follows the primary window's logical size, and is
    /// written only when that size changes: a change to anything else about
    /// the window leaves it untouched, which is what lets a layout pass skip
    /// the frames where nothing moved.
    #[test]
    fn the_window_size_follows_the_primary_window_and_only_its_size() {
        use bevy::window::WindowResolution;

        #[derive(Resource, Default)]
        struct Seen(Vec<Vec2>);
        fn record(size: Res<WindowSize>, mut seen: ResMut<Seen>) {
            if size.is_changed() {
                seen.0.push(size.0);
            }
        }

        let mut app = App::new();
        app.init_resource::<WindowSize>()
            .init_resource::<Seen>()
            .add_systems(Update, (track_window_size, record).chain());
        let window = app
            .world_mut()
            .spawn((
                Window {
                    // Physical pixels, at a scale factor of 2: 1040x900.
                    resolution: WindowResolution::new(2080, 1800).with_scale_factor_override(2.0),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, [Vec2::new(1040.0, 900.0)]);

        app.world_mut().get_mut::<Window>(window).unwrap().title = "renamed".into();
        app.update();
        assert_eq!(
            app.world().resource::<Seen>().0.len(),
            1,
            "a change to the title is not a change of size"
        );

        // Logical, not physical: at a scale factor of 2 this is a window
        // 2560 by 1282 physical pixels.
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(1280.0, 641.0);
        app.update();
        assert_eq!(
            app.world().resource::<Seen>().0,
            [Vec2::new(1040.0, 900.0), Vec2::new(1280.0, 641.0)]
        );
        assert_eq!(
            app.world()
                .get::<Window>(window)
                .unwrap()
                .resolution
                .physical_size(),
            UVec2::new(2560, 1282)
        );
    }

    #[test]
    fn a_framing_draws_the_world_point_it_places_where_it_was_asked() {
        let window = Vec2::new(1040.0, 900.0);
        assert_eq!(
            WorldFraming::IDENTITY.window_point(Vec2::ZERO, window),
            window / 2.0
        );
        // World y runs up, window y down.
        assert_eq!(
            WorldFraming::IDENTITY.window_point(Vec2::new(10.0, 22.0), window),
            Vec2::new(530.0, 428.0)
        );
        // The board as the opening window draws it, centred across the window
        // with its centre 472 px down it, is the camera as it always was: a
        // scale of 1 over the origin, exactly.
        assert_eq!(
            WorldFraming::placing(
                Vec2::new(0.0, BOARD_CENTER_Y),
                Vec2::new(520.0, 472.0),
                window,
                1.0
            ),
            WorldFraming::IDENTITY
        );
        for size in NAMED_WINDOW_SIZES {
            for zoom in [0.7, 1.0, 1.37, 2.0] {
                let world = Vec2::new(-3.0, BOARD_CENTER_Y);
                let target = Vec2::new(size.x * 0.4, size.y * 0.55);
                let framing = WorldFraming::placing(world, target, size, zoom);
                assert_eq!(framing.zoom, zoom);
                assert!(
                    framing.window_point(world, size).distance(target) < 1e-3,
                    "{size} at {zoom}"
                );
                // A world length is drawn `zoom` times as long.
                let step = framing.window_point(world + Vec2::new(GRID_SPACING, 0.0), size)
                    - framing.window_point(world, size);
                assert!((step.x - GRID_SPACING * zoom).abs() < 1e-3);
            }
        }
    }

    /// The main camera takes the framing as its projection scale and its
    /// translation, and nothing else does; a change of screen puts the
    /// framing back to a scale of 1 over the origin, and the camera with it.
    /// The plugin is the production one, so the schedule positions are the
    /// game's, and Bevy's transform propagation runs after it: the camera's
    /// global transform, which is what the frame is drawn from, takes a
    /// framing the same frame it is written, and would be a frame behind if
    /// the framing were applied after propagation.
    #[test]
    fn the_main_camera_takes_the_framing_and_a_change_of_screen_puts_it_back() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::transform::TransformPlugin)
            .init_state::<AppScreen>()
            .add_plugins(WindowLayoutPlugin);
        let orthographic = || Projection::Orthographic(OrthographicProjection::default_2d());
        let main = app
            .world_mut()
            .spawn((
                MainCamera,
                orthographic(),
                Transform::from_xyz(0.0, 0.0, 999.0),
            ))
            .id();
        let other = app
            .world_mut()
            .spawn((orthographic(), Transform::default()))
            .id();
        let camera = |app: &App, entity: Entity| {
            let Projection::Orthographic(projection) =
                app.world().get::<Projection>(entity).unwrap()
            else {
                panic!("an orthographic camera");
            };
            (
                projection.scale,
                app.world().get::<Transform>(entity).unwrap().translation,
            )
        };
        let drawn_from = |app: &App, entity: Entity| {
            app.world()
                .get::<GlobalTransform>(entity)
                .unwrap()
                .translation()
        };
        app.update();
        assert_eq!(camera(&app, main), (1.0, Vec3::new(0.0, 0.0, 999.0)));
        assert_eq!(drawn_from(&app, main), Vec3::new(0.0, 0.0, 999.0));

        *app.world_mut().resource_mut::<WorldFraming>() = WorldFraming {
            zoom: 2.0,
            center: Vec2::new(30.0, -40.0),
        };
        app.update();
        assert_eq!(camera(&app, main), (0.5, Vec3::new(30.0, -40.0, 999.0)));
        assert_eq!(
            drawn_from(&app, main),
            Vec3::new(30.0, -40.0, 999.0),
            "drawn from its new place the frame the framing was written"
        );
        assert_eq!(
            camera(&app, other),
            (1.0, Vec3::ZERO),
            "only the main camera"
        );

        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ReplayLibrary);
        app.update();
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            WorldFraming::IDENTITY
        );
        assert_eq!(camera(&app, main), (1.0, Vec3::new(0.0, 0.0, 999.0)));
        assert_eq!(drawn_from(&app, main), Vec3::new(0.0, 0.0, 999.0));
    }
}
