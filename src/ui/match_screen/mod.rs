//! The live match screen: board, stones, player cursors and the match HUD.
//!
//! # Responsibility
//!
//! This module renders [`GameState`]; its one write is the reset on entry
//! ([`initialize_match_state`], called from `prepare_match_screen`), after
//! which it only reads. Entering the screen resets the match and then spawns
//! the board with its five decorative star points, a stone entity and a ring
//! entity per intersection, a hidden capture mark per intersection (see
//! [`capture`]), two cursors, the Round 0 dot rows (see [`opening_dots`]),
//! temporary control hints and the HUD. The ring is hidden except behind a white or gray stone;
//! `stone_geometry` in `app::visuals` says why those two.
//!
//! Where all of it stands is [`geometry`]'s to say, as a pure function of
//! the window's size: the familiar layout in every window narrower than
//! about 1.12 times its height, and in a wider one until the wide
//! arrangement beside the board would draw the board more than 4% larger -
//! the opening shape stays familiar up to about 1182x1023 - with the board
//! itself placed and zoomed by the main camera. [`layout`] spawns the
//! furniture there and moves it when the size changes.
//!
//! # What the HUD deliberately does not show
//!
//! Round 0 cursors and selected locations are never drawn. There is no live
//! status line for rejected moves or opponent activity: any
//! such message would leak the other player's hidden selection, and
//! [`GameState`] has no field that could carry one - even the result of an
//! ended Match is worded here, from the authoritative score and end reasons.
//! The only text that appears mid-match is the compact end-gesture hint or
//! the hold-progress label, and the capture count under a player's side -
//! which says only what a resolution has already made public. After scoring
//! come the result - the winner in the heading, every end reason on the line
//! under it, and each player's final score beside their own name in their
//! corner - and the restart hint plus any replay-save problem.
//!
//! Match timer and round number are player preferences. Enabling Round Time
//! or Match Time in Settings switches the matching preference on at that
//! moment - reveals rather than forces - so the player can still hide either
//! readout afterwards, and a running Round Time no longer keeps the round
//! panel visible against the preference. What a running round clock does
//! keep on screen, and what no preference can remove, is the round-time bar
//! below the board: full when a round starts, shrinking from both ends toward
//! the centre as the Round Time runs down, present exactly while a round
//! clock is running. A hold gesture in progress takes that slot back for as
//! long as it runs, and a released end hold keeps it while its progress
//! drains; see [`bottom_bar_fractions`]. Hiding a clock never stops it.
//!
//! # Display writes are compared, not unconditional
//!
//! [`update_match_display`] runs every frame, but only *writes* a component
//! when the target value actually differs from the current one - see
//! [Presentation: continuous vs change-driven display](../../../docs/ARCHITECTURE.md#presentation-continuous-vs-change-driven-display)
//! for why a per-frame system still needs this (`GameState` changes every
//! frame the clock ticks, so gating on the whole resource being "changed"
//! would not help).

mod capture;
mod geometry;
mod layout;
mod opening_dots;

use bevy::ecs::query::QueryFilter;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::ControlBindings;
use super::help::restart_label;
use super::hud::{HudState, UiRole, modal_pause_state};
use crate::app::motion::MotionPolicy;
use crate::app::visuals::{
    BOARD_CENTER_Y, GRID_EXTENT, GRID_SPACING, STAR_POINT_RADIUS, STAR_POINTS, STONE_RADIUS,
    StoneMesh, StonePalette, WindowSize, board_position, cursor_visual_style, format_elapsed_time,
    stone_geometry,
};
use crate::app::{AppScreen, AppSystems};
use crate::gameplay::{
    GameState, InputGate, MatchPhase, active_hold_progress, countdown_text, initialize_match_state,
    player_index,
};
use crate::input::controller::{ControllerAssignments, ControllerBindings};
use crate::input::keyboard::KeyboardBindings;
use crate::input::{GameAction, SemanticInput};
use crate::match_settings::MatchSettings;
use crate::presentation::{BOARD_GRID, BOARD_WOOD};
use crate::replay::{EndReason, ReplayWinner};
use crate::rules::{BOARD_SIZE, Cell, Player, Point, Score};
use geometry::{Lines, match_layout};
use layout::{BOARD_OUTER_SIZE, bar_fill_width, place_match_furniture, spawn_cursor, spawn_ui};

/// Owns the match screen lifecycle and its display refresh.
pub(super) struct MatchScreenPlugin;

impl Plugin for MatchScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MotionPolicy>()
            .init_resource::<capture::CaptureIndication>()
            .init_resource::<opening_dots::OpeningDotRows>()
            .add_systems(
                OnEnter(AppScreen::Match),
                (
                    prepare_match_screen,
                    spawn_match_screen,
                    capture::spawn_capture_marks,
                    opening_dots::spawn_opening_dots,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                place_match_furniture
                    .run_if(in_state(AppScreen::Match))
                    .in_set(AppSystems::ScreenLayout),
            )
            .add_systems(
                Update,
                // Unordered against each other: they share no entity, and
                // all only read the Match. See `capture`'s and
                // `opening_dots`' module docs. The layout pass that places
                // some of their nodes runs before all three.
                (
                    update_match_display,
                    capture::update_capture_indication,
                    opening_dots::update_opening_dots,
                )
                    .run_if(in_state(AppScreen::Match))
                    .in_set(AppSystems::MatchDisplay),
            );
    }
}

#[derive(Component)]
struct Intersection(Point);

/// The ring disc behind one intersection, shown only while a white or gray
/// stone stands there; see [`stone_geometry`].
#[derive(Component)]
struct IntersectionRing(Point);

#[derive(Component)]
struct CursorVisual(Player);

#[derive(Component)]
struct HoldProgressTrack;

#[derive(Component)]
struct HoldProgressFill;

/// The round-time bar's track: the same shape as the hold bar's, in the same
/// slot, but its own entity and marker because the two mean different things
/// and are drawn in different colours.
#[derive(Component)]
struct RoundTimeTrack;

#[derive(Component)]
struct RoundTimeFill;

#[derive(Component)]
struct ControlHint(Player);

/// Resets the Match state and arms the input gate. The HUD's modal state is
/// not reset here: `ui::hud` closes every modal on the screen change
/// itself, for every screen at once.
fn prepare_match_screen(
    mut game: ResMut<GameState>,
    mut input_gate: ResMut<InputGate>,
    settings: Res<MatchSettings>,
) {
    initialize_match_state(&mut game, &mut input_gate, &settings);
}

fn spawn_match_screen(
    mut commands: Commands,
    palette: Res<StonePalette>,
    stone_mesh: Res<StoneMesh>,
    window: Res<WindowSize>,
) {
    commands.spawn((
        Sprite::from_color(BOARD_WOOD, Vec2::splat(BOARD_OUTER_SIZE)),
        Transform::from_xyz(0.0, BOARD_CENTER_Y, -2.0),
        DespawnOnExit(AppScreen::Match),
    ));

    for index in 0..BOARD_SIZE {
        let offset = index as f32 * GRID_SPACING - GRID_EXTENT / 2.0;
        commands.spawn((
            Sprite::from_color(BOARD_GRID, Vec2::new(GRID_EXTENT + 3.0, 2.5)),
            Transform::from_xyz(0.0, BOARD_CENTER_Y + offset, -1.0),
            DespawnOnExit(AppScreen::Match),
        ));
        commands.spawn((
            Sprite::from_color(BOARD_GRID, Vec2::new(2.5, GRID_EXTENT + 3.0)),
            Transform::from_xyz(offset, BOARD_CENTER_Y, -1.0),
            DespawnOnExit(AppScreen::Match),
        ));
    }

    // Decorative star points: inert board furniture between the grid lines
    // (z -1) and the intersections (z 2). No component marks them, so no
    // display system ever iterates or restyles them.
    for point in STAR_POINTS {
        let position = board_position(point);
        commands.spawn((
            Mesh2d(stone_mesh.0.clone()),
            MeshMaterial2d(palette.star.clone()),
            Transform::from_xyz(position.x, position.y, 0.0)
                .with_scale(Vec3::splat(STAR_POINT_RADIUS / STONE_RADIUS)),
            DespawnOnExit(AppScreen::Match),
        ));
    }

    for y in 0..BOARD_SIZE {
        for x in 0..BOARD_SIZE {
            let point = Point::new(x, y);
            let position = board_position(point);
            // The ring sits between the star points (z 0) and the stone it
            // surrounds (z 2), hidden until a white or gray stone stands here.
            commands.spawn((
                Mesh2d(stone_mesh.0.clone()),
                MeshMaterial2d(palette.ring.clone()),
                Transform::from_xyz(position.x, position.y, 1.0),
                Visibility::Hidden,
                IntersectionRing(point),
                DespawnOnExit(AppScreen::Match),
            ));
            commands.spawn((
                Mesh2d(stone_mesh.0.clone()),
                MeshMaterial2d(palette.empty.clone()),
                Transform::from_xyz(position.x, position.y, 2.0)
                    .with_scale(Vec3::splat(stone_geometry(Cell::Empty).fill_scale)),
                Intersection(point),
                DespawnOnExit(AppScreen::Match),
            ));
        }
    }

    for player in [Player::One, Player::Two] {
        let style = cursor_visual_style(player);
        spawn_cursor(&mut commands, player, style);
    }
    spawn_ui(&mut commands, &match_layout(window.layout()));
}

// The four bar nodes as four provably disjoint queries: each `With` its own
// marker and `Without` the other three, so one system can hold four
// `&mut Node` borrows at once.
type HoldTrackNode<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<HoldProgressTrack>,
        Without<HoldProgressFill>,
        Without<RoundTimeTrack>,
        Without<RoundTimeFill>,
    ),
>;

type HoldFillNode<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<HoldProgressFill>,
        Without<HoldProgressTrack>,
        Without<RoundTimeTrack>,
        Without<RoundTimeFill>,
    ),
>;

type RoundTrackNode<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<RoundTimeTrack>,
        Without<HoldProgressTrack>,
        Without<HoldProgressFill>,
        Without<RoundTimeFill>,
    ),
>;

type RoundFillNode<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<RoundTimeFill>,
        Without<HoldProgressTrack>,
        Without<HoldProgressFill>,
        Without<RoundTimeTrack>,
    ),
>;

/// The four nodes that draw the two bars in the slot below the board: the
/// hold-progress bar's track and fill, and the round-time bar's.
///
/// They are grouped because they are always written together, from the one
/// decision [`bottom_bar_fractions`] makes about which bar owns the slot, and
/// because their disjointness filters only make sense as a set.
#[derive(SystemParam)]
struct BottomBars<'w, 's> {
    hold_track: HoldTrackNode<'w, 's>,
    hold_fill: HoldFillNode<'w, 's>,
    round_track: RoundTrackNode<'w, 's>,
    round_fill: RoundFillNode<'w, 's>,
}

/// The ring behind each intersection. It writes `Transform` and
/// `Visibility`, as the intersections, cursors and control hints between
/// them also do, so it names all three markers in its filter: each of
/// those queries requires its own marker, which makes this one excluding
/// them enough to prove it shares no entity with any.
type IntersectionRings<'w, 's> = Query<
    'w,
    's,
    (
        &'static IntersectionRing,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (
        Without<Intersection>,
        Without<CursorVisual>,
        Without<ControlHint>,
    ),
>;

type ControlHints<'w, 's> =
    Query<'w, 's, (&'static ControlHint, &'static mut Visibility), Without<CursorVisual>>;

#[derive(SystemParam)]
struct MatchHintUi<'w, 's> {
    hints: ControlHints<'w, 's>,
    texts: Query<'w, 's, (&'static UiRole, &'static mut Text)>,
}

#[derive(SystemParam)]
struct MatchDisplayResources<'w> {
    game: Res<'w, GameState>,
    semantic: Res<'w, SemanticInput>,
    hud: Res<'w, HudState>,
    controls: ControlBindings<'w>,
    palette: Res<'w, StonePalette>,
    /// For how many lines the round panel is written on.
    window: Res<'w, WindowSize>,
}

