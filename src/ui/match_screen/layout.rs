//! Spawns the match screen furniture: player cursors, HUD text, the
//! countdown card, the end-gesture/hold-progress area and the return-home modal.
//!
//! The centre of the top edge is the heading and, under it, a column
//! holding the line that says why a Match ended, the Match timer and the
//! round panel, each laid out only while it is shown. The reason line is
//! shown only once a Match has ended, so during play the column is exactly
//! the timer and the round panel, where they have always stood. It is the
//! one line there long enough to matter, and it is held to the board's
//! width, which keeps it clear of the two corner stacks - a player's
//! heading, the Home or Help button, and their capture count - that stand
//! beyond the board on either side.
//!
//! Everything spawned here carries `DespawnOnExit(AppScreen::Match)` so it is
//! despawned when the match screen is left. The Help modal is not
//! spawned here: it is the HUD's one application-persistent entity, shown
//! from this screen's *Help* button or F1, and from Home alike.
//!
//! During active play the bottom area shows a compact end-gesture hint once
//! both temporary player onboarding hints are dismissed and while neither
//! Select nor Restart is pressed; a hold in progress replaces it with the hold
//! label, and an open modal hides it. The input-release gate does not: it
//! suppresses one action at a time, and a gated input is a held input, which
//! the "neither Select nor Restart is pressed" rule above already covers.
//!
//! Below that text is one slot that two bars share, never both at once. The
//! hold-progress bar grows from the centre outwards and keeps a fixed bright
//! outline, so the track is legible against the board even at zero progress;
//! it is hidden entirely when no hold is in progress. The round-time bar is
//! the same shape in different colours and runs the other way: full when a
//! round starts, it shrinks from both ends toward the centre as the Round
//! Time runs down, and it is present whenever a round clock is running and
//! the hold bar is not using the slot. Both get that symmetry for free from
//! the track's centre justification - one fill child whose width is a
//! percentage of the track needs no offset arithmetic to stay centred. Which
//! bar owns the slot is `super::bottom_bar_fractions`'s decision.

use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::{
    ControlHint, CursorVisual, HoldProgressFill, HoldProgressTrack, RoundTimeFill, RoundTimeTrack,
};
use crate::app::AppScreen;
use crate::app::visuals::{
    BOARD_CENTER_Y, CursorVisualStyle, GRID_EXTENT, GRID_SPACING, cursor_outline_layers,
    cursor_visual_style, spawn_outline_segments,
};
use crate::audio::UiSound;
use crate::presentation::{
    BLUE_BORDER, BUTTON_BACKGROUND, COUNTDOWN_BACKGROUND, DEEP_SURFACE_BACKGROUND, HINT_BACKGROUND,
    ORANGE, OVERLAY_BACKGROUND, PANEL_BACKGROUND, TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SUBTLE,
};
use crate::rules::Player;
use crate::ui::countdown_card::CountdownCard;
use crate::ui::hud::{HudAction, HudPanel, UiRole, hud_action_ui_cue};
use crate::ui::widgets::spawn_child_button;

// One geometry for both bars in the slot below the board, so swapping one for
// the other moves nothing around it.
const BAR_TRACK_WIDTH: f32 = 360.0;
const BAR_TRACK_HEIGHT: f32 = 12.0;
const BAR_OUTLINE_WIDTH: f32 = 3.0;
pub(super) const HOLD_PROGRESS_FILL_COLOR: Color = ORANGE;
pub(super) const HOLD_PROGRESS_OUTLINE_COLOR: Color = TEXT_HEADING;
/// The round-time bar's fill is the cream the `Round MM:SS` readout and the
/// Match timer are written in, so the bar and the numeric clock read as one
/// clock, and it is not the hold bar's orange - the two share a slot and must
/// never be mistaken for each other. Its length is the bar's meaning; the
/// colour only has to be seen, and cream on the track's
/// [`DEEP_SURFACE_BACKGROUND`] is about 14:1, far past the 3:1 a non-text
/// element needs (`round_time_bar_...` in this module's tests measures it).
pub(super) const ROUND_TIME_FILL_COLOR: Color = TEXT_HEADING;
/// Quieter than the hold bar's bright outline, because this bar is ambient:
/// the outline only has to show how long a full round would be once the fill
/// has shrunk away from the ends.
pub(super) const ROUND_TIME_OUTLINE_COLOR: Color = TEXT_SUBTLE;

