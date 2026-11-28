//! Spawns the viewer once, on entry: the board with its five decorative star
//! points, its stones and the rim disc behind each, both cursors, the
//! Teleport Markers and hidden selections, the score badges, the header, the
//! result line and the transport bar - and keeps them where the window's
//! size puts them.
//!
//! Everything is spawned in its final quantity and then only ever restyled by
//! [`super::display`] and moved by [`place_viewer_furniture`] - nothing on
//! this screen is spawned or despawned while it is open, which is why the
//! helpers here run from a [`Replay`] and never from playback position. The
//! two overlays this module asks [`super::modals`] to build are the same
//! kind of code, split out only for their size.
//!
//! # Where things stand
//!
//! [`super::geometry`] decides it, as a pure function of the window's size
//! and of the room the Replay's label needs in tall ([`TallLabelRoom`]),
//! which cannot change while the screen is open. Every node whose place
//! depends on the window carries a [`ViewerPlacement`] naming its place,
//! and [`place`] is the one function that writes a place into a node, used
//! by [`place_viewer_furniture`] and, through [`placed`], at spawn, so the
//! two cannot disagree. The countdown card's row is the one node spawned
//! another way, through the shared card's own `overlay_node`, which
//! [`place`] keeps to: both read the same `overlay_top`. That system also
//! gives the main camera the board's framing, which is how the board, and
//! everything drawn on it, is placed and scaled; the score badges, which
//! are drawn at their own size whatever the board does, are placed with
//! the rest of the board by [`super::display`].
//!
//! The header's four texts - the label, the timer, the round and the
//! result line - are spawned twice, once for each arrangement, and the
//! layout shows one set. Tall's is the header as it has always been: the
//! label in the row between Back and Match Settings, centred on them, and
//! the three lines under the row. Wide's stands the four in a column under
//! Back, each under the one before it, so a label that wraps pushes the
//! lines under it down rather than running into them. Both carry the same
//! [`ViewerText`] roles, so the display pass writes both alike.
//!
//! # Replay-only visuals
//!
//! Markers and hidden selections are spawned here even though they are
//! invisible during live play - the match is over, so there is nothing left to
//! hide, and seeing them is what makes a replay readable.
//!
//! Neither is marked by opacity, which is what fades into a background of
//! its own lightness - and, for a selection of the player's own stone, into
//! nothing at all. Each has a shape and a drawn edge instead. A Teleport
//! Marker is a diamond: a square outline with closed sides, turned on its
//! point ([`spawn_viewer_marker`]). A hidden selection is a ring around a
//! bare middle, through which the wood or the stone beneath is seen
//! ([`spawn_viewer_selections`]). Each is three strokes, outermost first,
//! and a player's two marks share their inks but for Player One's
//! innermost: the inks, and why there are three, are with
//! `hidden_selection_strokes` and `teleport_marker_strokes` in
//! `presentation.rs`; the sizes are in [`super::geometry`], with what each
//! has to fit inside.
//!
//! They are stacked cursors lowest, then markers, then selections, and each
//! step has a reason. A hidden selection is what this screen exists to show,
//! so its mark is drawn over every cursor and marker and neither can cover
//! it; where a diamond shares its point, it is the diamond that gives way.
//! (A score badge is above all three, and overlies a mark on the point up
//! and to the right of its cursor exactly as it overlies a stone there.)
//! A marker is drawn over the cursors because the two cannot both be
//! whole when Player Two's cursor stands on Player One's marker, and the
//! cursor loses less: on top, it would hide a quarter of the diamond, in
//! eight pieces between the middle of each side and its tips, and break
//! its outline; underneath, it gives up about a sixth of its heavier
//! border.
//!
//! # The focus ring
//!
//! Every control that carries a [`ViewerAction`] is marked [`Focusable`]
//! here, by [`viewer_focusable`], in the order the screen is drawn: the two
//! header actions, then the nine transport controls, then each overlay's
//! Close alone in the modal group. The two full-screen overlay roots are
//! plain nodes, not buttons, which block the pointer by being under it
//! (`ui::button`), so there is nothing there to mark. A control is disabled while on
//! screen at either end of a replay, and [`super::display`] carries that into
//! the ring rather than this module. One visible consequence of that rule:
//! a control that goes dead under the ring - Jump Back, once it has returned
//! playback to the start - leaves the ring, which clamps to the nearest
//! lower enabled control, and with Key drawn immediately before the
//! transport that is Key. The ring is drawn there, so a further Enter opens
//! the Key overlay rather than repeating a jump that has nothing left to
//! jump to.

use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::geometry::{
    HeaderLine, KEY_COLUMN_GAP, KEY_PANEL_FRAME, KeyColumns, SCORE_BADGE_SIZE, TallLabelRoom,
    VIEWER_BACK_WIDTH, VIEWER_BOARD_CENTER_Y, VIEWER_BOARD_OUTER_SIZE, VIEWER_CONTROL_HEIGHT,
    VIEWER_GRID_EXTENT, VIEWER_GRID_SPACING, VIEWER_HEADER_INSET, VIEWER_LABEL_SIZE,
    VIEWER_MATCH_SETTINGS_WIDTH, VIEWER_RESULT_HEIGHT, VIEWER_RESULT_SIZE, VIEWER_ROUND_SIZE,
    VIEWER_SELECTION_STROKES, VIEWER_TIMER_SIZE, ViewerLayout, WIDE_COLUMN_GAPS,
    viewer_board_position, viewer_layout, viewer_marker_layout, viewer_selection_radius,
};
use super::modals::{KEY_ROW_GAP, spawn_viewer_key_modal, spawn_viewer_match_settings_modal};
use super::{
    ReplayViewerState, ViewerAction, ViewerButtonLabel, ViewerCountdown, ViewerCursor,
    ViewerIntersection, ViewerMarker, ViewerScoreBadge, ViewerSelection, ViewerStoneRing,
    ViewerText, player_index,
};
use crate::app::AppScreen;
use crate::app::visuals::{
    STAR_POINT_RADIUS, STAR_POINTS, STONE_RADIUS, StoneMesh, StonePalette, WindowSize,
    WorldFraming, cursor_outline_layers, cursor_visual_style, spawn_outline_segments,
    stone_geometry,
};
use crate::audio::UiSound;
use crate::presentation::{
    BLUE_BORDER, BOARD_GRID, BOARD_WOOD, BUTTON_BACKGROUND, REPLAY_COUNTDOWN_BACKGROUND,
    SCORE_BADGE_BACKGROUND, SCORE_BADGE_TEXT, TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY,
    hidden_selection_strokes, player_accent, teleport_marker_strokes,
};
use crate::replay::Replay;
use crate::rules::{BOARD_SIZE, Cell, Player, Point};
use crate::ui::arrangement::Arrangement;
use crate::ui::countdown_card::CountdownCard;
use crate::ui::focus::{FocusGroup, Focusable};
/// The speed buttons, as (multiplier, label).
///
/// The multipliers must be exactly [`crate::replay::PLAYBACK_SPEEDS`], in that
/// order: `PlaybackController::set_speed` ignores any other value, so a button
/// for a speed missing there would silently do nothing.
/// `replay_generated_metadata_and_speed_labels_are_ascii_safe` in
/// [`super::display`] pins the match.
pub(super) const PLAYBACK_SPEEDS: [(f64, &str); 3] = [(0.5, "0.5x"), (1.0, "1x"), (2.0, "2x")];