fn update_match_display(
    resources: MatchDisplayResources,
    mut intersections: Query<(
        &Intersection,
        &mut MeshMaterial2d<ColorMaterial>,
        &mut Transform,
    )>,
    mut rings: IntersectionRings,
    mut cursors: Query<(&CursorVisual, &mut Transform, &mut Visibility), Without<Intersection>>,
    mut hint_ui: MatchHintUi,
    mut bars: BottomBars,
) {
    let MatchDisplayResources {
        game,
        semantic,
        hud,
        controls,
        palette,
        window,
    } = resources;
    let layout = match_layout(window.layout());

    for (intersection, mut material, mut transform) in &mut intersections {
        let cell = game.board.get(intersection.0);
        let handle = palette.fill(cell);
        if material.0 != *handle {
            material.0 = handle.clone();
        }
        let target_scale = Vec3::splat(stone_geometry(cell).fill_scale);
        if transform.scale != target_scale {
            transform.scale = target_scale;
        }
    }
    for (ring, mut transform, mut visibility) in &mut rings {
        let ring_scale = stone_geometry(game.board.get(ring.0)).ring_scale;
        if let Some(scale) = ring_scale {
            let target_scale = Vec3::splat(scale);
            if transform.scale != target_scale {
                transform.scale = target_scale;
            }
        }
        let target_visibility = if ring_scale.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != target_visibility {
            *visibility = target_visibility;
        }
    }

    let target_cursor_visibility = live_cursor_visibility(game.phase);
    for (cursor, mut transform, mut visibility) in &mut cursors {
        let point = game.cursors[player_index(cursor.0)];
        let position = board_position(point);
        if transform.translation.x != position.x || transform.translation.y != position.y {
            transform.translation.x = position.x;
            transform.translation.y = position.y;
        }
        if *visibility != target_cursor_visibility {
            *visibility = target_cursor_visibility;
        }
    }

    for (hint, mut visibility) in &mut hint_ui.hints {
        let target = if game.control_hints_visible[player_index(hint.0)] {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
    let gesture_text = gesture_text(
        &game,
        &semantic,
        &hud,
        &controls.keyboard,
        &controls.controller,
        &controls.assignments,
    );
    let final_score = (game.phase == MatchPhase::Ended).then(|| game.current_score());
    for (role, mut text) in &mut hint_ui.texts {
        let value = match role {
            UiRole::Heading => final_score.map_or_else(|| "GO TOGETHER".into(), result_heading),
            UiRole::EndReasons => end_reasons_text(&game.end_reasons),
            UiRole::PlayerOne => player_heading(Player::One, final_score),
            UiRole::PlayerTwo => player_heading(Player::Two, final_score),
            UiRole::Timer => timer_text(game.elapsed.as_secs_f64()),
            UiRole::Round => round_panel_text(
                game.round,
                game.round_time_remaining_seconds(),
                layout.round_lines,
            ),
            UiRole::Countdown => countdown_text(&game).unwrap_or_default().into(),
            UiRole::ControlHint(player) => control_hint_text(*player, &controls.keyboard),
            UiRole::Gesture => gesture_text.clone(),
            UiRole::HelpTitle | UiRole::HelpBody | UiRole::HelpPage | UiRole::ReturnHomeBody => {
                continue;
            }
        };
        if **text != value {
            **text = value;
        }
    }

    let (hold, round) = bottom_bar_fractions(&game, &hud);
    draw_centred_bar(&mut bars.hold_track, &mut bars.hold_fill, hold);
    draw_centred_bar(&mut bars.round_track, &mut bars.round_fill, round);
}

/// The round panel's text: the round number and, while a Round Time clock is
/// running, how much of it is left. `remaining` is that clock in whole
/// seconds, rounded up, from `MatchState::round_time_remaining_seconds`.
/// On two lines, for a title row with no room for one, the countdown
/// stands under the round instead of beside it.
///
/// Printable ASCII throughout, which Bevy's bundled fallback font requires
/// and `crate::architecture`'s guard enforces: the separator was once a
/// middle dot, which the font drew as a box. The clock is worded as the
/// countdown it is rather than as a second "Round", which read as two
/// different rounds side by side.
fn round_panel_text(round: usize, remaining: Option<u64>, lines: Lines) -> String {
    let separator = match lines {
        Lines::One => "  |  ",
        Lines::Two => "\n",
    };
    match remaining {
        Some(remaining) => format!(
            "Round {round}{separator}{:02}:{:02} left",
            remaining / 60,
            remaining % 60
        ),
        None => format!("Round {round}"),
    }
}

/// The Match timer's text: `TIME` and the time the Match has run.
fn timer_text(elapsed_seconds: f64) -> String {
    format!("TIME  {}", format_elapsed_time(elapsed_seconds))
}

/// What the slot below the board draws this frame, as `(hold, round)`
/// fractions of which at most one is `Some`.
///
/// A hold gesture wins the slot: it is transient and about something the
/// player is doing right now, while the round bar is ambient. So while
/// [`visible_hold_progress`] has a bar to show, the round bar is hidden, and
/// it returns when the hold has gone - for a released end hold, once its
/// timer has drained, since the draining bar is still about something the
/// player can act on. *Which* hold is showing - end or restart - is
/// [`active_hold_progress`]'s decision and is not re-derived here, so the
/// slot follows any change to that priority automatically.
///
/// The round bar is not a preference. It shows whenever a Round Time clock is
/// running ([`crate::authority::MatchState::round_time_remaining_fraction`]
/// is `Some`) and the slot is free, and is absent otherwise, so a player is
/// never surprised by a round ending. Unlike the hold bar it is not hidden
/// while a modal is open: the modal's overlay covers it anyway, and what it
/// measures - the paused round - is still true when the modal closes.
fn bottom_bar_fractions(game: &GameState, hud: &HudState) -> (Option<f32>, Option<f32>) {
    let hold = visible_hold_progress(game, hud).map(|(_, fraction)| fraction);
    let round = if hold.is_some() {
        None
    } else {
        game.round_time_remaining_fraction()
    };
    (hold, round)
}

/// Shows a bar at `fraction` of its track, or hides it for `None`. A hidden
/// bar keeps its last width; nothing reads it while the track is hidden.
fn draw_centred_bar<Track: QueryFilter, Fill: QueryFilter>(
    tracks: &mut Query<&mut Node, Track>,
    fills: &mut Query<&mut Node, Fill>,
    fraction: Option<f32>,
) {
    let track_display = if fraction.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut track in tracks.iter_mut() {
        if track.display != track_display {
            track.display = track_display;
        }
    }
    if let Some(fraction) = fraction {
        let fill_width = bar_fill_width(fraction);
        for mut fill in fills.iter_mut() {
            if fill.width != fill_width {
                fill.width = fill_width;
            }
        }
    }
}

fn gesture_text(
    game: &GameState,
    semantic: &SemanticInput,
    hud: &HudState,
    keyboard: &KeyboardBindings,
    controller: &ControllerBindings,
    assignments: &ControllerAssignments,
) -> String {
    // Only an open modal blanks the line. The release gate no longer has a
    // single "everything is blocked" state to reflect, and what it does gate
    // already shows through what is being held: a gated Select is a held
    // Select, which hides the end hint below, and a gesture whose input is
    // gated is not running, so `active_hold_progress` does not name it.
    if modal_pause_state(hud).input_blocked {
        return String::new();
    }
    if let Some((label, _)) = active_hold_progress(game) {
        return label.into();
    }
    if game.phase == MatchPhase::Playing {
        if game
            .control_hints_visible
            .into_iter()
            .any(|visible| visible)
            || semantic.pressed(GameAction::PlayerOneSelect)
            || semantic.pressed(GameAction::PlayerTwoSelect)
            || semantic.pressed(GameAction::RestartMatch)
        {
            return String::new();
        }
        return end_control_hint(keyboard);
    }
    if matches!(
        game.phase,
        MatchPhase::Opening | MatchPhase::OpeningCountdown
    ) {
        return String::new();
    }

    let replay_notice = replay_notice_suffix(&game.replay_notice);
    format!(
        "Hold {} for 3 seconds to restart{replay_notice}",
        restart_label(keyboard, controller, assignments)
    )
}

const fn live_cursor_visibility(phase: MatchPhase) -> Visibility {
    if matches!(phase, MatchPhase::Playing) {
        Visibility::Visible
    } else {
        Visibility::Hidden
    }
}

/// Both players' current keyboard Select keys, joined for prose: `M and 2`.
///
/// Shared with [`crate::ui::hud`], which names the end gesture in the
/// return-home confirmation, so that modal cannot name keys this screen's own
/// bottom hint does not. `compact_display_name` keeps an arrow-key binding to
/// `Up` rather than `Up Arrow` in both.
///
/// [`super::help::keyboard_end_gesture_label`] renders the same pair as
/// `M + 2`, with `display_name`, for the Help pages. Those pages
/// are never read beside this screen's bottom hint, so they do not need the
/// compact form; unifying the two would change the help copy.
pub(super) fn end_gesture_keys(bindings: &KeyboardBindings) -> String {
    format!(
        "{} and {}",
        bindings
            .key(GameAction::PlayerOneSelect)
            .compact_display_name(),
        bindings
            .key(GameAction::PlayerTwoSelect)
            .compact_display_name()
    )
}

/// The bottom hint shown during ordinary play: the end gesture and how long
/// it takes.
///
/// The duration is stated because the restart half of [`gesture_text`] has
/// always said "for 3 seconds" in this same text slot, and one of the two
/// hints leaving it out was an accident rather than a style.
fn end_control_hint(bindings: &KeyboardBindings) -> String {
    format!(
        "Hold {} for 3 seconds to end the match",
        end_gesture_keys(bindings)
    )
}

/// The hold gesture to draw, or `None`.
///
/// Nothing is drawn while a modal is open. Gameplay already reports no
/// gesture then - the gate stays fully armed while a modal is up, so neither
/// gesture is running, a running end hold is returned to zero as the modal
/// opens, and [`active_hold_progress`] does not report a restart hold left
/// frozen behind one - so this check is redundant by construction. It is
/// kept because the modal is this module's knowledge: whether the bar hides
/// under one should be readable here, and should not depend on how gameplay
/// happens to represent a frozen hold. The release gate needs no check of
/// its own: a gesture whose input is gated is not running, and
/// `active_hold_progress` names only a running gesture or, during play, an
/// end hold draining after a release.
fn visible_hold_progress(game: &GameState, hud: &HudState) -> Option<(&'static str, f32)> {
    (!modal_pause_state(hud).input_blocked)
        .then(|| active_hold_progress(game))
        .flatten()
}

fn control_hint_text(player: Player, bindings: &KeyboardBindings) -> String {
    let [up, left, down, right, select] = match player {
        Player::One => [
            GameAction::PlayerOneUp,
            GameAction::PlayerOneLeft,
            GameAction::PlayerOneDown,
            GameAction::PlayerOneRight,
            GameAction::PlayerOneSelect,
        ],
        Player::Two => [
            GameAction::PlayerTwoUp,
            GameAction::PlayerTwoLeft,
            GameAction::PlayerTwoDown,
            GameAction::PlayerTwoRight,
            GameAction::PlayerTwoSelect,
        ],
    };
    format!(
        "{} / P{}\nUp: {}\nLeft: {}\nDown: {}\nRight: {}\nSelect: {}",
        player_label(player),
        player.index() + 1,
        bindings.key(up).compact_display_name(),
        bindings.key(left).compact_display_name(),
        bindings.key(down).compact_display_name(),
        bindings.key(right).compact_display_name(),
        bindings.key(select).compact_display_name(),
    )
}

pub(crate) fn replay_notice_suffix(notice: &str) -> String {
    if notice.is_empty() {
        String::new()
    } else {
        format!("  |  {notice}")
    }
}

pub(crate) fn player_label(player: Player) -> &'static str {
    match player {
        Player::One => "BLACK",
        Player::Two => "WHITE",
    }
}

/// A player's corner heading: their colour, and once the Match has ended
/// their final score beside it.
///
/// The number goes on the board side of the name - `WHITE 23` in the left
/// corner, `24 BLACK` in the right, which is Player One's
/// ([`layout::player_header_node`]) - so the two names stay outermost and
/// the two numbers face each other across the board. Each number sits beside
/// its own name, which is why neither the Black-first order of the game's
/// other score readouts nor the White-left order of its sides has to give
/// way to the other here.
fn player_heading(player: Player, final_score: Option<Score>) -> String {
    let label = player_label(player);
    match (player, final_score) {
        (_, None) => label.into(),
        (Player::One, Some(score)) => format!("{} {label}", score.black),
        (Player::Two, Some(score)) => format!("{label} {}", score.white),
    }
}

/// The heading once the Match has ended: who won, by the colour names the
/// corners carry.
///
/// Who won is [`ReplayWinner::from_score`]'s comparison - the one the saved
/// Replay's result is read from - so this screen and the Replay it saves
/// cannot name different winners.
fn result_heading(final_score: Score) -> String {
    match ReplayWinner::from_score(final_score) {
        ReplayWinner::Draw => "Draw!".into(),
        winner => format!("{} Wins!", winner.label()),
    }
}

/// The line under an ended Match's result: why it ended. Every reason that
/// crossed on the final round is named, in the order the Replay records them,
/// as a plain list. The longest that list can be is what sizes the line; see
/// [`layout::END_REASONS_SIZE`].
fn end_reasons_text(reasons: &[EndReason]) -> String {
    reasons
        .iter()
        .map(|reason| reason.label())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::geometry::{
        END_REASONS_MAX_CHARS, HEADING_MAX_CHARS, PLAYER_HEADING_MAX_CHARS, ROUND_PANEL_MAX_CHARS,
        STACK_GAP, TIMER_MAX_CHARS,
    };
    use super::layout::{
        CONTROL_HINT_WIDTH, END_REASONS_SIZE, HOLD_PROGRESS_FILL_COLOR,
        HOLD_PROGRESS_OUTLINE_COLOR, ROUND_TIME_FILL_COLOR, ROUND_TIME_OUTLINE_COLOR,
        bar_fill_width, control_hint_node, hold_progress_track_node, hold_progress_track_outline,
        player_header_node, round_time_track_node, round_time_track_outline, spawn_ui,
        under_heading_node,
    };
    use super::*;
    use crate::app::visuals::{
        NAMED_WINDOW_SIZES, WorldFraming, cursor_outline_layers, cursor_visual_style, layout_sweep,
    };
    use crate::architecture::{source_file, sources_in};
    use crate::clock::MatchDuration;
    use crate::gameplay::HoldGesture;
    use crate::gameplay::countdown::begin_active_match;
    use crate::input::keyboard::KeyboardKey;
    use crate::match_settings::MatchSettingField;
    use crate::settings::GamePreferences;
    use bevy::color::Luminance;

    use crate::presentation::{
        BLACK_P1_RED, BLACK_STONE, DEEP_SURFACE_BACKGROUND, ORANGE, TEXT_HEADING, TEXT_SUBTLE,
        WHITE_P2_BLUE, WHITE_STONE,
    };

    use crate::ui::hud::{HudAction, match_hud_visibility};

    #[test]
    fn cursor_styles_are_prominent_distinct_and_overlap_safe() {
        let player_one = cursor_visual_style(Player::One);
        let player_two = cursor_visual_style(Player::Two);

        assert_eq!(player_one.color, BLACK_P1_RED);
        assert_eq!(player_two.color, WHITE_P2_BLUE);
        assert_eq!(player_one.border_color, BLACK_STONE);
        assert_eq!(player_two.border_color, WHITE_STONE);
        assert!(player_one.thickness >= 6.0);
        assert!(player_two.thickness >= 6.0);
        assert!(player_one.border_thickness > player_one.thickness);
        assert!(player_two.border_thickness > player_two.thickness);
        assert_eq!(cursor_outline_layers(player_one).len(), 2);
        assert_eq!(cursor_outline_layers(player_two).len(), 2);
        assert!(player_one.size > player_two.size);
    }

    #[test]
    fn live_cursors_stay_hidden_through_round_zero_and_show_at_reveal() {
        assert_eq!(
            live_cursor_visibility(MatchPhase::Opening),
            Visibility::Hidden
        );
        assert_eq!(
            live_cursor_visibility(MatchPhase::OpeningCountdown),
            Visibility::Hidden
        );
        assert_eq!(
            live_cursor_visibility(MatchPhase::Playing),
            Visibility::Visible
        );
        assert_eq!(
            live_cursor_visibility(MatchPhase::Ended),
            Visibility::Hidden
        );
    }

    /// The Match screen marks nothing [`Focusable`]: not *Home*, not
    /// *Help*, not anything else its spawner produces. This is
    /// load-bearing and invisible - the ring is admitted onto the Match
    /// whenever a HUD modal is open, so the absence of `Focusable` here is
    /// what keeps a ring from ever being live while gameplay reads the arrow
    /// keys and Space - and it fails the moment anyone marks a HUD button.
    /// The Help modal's three marked buttons are not this screen's:
    /// the modal is the HUD's, spawned once at `Startup`, and its ring is
    /// pinned in `ui::help`.
    #[test]
    fn the_match_screen_marks_nothing_focusable() {
        use crate::ui::focus::Focusable;

        let mut app = match_display_test_app();
        app.update();

        let world = app.world_mut();
        let actions: Vec<HudAction> = world.query::<&HudAction>().iter(world).copied().collect();
        for wanted in [HudAction::OpenReturnHome, HudAction::OpenHelp] {
            assert!(
                actions.contains(&wanted),
                "the spawner must have produced the {wanted:?} button, or the zero below \
                 does not cover it"
            );
        }
        let marked: Vec<Focusable> = world.query::<&Focusable>().iter(world).copied().collect();
        assert_eq!(
            marked,
            [],
            "the Match screen's buttons stay pointer-only: a ring here would be live \
             during gameplay"
        );
    }

    /// A Match under real `bevy_ui` layout and the production graph - the
    /// focus core, the HUD, the whole gameplay pipeline with semantic fusion
    /// in front of it, the window layout, and this screen's own plugin -
    /// entered through a real state transition and advanced until play is
    /// live, in a window of `window` logical pixels, with the framing set
    /// for the board where the layout puts it. Returns the app and the
    /// temporary Replay root, which the caller removes.
    fn live_match_app(window: Vec2) -> (App, std::path::PathBuf) {
        use crate::app::configure_app_systems;
        use crate::app::visuals::WindowLayoutPlugin;
        use crate::gameplay::GameplayPlugin;
        use crate::input::controller::ControllerHysteresisState;
        use crate::input::update_semantic_input;
        use crate::replay_library::{ReplayLibrary, ReplayStore};
        use crate::test_support::advance_test_app;
        use crate::ui::focus::{UiFocusPlugin, real_layout_app};
        use crate::ui::hud::HudPlugin;

        let root =
            std::env::temp_dir().join(format!("go-together-modal-ring-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();

        let mut app = real_layout_app(window);
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Mesh>>();
        let palette = {
            let mut materials = app.world_mut().resource_mut::<Assets<ColorMaterial>>();
            crate::app::visuals::test_stone_palette(&mut materials)
        };
        let stone_mesh = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            StoneMesh(meshes.add(Circle::new(1.0)))
        };
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_plugins(WindowLayoutPlugin)
            .add_plugins(UiFocusPlugin)
            .add_plugins(HudPlugin)
            .add_plugins(GameplayPlugin)
            .add_plugins(MatchScreenPlugin)
            .insert_resource(palette)
            .insert_resource(stone_mesh)
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .insert_resource(ControllerHysteresisState::default())
            .insert_resource(SemanticInput::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(Time::<()>::default())
            .insert_resource(ReplayLibrary::from_store(ReplayStore::new(root.clone())))
            .insert_resource(crate::diagnostics::Diagnostics::default())
            .insert_resource(crate::audio::AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(
                Update,
                update_semantic_input.in_set(AppSystems::DeviceInput),
            );
        configure_app_systems(&mut app);
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Match);
        advance_test_app(&mut app, 0.1);
        assert_match_frames_its_board(&app, window, "on entry");
        // Screen entry armed the gate; nothing is held, so this frame
        // releases every action and play is live.
        advance_test_app(&mut app, 0.1);
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::Match
        );
        assert!(!app.world().resource::<InputGate>().any_gated());
        (app, root)
    }

    /// The camera's framing in `app` is the one the Match's layout gives
    /// the board in a window of `window` logical pixels.
    fn assert_match_frames_its_board(app: &App, window: Vec2, context: &str) {
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            match_layout(WindowSize(window).layout()).framing(),
            "{window} {context}"
        );
    }

    /// Through the game's own schedule, at every named size: the Match
    /// frames its board on entry, a change of screen puts the framing back,
    /// and coming back frames the board again. The layout pass writes the
    /// framing on every frame of the Match, so it holds even on an entry
    /// where the window's size has not changed since the last; a pass run
    /// only when the size changes, or not at all, would leave the board
    /// drawn at its own size over the origin.
    #[test]
    fn leaving_the_match_and_coming_back_frames_the_board_again() {
        use crate::test_support::advance_test_app;

        for window in NAMED_WINDOW_SIZES {
            let (mut app, root) = live_match_app(window);
            assert_match_frames_its_board(&app, window, "in play");
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(AppScreen::ReplayLibrary);
            advance_test_app(&mut app, 0.1);
            assert_eq!(
                *app.world().resource::<WorldFraming>(),
                WorldFraming::IDENTITY,
                "{window} off the Match"
            );
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(AppScreen::Match);
            advance_test_app(&mut app, 0.1);
            assert_match_frames_its_board(&app, window, "back on the Match");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    /// The one button carrying `wanted`, on the Match HUD or in the
    /// Help modal.
    fn hud_button(app: &mut App, wanted: HudAction) -> Entity {
        let world = app.world_mut();
        let found: Vec<Entity> = world
            .query::<(Entity, &HudAction)>()
            .iter(world)
            .filter(|(_, action)| **action == wanted)
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one {wanted:?} button");
        found[0]
    }

    /// The Round 0 countdown card, laid out with its text by `bevy_ui`
    /// itself: the digit drawn in the middle of the card, and the card over
    /// the same rows of the board at every size, at whatever zoom - where
    /// the board's centre really is drawn, by the camera's framing. In the
    /// opening window it stands where it always has, its top 250 px down.
    /// This card is the reference the Replay Viewer's agrees with on its own
    /// board - see `crate::ui::countdown_card`. The HUD's own pass writes
    /// the stage in play; here it is written directly.
    #[test]
    fn the_countdown_digit_is_drawn_centred_on_a_card_over_the_board() {
        use super::layout::spawn_countdown_overlay;
        use crate::app::visuals::GRID_SPACING;
        use crate::ui::countdown_card::{
            assert_countdown_card_lands_on_board, measure_countdown_card,
        };
        use crate::ui::focus::real_layout_app;
        use crate::ui::hud::UiRole;

        for window in NAMED_WINDOW_SIZES {
            let layout = match_layout(window);
            let mut app = real_layout_app(window);
            let world = app.world_mut();
            spawn_countdown_overlay(&mut world.commands(), &layout);
            world.flush();
            let mut digits = world.query::<(&UiRole, &mut Text)>();
            for (role, mut text) in digits.iter_mut(world) {
                if matches!(role, UiRole::Countdown) {
                    text.0 = "3".into();
                }
            }
            app.update();
            app.update();

            let landing = measure_countdown_card(&mut app, "3");
            let board_center = layout
                .framing()
                .window_point(Vec2::new(0.0, BOARD_CENTER_Y), window);
            let top = assert_countdown_card_lands_on_board(
                &landing,
                board_center,
                GRID_SPACING,
                layout.zoom(),
            );
            if window == WindowSize::OPENING.0 {
                assert_eq!(top, 250.0);
            }
        }
    }

    /// Every text `app` shows lies inside a window of `window` logical
    /// pixels, and there is some to show.
    fn assert_every_text_is_inside(app: &mut App, window: Vec2, context: &str) {
        use crate::ui::focus::laid_out_texts;

        let texts = laid_out_texts(app);
        assert!(texts.len() > 5, "{context}: {texts:?}");
        let inside = Rect::from_corners(Vec2::ZERO, window);
        for (text, rect) in texts {
            assert!(
                inside.contains(rect.min) && inside.contains(rect.max),
                "{context}: {text:?} at {rect:?} leaves the {window} window"
            );
        }
    }

    /// At every named size, the pointer reaches the Match HUD's two buttons
    /// at their centres over a live board, under real layout and real
    /// picking - nothing the HUD draws over the board stops it - and every
    /// text stands inside the window; with either modal open, the pointer
    /// reaches that modal's buttons and neither HUD button. The Help modal
    /// keeps its one size for now, so it is opened only in a window that
    /// holds it.
    #[test]
    fn the_pointer_reaches_the_hud_buttons_and_a_modal_covers_them() {
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::help::{HELP_MODAL_HEIGHT, HELP_MODAL_WIDTH};

        for window in NAMED_WINDOW_SIZES {
            let (mut app, root) = live_match_app(window);
            let hud_buttons = reachable_buttons(&mut app);
            let mut actions: Vec<_> = hud_buttons
                .iter()
                .map(|entity| *app.world().get::<HudAction>(*entity).unwrap())
                .collect();
            actions.sort_by_key(|action| format!("{action:?}"));
            assert_eq!(
                actions,
                [HudAction::OpenHelp, HudAction::OpenReturnHome],
                "{window}"
            );
            assert_eq!(
                covered_at_their_centres(&mut app, &hud_buttons),
                [],
                "{window}"
            );
            assert_every_text_is_inside(&mut app, window, &format!("{window}"));

            let help_fits = window.x > HELP_MODAL_WIDTH && window.y > HELP_MODAL_HEIGHT;
            for modal in ["Help", "Return Home"] {
                if modal == "Help" && !help_fits {
                    continue;
                }
                {
                    let mut hud = app.world_mut().resource_mut::<HudState>();
                    hud.help_open = modal == "Help";
                    hud.return_home_open = modal == "Return Home";
                }
                app.update();
                app.update();
                let in_modal: Vec<_> = reachable_buttons(&mut app)
                    .into_iter()
                    .filter(|entity| !hud_buttons.contains(entity))
                    .collect();
                assert!(!in_modal.is_empty(), "{window} {modal}");
                assert_eq!(
                    covered_at_their_centres(&mut app, &in_modal),
                    [],
                    "{window} {modal}"
                );
                assert_eq!(
                    covered_at_their_centres(&mut app, &hud_buttons),
                    hud_buttons,
                    "{window} {modal}"
                );
                assert_every_text_is_inside(&mut app, window, &format!("{window} {modal}"));
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    /// How many Round 0 choices each player holds - what a Select that
    /// reached gameplay would change.
    fn held_choices(app: &App) -> [usize; 2] {
        let game = app.world().resource::<GameState>();
        [
            game.opening_selections[player_index(Player::One)].len(),
            game.opening_selections[player_index(Player::Two)].len(),
        ]
    }

    /// The end-to-end guard for the outcome that must stay impossible, run
    /// against hostile defaults: Player One's movement is the four arrow
    /// keys and Restart is Space, all of them focus navigation keys. The
    /// Help modal is opened during a Match by its pointer button,
    /// then an arrow and Space are held while the ring walks to Close and
    /// activates it. Gameplay must read none of it: the cursor never moves,
    /// no restart hold starts, and on the frame the modal closes the input
    /// gate still holds both held actions - Restart included - until they
    /// are physically released, after which the same arrow moves the cursor
    /// again. Under real layout and the production graph, through
    /// [`live_match_app`].
    #[test]
    fn a_match_opened_modal_is_operated_by_the_ring_without_gameplay_reading_a_key() {
        use bevy::input_focus::InputFocus;

        use crate::gameplay::{GameplayPause, active_hold_progress};
        use crate::test_support::advance_test_app;

        let (mut app, root) = live_match_app(WindowSize::OPENING.0);
        let cursors_before = app.world().resource::<GameState>().cursors;

        let help = hud_button(&mut app, HudAction::OpenHelp);
        let left = hud_button(&mut app, HudAction::PreviousHelpPage);
        let right = hud_button(&mut app, HudAction::NextHelpPage);
        let close = hud_button(&mut app, HudAction::CloseHelp);
        let focused = |app: &App| app.world().resource::<InputFocus>().get();
        let gameplay_untouched = |app: &App| {
            let game = app.world().resource::<GameState>();
            assert_eq!(game.cursors, cursors_before, "an arrow reached the cursor");
            assert_eq!(
                game.restart_hold,
                MatchDuration::ZERO,
                "Space started a restart hold"
            );
            assert_eq!(active_hold_progress(game), None);
        };

        // The pointer opens the modal from the Match HUD's own Help button:
        // a real click, through picking, on the button where it is drawn.
        crate::ui::button::click(&mut app, help);
        assert!(app.world().resource::<HudState>().help_open);
        assert!(app.world().resource::<GameplayPause>().input_blocked);
        assert_eq!(focused(&app), Some(left), "the ring seeds on Left, hidden");

        // Hold an arrow and Space, as a player resting on the keys would.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ArrowRight);
            keys.press(KeyCode::Space);
        }
        advance_test_app(&mut app, 0.1);
        let semantic = app.world().resource::<SemanticInput>();
        assert!(
            semantic.pressed(GameAction::PlayerOneRight)
                && semantic.pressed(GameAction::RestartMatch),
            "the held keys must reach semantic input, or the checks below are vacuous"
        );
        // Space activated the seed, Left, which is a no-op on page 1; the
        // arrow's movement lost to the same-frame activation.
        assert_eq!(focused(&app), Some(left));
        assert_eq!(app.world().resource::<HudState>().help_page, 0);
        assert!(app.world().resource::<HudState>().help_open);
        gameplay_untouched(&app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        for _ in 0..3 {
            advance_test_app(&mut app, 0.1);
        }
        gameplay_untouched(&app);

        // Walk the ring to Close with the keys still held, and activate it.
        let tap_while_holding = |app: &mut App, key: KeyCode| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            advance_test_app(app, 0.1);
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
        };
        tap_while_holding(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(right));
        tap_while_holding(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(close));
        gameplay_untouched(&app);
        tap_while_holding(&mut app, KeyCode::Enter);

        // The release frame: the modal is closed, gameplay is unpaused, and
        // the gate holds exactly what is still held - both players' other
        // actions are already free.
        assert!(!app.world().resource::<HudState>().help_open);
        assert!(!app.world().resource::<GameplayPause>().input_blocked);
        let gate = app.world().resource::<InputGate>();
        assert!(gate.is_gated(GameAction::RestartMatch));
        assert!(gate.is_gated(GameAction::PlayerOneRight));
        assert!(!gate.is_gated(GameAction::PlayerOneLeft));
        assert!(!gate.is_gated(GameAction::PlayerTwoSelect));
        gameplay_untouched(&app);
        for _ in 0..3 {
            advance_test_app(&mut app, 0.1);
        }
        assert!(
            app.world()
                .resource::<InputGate>()
                .is_gated(GameAction::RestartMatch)
        );
        gameplay_untouched(&app);
        assert_eq!(focused(&app), None, "nothing on the Match is left to focus");

        // Released, the gate opens; pressed again, the same arrow is a move.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::ArrowRight);
            keys.release(KeyCode::Space);
            keys.clear();
        }
        advance_test_app(&mut app, 0.1);
        assert!(!app.world().resource::<InputGate>().any_gated());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        advance_test_app(&mut app, 0.1);
        let cursor = app.world().resource::<GameState>().cursors[player_index(Player::One)];
        assert_eq!(
            cursor,
            Point::new(
                cursors_before[player_index(Player::One)].x + 1,
                cursors_before[player_index(Player::One)].y
            ),
            "gameplay is live again once the keys were released and pressed afresh"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// The keyboard's way in, under the same production graph. F1 pressed
    /// on the very frame one arrow is held, a second arrow is pressed and
    /// Select is pressed opens the modal, and gameplay reads none of the
    /// three: the cursor stays where the held arrow's own earlier press
    /// left it - the fresh arrow would have moved it back - no Round 0
    /// choice is recorded and no hold starts, because the open lands in
    /// `PreUpdate`, arms the gate, and blocks gameplay input before
    /// `MatchUpdate` runs. (A merely held arrow never moves the cursor, so
    /// the fresh press and the Select are the controls; the held one shows
    /// the gate keeps holding what was already down.) The ring arrives
    /// drawn on Left, since a key opened the modal. F1 again closes it; the
    /// keys still down stay gated until physically released, and a fresh
    /// arrow moves the cursor once they are.
    #[test]
    fn f1_opens_the_modal_during_a_match_and_no_gameplay_key_fires_that_frame() {
        use bevy::input_focus::{InputFocus, InputFocusVisible};

        use crate::gameplay::{GameplayPause, active_hold_progress};
        use crate::test_support::advance_test_app;

        let (mut app, root) = live_match_app(WindowSize::OPENING.0);
        let left = hud_button(&mut app, HudAction::PreviousHelpPage);
        let cursors_at_entry = app.world().resource::<GameState>().cursors;
        let one = player_index(Player::One);

        // Live play: the arrow's own press moves the cursor once, and it
        // stays held from here on.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        advance_test_app(&mut app, 0.1);
        let cursors_after_move = app.world().resource::<GameState>().cursors;
        assert_eq!(
            cursors_after_move[one],
            Point::new(cursors_at_entry[one].x + 1, cursors_at_entry[one].y),
            "the arrow must reach the cursor during live play, or the checks below are vacuous"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        let choices_before = held_choices(&app);
        let gameplay_untouched = |app: &App| {
            let game = app.world().resource::<GameState>();
            assert_eq!(
                game.cursors, cursors_after_move,
                "the held arrow moved the cursor"
            );
            assert_eq!(
                held_choices(app),
                choices_before,
                "Select recorded a choice"
            );
            assert_eq!(
                game.restart_hold,
                MatchDuration::ZERO,
                "a restart hold started"
            );
            assert_eq!(active_hold_progress(game), None);
        };

        // F1, Select and a fresh arrow land in one frame, with the first
        // arrow still held.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::F1);
            keys.press(KeyCode::KeyM);
            keys.press(KeyCode::ArrowLeft);
        }
        advance_test_app(&mut app, 0.1);
        let semantic = app.world().resource::<SemanticInput>();
        assert!(
            semantic.just_pressed(GameAction::PlayerOneSelect)
                && semantic.just_pressed(GameAction::PlayerOneLeft)
                && semantic.pressed(GameAction::PlayerOneRight),
            "the keys must reach semantic input, or the checks below are vacuous"
        );
        {
            let hud = app.world().resource::<HudState>();
            assert!(hud.help_open);
            assert_eq!(hud.help_page, 0);
            assert!(!hud.return_home_open);
        }
        assert!(app.world().resource::<GameplayPause>().input_blocked);
        gameplay_untouched(&app);
        {
            let gate = app.world().resource::<InputGate>();
            assert!(gate.is_gated(GameAction::PlayerOneRight));
            assert!(gate.is_gated(GameAction::PlayerOneLeft));
            assert!(gate.is_gated(GameAction::PlayerOneSelect));
        }
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(left),
            "the ring seeds on Left"
        );
        assert!(
            app.world().resource::<InputFocusVisible>().0,
            "a key opened the modal, so the ring is drawn"
        );
        assert_eq!(app.world().get::<Outline>(left).unwrap().color, ORANGE);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        for _ in 0..3 {
            advance_test_app(&mut app, 0.1);
        }
        gameplay_untouched(&app);

        // F1 again, a fresh press, closes it; the keys still down stay
        // gated, and only those.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::F1);
            keys.clear();
            keys.press(KeyCode::F1);
        }
        advance_test_app(&mut app, 0.1);
        assert!(!app.world().resource::<HudState>().help_open);
        assert!(!app.world().resource::<GameplayPause>().input_blocked);
        {
            let gate = app.world().resource::<InputGate>();
            assert!(gate.is_gated(GameAction::PlayerOneRight));
            assert!(gate.is_gated(GameAction::PlayerOneLeft));
            assert!(gate.is_gated(GameAction::PlayerOneSelect));
            assert!(!gate.is_gated(GameAction::PlayerOneUp));
            assert!(!gate.is_gated(GameAction::PlayerTwoSelect));
        }
        gameplay_untouched(&app);
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            None,
            "nothing on the Match is left to focus"
        );

        // Released, the gate opens; a fresh arrow is a move again.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::ArrowRight);
            keys.release(KeyCode::ArrowLeft);
            keys.release(KeyCode::KeyM);
            keys.release(KeyCode::F1);
            keys.clear();
        }
        advance_test_app(&mut app, 0.1);
        assert!(!app.world().resource::<InputGate>().any_gated());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowLeft);
        advance_test_app(&mut app, 0.1);
        assert_eq!(
            app.world().resource::<GameState>().cursors[one],
            cursors_at_entry[one],
            "gameplay is live again once the keys were released and pressed afresh"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn match_display_queries_initialize_without_component_conflicts() {
        let mut world = World::new();
        let mut system = IntoSystem::into_system(update_match_display);

        system.initialize(&mut world);
    }

    #[test]
    fn match_header_aligns_white_left_and_black_right() {
        assert_eq!(player_label(Player::Two), "WHITE");
        assert_eq!(player_label(Player::One), "BLACK");
        for window in NAMED_WINDOW_SIZES {
            let layout = match_layout(window);
            let (white, _) = player_header_node(Player::Two, &layout);
            let (black, _) = player_header_node(Player::One, &layout);
            assert!(matches!(white.left, Val::Px(_)), "{window}");
            assert_eq!(white.right, Val::Auto, "{window}");
            assert_eq!(black.left, Val::Auto, "{window}");
            assert!(matches!(black.right, Val::Px(_)), "{window}");
            assert_eq!(
                white.left, black.right,
                "{window}: the two mirror each other"
            );
        }
        let (white, _) = player_header_node(Player::Two, &match_layout(WindowSize::OPENING.0));
        assert_eq!(white.left, px(28), "where the heading has always stood");
    }

    /// The result in words. The centre names the winner by the colour the
    /// corners carry, each corner puts its player's score beside their own
    /// name with the two numbers facing each other, and the line under the
    /// result names every reason, in the order the Replay records them.
    /// During play the corners are the bare names.
    #[test]
    fn the_result_is_named_by_colour_with_each_score_beside_its_own_name() {
        let black_wins = Score {
            black: 24,
            white: 23,
        };
        let white_wins = Score {
            black: 19,
            white: 23,
        };
        let draw = Score {
            black: 20,
            white: 20,
        };
        assert_eq!(result_heading(black_wins), "Black Wins!");
        assert_eq!(result_heading(white_wins), "White Wins!");
        assert_eq!(result_heading(draw), "Draw!");

        assert_eq!(player_heading(Player::Two, Some(black_wins)), "WHITE 23");
        assert_eq!(player_heading(Player::One, Some(black_wins)), "24 BLACK");
        assert_eq!(player_heading(Player::Two, Some(draw)), "WHITE 20");
        assert_eq!(player_heading(Player::One, Some(draw)), "20 BLACK");
        assert_eq!(player_heading(Player::Two, None), "WHITE");
        assert_eq!(player_heading(Player::One, None), "BLACK");

        assert_eq!(end_reasons_text(&[EndReason::Manual]), "Agreed Ending");
        assert_eq!(
            end_reasons_text(&[EndReason::RoundLimit, EndReason::PointTarget]),
            "Round Limit, Point Target"
        );
        assert_eq!(
            end_reasons_text(&EVERY_AUTOMATIC_END_REASON),
            "Match Time Limit, Round Limit, No Board Change Limit, Point Target"
        );
    }

    /// Every automatic limit crossing on the same round: the longest line
    /// the end-reason row can hold, and an ending a Match can really reach -
    /// `gameplay::countdown`'s
    /// `match_time_precedes_all_simultaneous_post_round_reasons` reaches it
    /// and saves a valid Replay. `Manual` is only ever recorded alone - a
    /// completed end gesture rolls back whatever else its hold reached, and
    /// `replay::validation::end_reasons` refuses a v5 or later Replay that
    /// pairs it with anything - so this is the whole worst case.
    const EVERY_AUTOMATIC_END_REASON: [EndReason; 4] = [
        EndReason::MatchTimeLimit,
        EndReason::RoundLimit,
        EndReason::NoStoneRoundLimit,
        EndReason::PointTarget,
    ];

    /// The Match screen's top edge laid out by `bevy_ui` with the font the
    /// game draws with, its text written by the real display passes, at the
    /// end of a Match, in a window of `window` logical pixels. `prepare`
    /// sets the Match up while it is still `Playing`, and the Match is
    /// finished with `reasons`.
    fn ended_match_layout_app(
        window: Vec2,
        prepare: impl FnOnce(&mut GameState),
        reasons: &[EndReason],
    ) -> App {
        let mut game = GameState::default();
        game.phase = MatchPhase::Playing;
        prepare(&mut game);
        assert!(game.finish(reasons.to_vec()));
        match_layout_app(window, game)
    }

    /// The Match screen laid out with its text, as
    /// [`ended_match_layout_app`] describes, for any `game`, with the
    /// placement pass that keeps the furniture where the window puts it.
    /// Nothing here runs gameplay, so the Match stays exactly as it is
    /// handed in.
    fn match_layout_app(window: Vec2, game: GameState) -> App {
        use crate::ui::focus::real_layout_app;
        use crate::ui::hud::update_shared_display;

        let mut app = real_layout_app(window);
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Mesh>>();
        let palette = {
            let mut materials = app.world_mut().resource_mut::<Assets<ColorMaterial>>();
            crate::app::visuals::test_stone_palette(&mut materials)
        };
        let stone_mesh = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            StoneMesh(meshes.add(Circle::new(1.0)))
        };
        app.insert_resource(palette)
            .insert_resource(stone_mesh)
            .insert_resource(game)
            .insert_resource(SemanticInput::default())
            .insert_resource(HudState::default())
            // Both readouts on, so the column under the heading is as tall
            // as it can be.
            .insert_resource(GamePreferences {
                show_match_timer: true,
                show_round_number: true,
                ..default()
            })
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .init_resource::<WorldFraming>()
            .init_resource::<MotionPolicy>()
            .init_resource::<capture::CaptureIndication>()
            .init_resource::<opening_dots::OpeningDotRows>()
            .add_systems(
                Startup,
                (
                    spawn_match_screen,
                    capture::spawn_capture_marks,
                    opening_dots::spawn_opening_dots,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    place_match_furniture,
                    (
                        update_match_display,
                        update_shared_display,
                        opening_dots::update_opening_dots,
                    ),
                )
                    .chain(),
            );
        app.update();
        app.update();
        app
    }

    /// The line under the result is laid out only once the Match has
    /// ended. During play it takes no room: in tall the Match timer stands
    /// where it always has, directly under the heading, and when the Match
    /// ends the line appears above the timer and moves it down; in wide the
    /// timer stands beside the heading and the line appears under the
    /// heading, moving nothing. The positive half is what shows the negative
    /// one is not vacuous: in tall the same timer, in the same app, does
    /// move once there is a line to make room for, and in wide the line
    /// really is shown.
    #[test]
    fn the_end_reason_line_takes_no_room_until_the_match_has_ended() {
        use crate::ui::focus::measure_drawn_lines;
        use crate::ui::hud::HudPanel;

        let reason_display = |app: &mut App| {
            let world = app.world_mut();
            let mut panels = world.query::<(&HudPanel, &Node)>();
            let found: Vec<Display> = panels
                .iter(world)
                .filter(|(panel, _)| matches!(panel, HudPanel::EndReasons))
                .map(|(_, node)| node.display)
                .collect();
            assert_eq!(found.len(), 1, "one end-reason line");
            found[0]
        };

        for window in [WindowSize::OPENING.0, Vec2::new(1280.0, 641.0)] {
            let layout = match_layout(window);
            let timer = timer_text(0.0);
            let mut game = GameState::default();
            begin_active_match(&mut game);
            let mut app = match_layout_app(window, game);
            assert_eq!(reason_display(&mut app), Display::None, "{window}");
            let in_play = measure_drawn_lines(&mut app, &timer)[0];
            let heading = measure_drawn_lines(&mut app, "GO TOGETHER")[0];
            if layout.has_title_row() {
                assert!(
                    in_play.min.y < heading.max.y && in_play.max.y > heading.min.y,
                    "{window}: the timer stands beside the heading: {heading:?} {in_play:?}"
                );
                assert!(in_play.min.x > heading.max.x, "{window}: right of it");
            } else {
                assert!(
                    in_play.min.y - heading.max.y < 20.0,
                    "{window}: the timer sits directly under the heading: {heading:?} \
                     {in_play:?}"
                );
            }

            assert!(
                app.world_mut()
                    .resource_mut::<GameState>()
                    .finish(vec![EndReason::Manual])
            );
            app.update();
            app.update();
            assert_eq!(reason_display(&mut app), Display::Flex, "{window}");
            let reason = measure_drawn_lines(&mut app, "Agreed Ending")[0];
            let ended = measure_drawn_lines(&mut app, &timer)[0];
            let result = measure_drawn_lines(&mut app, "Draw!")[0];
            assert!(
                reason.min.y > result.max.y,
                "{window}: the reason {reason:?} stands under the result {result:?}"
            );
            if layout.has_title_row() {
                assert_eq!(ended, in_play, "{window}: the timer did not move");
            } else {
                assert!(
                    reason.max.y < ended.min.y,
                    "{window}: the reason {reason:?} stands above the timer {ended:?}"
                );
                assert!(
                    ended.min.y - in_play.min.y >= END_REASONS_SIZE,
                    "{window}: the timer made room: {in_play:?} -> {ended:?}"
                );
            }
        }
    }

    /// The worst case, measured rather than counted, at every named size.
    /// The line under the result is one line, and it stands clear of both
    /// stacks beside it - their names and their buttons; the corners, at a
    /// three-digit score, stay outside the board's width in turn; and the
    /// readouts - the Match timer and the round panel, both shown - end
    /// above the board and clear of the corners, under the heading in tall
    /// and beside it in wide.
    ///
    /// A test with one or two reasons would not find this case. When the
    /// banner was a single 34 px line, this case made it 110 characters and
    /// 2,239 px of ink in a 1,040 px window, wrapping to three lines across
    /// both corner headings and the clocks (measured on 2026-10-07 with this
    /// app, on that earlier layout). In the smallest window the line now
    /// reaches past the board's width, which is why the stacks there stand
    /// further out.
    #[test]
    fn the_longest_end_reason_line_fits_on_one_line_clear_of_both_stacks() {
        use crate::ui::focus::measure_drawn_lines;

        for window in NAMED_WINDOW_SIZES {
            let layout = match_layout(window);
            let mut app = ended_match_layout_app(
                window,
                |game| {
                    let mut rules = MatchSettings::default();
                    rules.set_capture_points(true);
                    game.match_rules = crate::gameplay::MatchRules::snapshot(&rules);
                    game.capture_totals = Score {
                        black: 456,
                        white: 789,
                    };
                },
                &EVERY_AUTOMATIC_END_REASON,
            );
            let score = app.world().resource::<GameState>().current_score();
            assert!(score.black >= 100 && score.white >= 100);
            let one_line = |app: &mut App, content: &str| {
                let lines = measure_drawn_lines(app, content);
                assert_eq!(lines.len(), 1, "{window}: {content:?} wrapped: {lines:?}");
                lines[0]
            };

            let heading = one_line(&mut app, &result_heading(score));
            let reasons = one_line(&mut app, &end_reasons_text(&EVERY_AUTOMATIC_END_REASON));
            let left = one_line(&mut app, &player_heading(Player::Two, Some(score)));
            let right = one_line(&mut app, &player_heading(Player::One, Some(score)));
            let round = one_line(&mut app, "Round 0");
            let timer = one_line(&mut app, &timer_text(0.0));
            let buttons = [
                hud_button_rect(&mut app, HudAction::OpenReturnHome),
                hud_button_rect(&mut app, HudAction::OpenHelp),
            ];

            let board = layout.board;
            assert!(
                heading.max.y < reasons.min.y,
                "{window}: the reasons overlap the result"
            );
            for corner in [left, right].into_iter().chain(buttons) {
                for line in [reasons, round, timer] {
                    assert!(
                        line.intersect(corner).is_empty(),
                        "{window}: {line:?} meets a corner {corner:?}"
                    );
                }
            }
            assert!(
                left.max.x < board.min.x && right.min.x > board.max.x,
                "{window}: a corner heading reaches into the board's width: {left:?} {right:?}"
            );
            for readout in [round, timer, reasons] {
                assert!(
                    readout.max.y < board.min.y,
                    "{window}: {readout:?} runs into the board at {}",
                    board.min.y
                );
            }
        }
    }

    /// The box of the one button carrying `action`, in logical pixels from
    /// the window's top-left, as `bevy_ui` laid it out.
    fn hud_button_rect(app: &mut App, action: HudAction) -> Rect {
        use bevy::ui::{ComputedNode, UiGlobalTransform};

        let entity = hud_button(app, action);
        let world = app.world();
        let node = world.get::<ComputedNode>(entity).unwrap();
        let transform = world.get::<UiGlobalTransform>(entity).unwrap();
        let scale = node.inverse_scale_factor;
        Rect::from_center_size(transform.translation * scale, node.size * scale)
    }

    /// The Match in one of the states it is drawn in, with the longest text
    /// each place has room for: a four-digit round with an hour of Round
    /// Time left and the timer at 999:59 during play, every end reason and
    /// three-digit scores with the Replay-save notice at the end. The two
    /// states "past the budgets" go a digit past each of those - round
    /// 10,000, 1,000 minutes, four-digit scores - which a Match reaches only
    /// with the limit that would have ended it switched off.
    fn worst_case_match(state: &str) -> GameState {
        let mut game = GameState::default();
        match state {
            "Round 0" => {}
            "countdown" => {
                game.phase = MatchPhase::OpeningCountdown;
                game.countdown_elapsed = MatchDuration::ZERO;
            }
            "play" | "play, hints dismissed" | "play, past the budgets" => {
                let mut rules = MatchSettings::default();
                rules.set_enabled(MatchSettingField::RoundTime, true);
                rules.set_value(MatchSettingField::RoundTime, 3_600);
                game.match_rules = crate::gameplay::MatchRules::snapshot(&rules);
                begin_active_match(&mut game);
                game.round = 9_999;
                game.elapsed = crate::clock::MatchInstant::from_secs(999 * 60 + 59);
                if state == "play, past the budgets" {
                    game.round = 10_000;
                    game.elapsed = crate::clock::MatchInstant::from_secs(1_000 * 60);
                }
                if state == "play, hints dismissed" {
                    game.control_hints_visible = [false, false];
                }
            }
            "end" | "end, past the budgets" => {
                let mut rules = MatchSettings::default();
                rules.set_capture_points(true);
                game.match_rules = crate::gameplay::MatchRules::snapshot(&rules);
                game.phase = MatchPhase::Playing;
                game.capture_totals = Score {
                    black: 456,
                    white: 789,
                };
                game.elapsed = crate::clock::MatchInstant::from_secs(999 * 60 + 59);
                if state == "end, past the budgets" {
                    game.round = 10_000;
                    game.capture_totals = Score {
                        black: 1_456,
                        white: 1_789,
                    };
                    game.elapsed = crate::clock::MatchInstant::from_secs(1_000 * 60);
                }
                assert!(game.finish(EVERY_AUTOMATIC_END_REASON.to_vec()));
                game.replay_notice = crate::gameplay::REPLAY_SAVE_FAILED_NOTICE.into();
            }
            _ => unreachable!("{state}"),
        }
        game
    }

    /// The real-layout half of `geometry`'s sweep. At every named size, in
    /// every state a Match is drawn in, with the longest text each place
    /// can hold, nothing the screen draws meets anything else and nothing
    /// leaves the window - measured from what `bevy_ui` laid out: glyphs
    /// for text, and boxes for the buttons, the control-hint cards, the
    /// Round 0 dot rows, the countdown card and the bars; and the board,
    /// where the camera's framing draws it. A text inside one of those
    /// boxes is that box's, and is not counted again. The countdown card is
    /// the one thing meant to stand over another, the board, and it stands
    /// inside it. The capture counts, which only their own pass writes, are
    /// written here, longest first. Two states go a digit past every text
    /// budget the layout keeps, and stand clear all the same: the timer
    /// wraps in its slot, and the rest grow into the room kept beside them.
    #[test]
    fn nothing_the_match_screen_draws_meets_anything_else_at_the_named_sizes() {
        use super::geometry::StackItem;
        use super::layout::MatchPlacement;
        use crate::ui::focus::drawn_texts;
        use crate::ui::hud::HudPanel;
        use bevy::ui::{ComputedNode, UiGlobalTransform};

        for window in NAMED_WINDOW_SIZES {
            for state in [
                "Round 0",
                "countdown",
                "play",
                "play, hints dismissed",
                "end",
                "play, past the budgets",
                "end, past the budgets",
            ] {
                let mut app = match_layout_app(window, worst_case_match(state));
                {
                    let world = app.world_mut();
                    let mut counts = world.query::<(&MatchPlacement, &mut Text)>();
                    for (placement, mut text) in counts.iter_mut(world) {
                        if let MatchPlacement::Stack(player, StackItem::CaptureCount) = placement {
                            text.0 = format!("Captured {}", 80 + player.index());
                        }
                    }
                }
                app.update();
                app.update();
                let context = format!("{window} {state}");

                // The boxes that draw something of their own: a button, a
                // card, a row of dots, a bar. A closed modal's are laid out
                // at no size, so only the screen's own are found.
                let mut boxes: Vec<(String, Entity, Rect)> = Vec::new();
                {
                    let world = app.world_mut();
                    let countdown_rows: Vec<Entity> = world
                        .query::<(Entity, &HudPanel)>()
                        .iter(world)
                        .filter(|(_, panel)| matches!(panel, HudPanel::Countdown))
                        .map(|(entity, _)| entity)
                        .collect();
                    let mut nodes = world.query::<(
                        Entity,
                        &ComputedNode,
                        &UiGlobalTransform,
                        &InheritedVisibility,
                        Option<&HudAction>,
                        Option<&ControlHint>,
                        Option<&MatchPlacement>,
                        Option<&ChildOf>,
                        Has<HoldProgressTrack>,
                        Has<RoundTimeTrack>,
                    )>();
                    for (
                        entity,
                        node,
                        transform,
                        visibility,
                        action,
                        hint,
                        placement,
                        parent,
                        hold_bar,
                        round_bar,
                    ) in nodes.iter(world)
                    {
                        if !visibility.get() || node.size() == Vec2::ZERO {
                            continue;
                        }
                        let in_countdown =
                            parent.is_some_and(|parent| countdown_rows.contains(&parent.parent()));
                        let label = match (action, hint, placement) {
                            (Some(action), ..) => format!("{action:?} button"),
                            (_, Some(hint), _) => format!("{:?}'s control hints", hint.0),
                            (_, _, Some(MatchPlacement::Stack(player, StackItem::OpeningDots))) => {
                                format!("{player:?}'s dots")
                            }
                            _ if in_countdown => "countdown card".into(),
                            _ if hold_bar => "hold bar".into(),
                            _ if round_bar => "round-time bar".into(),
                            _ => continue,
                        };
                        let scale = node.inverse_scale_factor;
                        let rect = Rect::from_center_size(
                            transform.translation * scale,
                            node.size * scale,
                        );
                        boxes.push((label, entity, rect));
                    }
                }

                let inside_a_box = |app: &App, mut entity: Entity| loop {
                    if boxes.iter().any(|(_, owner, _)| *owner == entity) {
                        break true;
                    }
                    match app.world().get::<ChildOf>(entity) {
                        Some(parent) => entity = parent.parent(),
                        None => break false,
                    }
                };
                let mut items: Vec<(String, Rect)> = boxes
                    .iter()
                    .map(|(label, _, rect)| (label.clone(), *rect))
                    .collect();
                for (entity, text, ink) in drawn_texts(&mut app) {
                    if !inside_a_box(&app, entity) {
                        items.push((format!("{text:?}"), ink));
                    }
                }
                // The board's wood, where the framing the layout pass wrote
                // has the camera draw it.
                let framing = *app.world().resource::<WorldFraming>();
                let (centre, half) = (
                    Vec2::new(0.0, BOARD_CENTER_Y),
                    Vec2::splat(BOARD_OUTER_SIZE / 2.0),
                );
                let board = Rect::from_corners(
                    framing.window_point(centre - half, window),
                    framing.window_point(centre + half, window),
                );
                items.push(("board".into(), board));
                // Both buttons always; both control-hint cards until they are
                // dismissed; both dot rows in Round 0; the countdown card in
                // the countdown; and the round-time bar while Round Time
                // runs.
                let expected_boxes = match state {
                    "Round 0" => 6,
                    "countdown" => 7,
                    "play" | "play, past the budgets" => 5,
                    "play, hints dismissed" => 3,
                    _ => 4,
                };
                assert_eq!(boxes.len(), expected_boxes, "{context}: {boxes:?}");
                assert!(items.len() >= expected_boxes + 7, "{context}: {items:?}");

                let inside = Rect::from_corners(Vec2::ZERO, window);
                for (label, rect) in &items {
                    assert!(
                        inside.contains(rect.min) && inside.contains(rect.max),
                        "{context}: {label} at {rect:?} leaves the window"
                    );
                }
                let mut card_over_board = false;
                for (index, (label, rect)) in items.iter().enumerate() {
                    for (other, other_rect) in &items[index + 1..] {
                        let pair = [label.as_str(), other.as_str()];
                        if pair.contains(&"board") && pair.contains(&"countdown card") {
                            let card = if label == "board" { other_rect } else { rect };
                            assert!(
                                board.contains(card.min) && board.contains(card.max),
                                "{context}: the countdown card {card:?} stands over the board \
                                 {board:?}"
                            );
                            card_over_board = true;
                            continue;
                        }
                        assert!(
                            rect.intersect(*other_rect).is_empty(),
                            "{context}: {label} {rect:?} meets {other} {other_rect:?}"
                        );
                    }
                }
                assert_eq!(card_over_board, state == "countdown", "{context}");
            }
        }
    }

    /// In a wide window the round panel and the Match timer stand beside the
    /// heading, centred down its line whether the round panel is one line -
    /// no Round Time - or two, with its countdown under it; under the
    /// heading in the opening window. Measured by node, which for text is the
    /// line box the layout centres.
    #[test]
    fn the_readouts_beside_the_heading_stand_centred_on_its_line() {
        use crate::ui::focus::laid_out_texts;

        for window in [
            WindowSize::OPENING.0,
            WindowSize::MINIMUM.0,
            Vec2::new(1280.0, 641.0),
            Vec2::new(1920.0, 1080.0),
        ] {
            let layout = match_layout(window);
            for round_time in [false, true] {
                let mut game = GameState::default();
                if round_time {
                    let mut rules = MatchSettings::default();
                    rules.set_enabled(MatchSettingField::RoundTime, true);
                    rules.set_value(MatchSettingField::RoundTime, 45);
                    game.match_rules = crate::gameplay::MatchRules::snapshot(&rules);
                }
                begin_active_match(&mut game);
                let round =
                    round_panel_text(1, game.round_time_remaining_seconds(), layout.round_lines);
                let mut app = match_layout_app(window, game);
                let texts = laid_out_texts(&mut app);
                let find = |content: &str| {
                    texts
                        .iter()
                        .find(|(text, _)| text == content)
                        .unwrap_or_else(|| panic!("{window}: no {content:?} in {texts:?}"))
                        .1
                };
                let heading = find("GO TOGETHER");
                let (round, timer) = (find(&round), find(&timer_text(0.0)));
                let context = format!("{window}, Round Time {round_time}");
                if layout.has_title_row() {
                    for readout in [round, timer] {
                        assert!(
                            (readout.center().y - heading.center().y).abs() < 1.0,
                            "{context}: {readout:?} beside {heading:?}"
                        );
                    }
                    assert_eq!(
                        round_time && layout.round_lines == Lines::Two,
                        round.height() > 30.0
                    );
                } else {
                    // Under the heading, their line boxes meeting by no more
                    // than the leading, as they always have.
                    assert!(
                        timer.min.y >= heading.max.y - 1.0,
                        "{context}: {heading:?} {timer:?}"
                    );
                    assert!(round.min.y > timer.max.y, "{context}");
                }
            }
        }
    }

    /// Every placed node on the Match screen, with its place, in an order
    /// that does not depend on the ECS, and how the round panel's text is
    /// justified, which the placement pass sets too.
    fn placed_nodes(app: &mut App) -> (Vec<(String, Node)>, Justify) {
        use super::layout::MatchPlacement;

        let world = app.world_mut();
        let mut nodes: Vec<(String, Node)> = world
            .query::<(&MatchPlacement, &Node)>()
            .iter(world)
            .map(|(placement, node)| (format!("{placement:?}"), node.clone()))
            .collect();
        nodes.sort_by(|a, b| a.0.cmp(&b.0));
        let justify: Vec<Justify> = world
            .query::<(&UiRole, &TextLayout)>()
            .iter(world)
            .filter(|(role, _)| matches!(role, UiRole::Round))
            .map(|(_, layout)| layout.justify)
            .collect();
        assert_eq!(justify.len(), 1, "one round panel");
        (nodes, justify[0])
    }

    /// When the window's size changes mid-Match, the furniture moves to
    /// exactly where a Match entered at the new size would have it, and the
    /// camera frames the board anew; back in the opening window every node
    /// is again the one the opening window spawns, so wide leaves nothing
    /// behind. The positive half: the wide nodes really are different.
    #[test]
    fn a_change_of_window_size_moves_the_match_and_moving_back_restores_it() {
        let laptop = Vec2::new(1280.0, 641.0);
        let in_play = || {
            let mut game = GameState::default();
            begin_active_match(&mut game);
            game
        };
        let mut app = match_layout_app(WindowSize::OPENING.0, in_play());
        let opening = placed_nodes(&mut app);
        // The heading, the column under it and its three lines, five items
        // in each stack, the countdown and the overlay under the board.
        assert_eq!(opening.0.len(), 17);
        assert_eq!(opening.1, Justify::Left, "as the round panel always was");
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            WorldFraming::IDENTITY
        );

        app.insert_resource(WindowSize(laptop));
        app.update();
        let moved = placed_nodes(&mut app);
        assert_ne!(moved.0, opening.0);
        assert_eq!(moved.1, Justify::Right, "against the heading beside it");
        let mut fresh = match_layout_app(laptop, in_play());
        assert_eq!(moved, placed_nodes(&mut fresh));
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            match_layout(laptop).framing()
        );

        app.insert_resource(WindowSize::OPENING);
        app.update();
        assert_eq!(placed_nodes(&mut app), opening);
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            WorldFraming::IDENTITY
        );
    }

    /// The control-hint cards stand on their own players' sides - White's
    /// from the left edge, Black's from the right, mirroring each other -
    /// at least their gap from the board, at every size.
    #[test]
    fn control_hints_align_white_left_and_black_right_without_board_overlap() {
        for window in layout_sweep() {
            let layout = match_layout(window);
            let (white, _) = control_hint_node(Player::Two, &layout);
            let (black, _) = control_hint_node(Player::One, &layout);
            assert_eq!(white.right, Val::Auto, "{window}");
            assert_eq!(black.left, Val::Auto, "{window}");
            assert_eq!(
                white.left, black.right,
                "{window}: the two mirror each other"
            );
            let (Val::Px(left), Val::Px(right)) = (white.left, black.right) else {
                panic!("{window}: placed in pixels from the sides");
            };
            assert!(left >= 12.0 && right >= 12.0, "{window}");
            assert!(
                left + CONTROL_HINT_WIDTH + STACK_GAP <= layout.board.min.x + 1e-3,
                "{window}"
            );
            assert!(
                window.x - right - CONTROL_HINT_WIDTH - STACK_GAP >= layout.board.max.x - 1e-3,
                "{window}"
            );
        }
        let (white, _) = control_hint_node(Player::Two, &match_layout(WindowSize::OPENING.0));
        assert_eq!(white.left, px(18), "where the hints have always stood");
    }

    #[test]
    fn control_hint_copy_uses_current_bindings_and_only_move_and_select() {
        let defaults = KeyboardBindings::default();
        assert_eq!(
            control_hint_text(Player::Two, &defaults),
            "WHITE / P2\nUp: E\nLeft: S\nDown: D\nRight: F\nSelect: 2"
        );
        assert_eq!(
            control_hint_text(Player::One, &defaults),
            "BLACK / P1\nUp: Up\nLeft: Left\nDown: Down\nRight: Right\nSelect: M"
        );
        assert!(!control_hint_text(Player::One, &defaults).contains("Arrow"));

        let mut remapped = defaults;
        remapped.assign(GameAction::PlayerTwoUp, KeyboardKey::F8);
        remapped.assign(GameAction::PlayerTwoSelect, KeyboardKey::F9);
        let hint = control_hint_text(Player::Two, &remapped);
        assert!(hint.contains("Up: F8"));
        assert!(hint.contains("Select: F9"));
        for excluded in ["Teleport", "Marker", "Restart", "Timing", "Rules", "End"] {
            assert!(!hint.contains(excluded));
        }
    }

    #[test]
    fn end_control_hint_states_the_hold_and_uses_current_keys_and_player_order() {
        let mut bindings = KeyboardBindings::default();
        assert_eq!(
            end_control_hint(&bindings),
            "Hold M and 2 for 3 seconds to end the match"
        );

        bindings.assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        bindings.assign(GameAction::PlayerTwoSelect, KeyboardKey::F9);
        assert_eq!(
            end_control_hint(&bindings),
            "Hold F8 and F9 for 3 seconds to end the match"
        );

        bindings.assign(GameAction::PlayerTwoSelect, KeyboardKey::Backtick);
        assert_eq!(
            end_control_hint(&bindings),
            "Hold F8 and Backtick for 3 seconds to end the match"
        );
    }

    #[test]
    fn end_control_hint_waits_for_both_onboarding_hints_and_select_release() {
        let keyboard = KeyboardBindings::default();
        let controller = ControllerBindings::default();
        let assignments = ControllerAssignments::default();
        let mut game = GameState::default();
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();
        let released = SemanticInput::default();
        let text = |game: &GameState, semantic: &SemanticInput, hud: &HudState| {
            gesture_text(game, semantic, hud, &keyboard, &controller, &assignments)
        };

        begin_active_match(&mut game);
        assert_eq!(game.control_hints_visible, [true, true]);
        assert_eq!(text(&game, &released, &hud), "");

        game.control_hints_visible = [false, true];
        assert_eq!(text(&game, &released, &hud), "");
        game.control_hints_visible = [true, false];
        assert_eq!(text(&game, &released, &hud), "");

        game.control_hints_visible = [false, false];
        let second_select_held = SemanticInput::from_pressed(&[GameAction::PlayerTwoSelect]);
        assert_eq!(text(&game, &second_select_held, &hud), "");
        assert_eq!(
            text(&game, &released, &hud),
            "Hold M and 2 for 3 seconds to end the match"
        );

        initialize_match_state(&mut game, &mut input_gate, &MatchSettings::default());
        hud = HudState::default();
        begin_active_match(&mut game);
        assert_eq!(game.control_hints_visible, [true, true]);
        assert_eq!(text(&game, &released, &hud), "");
    }

    #[test]
    fn end_control_hint_is_active_only_and_yields_to_modal_and_progress_ui() {
        let keyboard = KeyboardBindings::default();
        let controller = ControllerBindings::default();
        let assignments = ControllerAssignments::default();
        let mut game = GameState::default();
        let mut hud = HudState::default();
        let released = SemanticInput::default();
        let text = |game: &GameState, semantic: &SemanticInput, hud: &HudState| {
            gesture_text(game, semantic, hud, &keyboard, &controller, &assignments)
        };

        assert_eq!(text(&game, &released, &hud), "");
        game.phase = MatchPhase::OpeningCountdown;
        game.control_hints_visible = [false, false];
        assert_eq!(text(&game, &released, &hud), "");

        begin_active_match(&mut game);
        assert_eq!(
            text(&game, &released, &hud),
            "Hold M and 2 for 3 seconds to end the match"
        );
        for action in [
            GameAction::PlayerOneSelect,
            GameAction::PlayerTwoSelect,
            GameAction::RestartMatch,
        ] {
            let pressed = SemanticInput::from_pressed(&[action]);
            assert_eq!(text(&game, &pressed, &hud), "");
        }

        game.running_hold = Some(HoldGesture::End);
        game.end_hold = MatchDuration::from_secs_f64(0.5);
        let both_selects = SemanticInput::from_pressed(&[
            GameAction::PlayerOneSelect,
            GameAction::PlayerTwoSelect,
        ]);
        assert_eq!(text(&game, &both_selects, &hud), "Ending Match");
        assert_eq!(
            visible_hold_progress(&game, &hud),
            Some((
                "Ending Match",
                (0.5 / crate::gameplay::HOLD_DURATION.as_secs_f64()) as f32
            ))
        );

        // Both holds non-zero: the gesture that is running is the one named -
        // `active_hold_progress`'s decision, not this module's, which is why
        // the label followed the priority flip and now follows the drain. A
        // restart hold running while a released end hold drains is the case
        // the drain created: the end hold is the larger timer, and Restart is
        // what will fire.
        game.restart_hold = MatchDuration::from_secs_f64(0.75);
        assert_eq!(text(&game, &both_selects, &hud), "Ending Match");
        // The label ignores the semantic input; the swap to Restart alone is
        // for realism, since a restart hold with both Selects down and
        // ungated cannot happen during play any more.
        let restart_only = SemanticInput::from_pressed(&[GameAction::RestartMatch]);
        game.running_hold = Some(HoldGesture::Restart);
        assert_eq!(text(&game, &restart_only, &hud), "Restarting Match");
        // Nothing running, the end hold draining: still named, so the player
        // sees the progress they are losing while they can still act on it.
        game.running_hold = None;
        game.restart_hold = MatchDuration::ZERO;
        assert_eq!(text(&game, &released, &hud), "Ending Match");
        game.end_hold = MatchDuration::ZERO;
        assert_eq!(
            text(&game, &released, &hud),
            "Hold M and 2 for 3 seconds to end the match"
        );

        // A modal blanks the line and hides the bar whatever gameplay reports
        // - this module's own rule, stated here with a running restart hold
        // although gameplay reads every gesture as not running under a
        // modal, so that the check is seen to do something.
        hud.help_open = true;
        game.running_hold = Some(HoldGesture::Restart);
        game.restart_hold = MatchDuration::from_secs_f64(0.5);
        assert_eq!(text(&game, &restart_only, &hud), "");
        assert_eq!(visible_hold_progress(&game, &hud), None);
        hud.help_open = false;
        hud.return_home_open = true;
        assert_eq!(text(&game, &restart_only, &hud), "");
        assert_eq!(visible_hold_progress(&game, &hud), None);
        hud.return_home_open = false;
        game.running_hold = None;
        game.restart_hold = MatchDuration::ZERO;
        game.phase = MatchPhase::Ended;
        let ended_text = text(&game, &released, &hud);
        assert!(ended_text.starts_with("Hold Space for 3 seconds to restart"));
        assert!(!ended_text.contains("to end the match"));
        game.running_hold = Some(HoldGesture::Restart);
        game.restart_hold = MatchDuration::from_secs_f64(0.75);
        assert_eq!(text(&game, &released, &hud), "Restarting Match");
    }

    #[test]
    fn match_ui_spawns_one_gesture_entity_and_cleans_it_with_the_screen() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_systems(OnEnter(AppScreen::Match), |mut commands: Commands| {
                spawn_ui(&mut commands, &match_layout(WindowSize::OPENING.0))
            });
        app.update();
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Match);
        app.update();

        let hints = app
            .world_mut()
            .query::<(&ControlHint, &DespawnOnExit<AppScreen>)>()
            .iter(app.world())
            .map(|(hint, screen)| (hint.0, screen.0))
            .collect::<Vec<_>>();
        assert_eq!(hints.len(), 2);
        assert!(hints.contains(&(Player::Two, AppScreen::Match)));
        assert!(hints.contains(&(Player::One, AppScreen::Match)));
        assert_eq!(
            app.world_mut()
                .query::<&UiRole>()
                .iter(app.world())
                .filter(|role| matches!(role, UiRole::Gesture))
                .count(),
            1
        );

        // Leaving the screen despawns everything tagged for it automatically -
        // no cleanup system is registered here at all.
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Home);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&ControlHint>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world_mut()
                .query::<&UiRole>()
                .iter(app.world())
                .filter(|role| matches!(role, UiRole::Gesture))
                .count(),
            0
        );
    }

    /// Both players' headings are in their accents at 24px, the size from
    /// which text owes its background 3:1 rather than 4.5:1 - a bar Player
    /// One's red meets, at 3.97:1, and at the old 19px did not. Both control
    /// hints are written in `TEXT_MUTED` inside a frame in the accent, for
    /// the same reason at 14px. The two players are drawn alike throughout,
    /// so neither heading nor hint is the odd one out.
    #[test]
    fn player_headings_are_large_text_and_hints_are_muted_in_an_accent_frame() {
        use crate::presentation::TEXT_MUTED;

        let mut world = World::new();
        spawn_ui(&mut world.commands(), &match_layout(WindowSize::OPENING.0));
        world.flush();

        for player in Player::ALL {
            let accent = cursor_visual_style(player).color;
            let heading = world
                .query::<(&UiRole, &TextFont, &TextColor)>()
                .iter(&world)
                .find(|(role, ..)| match player {
                    Player::One => matches!(role, UiRole::PlayerOne),
                    Player::Two => matches!(role, UiRole::PlayerTwo),
                })
                .map(|(_, font, color)| (font.font_size, color.0))
                .expect("each player has a heading");
            let FontSize::Px(size) = heading.0 else {
                panic!("{player:?}'s heading is not sized in pixels");
            };
            assert_eq!(size, 24.0, "{player:?}'s heading is large text");
            assert_eq!(heading.1, accent, "{player:?}'s heading is in their accent");

            let hint_text = world
                .query::<(&UiRole, &TextColor)>()
                .iter(&world)
                .find(|(role, _)| matches!(role, UiRole::ControlHint(owner) if *owner == player))
                .map(|(_, color)| color.0)
                .expect("each player has a control hint");
            assert_eq!(hint_text, TEXT_MUTED, "{player:?}'s hint text");
            let frame = world
                .query::<(&ControlHint, &BorderColor)>()
                .iter(&world)
                .find(|(hint, _)| hint.0 == player)
                .map(|(_, border)| *border)
                .expect("each player's hint has a frame");
            assert_eq!(frame, BorderColor::all(accent), "{player:?}'s hint frame");
        }
    }

    #[test]
    fn hold_progress_track_has_bright_fixed_outline_and_center_out_geometry() {
        let track = hold_progress_track_node();
        let outline = hold_progress_track_outline();
        assert_eq!(track.width, px(360));
        assert_eq!(track.height, px(12));
        assert_eq!(track.justify_content, JustifyContent::Center);
        assert_eq!(track.align_items, AlignItems::Center);
        for fraction in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(bar_fill_width(fraction), Val::Percent(fraction * 100.0));
        }
        assert_eq!(outline.width, px(3));
        assert_eq!(outline.offset, px(0));
        assert_eq!(outline.color, Color::srgb(0.98, 0.86, 0.66));
        assert_eq!(HOLD_PROGRESS_OUTLINE_COLOR, TEXT_HEADING);
        assert_eq!(HOLD_PROGRESS_FILL_COLOR, ORANGE);
        assert_eq!(ORANGE, Color::srgb(0.95, 0.56, 0.24));
        assert_ne!(ORANGE, BLACK_P1_RED);
        assert_ne!(ORANGE, WHITE_P2_BLUE);

        // Every `.rs` file in this directory, read from disk at test time rather
        // than from a hand-written `include_str!` list - see
        // `crate::architecture` for why a list cannot guard a negative
        // assertion, and why the count is a floor the walk must reach.
        let source = sources_in("ui/match_screen", 2);
        // The fragments below are the pre-compaction hold captions: uppercase
        // progress text with an ellipsis, an elapsed-against-three-seconds
        // count, and a parenthetical prompt naming the control to hold on. The
        // fixed labels from `gameplay::holds::active_hold_progress` plus the
        // bar measured above replaced them, so none may come back.
        for removed in [
            ["ENDING MATCH", "..."].concat(),
            ["RESTARTING", "..."].concat(),
            ["/ 3.0", " seconds"].concat(),
            ["keep ", "holding"].concat(),
        ] {
            assert!(!source.contains(&removed));
        }
    }

    /// WCAG relative-luminance contrast, `(lighter + 0.05) / (darker + 0.05)`.
    /// Bevy's [`Luminance`] is the same Rec. 709 weighting over linear light.
    fn contrast_ratio(a: Color, b: Color) -> f32 {
        let (a, b) = (a.luminance(), b.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn round_time_bar_shares_the_hold_bars_slot_and_shrinks_toward_the_centre() {
        let track = round_time_track_node();
        let hold = hold_progress_track_node();
        let outline = round_time_track_outline();

        // Same slot, same shape: swapping one bar for the other moves nothing
        // around it, and the centre justification is what makes a shrinking
        // fill shrink from both ends at once.
        assert_eq!(track.width, hold.width);
        assert_eq!(track.height, hold.height);
        assert_eq!(track.justify_content, JustifyContent::Center);
        assert_eq!(track.align_items, AlignItems::Center);
        assert_eq!(track.display, Display::None);
        assert_eq!(outline.width, hold_progress_track_outline().width);
        assert_eq!(outline.offset, px(0));

        // Different colours from the hold bar, in both fill and outline, and
        // no player's accent. The bar's length is its meaning; the colour
        // only has to be visible, and the fill must read against the track
        // at 3:1 or better - the floor for a non-text element.
        assert_eq!(ROUND_TIME_FILL_COLOR, TEXT_HEADING);
        assert_eq!(ROUND_TIME_OUTLINE_COLOR, TEXT_SUBTLE);
        assert_eq!(outline.color, ROUND_TIME_OUTLINE_COLOR);
        assert_ne!(ROUND_TIME_FILL_COLOR, HOLD_PROGRESS_FILL_COLOR);
        assert_ne!(ROUND_TIME_OUTLINE_COLOR, HOLD_PROGRESS_OUTLINE_COLOR);
        for accent in [BLACK_P1_RED, WHITE_P2_BLUE] {
            assert_ne!(ROUND_TIME_FILL_COLOR, accent);
            assert_ne!(ROUND_TIME_OUTLINE_COLOR, accent);
        }
        let fill_contrast = contrast_ratio(ROUND_TIME_FILL_COLOR, DEEP_SURFACE_BACKGROUND);
        assert!(fill_contrast >= 3.0, "fill contrast {fill_contrast}");
        assert!(fill_contrast >= 13.0, "the doc comment claims about 14:1");
        let outline_contrast = contrast_ratio(
            ROUND_TIME_OUTLINE_COLOR,
            crate::presentation::APP_BACKGROUND,
        );
        assert!(
            outline_contrast >= 3.0,
            "outline contrast {outline_contrast}"
        );
    }

    #[test]
    fn a_running_hold_owns_the_bottom_slot_and_the_round_clock_has_it_otherwise() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundTime, 60);
        settings.set_enabled(MatchSettingField::RoundTime, true);
        let mut game = GameState::with_match_settings(&settings);
        let mut hud = HudState::default();

        // Round 0 has no round clock and no bar of either kind.
        assert_eq!(bottom_bar_fractions(&game, &hud), (None, None));

        begin_active_match(&mut game);
        assert_eq!(bottom_bar_fractions(&game, &hud), (None, Some(1.0)));
        game.round_elapsed = MatchDuration::from_secs_f64(45.0);
        assert_eq!(bottom_bar_fractions(&game, &hud), (None, Some(0.25)));

        // A hold takes the slot for exactly as long as it runs - or, for the
        // end hold, drains - at whatever fraction `active_hold_progress`
        // reports. Which hold that is, and what it outranks, is that
        // function's decision and is deferred to, not repeated here.
        game.running_hold = Some(HoldGesture::End);
        game.end_hold = MatchDuration::from_secs_f64(0.5);
        let (label, hold_fraction) = active_hold_progress(&game).unwrap();
        assert_eq!(label, "Ending Match");
        assert_eq!(
            bottom_bar_fractions(&game, &hud),
            (Some(hold_fraction), None)
        );
        game.running_hold = Some(HoldGesture::Restart);
        game.restart_hold = MatchDuration::from_secs_f64(0.75);
        let (label, hold_fraction) = active_hold_progress(&game).unwrap();
        assert_eq!(label, "Restarting Match");
        assert_eq!(
            bottom_bar_fractions(&game, &hud),
            (Some(hold_fraction), None)
        );
        game.running_hold = None;
        game.restart_hold = MatchDuration::ZERO;
        let (label, hold_fraction) = active_hold_progress(&game).unwrap();
        assert_eq!(
            label, "Ending Match",
            "a released end hold drains in the slot"
        );
        assert_eq!(
            bottom_bar_fractions(&game, &hud),
            (Some(hold_fraction), None)
        );
        game.end_hold = MatchDuration::ZERO;
        assert_eq!(bottom_bar_fractions(&game, &hud), (None, Some(0.25)));

        // A modal hides a hold but not the round bar, which the overlay
        // covers anyway and which is still true when it closes.
        game.running_hold = Some(HoldGesture::Restart);
        game.restart_hold = MatchDuration::from_secs_f64(0.5);
        hud.help_open = true;
        assert_eq!(bottom_bar_fractions(&game, &hud), (None, Some(0.25)));
        hud.help_open = false;
        game.running_hold = None;
        game.restart_hold = MatchDuration::ZERO;

        // No round clock, no round bar: after scoring, and in a Match whose
        // Round Time is off - a hold still draws in both.
        game.phase = MatchPhase::Ended;
        assert_eq!(bottom_bar_fractions(&game, &hud), (None, None));
        game.running_hold = Some(HoldGesture::Restart);
        game.restart_hold = MatchDuration::from_secs_f64(1.5);
        assert_eq!(bottom_bar_fractions(&game, &hud), (Some(0.5), None));

        let mut untimed = GameState::default();
        begin_active_match(&mut untimed);
        assert_eq!(bottom_bar_fractions(&untimed, &hud), (None, None));
        untimed.end_hold = MatchDuration::from_secs_f64(1.5);
        assert_eq!(bottom_bar_fractions(&untimed, &hud), (Some(0.5), None));
    }

    /// The wiring, not the decision: the display pass really moves the two
    /// bars' nodes the way [`bottom_bar_fractions`] says, and never shows both.
    #[test]
    fn match_display_draws_the_round_bar_and_yields_it_to_a_hold() {
        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundTime, 60);
        settings.set_enabled(MatchSettingField::RoundTime, true);
        let mut game = GameState::with_match_settings(&settings);
        begin_active_match(&mut game);
        let mut app = match_display_test_app();
        app.insert_resource(game);

        /// A bar's `(track display, fill width)`, read back from its nodes.
        fn bar_state<Track: Component, Fill: Component>(app: &mut App) -> (Display, Val) {
            let world = app.world_mut();
            let display = world
                .query_filtered::<&Node, With<Track>>()
                .single(world)
                .expect("exactly one track")
                .display;
            let width = world
                .query_filtered::<&Node, With<Fill>>()
                .single(world)
                .expect("exactly one fill")
                .width;
            (display, width)
        }
        fn bars(app: &mut App) -> ((Display, Val), (Display, Val)) {
            (
                bar_state::<HoldProgressTrack, HoldProgressFill>(app),
                bar_state::<RoundTimeTrack, RoundTimeFill>(app),
            )
        }

        app.update();
        assert_eq!(
            bars(&mut app),
            (
                (Display::None, Val::Percent(0.0)),
                (Display::Flex, Val::Percent(100.0))
            )
        );

        app.world_mut().resource_mut::<GameState>().round_elapsed =
            MatchDuration::from_secs_f64(30.0);
        app.update();
        assert_eq!(bars(&mut app).1, (Display::Flex, Val::Percent(50.0)));

        app.world_mut().resource_mut::<GameState>().end_hold = MatchDuration::from_secs_f64(1.5);
        app.update();
        assert_eq!(
            bars(&mut app),
            (
                (Display::Flex, Val::Percent(50.0)),
                (Display::None, Val::Percent(50.0))
            )
        );

        app.world_mut().resource_mut::<GameState>().end_hold = MatchDuration::ZERO;
        app.update();
        assert_eq!(bars(&mut app).0.0, Display::None);
        assert_eq!(bars(&mut app).1, (Display::Flex, Val::Percent(50.0)));
    }

    #[test]
    fn serious_replay_save_errors_remain_in_ended_status() {
        assert_eq!(replay_notice_suffix(""), "");
        // Rendered verbatim, which is why `GameState.replay_notice` carries
        // the player-safe sentence and not the storage error behind it.
        assert_eq!(
            replay_notice_suffix(crate::gameplay::REPLAY_SAVE_FAILED_NOTICE),
            format!("  |  {}", crate::gameplay::REPLAY_SAVE_FAILED_NOTICE)
        );
    }

    #[test]
    fn timer_and_round_preferences_cover_all_hud_combinations() {
        let (group, _) = under_heading_node(&match_layout(WindowSize::OPENING.0));
        let game = GameState::default();

        assert_eq!(group.flex_direction, FlexDirection::Column);
        assert_eq!(group.align_items, AlignItems::Center);
        for (timer, round) in [(false, false), (false, true), (true, false), (true, true)] {
            let preferences = GamePreferences {
                show_match_timer: timer,
                show_round_number: round,
                ..default()
            };
            assert_eq!(match_hud_visibility(&preferences), (timer, round));
        }
        assert_eq!(game.round, 0);
        assert_eq!(
            round_panel_text(game.round, game.round_time_remaining_seconds(), Lines::One),
            "Round 0"
        );
        // Every `.rs` file in this directory, plus the HUD, read from disk at test time rather
        // than from a hand-written `include_str!` list - see
        // `crate::architecture` for why a list cannot guard a negative
        // assertion, and why the count is a floor the walk must reach.
        let source = [sources_in("ui/match_screen", 2), source_file("ui/hud.rs")].concat();
        assert!(!source.contains(&["GO TOGETHER", "  -  ROUND"].concat()));
        assert!(!source.contains(&["HudAction::", "HideTimer"].concat()));
        assert!(!source.contains(&["HudAction::", "ShowTimer"].concat()));
        // The helper that once forced the round panel on while a Round Time
        // clock ran, overriding the preference. Both halves of the HUD pair
        // are the preference alone now; the un-hideable indication of a
        // running round clock is the bar below the board.
        assert!(!source.contains(&["round_hud", "_visible"].concat()));
    }

    /// The round panel with and without a Round Time clock, on one line and
    /// on two. The separator was once a middle dot, which Bevy's bundled
    /// fallback font drew as a box, and the clock read `Round 01:30` beside
    /// `Round 3`, as if there were two rounds; it is a countdown, and now
    /// says so. On two lines the countdown stands under its round. Beyond
    /// the texts written out here, every form the panel writes, across the
    /// rounds and the times it can show, is printable ASCII but for the one
    /// line break of the two-line form.
    #[test]
    fn round_panel_text_is_printable_ascii_and_reads_as_a_countdown() {
        assert_eq!(round_panel_text(3, None, Lines::One), "Round 3");
        assert_eq!(
            round_panel_text(3, Some(90), Lines::One),
            "Round 3  |  01:30 left"
        );
        assert_eq!(
            round_panel_text(12, Some(5), Lines::One),
            "Round 12  |  00:05 left"
        );
        assert_eq!(
            round_panel_text(1, Some(3600), Lines::One),
            "Round 1  |  60:00 left"
        );
        assert_eq!(round_panel_text(3, None, Lines::Two), "Round 3");
        assert_eq!(
            round_panel_text(3, Some(90), Lines::Two),
            "Round 3\n01:30 left"
        );
        for round in [0, 1, 9, 10, 99, 100, 999, 1_000, 9_999, 10_000] {
            for remaining in [
                None,
                Some(0),
                Some(5),
                Some(59),
                Some(60),
                Some(3_599),
                Some(3_600),
            ] {
                for lines in [Lines::One, Lines::Two] {
                    let text = round_panel_text(round, remaining, lines);
                    let breaks = text.matches('\n').count();
                    assert_eq!(
                        breaks,
                        usize::from(lines == Lines::Two && remaining.is_some()),
                        "{text:?}"
                    );
                    assert!(
                        text.chars().all(|character| {
                            character == '\n' || (' '..='~').contains(&character)
                        }),
                        "{text:?} is not printable ASCII"
                    );
                }
            }
        }
    }

    #[test]
    fn the_timer_reads_time_and_how_long_the_match_has_run() {
        assert_eq!(timer_text(0.0), "TIME  00:00");
        assert_eq!(timer_text(7_507.9), "TIME  125:07");
    }

    /// Each longest text `geometry` makes room for is the longest this
    /// screen writes there, in characters of its monospaced font: the
    /// heading and every result; a corner's name with a three-digit score;
    /// every automatic end reason at once; the round panel at the largest
    /// Round Limit with a full hour of Round Time left, on one line and on
    /// two; and the timer at 999:59. A budget shorter than its text would let the text run into
    /// its neighbour; these hold the two together.
    #[test]
    fn the_layouts_longest_texts_are_the_screens_longest_texts() {
        use crate::match_settings::{DURATION_MAX_SECONDS, ROUND_LIMIT_MAX};

        let longest_line = |text: &str| text.lines().map(str::len).max().unwrap();
        let mut headings = vec!["GO TOGETHER".to_string()];
        for score in [
            Score { black: 2, white: 1 },
            Score { black: 1, white: 2 },
            Score { black: 1, white: 1 },
        ] {
            headings.push(result_heading(score));
        }
        assert_eq!(
            headings.iter().map(|heading| heading.len()).max(),
            Some(HEADING_MAX_CHARS),
            "{headings:?}"
        );
        let three_digits = Score {
            black: 999,
            white: 999,
        };
        for player in Player::ALL {
            assert_eq!(
                player_heading(player, Some(three_digits)).len(),
                PLAYER_HEADING_MAX_CHARS
            );
        }
        assert_eq!(
            end_reasons_text(&EVERY_AUTOMATIC_END_REASON).len(),
            END_REASONS_MAX_CHARS
        );
        let round = usize::from(ROUND_LIMIT_MAX);
        let hour = u64::from(DURATION_MAX_SECONDS);
        // The two-line form's longest line, with Round Time running or not.
        let two_lines = longest_line(&round_panel_text(round, Some(hour), Lines::Two))
            .max(longest_line(&round_panel_text(round, None, Lines::Two)));
        assert_eq!(
            ROUND_PANEL_MAX_CHARS,
            [
                round_panel_text(round, Some(hour), Lines::One).len(),
                two_lines
            ]
        );
        assert_eq!(timer_text((999 * 60 + 59) as f64).len(), TIMER_MAX_CHARS);
    }

    #[test]
    fn live_match_ui_has_no_contextual_alert_strip() {
        // Every `.rs` file in this directory, plus the HUD, read from disk at test time rather
        // than from a hand-written `include_str!` list - see
        // `crate::architecture` for why a list cannot guard a negative
        // assertion, and why the count is a floor the walk must reach.
        let source = [sources_in("ui/match_screen", 2), source_file("ui/hud.rs")].concat();
        let removed_role = ["UiRole", "::Alert"].concat();
        let removed_prompt = ["cannot be ", "selected"].concat();

        assert!(!source.contains(&removed_role));
        assert!(!source.contains(&removed_prompt));
    }

    /// What of the live board a drawn hidden selection would change: what
    /// each intersection is drawn with and at what size, how many entities
    /// are on show, and how many exist at all. (It would not see an entity
    /// already on show being recoloured or moved; the board has none that
    /// a selection could be drawn with but the intersections themselves.)
    type LiveBoard = (
        Vec<((usize, usize), Handle<ColorMaterial>, f32)>,
        usize,
        usize,
    );

    fn live_board(app: &mut App) -> LiveBoard {
        let mut drawn: Vec<_> = app
            .world_mut()
            .query::<(&Intersection, &MeshMaterial2d<ColorMaterial>, &Transform)>()
            .iter(app.world())
            .map(|(intersection, material, transform)| {
                (
                    (intersection.0.x, intersection.0.y),
                    material.0.clone(),
                    transform.scale.x,
                )
            })
            .collect();
        drawn.sort_by_key(|(point, ..)| *point);
        let on_show = app
            .world_mut()
            .query::<&Visibility>()
            .iter(app.world())
            .filter(|visibility| **visibility != Visibility::Hidden)
            .count();
        let entities = app.world_mut().query::<Entity>().iter(app.world()).count();
        (drawn, on_show, entities)
    }

    /// The live board never draws a hidden selection: that would hand each
    /// player the other's secret. This is the behaviour itself rather than
    /// a scan for a name. With both players holding selections - in Round
    /// 0, and in an ordinary round, one of them on the selector's own
    /// stone - every intersection is still drawn as nothing but its own
    /// cell, and nothing else on the board appeared or was spawned.
    ///
    /// The Replay Viewer's mark for a revealed selection is no help to a
    /// mistake here either: its meshes and materials are made by the Viewer
    /// on entry and are in no shared resource.
    #[test]
    fn the_live_board_draws_no_hidden_selection() {
        use crate::clock::MatchInstant;
        use crate::rules::LockedMove;

        let expected = |app: &App| -> Vec<((usize, usize), Handle<ColorMaterial>, f32)> {
            let game = app.world().resource::<GameState>();
            let palette = app.world().resource::<StonePalette>();
            let mut cells: Vec<_> = (0..BOARD_SIZE)
                .flat_map(|y| (0..BOARD_SIZE).map(move |x| Point::new(x, y)))
                .map(|point| {
                    let cell = game.board.get(point);
                    (
                        (point.x, point.y),
                        palette.fill(cell).clone(),
                        stone_geometry(cell).fill_scale,
                    )
                })
                .collect();
            cells.sort_by_key(|(point, ..)| *point);
            cells
        };

        // Round 0: three selections each, one point chosen by both.
        let mut app = match_display_test_app();
        app.update();
        let before = live_board(&mut app);
        assert_eq!(before.0, expected(&app));
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            for point in [Point::new(2, 2), Point::new(4, 4), Point::new(0, 8)] {
                game.opening_selections[Player::One.index()].add(point);
            }
            for point in [Point::new(4, 4), Point::new(6, 6), Point::new(8, 0)] {
                game.opening_selections[Player::Two.index()].add(point);
            }
        }
        app.update();
        // The positive half: the selections really are held.
        let game = app.world().resource::<GameState>();
        assert_eq!(game.opening_selections[Player::One.index()].len(), 3);
        assert_eq!(game.opening_selections[Player::Two.index()].len(), 3);
        assert_eq!(live_board(&mut app), before);

        // An ordinary round: Player One selects one of their own stones,
        // Player Two an empty point.
        let (own_stone, empty_point) = (Point::new(3, 3), Point::new(5, 5));
        let mut app = match_display_test_app();
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.phase = MatchPhase::Playing;
            game.board.set(own_stone, Cell::Black);
            game.board.set(Point::new(6, 2), Cell::White);
        }
        app.update();
        let before = live_board(&mut app);
        assert_eq!(before.0, expected(&app));
        app.world_mut().resource_mut::<GameState>().locks = [
            Some(LockedMove {
                player: Player::One,
                point: own_stone,
                at: MatchInstant::from_secs_f64(1.0),
            }),
            Some(LockedMove {
                player: Player::Two,
                point: empty_point,
                at: MatchInstant::from_secs_f64(2.0),
            }),
        ];
        app.update();
        let game = app.world().resource::<GameState>();
        assert!(game.locks.iter().all(Option::is_some));
        assert_eq!(live_board(&mut app), before);

        // And the other way round - Player One an empty point, Player Two
        // one of their own stones - since a selection of the selector's
        // own stone, drawn as that stone, would change nothing to notice.
        app.world_mut().resource_mut::<GameState>().locks = [
            Some(LockedMove {
                player: Player::One,
                point: empty_point,
                at: MatchInstant::from_secs_f64(3.0),
            }),
            Some(LockedMove {
                player: Player::Two,
                point: Point::new(6, 2),
                at: MatchInstant::from_secs_f64(4.0),
            }),
        ];
        app.update();
        assert_eq!(live_board(&mut app), before);
    }

    /// Counts what [`update_match_display`] actually rewrote each frame -
    /// entities marked changed since this counting system's own previous
    /// run, i.e. by the immediately preceding call to `update_match_display`
    /// in the same `app.update()` - to prove the compare-before-write
    /// pattern skips untouched visuals rather than merely producing the same
    /// end value another way.
    #[derive(Resource, Default)]
    struct DisplayChangeCounts {
        intersections_changed: usize,
        changed_points: Vec<Point>,
        rings_changed: Vec<Point>,
        cursors_changed: usize,
        timer_text_changed: bool,
        other_text_changed: usize,
        hint_visibility_changed: usize,
    }

    /// A ring the display pass rewrote: its scale, its visibility, or both.
    type ChangedRing = Or<(Changed<Transform>, Changed<Visibility>)>;

    fn record_display_changes(
        intersections: Query<(&Intersection, Entity), Changed<MeshMaterial2d<ColorMaterial>>>,
        rings: Query<&IntersectionRing, ChangedRing>,
        cursors: Query<Entity, (With<CursorVisual>, Changed<Transform>)>,
        texts: Query<&UiRole, Changed<Text>>,
        hints: Query<Entity, (With<ControlHint>, Changed<Visibility>)>,
        mut counts: ResMut<DisplayChangeCounts>,
    ) {
        counts.changed_points = intersections.iter().map(|(point, _)| point.0).collect();
        counts.intersections_changed = counts.changed_points.len();
        counts.rings_changed = rings.iter().map(|ring| ring.0).collect();
        counts.cursors_changed = cursors.iter().count();
        counts.timer_text_changed = false;
        counts.other_text_changed = 0;
        for role in &texts {
            if matches!(role, UiRole::Timer) {
                counts.timer_text_changed = true;
            } else {
                counts.other_text_changed += 1;
            }
        }
        counts.hint_visibility_changed = hints.iter().count();
    }

    /// Builds a minimal headless app with the real [`spawn_match_screen`] and
    /// [`update_match_display`] systems and every resource they need, plus
    /// [`record_display_changes`] chained right after so tests can inspect
    /// exactly what one `update_match_display` pass touched.
    fn match_display_test_app() -> App {
        let mut app = App::new();
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Mesh>>();
        let palette = {
            let mut materials = app.world_mut().resource_mut::<Assets<ColorMaterial>>();
            crate::app::visuals::test_stone_palette(&mut materials)
        };
        let stone_mesh = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            StoneMesh(meshes.add(Circle::new(1.0)))
        };
        app.insert_resource(palette)
            .insert_resource(stone_mesh)
            .insert_resource(GameState::default())
            .insert_resource(SemanticInput::default())
            .insert_resource(HudState::default())
            .insert_resource(InputGate::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .init_resource::<WindowSize>()
            .init_resource::<DisplayChangeCounts>()
            .add_systems(Startup, spawn_match_screen)
            .add_systems(
                Update,
                (update_match_display, record_display_changes).chain(),
            );
        app
    }

    #[test]
    fn unchanged_board_does_not_rewrite_any_intersection() {
        let mut app = match_display_test_app();
        app.update(); // Startup spawn plus the first, everything-is-new pass.
        app.update(); // Nothing changed: this pass should touch nothing.

        let counts = app.world().resource::<DisplayChangeCounts>();
        assert_eq!(counts.intersections_changed, 0);
        assert_eq!(counts.rings_changed, Vec::new());
    }

    /// Each intersection's ring as the display pass left it: its scale while
    /// shown, `None` while hidden.
    fn ring_scales(app: &mut App) -> std::collections::BTreeMap<(usize, usize), Option<f32>> {
        app.world_mut()
            .query::<(&IntersectionRing, &Transform, &Visibility)>()
            .iter(app.world())
            .map(|(ring, transform, visibility)| {
                let shown = (*visibility == Visibility::Visible).then_some(transform.scale.x);
                ((ring.0.x, ring.0.y), shown)
            })
            .collect()
    }

    #[test]
    fn a_ring_is_drawn_behind_white_and_gray_stones_and_no_others() {
        let mut app = match_display_test_app();
        app.update();
        let rings = ring_scales(&mut app);
        assert_eq!(rings.len(), BOARD_SIZE * BOARD_SIZE);
        assert!(
            rings.values().all(Option::is_none),
            "an empty board shows no ring"
        );

        let (black, white, gray) = (Point::new(1, 1), Point::new(3, 4), Point::new(7, 2));
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.board.set(black, Cell::Black);
            game.board.set(white, Cell::White);
            game.board.set(gray, Cell::Gray);
        }
        app.update();

        let rings = ring_scales(&mut app);
        assert_eq!(rings[&(black.x, black.y)], None);
        assert_eq!(
            rings[&(white.x, white.y)],
            stone_geometry(Cell::White).ring_scale
        );
        assert_eq!(
            rings[&(gray.x, gray.y)],
            stone_geometry(Cell::Gray).ring_scale
        );
        assert_eq!(rings.values().filter(|ring| ring.is_some()).count(), 2);
        let mut changed = app
            .world()
            .resource::<DisplayChangeCounts>()
            .rings_changed
            .clone();
        changed.sort_by_key(|point| (point.x, point.y));
        assert_eq!(
            changed,
            vec![white, gray],
            "only the two rings that appeared are rewritten"
        );

        // A captured stone takes its ring with it, and a white stone that
        // turns gray keeps one at gray's smaller footprint.
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.board.set(gray, Cell::Empty);
            game.board.set(white, Cell::Gray);
        }
        app.update();
        let rings = ring_scales(&mut app);
        assert_eq!(rings[&(gray.x, gray.y)], None);
        assert_eq!(
            rings[&(white.x, white.y)],
            stone_geometry(Cell::Gray).ring_scale
        );
    }

    #[test]
    fn a_single_point_change_updates_only_that_intersections_visual() {
        let mut app = match_display_test_app();
        app.update();

        let point = Point::new(3, 4);
        app.world_mut()
            .resource_mut::<GameState>()
            .board
            .set(point, Cell::Black);
        app.update();

        let counts = app.world().resource::<DisplayChangeCounts>();
        assert_eq!(counts.intersections_changed, 1);
        assert_eq!(counts.changed_points, vec![point]);
    }

    #[test]
    fn cursor_movement_does_not_require_a_full_board_refresh() {
        let mut app = match_display_test_app();
        app.update();

        app.world_mut().resource_mut::<GameState>().cursors[0] = Point::new(2, 2);
        // Cursor translation is written in every phase; cursors are merely
        // hidden outside `MatchPhase::Playing`. Switching the fresh
        // `GameState::default()` to Playing only makes this the case a player
        // would see - a visible cursor moving - it is not what triggers the
        // Transform write counted below.
        app.world_mut().resource_mut::<GameState>().phase = MatchPhase::Playing;
        app.update();

        let counts = app.world().resource::<DisplayChangeCounts>();
        assert_eq!(
            counts.intersections_changed, 0,
            "moving a cursor must not touch any board intersection"
        );
        assert!(counts.cursors_changed >= 1);
    }

    #[test]
    fn timer_advancement_does_not_refresh_unrelated_static_text() {
        let mut app = match_display_test_app();
        app.world_mut().resource_mut::<GameState>().phase = MatchPhase::Playing;
        app.update();

        app.world_mut().resource_mut::<GameState>().elapsed =
            crate::clock::MatchInstant::from_secs_f64(5.0);
        app.update();

        let counts = app.world().resource::<DisplayChangeCounts>();
        assert!(counts.timer_text_changed);
        assert_eq!(
            counts.other_text_changed, 0,
            "advancing only the clock must not rewrite any other text role"
        );
        assert_eq!(counts.intersections_changed, 0);
    }

    #[test]
    fn control_hint_visibility_changes_apply_the_same_frame() {
        let mut app = match_display_test_app();
        app.update();

        app.world_mut()
            .resource_mut::<GameState>()
            .control_hints_visible = [false, true];
        app.update();

        let counts = app.world().resource::<DisplayChangeCounts>();
        assert!(counts.hint_visibility_changed >= 1);
    }
}