/// The board's wood, from edge to edge: the grid and the margin around it.
/// The end-reason line under the heading is held to this width so that it
/// never reaches the corners, which stand outside it.
pub(super) const BOARD_OUTER_SIZE: f32 = GRID_EXTENT + 78.0;
/// The line under an ended Match's result that says why it ended.
///
/// Its size is set by the longest line it can hold, not the common one. End
/// reasons that cross on the same round are all shown, so the worst case is
/// every automatic limit at once - `Match Time Limit, Round Limit, No Board
/// Change Limit, Point Target`, 66 characters - and at this size it still
/// fits on one line within [`BOARD_OUTER_SIZE`].
/// `the_longest_end_reason_line_fits_on_one_line_inside_the_board_width`
/// measures it by its glyphs.
pub(super) const END_REASONS_SIZE: f32 = 16.0;
const END_REASONS_GAP_BELOW: f32 = 6.0;
pub(super) const PLAYER_HEADER_OFFSET: f32 = 28.0;
/// The two players' headings are written in their accents, and at 24px,
/// the size from which WCAG holds text to 3:1 rather than 4.5:1. Player
/// One's red is 3.97:1 on the window background, so a smaller heading in
/// it would be below the floor; Player Two's is the same size so the two
/// still match. `src/contrast.rs` holds both at this size.
///
/// The accent is not lightened instead because it is one colour, written
/// as text on the Settings columns too, where no red at any saturation
/// reaches 4.5:1 (`docs/ideas/accessibility-findings.md` has the proof).
pub(super) const PLAYER_HEADING_SIZE: f32 = 24.0;
pub(super) const CONTROL_HINT_SIDE_OFFSET: f32 = 18.0;
pub(super) const CONTROL_HINT_WIDTH: f32 = 156.0;

pub(super) fn spawn_cursor(commands: &mut Commands, player: Player, style: CursorVisualStyle) {
    commands
        .spawn((
            Transform::from_xyz(0.0, 0.0, 8.0),
            Visibility::Visible,
            CursorVisual(player),
            DespawnOnExit(AppScreen::Match),
        ))
        .with_children(|parent| {
            for layer in cursor_outline_layers(style) {
                spawn_outline_segments(parent, layer.color, style.size, layer.thickness, layer.z);
            }
        });
}

pub(super) fn spawn_ui(commands: &mut Commands) {
    spawn_text(
        commands,
        UiRole::Heading,
        FontSize::Px(34.0),
        TEXT_HEADING,
        Node {
            position_type: PositionType::Absolute,
            top: px(14),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    commands
        .spawn((under_heading_node(), DespawnOnExit(AppScreen::Match)))
        .with_children(|parent| {
            parent.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(END_REASONS_SIZE),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                TextLayout::justify(Justify::Center),
                Node {
                    max_width: px(BOARD_OUTER_SIZE),
                    // A little more room below than above, so the line reads
                    // with the result it explains rather than with the
                    // clocks under it.
                    margin: UiRect::bottom(px(END_REASONS_GAP_BELOW)),
                    display: Display::None,
                    ..default()
                },
                UiRole::EndReasons,
                HudPanel::EndReasons,
            ));
            parent
                .spawn((
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    HudPanel::Timer,
                ))
                .with_children(|timer| {
                    timer.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                        UiRole::Timer,
                    ));
                });
            parent.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(TEXT_HEADING),
                UiRole::Round,
                HudPanel::Round,
            ));
        });
    spawn_countdown_overlay(commands);
    spawn_hud_button(
        commands,
        HudAction::OpenReturnHome,
        "Home",
        84.0,
        Node {
            position_type: PositionType::Absolute,
            top: px(94),
            left: px(28),
            ..default()
        },
    );
    spawn_hud_button(
        commands,
        HudAction::OpenHelp,
        "Help",
        84.0,
        Node {
            position_type: PositionType::Absolute,
            top: px(94),
            right: px(28),
            ..default()
        },
    );
    spawn_text(
        commands,
        UiRole::PlayerTwo,
        FontSize::Px(PLAYER_HEADING_SIZE),
        cursor_visual_style(Player::Two).color,
        player_header_node(Player::Two),
    );
    spawn_text(
        commands,
        UiRole::PlayerOne,
        FontSize::Px(PLAYER_HEADING_SIZE),
        cursor_visual_style(Player::One).color,
        player_header_node(Player::One),
    );
    spawn_control_hint(commands, Player::Two);
    spawn_control_hint(commands, Player::One);
    spawn_bottom_overlay(commands);
    spawn_return_home_modal(commands);
}