/// The label of the speed button for `speed`, unbracketed: the display pass
/// brackets the armed one (see `crate::ui::widgets::selected_label`).
pub(super) fn speed_label(speed: f64) -> &'static str {
    PLAYBACK_SPEEDS
        .iter()
        .find_map(|(value, label)| (*value == speed).then_some(*label))
        .expect("viewer control speed has a label")
}
const VIEWER_HEADER_ACTIONS: [(ViewerAction, &str); 2] = [
    (ViewerAction::BackToReplays, "Back"),
    (ViewerAction::MatchSettings, "Match Settings"),
];
const VIEWER_CONTROL_GAP: f32 = 8.0;
const VIEWER_CONTROL_GROUP_GAP: f32 = 28.0;
const VIEWER_CONTROL_ORDER: [ViewerAction; 9] = [
    ViewerAction::Key,
    ViewerAction::Rewind,
    ViewerAction::JumpBack,
    ViewerAction::PlayPause,
    ViewerAction::JumpForward,
    ViewerAction::FastForward,
    ViewerAction::Speed(0.5),
    ViewerAction::Speed(1.0),
    ViewerAction::Speed(2.0),
];
const SCORE_BADGE_INNER_SIZE: Vec2 = Vec2::new(38.0, 24.0);
const SCORE_BADGE_Z: f32 = 12.0;
/// How the marks on one point are stacked, and why, is in the module docs:
/// cursors, then markers, then selections, all beneath the score badges.
const VIEWER_CURSOR_Z: f32 = 8.0;
const VIEWER_MARKER_Z: f32 = 9.0;
const VIEWER_SELECTION_Z: f32 = 10.0;
/// Segments in each circle of a hidden-selection stroke: twice the mesh
/// builder's default, so the facets do not show at this size.
const VIEWER_SELECTION_RESOLUTION: u32 = 64;
/// The Round 0 countdown, standing at the same point of this board as the
/// Match screen's stands on the live one, at whatever zoom the board is
/// drawn: see [`crate::ui::countdown_card`]. Smaller than the live card, and
/// its fill a little more translucent so the recorded opening stays readable
/// behind it.
pub(super) const VIEWER_COUNTDOWN_CARD: CountdownCard = CountdownCard {
    grid_spacing: VIEWER_GRID_SPACING,
    size: Vec2::new(150.0, 104.0),
    font_size: FontSize::Px(58.0),
    background: REPLAY_COUNTDOWN_BACKGROUND,
};

/// Where in the Viewer a piece of its furniture stands, for [`place`] to
/// write and [`place_viewer_furniture`] to keep written. Only the nodes
/// whose place depends on the window carry it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewerPlacement {
    /// The row of Back, tall's label and Match Settings.
    HeaderRow,
    /// Tall's label, in the middle of that row.
    TallLabel,
    /// One of tall's three lines under the row.
    TallLine(HeaderLine),
    /// Wide's column under Back, holding its own four header texts.
    Column,
    /// The full-width row the Round 0 countdown card is centred in.
    Countdown,
    /// The transport row.
    Controls,
    /// The Key's panel.
    KeyPanel,
    /// What holds the Key's two columns of rows, one over the other or side
    /// by side.
    KeyBody,
    /// One of the Key's two columns of rows.
    KeyColumn,
    /// The wording of a Key sentence for one arrangement, shown in that
    /// arrangement only: the score badges' sentence points to where the
    /// result line stands.
    KeySentence(Arrangement),
}

/// Writes into `node` the fields that `placement` decides in `layout`, and
/// leaves every other field alone: what a piece looks like is its
/// spawner's, and whether the countdown card is shown is the display
/// pass's. Each arrangement sets every field it owns, so a node moved into
/// wide and back is the node it was.
pub(super) fn place(layout: &ViewerLayout, placement: ViewerPlacement, node: &mut Node) {
    let shown = |shown: bool| {
        if shown { Display::Flex } else { Display::None }
    };
    match placement {
        ViewerPlacement::HeaderRow => node.top = px(layout.header_top()),
        ViewerPlacement::TallLabel => node.display = shown(!layout.has_column()),
        ViewerPlacement::TallLine(line) => {
            node.display = shown(!layout.has_column());
            node.top = px(layout.tall_line_top(line));
        }
        ViewerPlacement::Column => {
            let (left, top, width) = layout.column();
            node.display = shown(layout.has_column());
            node.left = px(left);
            node.top = px(top);
            node.width = px(width);
        }
        ViewerPlacement::Countdown => {
            node.top =
                px(VIEWER_COUNTDOWN_CARD.overlay_top(layout.board.center().y, layout.zoom()));
        }
        ViewerPlacement::Controls => node.bottom = px(layout.controls_bottom()),
        ViewerPlacement::KeyPanel => node.width = px(layout.key_width()),
        ViewerPlacement::KeyBody => match layout.key {
            KeyColumns::One => {
                node.flex_direction = FlexDirection::Column;
                node.row_gap = px(KEY_ROW_GAP);
                node.column_gap = px(0);
            }
            KeyColumns::Two => {
                node.flex_direction = FlexDirection::Row;
                node.row_gap = px(0);
                node.column_gap = px(KEY_COLUMN_GAP);
            }
        },
        ViewerPlacement::KeyColumn => {
            node.width = match layout.key {
                KeyColumns::One => percent(100),
                KeyColumns::Two => {
                    px((layout.key_width() - KEY_PANEL_FRAME - KEY_COLUMN_GAP) / 2.0)
                }
            };
        }
        ViewerPlacement::KeySentence(arrangement) => {
            node.display = shown(layout.arrangement == arrangement);
        }
    }
}

/// A node with its place in `layout` written into it, and the marker that
/// keeps it there.
pub(super) fn placed(
    layout: &ViewerLayout,
    placement: ViewerPlacement,
    mut node: Node,
) -> (Node, ViewerPlacement) {
    place(layout, placement, &mut node);
    (node, placement)
}

/// The layout for the window's size and the open Replay's label: what the
/// placement pass places by, and the display pass the board's marks.
pub(super) fn current_layout(window: &WindowSize, replay: &Replay) -> ViewerLayout {
    viewer_layout(window.layout(), TallLabelRoom::for_label(&replay.label))
}

/// Keeps the Viewer's furniture where [`super::geometry`] puts it for the
/// window's size, and the main camera framing the board there.
///
/// The framing is written every frame, only where it differs, because a
/// change of screen puts it back to the world's identity framing and this
/// screen's first frame has to take it again. The nodes are moved only on a
/// frame the window's size changed or new furniture appeared, which is the
/// screen's first; their spawners place them already, through the same
/// [`place`]. The score badges are the display pass's to move, with the
/// rest of the board.
///
/// It runs first in the Viewer's chain, before the display pass, which
/// writes another part of one of the same nodes: whether the countdown
/// card's row is shown, where this pass writes how far down the window it
/// stands. Each writes only its own fields of that node.
pub(super) fn place_viewer_furniture(
    window: Res<WindowSize>,
    viewer: Res<ReplayViewerState>,
    added: Query<(), Added<ViewerPlacement>>,
    mut nodes: Query<(&ViewerPlacement, &mut Node)>,
    mut framing: ResMut<WorldFraming>,
) {
    let Some(replay) = viewer.replay.as_ref() else {
        return;
    };
    let layout = current_layout(&window, replay);
    framing.set_if_neq(layout.framing());
    if !window.is_changed() && added.is_empty() {
        return;
    }
    for (placement, mut node) in &mut nodes {
        let mut placed = node.clone();
        place(&layout, *placement, &mut placed);
        if *node != placed {
            *node = placed;
        }
    }
}

pub(super) fn spawn_viewer_screen(
    mut commands: Commands,
    viewer: Res<ReplayViewerState>,
    window: Res<WindowSize>,
    palette: Res<StonePalette>,
    stone_mesh: Res<StoneMesh>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Some(replay) = viewer.replay.as_ref() else {
        return;
    };
    spawn_viewer_board(&mut commands, &palette, &stone_mesh);
    spawn_viewer_selections(&mut commands, &mut meshes, &mut materials);
    spawn_viewer_ui(&mut commands, replay, &current_layout(&window, replay));
}