/// A player's control hint: a card in their accent on their own side of
/// the screen, under their heading, whose first line names them.
///
/// The text is [`TEXT_MUTED`], not the accent. At 14px text owes its
/// background 4.5:1, and Player One's red is 3.78:1 on the card, so the
/// accent stays on the frame - a mark, which owes 3:1 - and both players'
/// text is the same colour, so neither hint reads as the odd one out.
fn spawn_control_hint(commands: &mut Commands, player: Player) {
    commands
        .spawn((
            control_hint_node(player),
            BackgroundColor(HINT_BACKGROUND),
            BorderColor::all(cursor_visual_style(player).color),
            Visibility::Visible,
            ControlHint(player),
            DespawnOnExit(AppScreen::Match),
        ))
        .with_child((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            TextLayout::justify(Justify::Center),
            UiRole::ControlHint(player),
        ));
}

pub(super) fn control_hint_node(player: Player) -> Node {
    let mut node = Node {
        position_type: PositionType::Absolute,
        top: px(214),
        width: px(CONTROL_HINT_WIDTH),
        min_height: px(142),
        padding: UiRect::all(px(10)),
        border: UiRect::all(px(2)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    match player {
        Player::One => node.right = px(CONTROL_HINT_SIDE_OFFSET),
        Player::Two => node.left = px(CONTROL_HINT_SIDE_OFFSET),
    }
    node
}

/// The gesture text and, under it, the slot the hold-progress and round-time
/// bars share. Both tracks are spawned hidden; `super::update_match_display`
/// shows at most one per frame.
fn spawn_bottom_overlay(commands: &mut Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(18),
                width: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(7),
                ..default()
            },
            DespawnOnExit(AppScreen::Match),
        ))
        .with_children(|overlay| {
            overlay.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                TextLayout::justify(Justify::Center),
                UiRole::Gesture,
            ));
            overlay
                .spawn((
                    hold_progress_track_node(),
                    BackgroundColor(DEEP_SURFACE_BACKGROUND),
                    hold_progress_track_outline(),
                    HoldProgressTrack,
                ))
                .with_child((
                    Node {
                        width: bar_fill_width(0.0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(HOLD_PROGRESS_FILL_COLOR),
                    HoldProgressFill,
                ));
            overlay
                .spawn((
                    round_time_track_node(),
                    BackgroundColor(DEEP_SURFACE_BACKGROUND),
                    round_time_track_outline(),
                    RoundTimeTrack,
                ))
                .with_child((
                    Node {
                        width: bar_fill_width(1.0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(ROUND_TIME_FILL_COLOR),
                    RoundTimeFill,
                ));
        });
}

pub(super) fn hold_progress_track_node() -> Node {
    bar_track_node()
}

pub(super) fn round_time_track_node() -> Node {
    bar_track_node()
}

/// The shared track: centre-justified, so a fill child sized as a percentage
/// of it grows and shrinks symmetrically about the middle, and hidden until a
/// display pass has something to draw in it.
fn bar_track_node() -> Node {
    Node {
        width: px(BAR_TRACK_WIDTH),
        height: px(BAR_TRACK_HEIGHT),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        display: Display::None,
        ..default()
    }
}

pub(super) fn hold_progress_track_outline() -> Outline {
    Outline::new(px(BAR_OUTLINE_WIDTH), px(0), HOLD_PROGRESS_OUTLINE_COLOR)
}

pub(super) fn round_time_track_outline() -> Outline {
    Outline::new(px(BAR_OUTLINE_WIDTH), px(0), ROUND_TIME_OUTLINE_COLOR)
}

/// A bar fill's width for a `0.0..=1.0` fraction of its track. The hold bar
/// passes its progress, so it grows; the round bar passes the share of the
/// round still to run, so it shrinks.
pub(super) fn bar_fill_width(fraction: f32) -> Val {
    percent(fraction.clamp(0.0, 1.0) * 100.0)
}

/// The column under the heading: the end-reason line, the Match timer and
/// the round panel, whichever of them are shown.
pub(super) fn under_heading_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        top: px(54),
        width: percent(100),
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        row_gap: px(4),
        ..default()
    }
}

/// The Round 0 countdown, built and placed by the rule the Replay Viewer's
/// shares: see [`crate::ui::countdown_card`].
const MATCH_COUNTDOWN_CARD: CountdownCard = CountdownCard {
    board_center_y: BOARD_CENTER_Y,
    grid_spacing: GRID_SPACING,
    size: Vec2::new(180.0, 130.0),
    font_size: FontSize::Px(64.0),
    background: COUNTDOWN_BACKGROUND,
};

pub(super) fn spawn_countdown_overlay(commands: &mut Commands) {
    commands
        .spawn((
            MATCH_COUNTDOWN_CARD.overlay_node(),
            GlobalZIndex(90),
            HudPanel::Countdown,
            DespawnOnExit(AppScreen::Match),
        ))
        .with_children(|overlay| MATCH_COUNTDOWN_CARD.spawn(overlay, UiRole::Countdown));
}

pub(super) fn player_header_node(player: Player) -> Node {
    let mut node = Node {
        position_type: PositionType::Absolute,
        top: px(58),
        ..default()
    };
    match player {
        Player::One => node.right = px(PLAYER_HEADER_OFFSET),
        Player::Two => node.left = px(PLAYER_HEADER_OFFSET),
    }
    node
}

fn spawn_text(commands: &mut Commands, role: UiRole, size: FontSize, color: Color, node: Node) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: size,
            ..default()
        },
        TextColor(color),
        TextLayout::justify(Justify::Center),
        node,
        role,
        DespawnOnExit(AppScreen::Match),
    ));
}

fn spawn_hud_button(
    commands: &mut Commands,
    action: HudAction,
    label: &'static str,
    width: f32,
    mut node: Node,
) {
    node.width = px(width);
    node.height = px(34);
    node.justify_content = JustifyContent::Center;
    node.align_items = AlignItems::Center;
    commands
        .spawn((
            Button,
            UiSound::new(hud_action_ui_cue(action)),
            node,
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BLUE_BORDER),
            action,
            DespawnOnExit(AppScreen::Match),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            TextLayout::justify(Justify::Center),
            Node {
                width: percent(100),
                ..default()
            },
        ));
}

fn spawn_return_home_modal(commands: &mut Commands) {
    commands
        .spawn((
            // A plain node: picking stops at it, so it blocks every click
            // to the screen beneath (`ui::button`).
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                display: Display::None,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(OVERLAY_BACKGROUND),
            GlobalZIndex(110),
            HudPanel::ReturnHomeModal,
            DespawnOnExit(AppScreen::Match),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(540),
                        padding: UiRect::all(px(34)),
                        border: UiRect::all(px(2)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(22),
                        ..default()
                    },
                    BackgroundColor(PANEL_BACKGROUND),
                    BorderColor::all(ORANGE),
                ))
                .with_children(|modal| {
                    modal.spawn((
                        Text::new("Return to Home?"),
                        TextFont {
                            font_size: FontSize::Px(32.0),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                    ));
                    modal.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(19.0),
                            ..default()
                        },
                        TextColor(TEXT_PRIMARY),
                        TextLayout::justify(Justify::Center),
                        Node {
                            width: percent(100),
                            ..default()
                        },
                        UiRole::ReturnHomeBody,
                    ));
                    modal
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::Center,
                            column_gap: px(16),
                            ..default()
                        })
                        .with_children(|choices| {
                            spawn_child_button(
                                choices,
                                HudAction::ConfirmReturnHome,
                                "Return Home",
                                150.0,
                                None,
                            );
                            spawn_child_button(
                                choices,
                                HudAction::CancelReturnHome,
                                "Cancel",
                                120.0,
                                None,
                            );
                        });
                });
        });
}