fn spawn_viewer_board(commands: &mut Commands, palette: &StonePalette, stone_mesh: &StoneMesh) {
    commands.spawn((
        Sprite::from_color(BOARD_WOOD, Vec2::splat(VIEWER_BOARD_OUTER_SIZE)),
        Transform::from_xyz(0.0, VIEWER_BOARD_CENTER_Y, -2.0),
        DespawnOnExit(AppScreen::ReplayViewer),
    ));
    for index in 0..BOARD_SIZE {
        let offset = index as f32 * VIEWER_GRID_SPACING - VIEWER_GRID_EXTENT / 2.0;
        commands.spawn((
            Sprite::from_color(BOARD_GRID, Vec2::new(VIEWER_GRID_EXTENT + 3.0, 2.5)),
            Transform::from_xyz(0.0, VIEWER_BOARD_CENTER_Y + offset, -1.0),
            DespawnOnExit(AppScreen::ReplayViewer),
        ));
        commands.spawn((
            Sprite::from_color(BOARD_GRID, Vec2::new(2.5, VIEWER_GRID_EXTENT + 3.0)),
            Transform::from_xyz(offset, VIEWER_BOARD_CENTER_Y, -1.0),
            DespawnOnExit(AppScreen::ReplayViewer),
        ));
    }
    // Decorative star points, matching the live board's: inert furniture
    // between the grid lines (z -1) and the intersections (z 2), carrying no
    // component any display system iterates.
    for point in STAR_POINTS {
        let position = viewer_board_position(point);
        commands.spawn((
            Mesh2d(stone_mesh.0.clone()),
            MeshMaterial2d(palette.star.clone()),
            Transform::from_xyz(position.x, position.y, 0.0)
                .with_scale(Vec3::splat(STAR_POINT_RADIUS / STONE_RADIUS)),
            DespawnOnExit(AppScreen::ReplayViewer),
        ));
    }
    for y in 0..BOARD_SIZE {
        for x in 0..BOARD_SIZE {
            let point = Point::new(x, y);
            let position = viewer_board_position(point);
            // The ring, as on the live board: between the star points (z 0)
            // and the stone it surrounds (z 2), hidden until a white or gray
            // stone stands here.
            commands.spawn((
                Mesh2d(stone_mesh.0.clone()),
                MeshMaterial2d(palette.ring.clone()),
                Transform::from_xyz(position.x, position.y, 1.0),
                Visibility::Hidden,
                ViewerStoneRing(point),
                DespawnOnExit(AppScreen::ReplayViewer),
            ));
            commands.spawn((
                Mesh2d(stone_mesh.0.clone()),
                MeshMaterial2d(palette.empty.clone()),
                Transform::from_xyz(position.x, position.y, 2.0)
                    .with_scale(Vec3::splat(stone_geometry(Cell::Empty).fill_scale)),
                ViewerIntersection(point),
                DespawnOnExit(AppScreen::ReplayViewer),
            ));
        }
    }
    for player in Player::ALL {
        spawn_viewer_cursor(commands, player);
        spawn_viewer_score_badge(commands, player);
        spawn_viewer_marker(commands, player);
    }
}

/// Spawns both players' hidden-selection marks, hidden. Every Round 0 slot,
/// of which ordinary play uses the first, has two: the ordinary mark, and
/// the smaller one [`super::display`] shows in its place when the other
/// player selected the same point. The smaller is a mark of its own, with
/// strokes as wide as the larger's, and not the larger scaled down.
///
/// A mark is three concentric strokes around an empty middle. Each stroke
/// is an annulus reaching from the mark's inner edge out to that stroke's
/// own outer edge, and the three are stacked outermost lowest - so a stroke
/// is drawn over the one outside it rather than beside it, and no seam of
/// board can show between two. The meshes and materials are made here and
/// held only by the entities that draw them, so they are freed when the
/// screen is left, and no shared resource holds one.
fn spawn_viewer_selections(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
) {
    // Six materials, one per stroke of each player's mark, and six meshes,
    // one per stroke of each size.
    let inks =
        Player::ALL.map(|player| hidden_selection_strokes(player).map(|ink| materials.add(ink)));
    for shared in [false, true] {
        let radius = viewer_selection_radius(shared);
        let inner_edge = radius - VIEWER_SELECTION_STROKES.iter().sum::<f32>();
        let mut outer_edge = radius;
        let strokes = VIEWER_SELECTION_STROKES.map(|width| {
            let stroke = Annulus::new(inner_edge, outer_edge);
            outer_edge -= width;
            meshes.add(stroke.mesh().resolution(VIEWER_SELECTION_RESOLUTION))
        });
        for player in Player::ALL {
            let inks = &inks[player_index(player)];
            for slot in 0..crate::opening::OPENING_SELECTION_LIMIT {
                commands
                    .spawn((
                        Transform::from_xyz(0.0, 0.0, VIEWER_SELECTION_Z),
                        Visibility::Hidden,
                        ViewerSelection {
                            player,
                            slot,
                            shared,
                        },
                        DespawnOnExit(AppScreen::ReplayViewer),
                    ))
                    .with_children(|mark| {
                        for (layer, (stroke, ink)) in strokes.iter().zip(inks).enumerate() {
                            mark.spawn((
                                Mesh2d(stroke.clone()),
                                MeshMaterial2d(ink.clone()),
                                Transform::from_xyz(0.0, 0.0, layer as f32 * 0.1),
                            ));
                        }
                    });
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScoreBadgeVisualStyle {
    border_color: Color,
    outer_size: Vec2,
    inner_size: Vec2,
}

fn score_badge_visual_style(player: Player) -> ScoreBadgeVisualStyle {
    ScoreBadgeVisualStyle {
        border_color: player_accent(player),
        outer_size: SCORE_BADGE_SIZE,
        inner_size: SCORE_BADGE_INNER_SIZE,
    }
}

fn spawn_viewer_score_badge(commands: &mut Commands, player: Player) {
    let style = score_badge_visual_style(player);
    commands
        .spawn((
            Sprite::from_color(style.border_color, style.outer_size),
            Transform::from_xyz(0.0, 0.0, SCORE_BADGE_Z),
            ViewerScoreBadge(player),
            DespawnOnExit(viewer_score_badge_screen()),
        ))
        .with_children(|badge| {
            badge.spawn((
                Sprite::from_color(SCORE_BADGE_BACKGROUND, style.inner_size),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
            badge.spawn((
                Text2d::new("0"),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(SCORE_BADGE_TEXT),
                TextLayout::justify(Justify::Center).with_no_wrap(),
                Transform::from_xyz(0.0, 0.0, 0.2),
            ));
        });
}

fn spawn_viewer_cursor(commands: &mut Commands, player: Player) {
    let style = cursor_visual_style(player);
    commands
        .spawn((
            Transform::from_xyz(0.0, 0.0, VIEWER_CURSOR_Z),
            Visibility::Visible,
            ViewerCursor(player),
            DespawnOnExit(AppScreen::ReplayViewer),
        ))
        .with_children(|cursor| {
            for layer in cursor_outline_layers(style) {
                spawn_outline_segments(cursor, layer.color, style.size, layer.thickness, layer.z);
            }
        });
}

/// Spawns one player's Teleport Marker, hidden until a frame carries one.
///
/// A marker is a square outline turned 45 degrees to stand on a point,
/// which is what keeps it from being read as a cursor or a stone, and
/// unlike a cursor its sides are closed: at Player Two's size the gaps a
/// cursor leaves mid-side lined up into an X. The turn is set once, here;
/// [`super::display`] only ever moves a marker.
///
/// Each of its three strokes is four bars, a horizontal and a vertical
/// pair, laid out by [`viewer_marker_layout`]: every stroke is drawn in to
/// the one inner edge and the next painted over it, as a hidden-selection
/// mark's rings are, so that no two abut and no seam of board shows
/// between them. A bar runs to the corner of its stroke's own square, or
/// to the corner of the marker's `size` square if that comes first, which
/// notches each tip by half the stroke; a whole tip would leave Player
/// One's diamond no visible clearance in its cell.
fn spawn_viewer_marker(commands: &mut Commands, player: Player) {
    let layout = viewer_marker_layout(player);
    commands
        .spawn((
            Transform::from_xyz(0.0, 0.0, VIEWER_MARKER_Z)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
            Visibility::Hidden,
            ViewerMarker(player),
            DespawnOnExit(AppScreen::ReplayViewer),
        ))
        .with_children(|marker| {
            let strokes = teleport_marker_strokes(player)
                .into_iter()
                .zip(layout.outer_edges);
            for (layer, (ink, outer_edge)) in strokes.enumerate() {
                let width = outer_edge - layout.inner_edge;
                let middle = (outer_edge + layout.inner_edge) / 2.0;
                let length = 2.0 * layout.reach(outer_edge);
                let z = layer as f32 * 0.1;
                for sign in [-1.0, 1.0] {
                    marker.spawn((
                        Sprite::from_color(ink, Vec2::new(length, width)),
                        Transform::from_xyz(0.0, sign * middle, z),
                    ));
                    marker.spawn((
                        Sprite::from_color(ink, Vec2::new(width, length)),
                        Transform::from_xyz(sign * middle, 0.0, z),
                    ));
                }
            }
        });
}

/// The text style of one of the header's four texts: its size and colour,
/// centred, and for the label, wrapped within a word too long for a line.
/// Each size is written where it is set, as a constant, so that the text
/// size floor's scan can read it.
pub(super) fn header_text(
    role: ViewerText,
    content: String,
) -> (Text, TextFont, TextColor, TextLayout) {
    let (font_size, color) = match role {
        ViewerText::Label => (FontSize::Px(VIEWER_LABEL_SIZE), TEXT_HEADING),
        ViewerText::Timer => (FontSize::Px(VIEWER_TIMER_SIZE), TEXT_HEADING),
        ViewerText::Round => (FontSize::Px(VIEWER_ROUND_SIZE), TEXT_HEADING),
        ViewerText::Result => (FontSize::Px(VIEWER_RESULT_SIZE), TEXT_MUTED),
        ViewerText::Countdown | ViewerText::PlayPause => {
            unreachable!("not a header text: {role:?}")
        }
    };
    let layout = match role {
        ViewerText::Label => {
            TextLayout::justify(Justify::Center).with_linebreak(LineBreak::WordOrCharacter)
        }
        _ => TextLayout::justify(Justify::Center),
    };
    (
        Text::new(content),
        TextFont {
            font_size,
            ..default()
        },
        TextColor(color),
        layout,
    )
}

fn spawn_viewer_ui(commands: &mut Commands, replay: &Replay, layout: &ViewerLayout) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            DespawnOnExit(AppScreen::ReplayViewer),
        ))
        .with_children(|root| {
            root.spawn(placed(
                layout,
                ViewerPlacement::HeaderRow,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(VIEWER_HEADER_INSET),
                    right: px(VIEWER_HEADER_INSET),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|header| {
                header
                    .spawn(viewer_header_side_node(JustifyContent::FlexStart))
                    .with_children(|left| {
                        spawn_viewer_button(
                            left,
                            VIEWER_HEADER_ACTIONS[0].0,
                            VIEWER_HEADER_ACTIONS[0].1,
                            VIEWER_BACK_WIDTH,
                        );
                    });
                header
                    .spawn(placed(
                        layout,
                        ViewerPlacement::TallLabel,
                        viewer_header_center_node(),
                    ))
                    .with_child((
                        header_text(ViewerText::Label, replay.label.clone()),
                        Node {
                            width: percent(100),
                            ..default()
                        },
                        ViewerText::Label,
                    ));
                header
                    .spawn(viewer_header_side_node(JustifyContent::FlexEnd))
                    .with_children(|right| {
                        spawn_viewer_button(
                            right,
                            VIEWER_HEADER_ACTIONS[1].0,
                            VIEWER_HEADER_ACTIONS[1].1,
                            VIEWER_MATCH_SETTINGS_WIDTH,
                        );
                    });
            });
            for (line, role) in [
                (HeaderLine::Timer, ViewerText::Timer),
                (HeaderLine::Round, ViewerText::Round),
                (HeaderLine::Result, ViewerText::Result),
            ] {
                let min_height = match role {
                    ViewerText::Result => px(VIEWER_RESULT_HEIGHT),
                    _ => Val::Auto,
                };
                root.spawn((
                    header_text(role, String::new()),
                    placed(
                        layout,
                        ViewerPlacement::TallLine(line),
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            width: percent(100),
                            min_height,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    ),
                    role,
                ));
            }
            root.spawn(placed(
                layout,
                ViewerPlacement::Column,
                Node {
                    position_type: PositionType::Absolute,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ))
            .with_children(|column| {
                column.spawn((
                    header_text(ViewerText::Label, replay.label.clone()),
                    Node {
                        width: percent(100),
                        ..default()
                    },
                    ViewerText::Label,
                ));
                for (gap, role) in WIDE_COLUMN_GAPS.into_iter().zip([
                    ViewerText::Timer,
                    ViewerText::Round,
                    ViewerText::Result,
                ]) {
                    column.spawn((
                        header_text(role, String::new()),
                        Node {
                            width: percent(100),
                            margin: UiRect::top(px(gap)),
                            ..default()
                        },
                        role,
                    ));
                }
            });
            // Hidden until a frame carries a countdown stage; see
            // `super::display`. The card is the shared one, and why its
            // digit has to be a child of it rather than the card itself is
            // in `crate::ui::countdown_card`.
            root.spawn((
                Node {
                    display: Display::None,
                    ..VIEWER_COUNTDOWN_CARD.overlay_node(layout.board.center().y, layout.zoom())
                },
                ViewerPlacement::Countdown,
                GlobalZIndex(90),
                ViewerCountdown,
            ))
            .with_children(|overlay| VIEWER_COUNTDOWN_CARD.spawn(overlay, ViewerText::Countdown));
            root.spawn(placed(
                layout,
                ViewerPlacement::Controls,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: px(VIEWER_CONTROL_GROUP_GAP),
                    ..default()
                },
            ))
            .with_children(|controls| {
                for group in viewer_control_groups() {
                    spawn_viewer_control_group(controls, group);
                }
            });
            spawn_viewer_key_modal(root, layout);
            spawn_viewer_match_settings_modal(root, replay);
        });
}

/// Where a control sits in the focus ring: the header actions first, the
/// transport controls after them in [`VIEWER_CONTROL_ORDER`], and each
/// overlay's Close as the sole entry of the modal group. Keyed by action so
/// the two orders the screen is drawn in are also the order the ring walks.
fn viewer_focusable(action: ViewerAction) -> Focusable {
    if let Some(order) = VIEWER_HEADER_ACTIONS
        .iter()
        .position(|(header, _)| *header == action)
    {
        return Focusable {
            group: FocusGroup::Screen,
            order: order as u32,
        };
    }
    if let Some(order) = VIEWER_CONTROL_ORDER
        .iter()
        .position(|control| *control == action)
    {
        return Focusable {
            group: FocusGroup::Screen,
            order: (VIEWER_HEADER_ACTIONS.len() + order) as u32,
        };
    }
    debug_assert!(matches!(
        action,
        ViewerAction::CloseKey | ViewerAction::CloseMatchSettings
    ));
    Focusable {
        group: FocusGroup::Modal,
        order: 0,
    }
}

fn viewer_control_groups() -> [&'static [ViewerAction]; 3] {
    [
        &VIEWER_CONTROL_ORDER[..1],
        &VIEWER_CONTROL_ORDER[1..6],
        &VIEWER_CONTROL_ORDER[6..],
    ]
}

/// The width of one transport control.
fn control_width(action: ViewerAction) -> f32 {
    match action {
        ViewerAction::Key => 70.0,
        ViewerAction::Rewind | ViewerAction::PlayPause => 90.0,
        ViewerAction::JumpBack => 105.0,
        ViewerAction::JumpForward | ViewerAction::FastForward => 115.0,
        ViewerAction::Speed(_) => 62.0,
        ViewerAction::BackToReplays
        | ViewerAction::MatchSettings
        | ViewerAction::CloseMatchSettings
        | ViewerAction::CloseKey => unreachable!("not a transport control: {action:?}"),
    }
}

/// The transport row's width: its controls and the gaps between them.
#[cfg(test)]
pub(super) fn viewer_controls_width() -> f32 {
    let groups = viewer_control_groups();
    let gaps_between = VIEWER_CONTROL_GROUP_GAP * (groups.len() - 1) as f32;
    groups
        .iter()
        .map(|group| {
            group.iter().copied().map(control_width).sum::<f32>()
                + VIEWER_CONTROL_GAP * (group.len() - 1) as f32
        })
        .sum::<f32>()
        + gaps_between
}

fn spawn_viewer_control_group(parent: &mut ChildSpawnerCommands, actions: &[ViewerAction]) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(VIEWER_CONTROL_GAP),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|group| {
            for action in actions.iter().copied() {
                let width = control_width(action);
                match action {
                    ViewerAction::Key => spawn_viewer_button(group, action, "Key", width),
                    ViewerAction::Rewind => spawn_viewer_button(group, action, "Rewind", width),
                    ViewerAction::JumpBack => {
                        spawn_viewer_button(group, action, "Jump Back", width);
                    }
                    ViewerAction::PlayPause => spawn_play_pause_button(group, width),
                    ViewerAction::JumpForward => {
                        spawn_viewer_button(group, action, "Jump Forward", width);
                    }
                    ViewerAction::FastForward => {
                        spawn_viewer_button(group, action, "Fast Forward", width);
                    }
                    ViewerAction::Speed(speed) => {
                        spawn_viewer_button(group, action, speed_label(speed), width);
                    }
                    ViewerAction::BackToReplays
                    | ViewerAction::MatchSettings
                    | ViewerAction::CloseMatchSettings
                    | ViewerAction::CloseKey => {}
                }
            }
        });
}

fn viewer_header_side_node(justify_content: JustifyContent) -> Node {
    Node {
        flex_basis: px(0),
        flex_grow: 1.0,
        justify_content,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn viewer_header_center_node() -> Node {
    Node {
        min_width: px(0),
        flex_basis: px(0),
        flex_grow: 2.0,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

fn spawn_play_pause_button(parent: &mut ChildSpawnerCommands, width: f32) {
    parent
        .spawn((
            Button,
            ViewerAction::PlayPause,
            viewer_focusable(ViewerAction::PlayPause),
            UiSound::new(ViewerAction::PlayPause.sound()),
            Node {
                width: px(width),
                height: px(VIEWER_CONTROL_HEIGHT),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BLUE_BORDER),
        ))
        .with_child((
            Text::new("Play"),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            ViewerText::PlayPause,
            ViewerButtonLabel,
        ));
}

pub(super) fn spawn_viewer_button(
    parent: &mut ChildSpawnerCommands,
    action: ViewerAction,
    label: &str,
    width: f32,
) {
    parent
        .spawn((
            Button,
            action,
            viewer_focusable(action),
            UiSound::new(action.sound()),
            Node {
                width: px(width),
                height: px(VIEWER_CONTROL_HEIGHT),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BLUE_BORDER),
        ))
        .with_child((
            Text::new(label.to_string()),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            ViewerButtonLabel,
        ));
}

const fn viewer_score_badge_screen() -> AppScreen {
    AppScreen::ReplayViewer
}

#[cfg(test)]
mod tests {
    use super::super::super::test_support::fixed_metadata_replay;
    use super::super::apply_viewer_action;
    use super::super::geometry::ViewerMarkerLayout;
    use super::super::player_score;
    use super::*;
    use crate::app::visuals::{CursorVisualStyle, MIN_WINDOW_WIDTH};
    use crate::presentation::BLACK_STONE;
    use crate::presentation::WHITE_STONE;
    use crate::replay::PlaybackController;
    use crate::rules::Score;

    /// Opening a Replay, returning to the Library, and opening another must
    /// never leave the previous Replay's UI entities behind: entering the
    /// Viewer always produces exactly the same entity count, and leaving it
    /// always returns to zero, regardless of which Replay was open.
    #[test]
    fn viewer_ui_does_not_leak_into_another_replay_or_screen() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_systems(
                OnEnter(AppScreen::ReplayViewer),
                |mut commands: Commands| {
                    spawn_viewer_ui(&mut commands, &fixed_metadata_replay(), &opening());
                },
            );
        app.update();

        // Count UI-tree entities specifically, rather than every ECS entity:
        // Bevy's own state-transition/observer machinery allocates a few
        // internal bookkeeping entities that are irrelevant to whether the
        // Viewer's own content leaked.
        let ui_entity_count = |app: &mut App| {
            app.world_mut()
                .query_filtered::<Entity, With<Node>>()
                .iter(app.world())
                .count()
        };

        let mut entered_count = None;
        for _ in 0..3 {
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(AppScreen::ReplayViewer);
            app.update();
            let count = ui_entity_count(&mut app);
            assert!(count > 0);
            assert_eq!(
                *entered_count.get_or_insert(count),
                count,
                "opening a replay must spawn the same entity count every time, not accumulate"
            );

            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(AppScreen::ReplayLibrary);
            app.update();
            assert_eq!(
                ui_entity_count(&mut app),
                0,
                "leaving the Viewer must despawn everything it spawned"
            );
        }
    }

    #[test]
    fn replay_cursors_keep_player_identity_colors() {
        let black_cursor = cursor_visual_style(Player::One);
        let white_cursor = cursor_visual_style(Player::Two);

        assert_eq!(black_cursor.color, player_accent(Player::One));
        assert_eq!(white_cursor.color, player_accent(Player::Two));
        assert_eq!(black_cursor.border_color, BLACK_STONE);
        assert_eq!(white_cursor.border_color, WHITE_STONE);
        assert_eq!(cursor_outline_layers(black_cursor).len(), 2);
        assert_eq!(cursor_outline_layers(white_cursor).len(), 2);
    }

    /// A Teleport Marker as it is actually spawned: hidden, turned 45
    /// degrees, and drawn in its player's three marker inks - the stone
    /// colour outside, the accent, and inside whatever carries it on a
    /// stone of the outer stroke's colour - each stroke opaque and painted
    /// over the one outside it.
    #[test]
    fn a_teleport_marker_is_spawned_as_three_strokes_stood_on_a_point() {
        let mut app = App::new();
        app.add_systems(Update, |mut commands: Commands| {
            for player in Player::ALL {
                spawn_viewer_marker(&mut commands, player);
            }
        });
        app.update();

        let world = app.world_mut();
        let markers: Vec<_> = world
            .query::<(&ViewerMarker, &Transform, &Visibility, &Children)>()
            .iter(world)
            .map(|(marker, transform, visibility, children)| {
                (marker.0, *transform, *visibility, children.to_vec())
            })
            .collect();
        assert_eq!(markers.len(), 2);
        for (player, transform, visibility, children) in markers {
            assert_eq!(visibility, Visibility::Hidden, "{player:?}");
            assert_eq!(
                transform.rotation,
                Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
                "{player:?}'s marker is not on its point"
            );
            assert_eq!(transform.translation.z, VIEWER_MARKER_Z);

            let mut strokes: Vec<(Color, f32)> = children
                .into_iter()
                .map(|child| {
                    let sprite = world.get::<Sprite>(child).expect("a marker is sprites");
                    let z = world.get::<Transform>(child).unwrap().translation.z;
                    (sprite.color, z)
                })
                .collect();
            strokes.dedup();
            let inks = teleport_marker_strokes(player);
            assert_eq!(
                strokes,
                [(inks[0], 0.0), (inks[1], 0.1), (inks[2], 0.2)],
                "{player:?}'s marker is its three strokes, outermost lowest, four bars each"
            );
            for ink in inks {
                assert_eq!(ink.to_srgba().alpha, 1.0, "{player:?}'s marker is opaque");
            }
        }
        // Cursors, then markers, then selections, each clear of the layers
        // of the one beneath, and all of it beneath the score badges.
        const { assert!(VIEWER_MARKER_Z > VIEWER_CURSOR_Z + 0.1) };
        const { assert!(VIEWER_SELECTION_Z > VIEWER_MARKER_Z + 0.2) };
        const { assert!(VIEWER_SELECTION_Z + 0.2 < SCORE_BADGE_Z) };
    }

    /// A cursor outline as it was actually spawned, measured from its
    /// sprites rather than from the style it was meant to be drawn with:
    /// sixteen arms, eight to a layer, each lying on a side of the `size`
    /// square and running in from a corner, as thick as its layer, longer
    /// than the border is wide and stopping short of mid-side - and none
    /// of them hidden on its own account.
    fn assert_outline(world: &World, children: &[Entity], style: CursorVisualStyle) {
        assert_eq!(children.len(), 16);
        for (layer, arms) in cursor_outline_layers(style).iter().zip(children.chunks(8)) {
            for arm in arms {
                let sprite = world.get::<Sprite>(*arm).expect("an outline is sprites");
                let size = sprite.custom_size.expect("an arm has a size");
                let at = world.get::<Transform>(*arm).unwrap().translation;
                assert_ne!(
                    world.get::<Visibility>(*arm),
                    Some(&Visibility::Hidden),
                    "an arm is shown whenever its outline is"
                );
                assert_eq!(sprite.color, layer.color);
                assert_eq!(at.z, layer.z);
                assert_eq!(size.min_element(), layer.thickness);
                let length = size.max_element();
                let offsets = at.truncate().abs();
                assert_eq!(offsets.max_element(), style.size / 2.0);
                assert_eq!(
                    offsets.min_element(),
                    (style.size - length) / 2.0,
                    "an arm does not run in from its corner"
                );
                assert!(length < style.size / 2.0, "no gap mid-side");
                assert!(length > style.border_thickness, "an arm is a stub");
            }
        }
    }

    /// A Teleport Marker's diamond as it was actually spawned, measured
    /// from its sprites rather than from the layout it was meant to be
    /// drawn to: twelve bars, four to a stroke, in the stroke's ink, one
    /// on each of the four sides of its band and reaching in to the
    /// marker's one inner edge, so that the sides are closed. A bar's
    /// length is twice how far its stroke reaches along a side: to the
    /// corners of the `size` square for the two strokes whose outer edge
    /// lies past them, where the tip is notched, and to its own whole
    /// corners for the innermost, which lies inside them.
    fn assert_diamond(
        world: &World,
        children: &[Entity],
        layout: ViewerMarkerLayout,
        inks: [Color; 3],
    ) {
        assert_eq!(children.len(), 12);
        let strokes = layout.outer_edges.iter().zip(children.chunks(4));
        for (layer, (outer_edge, bars)) in strokes.enumerate() {
            let middle = (outer_edge + layout.inner_edge) / 2.0;
            let mut sides = Vec::new();
            for bar in bars {
                let sprite = world.get::<Sprite>(*bar).expect("a diamond is sprites");
                let size = sprite.custom_size.expect("a bar has a size");
                let at = world.get::<Transform>(*bar).unwrap().translation;
                assert_ne!(
                    world.get::<Visibility>(*bar),
                    Some(&Visibility::Hidden),
                    "a bar is shown whenever its diamond is"
                );
                assert_eq!(sprite.color, inks[layer], "a bar in the wrong ink");
                assert_eq!(at.z, layer as f32 * 0.1);
                let width = size.min_element();
                let offset = at.truncate().abs().max_element();
                assert_eq!(offset + width / 2.0, *outer_edge, "a bar is off its band");
                assert_eq!(
                    offset - width / 2.0,
                    layout.inner_edge,
                    "a stroke stops short"
                );
                let reach = outer_edge.min(layout.size / 2.0);
                assert_eq!(
                    size.max_element(),
                    2.0 * reach,
                    "a bar overruns or stops short"
                );
                // Which side of the square the bar lies on, as a sign and
                // an axis; a bar is along its side.
                let along_x = size.x > size.y;
                assert_eq!(
                    if along_x { at.x } else { at.y },
                    0.0,
                    "a bar is off centre"
                );
                sides.push((along_x, if along_x { at.y } else { at.x }));
            }
            sides.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(
                sides,
                [
                    (false, -middle),
                    (false, middle),
                    (true, -middle),
                    (true, middle)
                ],
                "a stroke is one bar on each side of its square"
            );
        }
    }

    /// Both cursors and both markers as `spawn_viewer_cursor` and
    /// `spawn_viewer_marker` spawn them, read back from the world: each
    /// drawn at its own style's size and strokes - a marker is not a
    /// cursor turned, it is smaller, finer and closed - with the cursors
    /// on show from the start and every one of them owned by the screen.
    #[test]
    fn cursors_and_markers_are_spawned_at_their_own_sizes_and_owned_by_the_screen() {
        let mut app = App::new();
        app.add_systems(Update, |mut commands: Commands| {
            for player in Player::ALL {
                spawn_viewer_cursor(&mut commands, player);
                spawn_viewer_marker(&mut commands, player);
            }
        });
        app.update();
        let world = app.world_mut();

        let cursors: Vec<_> = world
            .query::<(
                &ViewerCursor,
                &Visibility,
                &Children,
                &DespawnOnExit<AppScreen>,
            )>()
            .iter(world)
            .map(|(cursor, visibility, children, screen)| {
                (cursor.0, *visibility, children.to_vec(), screen.0)
            })
            .collect();
        assert_eq!(cursors.len(), 2);
        for (player, visibility, children, screen) in cursors {
            assert_eq!(visibility, Visibility::Visible, "{player:?}");
            assert_eq!(screen, AppScreen::ReplayViewer);
            assert_outline(world, &children, cursor_visual_style(player));
        }

        let markers: Vec<_> = world
            .query::<(&ViewerMarker, &Children, &DespawnOnExit<AppScreen>)>()
            .iter(world)
            .map(|(marker, children, screen)| (marker.0, children.to_vec(), screen.0))
            .collect();
        assert_eq!(markers.len(), 2);
        for (player, children, screen) in markers {
            assert_eq!(screen, AppScreen::ReplayViewer, "{player:?}");
            assert_diamond(
                world,
                &children,
                viewer_marker_layout(player),
                teleport_marker_strokes(player),
            );
        }
    }

    /// The hidden-selection marks as they are actually spawned: for each
    /// player, an ordinary and a smaller shared mark per Round 0 slot, all
    /// hidden and owned by the screen, each three strokes in that player's
    /// inks - and each stroke a ring around the same bare middle, which is
    /// what lets a selection of the player's own stone be seen at all.
    #[test]
    fn a_hidden_selection_is_spawned_as_three_strokes_around_a_bare_middle() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<ColorMaterial>>()
            .add_systems(
                Update,
                |mut commands: Commands,
                 mut meshes: ResMut<Assets<Mesh>>,
                 mut materials: ResMut<Assets<ColorMaterial>>| {
                    spawn_viewer_selections(&mut commands, &mut meshes, &mut materials);
                },
            );
        app.update();

        let world = app.world_mut();
        let mut marks: Vec<_> = world
            .query::<(
                &ViewerSelection,
                &Visibility,
                &Children,
                &DespawnOnExit<AppScreen>,
            )>()
            .iter(world)
            .map(|(selection, visibility, children, screen)| {
                assert_eq!(*visibility, Visibility::Hidden);
                assert_eq!(screen.0, AppScreen::ReplayViewer);
                (
                    (
                        player_index(selection.player),
                        selection.slot,
                        selection.shared,
                    ),
                    children.to_vec(),
                )
            })
            .collect();
        marks.sort_by_key(|(key, _)| *key);
        let expected: Vec<_> = Player::ALL
            .into_iter()
            .flat_map(|player| {
                (0..crate::opening::OPENING_SELECTION_LIMIT).flat_map(move |slot| {
                    [false, true].map(|shared| (player_index(player), slot, shared))
                })
            })
            .collect();
        assert_eq!(
            marks.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            expected,
            "one ordinary and one shared mark for every slot of both players"
        );

        for ((player, slot, shared), children) in marks {
            assert_eq!(children.len(), 3, "player {player} slot {slot}");
            let radius = viewer_selection_radius(shared);
            let opening = radius - VIEWER_SELECTION_STROKES.iter().sum::<f32>();
            let expected_outer_edges = {
                let mut edge = radius;
                VIEWER_SELECTION_STROKES.map(|width| {
                    let outer = edge;
                    edge -= width;
                    outer
                })
            };
            let inks = hidden_selection_strokes(Player::ALL[player]);
            let mut previous_z = f32::NEG_INFINITY;
            for (layer, child) in children.into_iter().enumerate() {
                let material = world.get::<MeshMaterial2d<ColorMaterial>>(child).unwrap();
                let ink = world
                    .resource::<Assets<ColorMaterial>>()
                    .get(&material.0)
                    .unwrap()
                    .color;
                assert_eq!(ink, inks[layer], "player {player} stroke {layer}");

                // An inner stroke is drawn over the one outside it.
                let z = world.get::<Transform>(child).unwrap().translation.z;
                assert!(z > previous_z);
                previous_z = z;

                // The stroke's own mesh: every vertex lies on its inner
                // or its outer circle, so nothing covers the middle.
                let mesh = world.get::<Mesh2d>(child).unwrap();
                let mesh = world.resource::<Assets<Mesh>>().get(&mesh.0).unwrap();
                let positions = mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .and_then(|positions| positions.as_float3())
                    .expect("a stroke has positions");
                let radii: Vec<f32> = positions
                    .iter()
                    .map(|[x, y, _]| Vec2::new(*x, *y).length())
                    .collect();
                let (nearest, farthest) = radii
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(low, high), radius| {
                        (low.min(*radius), high.max(*radius))
                    });
                assert!((nearest - opening).abs() < 0.001, "{nearest}");
                assert!((farthest - expected_outer_edges[layer]).abs() < 0.001);
                assert!(
                    radii.iter().all(|radius| {
                        (radius - nearest).abs() < 0.001 || (radius - farthest).abs() < 0.001
                    }),
                    "a stroke has a vertex inside its opening"
                );
                // Round enough that no facet shows: the flat between two
                // rim vertices cuts a quarter of a pixel into it at most.
                let rim = radii
                    .iter()
                    .filter(|radius| (**radius - farthest).abs() < 0.001)
                    .count();
                let sagitta = farthest * (1.0 - (std::f32::consts::PI / rim as f32).cos());
                assert!(sagitta < 0.25, "a stroke's facets cut {sagitta}px into it");
                assert_ne!(
                    world.get::<Visibility>(child),
                    Some(&Visibility::Hidden),
                    "a stroke is shown whenever its mark is"
                );
            }
        }
        // Twelve marks share six meshes - a stroke of each size - and six
        // materials, a stroke of each player's.
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 6);
        assert_eq!(world.resource::<Assets<ColorMaterial>>().len(), 6);
    }

    #[test]
    fn replay_score_badges_use_compact_high_contrast_player_styles() {
        let black = score_badge_visual_style(Player::One);
        let white = score_badge_visual_style(Player::Two);

        assert_eq!(black.border_color, player_accent(Player::One));
        assert_eq!(white.border_color, player_accent(Player::Two));
        assert_ne!(black.border_color, white.border_color);
        assert!(black.outer_size.x > black.inner_size.x);
        assert!(black.outer_size.y > black.inner_size.y);
        assert_eq!(black.outer_size, white.outer_size);
        assert_eq!(SCORE_BADGE_BACKGROUND, Color::srgb(0.07, 0.085, 0.105));
        assert_eq!(SCORE_BADGE_BACKGROUND.to_srgba().alpha, 1.0);
        assert_eq!(SCORE_BADGE_TEXT, Color::srgb(0.96, 0.97, 0.98));
        const { assert!(SCORE_BADGE_Z > VIEWER_CURSOR_Z) };
    }

    #[test]
    fn replay_score_badges_map_black_and_white_frame_scores() {
        let score = Score {
            black: 42,
            white: 38,
        };

        assert_eq!(player_score(score, Player::One), 42);
        assert_eq!(player_score(score, Player::Two), 38);
        assert_eq!(viewer_score_badge_screen(), AppScreen::ReplayViewer);
        assert_ne!(viewer_score_badge_screen(), AppScreen::Match);
        assert_ne!(viewer_score_badge_screen(), AppScreen::ReplayLibrary);
    }

    #[test]
    fn replay_score_badges_spawn_once_per_player_and_clean_up_with_viewer() {
        fn spawn_badges(mut commands: Commands) {
            for player in Player::ALL {
                spawn_viewer_score_badge(&mut commands, player);
            }
        }

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_systems(OnEnter(AppScreen::ReplayViewer), spawn_badges);
        app.update();
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ReplayViewer);
        app.update();
        let badge_count = {
            let world = app.world_mut();
            let mut badges =
                world.query::<(&ViewerScoreBadge, &DespawnOnExit<AppScreen>, &Transform)>();
            badges
                .iter(world)
                .filter(|(_, screen, transform)| {
                    screen.0 == AppScreen::ReplayViewer && transform.translation.z == SCORE_BADGE_Z
                })
                .count()
        };
        assert_eq!(badge_count, 2);

        // Leaving the screen despawns the badges automatically - no cleanup
        // system is registered here at all.
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ReplayLibrary);
        app.update();
        let remaining = {
            let world = app.world_mut();
            let mut badges = world.query::<&ViewerScoreBadge>();
            badges.iter(world).count()
        };
        assert_eq!(remaining, 0);
    }

    #[test]
    fn replay_header_uses_symmetric_sides_for_true_centering() {
        let left = viewer_header_side_node(JustifyContent::FlexStart);
        let right = viewer_header_side_node(JustifyContent::FlexEnd);
        let center = viewer_header_center_node();

        assert_eq!(left.flex_basis, px(0));
        assert_eq!(right.flex_basis, left.flex_basis);
        assert_eq!(right.flex_grow, left.flex_grow);
        assert_eq!(left.justify_content, JustifyContent::FlexStart);
        assert_eq!(right.justify_content, JustifyContent::FlexEnd);
        assert_eq!(center.min_width, px(0));
        assert_eq!(center.flex_basis, px(0));
        assert_eq!(center.flex_grow, left.flex_grow * 2.0);
    }

    #[test]
    fn replay_header_actions_are_exact_and_do_not_route_home() {
        assert_eq!(
            VIEWER_HEADER_ACTIONS,
            [
                (ViewerAction::BackToReplays, "Back"),
                (ViewerAction::MatchSettings, "Match Settings"),
            ]
        );
        let replay = fixed_metadata_replay();
        let mut viewer = ReplayViewerState {
            replay_id: Some(replay.id.clone()),
            ..default()
        };

        assert_eq!(
            apply_viewer_action(&mut viewer, ViewerAction::BackToReplays, &replay),
            Some(AppScreen::ReplayLibrary)
        );
        assert!(viewer.replay_id.is_none());
        assert_eq!(viewer.controller, PlaybackController::default());
    }

    /// Marking is authored, not derived: exactly the thirteen controls that
    /// carry an action are in the ring, in the order they are drawn - header,
    /// then transport - with each overlay's Close alone in the modal group,
    /// and no button outside the ring: the two overlay roots are plain
    /// nodes.
    #[test]
    fn viewer_controls_are_focusable_in_drawn_order_and_the_overlay_roots_are_not() {
        let mut app = App::new();
        app.add_systems(Update, |mut commands: Commands| {
            spawn_viewer_ui(&mut commands, &fixed_metadata_replay(), &opening());
        });
        app.update();

        let world = app.world_mut();
        let mut marked: Vec<_> = world
            .query::<(&Focusable, Option<&ViewerAction>)>()
            .iter(world)
            .map(|(focusable, action)| {
                (
                    focusable.group,
                    focusable.order,
                    *action.expect("a focusable control must carry an action"),
                )
            })
            .collect();
        marked.sort_by_key(|(group, order, _)| (*group, *order));
        let mut expected: Vec<_> = VIEWER_HEADER_ACTIONS
            .iter()
            .map(|(action, _)| *action)
            .chain(VIEWER_CONTROL_ORDER)
            .enumerate()
            .map(|(order, action)| (FocusGroup::Screen, order as u32, action))
            .collect();
        expected.push((FocusGroup::Modal, 0, ViewerAction::CloseKey));
        expected.push((FocusGroup::Modal, 0, ViewerAction::CloseMatchSettings));
        assert_eq!(marked, expected);

        let unmarked_buttons = world
            .query_filtered::<(), (With<Button>, Without<Focusable>)>()
            .iter(world)
            .count();
        assert_eq!(
            unmarked_buttons, 0,
            "every button is in the ring: the two overlay roots are plain nodes"
        );
    }

    #[test]
    fn key_control_is_immediately_before_rewind() {
        assert_eq!(VIEWER_CONTROL_ORDER[0], ViewerAction::Key);
        assert_eq!(VIEWER_CONTROL_ORDER[1], ViewerAction::Rewind);
    }

    #[test]
    fn replay_controls_are_ordered_in_visually_separated_groups() {
        let groups = viewer_control_groups();
        let flattened: Vec<_> = groups
            .iter()
            .flat_map(|group| group.iter().copied())
            .collect();

        assert_eq!(flattened, VIEWER_CONTROL_ORDER);
        assert_eq!(groups[0], [ViewerAction::Key]);
        assert_eq!(
            groups[1],
            [
                ViewerAction::Rewind,
                ViewerAction::JumpBack,
                ViewerAction::PlayPause,
                ViewerAction::JumpForward,
                ViewerAction::FastForward
            ]
        );
        assert_eq!(
            groups[2],
            [
                ViewerAction::Speed(0.5),
                ViewerAction::Speed(1.0),
                ViewerAction::Speed(2.0)
            ]
        );
        const { assert!(VIEWER_CONTROL_GROUP_GAP >= VIEWER_CONTROL_GAP * 3.0) };
    }

    /// The transport row is as wide as its controls and the gaps between
    /// them, worked out here from the widths each control has always had,
    /// and it fits the smallest window, which is as wide as the one it was
    /// made for.
    #[test]
    fn replay_control_groups_fit_the_smallest_window() {
        let button_widths = 70.0 + 90.0 + 105.0 + 90.0 + 115.0 + 115.0 + 62.0 * 3.0;
        let within_group_gaps = VIEWER_CONTROL_GAP * 6.0;
        let between_group_gaps = VIEWER_CONTROL_GROUP_GAP * 2.0;
        let width = button_widths + within_group_gaps + between_group_gaps;

        assert_eq!(viewer_controls_width(), width);
        assert_eq!(width, 875.0);
        assert!(width < MIN_WINDOW_WIDTH as f32);
    }

    /// The layout of the opening window, for a Replay whose label fits two
    /// lines.
    fn opening() -> ViewerLayout {
        viewer_layout(
            crate::app::visuals::WindowSize::OPENING.0,
            TallLabelRoom::TwoLines,
        )
    }

    const PLACEMENTS: [ViewerPlacement; 13] = [
        ViewerPlacement::HeaderRow,
        ViewerPlacement::TallLabel,
        ViewerPlacement::TallLine(HeaderLine::Timer),
        ViewerPlacement::TallLine(HeaderLine::Round),
        ViewerPlacement::TallLine(HeaderLine::Result),
        ViewerPlacement::Column,
        ViewerPlacement::Countdown,
        ViewerPlacement::Controls,
        ViewerPlacement::KeyPanel,
        ViewerPlacement::KeyBody,
        ViewerPlacement::KeyColumn,
        ViewerPlacement::KeySentence(Arrangement::Tall),
        ViewerPlacement::KeySentence(Arrangement::Wide),
    ];

    /// A node whose every field that [`place`] writes holds a value no
    /// placement writes, so that a field one arm of [`place`] leaves alone
    /// shows as this value rather than passing for a default.
    fn sentinel() -> Node {
        Node {
            display: Display::Grid,
            left: px(-1.5),
            top: px(-2.5),
            bottom: px(-3.5),
            width: px(-4.5),
            flex_direction: FlexDirection::RowReverse,
            row_gap: px(-5.5),
            column_gap: px(-6.5),
            ..default()
        }
    }

    /// Every placement, moved into another window's layout and back, is
    /// the node it was, starting from either layout: each arrangement, and
    /// each form of the Key, sets every field that any of them sets. And
    /// moving it changes it, so each really is placed from the window:
    /// every one between the opening window and 1040x700, whose layout
    /// differs in arrangement, in the board's size, in the piece's place
    /// down the window, and in the Key's form.
    #[test]
    fn a_placed_node_moved_and_moved_back_is_the_node_it_was() {
        let opening = opening();
        let other = viewer_layout(Vec2::new(1040.0, 700.0), TallLabelRoom::TwoLines);
        for placement in PLACEMENTS {
            for (from, to) in [(&opening, &other), (&other, &opening)] {
                let (start, _) = placed(from, placement, sentinel());
                let mut moved = start.clone();
                place(to, placement, &mut moved);
                assert_ne!(moved, start, "{placement:?} does not move");
                place(from, placement, &mut moved);
                assert_eq!(moved, start, "{placement:?} from {}", from.window);
            }
        }
    }

    /// What tall shows and wide hides, and the other way about: tall's
    /// label and its three lines, or wide's column.
    #[test]
    fn each_arrangement_shows_its_own_header() {
        let tall = opening();
        let wide = viewer_layout(
            crate::app::visuals::WindowSize::MINIMUM.0,
            TallLabelRoom::TwoLines,
        );
        let display =
            |layout: &ViewerLayout, placement| placed(layout, placement, Node::default()).0.display;
        for placement in [
            ViewerPlacement::TallLabel,
            ViewerPlacement::TallLine(HeaderLine::Timer),
            ViewerPlacement::TallLine(HeaderLine::Round),
            ViewerPlacement::TallLine(HeaderLine::Result),
        ] {
            assert_eq!(display(&tall, placement), Display::Flex, "{placement:?}");
            assert_eq!(display(&wide, placement), Display::None, "{placement:?}");
        }
        assert_eq!(display(&tall, ViewerPlacement::Column), Display::None);
        assert_eq!(display(&wide, ViewerPlacement::Column), Display::Flex);
        for (arrangement, in_tall, in_wide) in [
            (Arrangement::Tall, Display::Flex, Display::None),
            (Arrangement::Wide, Display::None, Display::Flex),
        ] {
            let placement = ViewerPlacement::KeySentence(arrangement);
            assert_eq!(display(&tall, placement), in_tall, "{placement:?}");
            assert_eq!(display(&wide, placement), in_wide, "{placement:?}");
        }
    }
}
