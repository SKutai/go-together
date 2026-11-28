//! The paginated Help modal and the copy it renders.
//!
//! # Responsibility
//!
//! One modal, spawned once at `Startup` by [`super::hud::HudPlugin`] and never
//! despawned. It is reachable from Home and from the Match, so no one
//! screen's lifetime is its own: it is application-persistent, like the
//! camera, and the first interactive entity to be - `docs/ARCHITECTURE.md`'s
//! entity-ownership section says why that amends the modal-scoped rule
//! rather than rejecting it. `HudState::help_open` toggles its display, and
//! a screen change closes it. [`help_page`] is the single source of the page
//! sequence: a keyboard-controls page, two pages of terms, five rules pages,
//! and three controller pages.
//!
//! # Why the terms come before the rules
//!
//! The rules pages are written in the game's own words - liberty, group,
//! suicide, ko, territory, Gray, Teleport Marker - and a player who does not
//! already know Go has none of them. The two terms pages define them, in the
//! order each builds on the last, so they read as a primer rather than an
//! alphabetical glossary. They sit directly before the first rules page
//! rather than at the back of the book because the modal pages one step at
//! a time with no way to jump: a reader who met "liberties" on the Round 0
//! page with the definitions five pages further on would have to count
//! presses there and back, which is the comprehension gap the pages exist to
//! close. The controls page stays first because a player who opens the
//! modal mid-match most often wants a key, not a rule.
//! `the_terms_pages_define_the_rules_words_before_any_page_uses_them` pins
//! both halves: that every defined word has its entry, and that no page
//! before the terms pages uses one.
//!
//! # The ring
//!
//! The two page arrows and Close are in the keyboard and controller focus
//! ring, in [`FocusGroup::Modal`] and in drawn order - Left 0, Right 1,
//! Close 2 ([`help_modal_focusable`]). All three are non-destructive, so any
//! seed is safe; drawn order makes the first Enter on page 1 a no-op, where
//! seeding on Close would make Enter-Enter an accidental dismissal for a
//! player who had just opened the modal with Enter. The ring runs on the
//! Match screen exactly while this modal is open - `ui`'s focus module owns
//! that gate and says why it is safe - so the modal is operable by keyboard
//! and pad wherever it is shown. Opening it during a Match takes F1 or the
//! pointer: the Match HUD's own buttons are not in the ring, because that
//! screen cannot have a ring during live play, so the keyboard's way in is
//! a reserved key, read by `ui`'s focus module beside the keys that close
//! it. No pad button has been given that job, so a pad opens the modal
//! there with the pointer.
//!
//! # Why the copy is generated
//!
//! Every key or button name is read from the live bindings and controller
//! assignments rather than hard-coded, so remapping a control immediately
//! updates the instructions. [`restart_label`] additionally joins the keyboard
//! key with each *assigned* controller's profile-local restart input, and
//! degrades to the bare keyboard key when no controller is connected.
//!
//! The one deliberate exception is the two menu-and-back blocks: those inputs
//! are fixed rather than remappable, so they belong to neither player, and
//! their names are therefore written out literally. They are read raw in two
//! places rather than one - [`crate::ui`]'s focus module reads the ring keys,
//! F1, and the Match screen's Escape and Start, while the Settings, Replay Library
//! and Replay Viewer back keys are read by those screens' own modules, beside
//! the input each already handled.

use bevy::prelude::*;

use super::focus::{FocusGroup, Focusable};
use super::hud::{HudAction, HudPanel, UiRole};
use super::widgets::spawn_child_button;
use crate::input::GameAction;
use crate::input::controller::{
    ControllerAssignments, ControllerBindings, ControllerSlotStatus, ProfileAction,
};
use crate::input::keyboard::KeyboardBindings;
use crate::presentation::{
    ORANGE, OVERLAY_BACKGROUND, PANEL_BACKGROUND, TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY,
};
use crate::rules::Player;

const KEYBOARD_CONTROLS_PAGE: usize = 0;
const BOARD_TERMS_PAGE: usize = 1;
const GAME_TERMS_PAGE: usize = 2;
const ROUND_ZERO_PAGE: usize = 3;
const HIDDEN_MOVES_PAGE: usize = 4;
const CAPTURES_PAGE: usize = 5;
const ENDING_PAGE: usize = 6;
const MATCH_RULES_PAGE: usize = 7;
const WHITE_CONTROLLER_PAGE: usize = 8;
const BLACK_CONTROLLER_PAGE: usize = 9;
const SHARED_CONTROLLER_PAGE: usize = 10;
/// One keyboard-controls page, two pages of terms and five rules pages,
/// followed by three controller pages.
pub(crate) const HELP_PAGE_COUNT: usize = 11;

/// Wide enough for `Page 11 of 11`, the longest string the indicator shows,
/// at its 17 px monospace font - about 10.4 px a character, so 135 px. At
/// 120 px it fitted `Page 9 of 9` and wrapped the moment the count reached
/// two digits, which only a capture of the running game showed.
const HELP_PAGE_INDICATOR_WIDTH: f32 = 150.0;
const HELP_PAGINATION_HEIGHT: f32 = 36.0;
const HELP_PAGE_INDICATOR_TOP_PADDING: f32 = 4.0;
pub(crate) const HELP_MODAL_WIDTH: f32 = 820.0;
pub(crate) const HELP_MODAL_HEIGHT: f32 = 760.0;

fn help_pagination_side_node(justify_content: JustifyContent) -> Node {
    Node {
        flex_basis: px(0),
        flex_grow: 1.0,
        justify_content,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn help_page_indicator_node() -> Node {
    Node {
        width: px(HELP_PAGE_INDICATOR_WIDTH),
        height: px(HELP_PAGINATION_HEIGHT),
        padding: UiRect::top(px(HELP_PAGE_INDICATOR_TOP_PADDING)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

pub(super) fn help_page_indicator(page: usize) -> String {
    format!(
        "Page {} of {}",
        page.min(HELP_PAGE_COUNT - 1) + 1,
        HELP_PAGE_COUNT
    )
}

pub(super) fn help_navigation_unavailable(action: HudAction, page: usize) -> bool {
    (action == HudAction::PreviousHelpPage && page == 0)
        || (action == HudAction::NextHelpPage && page >= HELP_PAGE_COUNT - 1)
}

/// The modal's place in the focus ring: `order` counts its buttons in drawn
/// order, Left 0, Right 1, Close 2 - see the module docs for why the seed is
/// an arrow rather than Close.
const fn help_modal_focusable(order: u32) -> Focusable {
    Focusable {
        group: FocusGroup::Modal,
        order,
    }
}

/// Spawns the one Help modal, hidden, with no screen tag: it is
/// shown from Home and from the Match, so it belongs to neither and lives
/// for the whole application. A `Startup` system of [`super::hud::HudPlugin`].
pub(super) fn spawn_help_modal(mut commands: Commands) {
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
            GlobalZIndex(100),
            HudPanel::HelpModal,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(HELP_MODAL_WIDTH),
                        height: px(HELP_MODAL_HEIGHT),
                        padding: UiRect::all(px(32)),
                        border: UiRect::all(px(2)),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(18),
                        ..default()
                    },
                    BackgroundColor(PANEL_BACKGROUND),
                    BorderColor::all(ORANGE),
                ))
                .with_children(|modal| {
                    modal.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(30.0),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                        TextLayout::justify(Justify::Center),
                        Node {
                            width: percent(100),
                            ..default()
                        },
                        UiRole::HelpTitle,
                    ));
                    modal.spawn((
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(TEXT_PRIMARY),
                        TextLayout::justify(Justify::Left),
                        Node {
                            width: percent(100),
                            flex_grow: 1.0,
                            ..default()
                        },
                        UiRole::HelpBody,
                    ));
                    modal
                        .spawn(Node {
                            width: percent(100),
                            height: px(42),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            column_gap: px(16),
                            ..default()
                        })
                        .with_children(|navigation| {
                            navigation
                                .spawn(help_pagination_side_node(JustifyContent::FlexEnd))
                                .with_children(|left| {
                                    spawn_child_button(
                                        left,
                                        HudAction::PreviousHelpPage,
                                        "Left",
                                        90.0,
                                        Some(help_modal_focusable(0)),
                                    );
                                });
                            navigation.spawn(help_page_indicator_node()).with_child((
                                Text::new(""),
                                TextFont {
                                    font_size: FontSize::Px(17.0),
                                    ..default()
                                },
                                TextColor(TEXT_MUTED),
                                TextLayout::justify(Justify::Center),
                                UiRole::HelpPage,
                            ));
                            navigation
                                .spawn(help_pagination_side_node(JustifyContent::FlexStart))
                                .with_children(|right| {
                                    spawn_child_button(
                                        right,
                                        HudAction::NextHelpPage,
                                        "Right",
                                        90.0,
                                        Some(help_modal_focusable(1)),
                                    );
                                });
                        });
                    modal
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::Center,
                            ..default()
                        })
                        .with_children(|footer| {
                            spawn_child_button(
                                footer,
                                HudAction::CloseHelp,
                                "Close",
                                120.0,
                                Some(help_modal_focusable(2)),
                            );
                        });
                });
        });
}

/// Title and body for one help page, generated from the live bindings.
///
/// The page index is clamped, so an out-of-range page shows the last one
/// rather than panicking.
pub(super) fn help_page(
    page: usize,
    bindings: &KeyboardBindings,
    controller_bindings: &ControllerBindings,
    assignments: &ControllerAssignments,
) -> (String, String) {
    match page.min(HELP_PAGE_COUNT - 1) {
        KEYBOARD_CONTROLS_PAGE => (
            "Keyboard Controls".into(),
            format!(
                "PLAYER 1 - BLACK\n\
                 Move: {} / {} / {} / {}\n\
                 {}: Select a spot\n\
                 {}: Set Teleport Marker\n\
                 {}: Teleport to Marker\n\n\
                 PLAYER 2 - WHITE\n\
                 Move: {} / {} / {} / {}\n\
                 {}: Select a spot\n\
                 {}: Set Teleport Marker\n\
                 {}: Teleport to Marker\n\n\
                 BOTH PLAYERS\n\
                 Starting in Round 1, hold {} together for 3 seconds to end the match.\n\
                 During play or after scoring, hold {} for 3 seconds to restart.\n\n\
                 MENU KEYS\n\
                 Tab or the arrow keys highlight a button on every screen but the match \
                 itself, and in this window even mid-match; Enter or Space presses it. \
                 F1 opens and closes this window, mid-match too. Escape closes windows \
                 like this one, steps back from Settings, Replays and an open replay, and \
                 during a match asks before returning; Enter confirms.",
                bindings.key(GameAction::PlayerOneUp).display_name(),
                bindings.key(GameAction::PlayerOneLeft).display_name(),
                bindings.key(GameAction::PlayerOneDown).display_name(),
                bindings.key(GameAction::PlayerOneRight).display_name(),
                bindings.key(GameAction::PlayerOneSelect).display_name(),
                bindings.key(GameAction::PlayerOneSetMarker).display_name(),
                bindings
                    .key(GameAction::PlayerOneJumpToMarker)
                    .display_name(),
                bindings.key(GameAction::PlayerTwoUp).display_name(),
                bindings.key(GameAction::PlayerTwoLeft).display_name(),
                bindings.key(GameAction::PlayerTwoDown).display_name(),
                bindings.key(GameAction::PlayerTwoRight).display_name(),
                bindings.key(GameAction::PlayerTwoSelect).display_name(),
                bindings.key(GameAction::PlayerTwoSetMarker).display_name(),
                bindings
                    .key(GameAction::PlayerTwoJumpToMarker)
                    .display_name(),
                keyboard_end_gesture_label(bindings),
                keyboard_restart_label(bindings),
            ),
        ),
        BOARD_TERMS_PAGE => (
            "Terms: Points, Groups & Liberties".into(),
            "POINT: A crossing of two lines on the board. Stones sit on the crossings, \
             not in the squares between them; 9 lines each way make 81 points. These \
             pages also call a point an intersection or a spot.\n\n\
             STONE: A piece placed on an empty point. Black and White stones belong to \
             the player of that color; Gray stones belong to no one. A stone never moves \
             once placed, and leaves only by being removed.\n\n\
             ADJACENT: Neighbors along a line with no point between them: up, down, left \
             or right, never diagonally. Stones and groups are adjacent, and border each \
             other, when any of their points are.\n\n\
             GROUP: Stones of one color on adjacent points connect, and a chain of \
             connected stones, however long, is one group, removed as a whole. A lone \
             Black or White stone is a group of one. Gray never connects.\n\n\
             LIBERTY: An empty point adjacent to a group. A group with none is removed, \
             at once in most cases; Round 1+ says how one can stand until a cleanup. \
             Gray stones and the edge of the board provide none.\n\n\
             CAPTURE: Removing an opposing group that has no liberties, by the placement \
             that fills its last one, by an active cleanup beside it, or at the Round 0 \
             reveal. Its points are empty again."
                .into(),
        ),
        GAME_TERMS_PAGE => (
            "Terms: Gray, Suicide, Ko & Territory".into(),
            "GRAY: A permanent neutral stone, also called a Gray wall. It belongs to \
             neither player, cannot be captured, and provides no liberties. Captures, \
             Repetition & Gray says how one is made.\n\n\
             SUICIDE: Placing a stone that leaves its own group with no liberties. \
             Captures are taken first, so a placement that captures survives. Otherwise \
             the acting group self-removes: the new stone and every stone connected to \
             it. Standard Go forbids this; this game allows it.\n\n\
             KO: Standard Go's rule against a capture and recapture that repeat a \
             position. This game has no separate ko rule; repetition covers it. A round \
             that would recreate any earlier board keeps the old board, and each valid \
             attempted point that was empty becomes Gray.\n\n\
             TERRITORY: Empty points in a region bordered by one player's stones alone; \
             Gray stones and the edge of the board count as neither player. When the \
             Match ends, each such region is filled with that player's color and \
             counted. Stones plus filled territory is the area score.\n\n\
             TELEPORT MARKER: A remembered point for your cursor. Set Teleport Marker \
             puts it under your cursor, even on a stone; Teleport to Marker jumps your \
             cursor back there. It is never drawn during play, so remember where it is. \
             Replays show both players' markers."
                .into(),
        ),
        ROUND_ZERO_PAGE => (
            "Round 0: Hidden Opening".into(),
            "Every Match begins immediately in Round 0 on an empty board. Both cursors and all \
             chosen locations are hidden during live play; each player's sidebar dots count only \
             their own held choices, filled on the board side of the line. Watch your dots to \
             know where you stand.\n\n\
             Each player privately selects three distinct intersections. Selecting a point you \
             already hold changes nothing: the wooden clack sounds and your line shows a cross \
             until you select again. Once you hold three, a new distinct choice removes your \
             oldest one and becomes the newest.\n\n\
             Nothing further happens until both players hold three choices; only then does the \
             3-2-1-Go countdown begin. You may keep moving, setting or using a Teleport Marker, \
             and replacing choices while it runs.\n\n\
             At Go, all six final slots reveal together. A point chosen by one player gets that \
             player's stone; a point chosen by both becomes permanent Gray. The revealed board is \
             then swept once: every Black or White group with no liberties is removed, both colors \
             judged on the same position, and Capture Points credits each player for the opposing \
             stones removed. Gray is never removed. No suicide, ko, or territory rule runs at this \
             reveal. Ordinary play then starts at Round 1."
                .into(),
        ),
        HIDDEN_MOVES_PAGE => (
            "Round 1+: Hidden Moves & Timing".into(),
            "Each player privately selects any intersection that is empty on the currently \
             visible board, including a suicidal placement. Selecting one of your own stones that \
             borders an opposing group with no liberties instead records an active cleanup: at \
             resolution it removes every such adjacent group at once, places no stone, and leaves \
             your own stone in place. A selected move is hidden from the other player and can be \
             changed by moving the cursor and selecting again before the other player makes a \
             selection.\n\n\
             The round resolves when both players have made a selection. Moves more than 0.1 \
             seconds apart resolve in selection order. Moves within 0.1 seconds resolve \
             simultaneously.\n\n\
             If both players select the same point within 0.1 seconds of each other, the point \
             becomes a permanent Gray stone. After 0.1 seconds, the first move claims the spot, \
             and the other player must select another intersection.\n\n\
             Simultaneous moves are each judged from the same starting board, and the merged \
             result is never re-checked for liberties, so a group can end a round with no \
             liberties and stay standing. Nothing removes it automatically - removing such a \
             group is exactly what an active cleanup is for. Until one does, the group stands, \
             and at scoring it counts like any other stones."
                .into(),
        ),
        CAPTURES_PAGE => (
            "Captures, Repetition & Gray".into(),
            "An ordinary placement removes adjacent opposing groups with no liberties first. If \
             the acting group then has no liberties, that entire group self-removes and gives \
             Capture Points to the opponent when enabled. Sequential moves use the board after \
             the earlier move; simultaneous moves are evaluated from the same starting board.\n\n\
             The side that took a round's stones shows how many; with Reduce Motion on, each \
             removed stone is also outlined where it stood, with an X through it, until the \
             next round resolves.\n\n\
             Full positional repetition prevents a round from recreating any prior completed \
             board position. The board from before the round is restored, and each valid attempted \
             point that was empty becomes Gray. On a one-player timeout, only that player's valid \
             attempt becomes Gray. Tentative captures are discarded.\n\n\
             Gray stones are permanent neutral walls, created by a point both players choose in \
             Round 0, by a same-point simultaneous selection, or by the repetition conversion \
             above. They belong to neither player, do not connect to player groups, cannot be \
             captured, and divide territory. Like the edge of the board, a Gray stone provides \
             no liberties to an adjacent group, so a group can be fully enclosed, and captured, \
             by any mix of Gray stones and opposing stones."
                .into(),
        ),
        ENDING_PAGE => (
            "Ending, Territory & Restart".into(),
            format!(
                "Starting in Round 1, both players hold {} together for 3 seconds to end the \
                 match. If a Select slips, the bar below the board drains at half the speed it \
                 filled rather than emptying, so the hold can be finished without starting over. \
                 The board then \
                 fills each empty region bordered by exactly one player's stones with that player's \
                 color. Regions touching both colors or neither player remain neutral. Gray walls can \
                 separate regions but never own territory.\n\n\
                 The final score counts black and white stones plus filled territory. Gray and \
                 neutral intersections score for neither player.\n\n\
                 During play or after scoring, hold {} for 3 seconds to start a fresh match. \
                 Restarting during play abandons the match without saving a replay. Restart \
                 never interrupts the end gesture, even if Restart was held first.\n\n\
                 Completed matches are saved automatically.",
                keyboard_end_gesture_label(bindings),
                keyboard_restart_label(bindings),
            ),
        ),
        MATCH_RULES_PAGE => (
            "Optional Match Rules".into(),
            "The Game tab in Settings can enable limits; each Match keeps the values it started \
             with. Match Time starts in Round 0. If it expires there, current opening choices \
             reveal and the Match is scored. In ordinary play, the current round first resolves \
             with the available selections.\n\n\
             Round Time starts at Round 1 and resets after each round. At timeout, two selections \
             resolve normally, one is attempted alone, and no selection advances with no stone.\n\n\
             Round Limit, Point Target, and rounds without a board change are checked after \
             ordinary rounds. That streak only grows when a completed round's board exactly \
             matches the board it started with; a placement, a capture, a suicide, a cleanup \
             removal, or a repetition Gray conversion all reset it. Several limits can end the \
             same Match.\n\n\
             Capture Points adds captures, the opponent's self-removed stones, and opposing stones \
             removed by the Round 0 reveal sweep to progressive and final area scores. Disabled \
             rules are Off."
                .into(),
        ),
        WHITE_CONTROLLER_PAGE => {
            controller_player_help_page(Player::Two, controller_bindings, assignments)
        }
        BLACK_CONTROLLER_PAGE => {
            controller_player_help_page(Player::One, controller_bindings, assignments)
        }
        SHARED_CONTROLLER_PAGE => shared_controller_help_page(controller_bindings, assignments),
        _ => unreachable!("help page index is clamped to the available range"),
    }
}

pub(crate) fn keyboard_end_gesture_label(bindings: &KeyboardBindings) -> String {
    format!(
        "{} + {}",
        bindings.key(GameAction::PlayerOneSelect).display_name(),
        bindings.key(GameAction::PlayerTwoSelect).display_name(),
    )
}

fn keyboard_restart_label(bindings: &KeyboardBindings) -> &'static str {
    bindings.key(GameAction::RestartMatch).display_name()
}

/// The restart-hold label: the keyboard key, plus each assigned controller's
/// profile-local Restart input, joined with `/`. Degrades to the bare
/// keyboard label when no controller is assigned to either player.
pub(super) fn restart_label(
    bindings: &KeyboardBindings,
    controller_bindings: &ControllerBindings,
    assignments: &ControllerAssignments,
) -> String {
    let mut labels = vec![
        bindings
            .key(GameAction::RestartMatch)
            .display_name()
            .to_string(),
    ];
    for player in Player::PRIORITY_ORDER {
        if let ControllerSlotStatus::Connected { family, .. } = assignments.status(player) {
            labels.push(
                controller_bindings
                    .input(player, ProfileAction::Restart)
                    .display_name(family)
                    .to_string(),
            );
        }
    }
    labels.join("/")
}

fn controller_player_help_page(
    player: Player,
    controller_bindings: &ControllerBindings,
    assignments: &ControllerAssignments,
) -> (String, String) {
    let title = match player {
        Player::One => "Black / Player 1 Controller",
        Player::Two => "White / Player 2 Controller",
    };
    let family = assignments.family(player);
    let input = |action| {
        controller_bindings
            .input(player, action)
            .display_name(family)
    };
    (
        title.into(),
        format!(
            "{}\n\n\
             Move Up: {}\n\
             Move Down: {}\n\
             Move Left: {}\n\
             Move Right: {}\n\n\
             Select: {}\n\
             Set Teleport Marker: {}\n\
             Teleport to Marker: {}",
            controller_assignment_status(player, assignments),
            input(ProfileAction::Up),
            input(ProfileAction::Down),
            input(ProfileAction::Left),
            input(ProfileAction::Right),
            input(ProfileAction::Select),
            input(ProfileAction::SetMarker),
            input(ProfileAction::JumpToMarker),
        ),
    )
}

fn controller_assignment_status(player: Player, assignments: &ControllerAssignments) -> String {
    match assignments.status(player) {
        ControllerSlotStatus::Unassigned => "No controller assigned".into(),
        ControllerSlotStatus::Connected { family, .. } => {
            format!("Assigned: {}", family.label())
        }
    }
}

fn shared_controller_help_page(
    controller_bindings: &ControllerBindings,
    assignments: &ControllerAssignments,
) -> (String, String) {
    let input = |player, action| {
        controller_bindings
            .input(player, action)
            .display_name(assignments.family(player))
    };
    (
        "Shared Controller Actions".into(),
        format!(
            "END THE MATCH\n\
             White / Player 2 Select: {}\n\
             Black / Player 1 Select: {}\n\
             Starting in Round 1, both players hold their own Select inputs for 3 seconds.\n\n\
             RESTART THE MATCH\n\
             White / Player 2 Restart: {}\n\
             Black / Player 1 Restart: {}\n\
             During play or after scoring, either controller holds its mapped Restart input for 3 seconds.\n\n\
             Controllers are assigned in order: White / Player 2, then Black / Player 1.\n\n\
             MENU BUTTONS\n\
             On every screen but the match itself, and in this window even mid-match, \
             the D-pad highlights a button and South presses it; opening it during a \
             match takes F1 or a pointer. Start goes back on every screen, closing \
             windows like this one first, and on the menu screens so does East - except \
             during a controller binding capture, when East is the input being bound and \
             Start cancels it. During a match, Start asks before returning to the title \
             screen; South confirms, Start cancels, and Start or East closes these \
             instructions there.",
            input(Player::Two, ProfileAction::Select),
            input(Player::One, ProfileAction::Select),
            input(Player::Two, ProfileAction::Restart),
            input(Player::One, ProfileAction::Restart),
        ),
    )
}

#[cfg(test)]
mod tests {
    use bevy::input_focus::InputFocus;
    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::app::{AppScreen, configure_app_systems};
    use crate::audio::AudioCueQueue;
    use crate::gameplay::{GameState, GameplayPause, InputGate};
    use crate::input::controller;
    use crate::input::controller::DeviceFamily;
    use crate::input::keyboard::KeyboardKey;
    use crate::settings::GamePreferences;
    use crate::ui::focus::{UiFocusPlugin, UiFocusState, real_layout_app};
    use crate::ui::hud::{HudPlugin, HudState};
    use crate::ui::widgets::spawn_menu_button;

    #[test]
    fn help_page_indicator_is_centered_and_aligned_with_navigation_buttons() {
        let left = help_pagination_side_node(JustifyContent::FlexEnd);
        let right = help_pagination_side_node(JustifyContent::FlexStart);
        let indicator = help_page_indicator_node();

        assert_eq!(left.flex_basis, px(0));
        assert_eq!(right.flex_basis, left.flex_basis);
        assert_eq!(right.flex_grow, left.flex_grow);
        assert_eq!(left.justify_content, JustifyContent::FlexEnd);
        assert_eq!(right.justify_content, JustifyContent::FlexStart);
        assert_eq!(indicator.width, px(HELP_PAGE_INDICATOR_WIDTH));
        assert_eq!(indicator.height, px(HELP_PAGINATION_HEIGHT));
        assert_eq!(indicator.padding.top, px(HELP_PAGE_INDICATOR_TOP_PADDING));
        assert_eq!(indicator.align_items, AlignItems::Center);
    }

    #[test]
    fn help_modal_fits_the_fixed_window() {
        assert!(HELP_MODAL_WIDTH < crate::app::visuals::WINDOW_WIDTH as f32);
        assert!(HELP_MODAL_HEIGHT < crate::app::visuals::WINDOW_HEIGHT as f32);
    }

    #[test]
    fn controller_pages_use_universal_fallbacks_and_device_aware_labels() {
        let controller_bindings = ControllerBindings::default();
        let mut assignments = ControllerAssignments::default();

        let (title, unassigned) = help_page(
            WHITE_CONTROLLER_PAGE,
            &KeyboardBindings::default(),
            &controller_bindings,
            &assignments,
        );
        assert_eq!(title, "White / Player 2 Controller");
        assert!(unassigned.starts_with("No controller assigned"));
        assert!(unassigned.contains("Select: South Button"));
        assert!(unassigned.contains("Set Teleport Marker: West Button"));

        let entity = Entity::from_raw_u32(1).unwrap();
        assignments.connect(entity, DeviceFamily::Xbox, "Xbox Pad".into());
        let (_, assigned) = help_page(
            WHITE_CONTROLLER_PAGE,
            &KeyboardBindings::default(),
            &controller_bindings,
            &assignments,
        );
        assert!(assigned.starts_with("Assigned: Xbox controller"));
        assert!(assigned.contains("Select: A"));
        assert!(assigned.contains("Set Teleport Marker: X"));
        assert!(assigned.contains("Teleport to Marker: B"));
    }

    #[test]
    fn controller_pages_reflect_remapped_profile_values() {
        let mut controller_bindings = ControllerBindings::default();
        let mut assignments = ControllerAssignments::default();
        assignments.connect(
            Entity::from_raw_u32(2).unwrap(),
            DeviceFamily::Xbox,
            "White Pad".into(),
        );
        assignments.connect(
            Entity::from_raw_u32(3).unwrap(),
            DeviceFamily::PlayStation,
            "Black Pad".into(),
        );
        let (_, original_white) = help_page(
            WHITE_CONTROLLER_PAGE,
            &KeyboardBindings::default(),
            &controller_bindings,
            &assignments,
        );
        assert!(original_white.contains("Select: A"));

        controller_bindings.assign(
            Player::Two,
            ProfileAction::Select,
            controller::ControllerInput::RightBumper,
        );
        controller_bindings.assign(
            Player::One,
            ProfileAction::SetMarker,
            controller::ControllerInput::LeftAnalogTrigger,
        );

        let (_, white) = help_page(
            WHITE_CONTROLLER_PAGE,
            &KeyboardBindings::default(),
            &controller_bindings,
            &assignments,
        );
        let (_, black) = help_page(
            BLACK_CONTROLLER_PAGE,
            &KeyboardBindings::default(),
            &controller_bindings,
            &assignments,
        );
        let (_, shared) = help_page(
            SHARED_CONTROLLER_PAGE,
            &KeyboardBindings::default(),
            &controller_bindings,
            &assignments,
        );

        assert!(white.contains("Select: Right Bumper"));
        assert!(!white.contains("Select: A"));
        assert!(black.starts_with("Assigned: PlayStation controller"));
        assert!(black.contains("Set Teleport Marker: Left Trigger"));
        assert!(shared.contains("White / Player 2 Select: Right Bumper"));
        assert!(shared.contains("Black / Player 1 Select: Cross"));
    }

    #[test]
    fn restart_label_joins_keyboard_with_either_assigned_controller() {
        let mut bindings = KeyboardBindings::default();
        bindings.assign(GameAction::RestartMatch, KeyboardKey::Enter);
        let controller_bindings = ControllerBindings::default();
        let mut assignments = ControllerAssignments::default();

        assert_eq!(
            restart_label(&bindings, &controller_bindings, &assignments),
            "Enter"
        );

        // First connect fills White/Player Two.
        assignments.connect(
            Entity::from_raw_u32(5).unwrap(),
            DeviceFamily::Xbox,
            "Pad".into(),
        );
        let with_one = restart_label(&bindings, &controller_bindings, &assignments);
        assert!(with_one.starts_with("Enter/"));

        // Second connect fills Black/Player One; the restart label should
        // include both assigned controllers' inputs (either restarts).
        assignments.connect(
            Entity::from_raw_u32(6).unwrap(),
            DeviceFamily::PlayStation,
            "Pad2".into(),
        );
        let with_both = restart_label(&bindings, &controller_bindings, &assignments);
        assert_eq!(with_both.matches('/').count(), 2);
    }

    #[test]
    fn help_content_uses_current_selection_and_restart_keys() {
        let mut bindings = KeyboardBindings::default();
        bindings.assign(GameAction::PlayerOneSelect, KeyboardKey::F4);
        bindings.assign(GameAction::PlayerTwoSelect, KeyboardKey::F5);
        bindings.assign(GameAction::RestartMatch, KeyboardKey::Enter);
        let controller_bindings = ControllerBindings::default();
        let assignments = ControllerAssignments::default();

        let controls = help_page(
            KEYBOARD_CONTROLS_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;
        let ending = help_page(ENDING_PAGE, &bindings, &controller_bindings, &assignments).1;

        assert!(controls.contains(
            "Starting in Round 1, hold F4 + F5 together for 3 seconds to end the match."
        ));
        assert!(
            controls.contains("During play or after scoring, hold Enter for 3 seconds to restart.")
        );
        assert!(
            ending.contains(
                "Starting in Round 1, both players hold F4 + F5 together for 3 seconds to end the match."
            )
        );
        // The drain is player-facing behaviour the bar shows but nothing else
        // explains, and the Ending page is the one with room for a sentence;
        // `docs/TESTING.md`'s margins table has its measurement.
        assert!(ending.contains(
            "If a Select slips, the bar below the board drains at half the speed it filled \
             rather than emptying, so the hold can be finished without starting over."
        ));
        assert!(ending.contains(
            "During play or after scoring, hold Enter for 3 seconds to start a fresh match."
        ));
        assert!(
            ending.contains("Restarting during play abandons the match without saving a replay.")
        );
        assert!(
            ending.contains(
                "Restart never interrupts the end gesture, even if Restart was held first."
            )
        );
    }

    /// Pinned whole: unlike every other line on these pages, the menu and
    /// back keys are fixed rather than read from the bindings (`ui`'s focus
    /// module takes them raw so they belong to neither player), so they are
    /// literals - and every scope inside them is load-bearing. The ring
    /// claims are stated as the rule - every screen but the match, plus
    /// this window wherever it is open - now that every menu screen and
    /// this modal have a ring. F1 is the one way into this window a
    /// keyboard has during a match, and the one place a player will find
    /// that out, so both blocks name it: the keyboard page as the key that
    /// opens and closes the window, the controller page as what opening it
    /// mid-match takes, since a pad has no button for it. The match
    /// clauses name Start/Escape asking first because leaving a match is
    /// destructive; the shared page carves out the binding capture, where
    /// East must bind rather than go back and Start is the pad's way to
    /// cancel, and names East as closing this window during a match, the
    /// one thing East does there.
    #[test]
    fn menu_and_back_keys_are_documented_with_their_scopes() {
        let bindings = KeyboardBindings::default();
        let controller_bindings = ControllerBindings::default();
        let assignments = ControllerAssignments::default();

        let keyboard = help_page(
            KEYBOARD_CONTROLS_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;
        assert!(keyboard.ends_with(
            "MENU KEYS\nTab or the arrow keys highlight a button on every screen but the \
             match itself, and in this window even mid-match; Enter or Space presses it. \
             F1 opens and closes this window, mid-match too. Escape closes windows like \
             this one, steps back from Settings, Replays and an open replay, and during \
             a match asks before returning; Enter confirms."
        ));

        let shared = help_page(
            SHARED_CONTROLLER_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;
        assert!(shared.ends_with(
            "MENU BUTTONS\nOn every screen but the match itself, and in this window even \
             mid-match, the D-pad highlights a button and South presses it; opening it \
             during a match takes F1 or a pointer. Start goes back on every screen, \
             closing windows like this one first, and on the menu screens so does East \
             - except during a controller binding capture, when East is the input \
             being bound and Start cancels it. During a match, Start asks before \
             returning to the title screen; South confirms, Start cancels, and Start \
             or East closes these instructions there."
        ));

        // Rebinding a gameplay action onto a menu key must not reword the
        // menu block - the menu keys are not the bindings'.
        let mut remapped = KeyboardBindings::default();
        remapped.assign(GameAction::PlayerOneSelect, KeyboardKey::Tab);
        assert!(
            help_page(
                KEYBOARD_CONTROLS_PAGE,
                &remapped,
                &controller_bindings,
                &assignments,
            )
            .1
            .contains("MENU KEYS\nTab or the arrow keys highlight a button on every screen but")
        );
    }

    #[test]
    fn default_keyboard_help_uses_esdf_and_m_plus_two_end_gesture() {
        let bindings = KeyboardBindings::default();
        let body = help_page(
            KEYBOARD_CONTROLS_PAGE,
            &bindings,
            &ControllerBindings::default(),
            &ControllerAssignments::default(),
        )
        .1;

        assert!(body.contains("Move: E / S / D / F"));
        assert!(body.contains("2: Select a spot"));
        assert!(body.contains("Backtick: Set Teleport Marker"));
        assert!(body.contains("1: Teleport to Marker"));
        assert!(
            body.contains(
                "Starting in Round 1, hold M + 2 together for 3 seconds to end the match."
            )
        );
        assert_eq!(keyboard_end_gesture_label(&bindings), "M + 2");
    }

    /// The modal under real `bevy_ui` layout and the production graph: the
    /// focus core, [`HudPlugin`] - which spawns the modal at `Startup`,
    /// whose button reader opens, pages and closes it, and whose display
    /// pass shows and hides it - and two stand-in menu buttons tagged for
    /// Home, so the Home case has a ring underneath the overlay and the
    /// Match case has none. Starts on Home with one frame run.
    fn help_modal_app() -> App {
        let mut app = real_layout_app(crate::app::visuals::WindowSize::OPENING.0);
        app.add_plugins(StatesPlugin)
            .init_state::<AppScreen>()
            .add_plugins(UiFocusPlugin)
            .add_plugins(HudPlugin)
            .insert_resource(InputGate::default())
            .insert_resource(GameplayPause::default())
            .insert_resource(GameState::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Startup, |mut commands: Commands| {
                commands
                    .spawn((
                        Node {
                            width: percent(100),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        DespawnOnExit(AppScreen::Home),
                    ))
                    .with_children(|menu| {
                        for (order, action) in [HudAction::OpenHelp, HudAction::OpenReplays]
                            .into_iter()
                            .enumerate()
                        {
                            spawn_menu_button(
                                menu,
                                Some(action),
                                "",
                                true,
                                Some(Focusable {
                                    group: FocusGroup::Screen,
                                    order: order as u32,
                                }),
                            );
                        }
                    });
            });
        configure_app_systems(&mut app);
        app.update();
        app
    }

    fn focused(app: &App) -> Option<Entity> {
        app.world().resource::<InputFocus>().get()
    }

    fn hud(app: &App) -> &HudState {
        app.world().resource::<HudState>()
    }

    /// This frame's ring, as the actions its entities carry.
    fn ring(app: &App) -> Vec<HudAction> {
        app.world()
            .resource::<UiFocusState>()
            .candidate_entities()
            .into_iter()
            .map(|entity| *app.world().get::<HudAction>(entity).unwrap())
            .collect()
    }

    fn press_key(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
        keys.press(key);
    }

    /// No `InputPlugin` runs headlessly, so `just_pressed` never expires on
    /// its own; every tap releases what it pressed after its frame.
    fn release_keys(app: &mut App) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        let held: Vec<KeyCode> = keys.get_pressed().copied().collect();
        for key in held {
            keys.release(key);
        }
        keys.clear();
    }

    /// One frame with `key` just pressed, then released.
    fn tap(app: &mut App, key: KeyCode) {
        press_key(app, key);
        app.update();
        release_keys(app);
    }

    /// One frame with `button` just pressed on `pad`, then released - the
    /// pad equivalent of [`tap`].
    fn pad_tap(app: &mut App, pad: Entity, button: GamepadButton) {
        {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            gamepad.digital_mut().release_all();
            gamepad.digital_mut().clear();
            gamepad.digital_mut().press(button);
        }
        app.update();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
        gamepad.digital_mut().release_all();
        gamepad.digital_mut().clear();
    }

    /// Opens the modal the way the Match HUD's pointer button does - the
    /// flag set from `Update`, the layout following the same frame.
    fn open_from_pointer(app: &mut App) {
        app.world_mut().resource_mut::<HudState>().help_open = true;
        app.update();
    }

    /// A pointer click on Close and Escape in the same frame - the click's
    /// release, which is when a click activates - close the modal once,
    /// with one cue: the Help keys see the click's activation and stand
    /// aside for it, as they do for the ring's. Each alone closes it with
    /// that same one cue, so neither half is silent. The modal is opened by
    /// a real click on Home's Help button each time.
    #[test]
    fn a_same_frame_click_on_close_and_escape_close_the_modal_once_with_one_cue() {
        use crate::audio::{AudioCue, UiCue, queue_activation_cue};
        use crate::ui::button::{click, hover, mouse_button, queue_mouse_button};
        use bevy::input::ButtonState;

        let mut app = help_modal_app();
        app.add_observer(queue_activation_cue);
        let button = |app: &mut App, wanted: HudAction| {
            let world = app.world_mut();
            let found: Vec<Entity> = world
                .query::<(Entity, &HudAction)>()
                .iter(world)
                .filter(|(_, action)| **action == wanted)
                .map(|(entity, _)| entity)
                .collect();
            assert_eq!(found.len(), 1, "one {wanted:?} button");
            found[0]
        };
        let help = button(&mut app, HudAction::OpenHelp);
        let close = button(&mut app, HudAction::CloseHelp);
        let open = |app: &mut App| {
            click(app, help);
            assert!(hud(app).help_open);
            app.world_mut().resource_mut::<AudioCueQueue>().drain();
        };
        let cues = |app: &mut App| app.world_mut().resource_mut::<AudioCueQueue>().drain();
        let cancel = AudioCue::Ui(UiCue::Cancel);

        open(&mut app);
        click(&mut app, close);
        assert!(!hud(&app).help_open);
        assert_eq!(cues(&mut app), [cancel], "the click alone");

        open(&mut app);
        tap(&mut app, KeyCode::Escape);
        assert!(!hud(&app).help_open);
        assert_eq!(cues(&mut app), [cancel], "Escape alone");

        open(&mut app);
        hover(&mut app, close);
        mouse_button(&mut app, MouseButton::Left, ButtonState::Pressed);
        assert!(hud(&app).help_open, "a press alone closes nothing");
        queue_mouse_button(&mut app, MouseButton::Left, ButtonState::Released);
        press_key(&mut app, KeyCode::Escape);
        app.update();
        release_keys(&mut app);
        assert!(!hud(&app).help_open);
        assert_eq!(cues(&mut app), [cancel], "both in one frame: one close");
    }

    /// On Home, the modal is entered by activating Help with the
    /// ring, and its buttons then own the ring: hidden they are not
    /// candidates, shown they are, in drawn order after the screen's own.
    /// The arrows page through the unmodified button reader, movement wraps
    /// within the modal, Close by the ring closes it and re-arms the gate,
    /// and the ring is handed back to Help - where it was, not to
    /// the first control. A reopened modal seeds on Left again.
    #[test]
    fn the_modal_takes_the_ring_when_opened_and_hands_it_back_where_it_was() {
        let mut app = help_modal_app();
        assert_eq!(ring(&app), [HudAction::OpenHelp, HudAction::OpenReplays]);

        tap(&mut app, KeyCode::Tab);
        tap(&mut app, KeyCode::Tab);
        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(
            app.world().get::<HudAction>(focused(&app).unwrap()),
            Some(&HudAction::OpenHelp)
        );
        tap(&mut app, KeyCode::Enter);
        assert!(hud(&app).help_open);
        assert!(app.world().resource::<InputGate>().any_gated());
        assert_eq!(
            ring(&app),
            [
                HudAction::OpenHelp,
                HudAction::OpenReplays,
                HudAction::PreviousHelpPage,
                HudAction::NextHelpPage,
                HudAction::CloseHelp,
            ],
            "shown under real layout, the modal's buttons join in drawn order"
        );
        let action_of = |app: &App| *app.world().get::<HudAction>(focused(app).unwrap()).unwrap();
        assert_eq!(action_of(&app), HudAction::PreviousHelpPage);

        tap(&mut app, KeyCode::Tab);
        assert_eq!(action_of(&app), HudAction::NextHelpPage);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(
            hud(&app).help_page,
            1,
            "Right pages through the real reader"
        );
        tap(&mut app, KeyCode::Tab);
        assert_eq!(action_of(&app), HudAction::CloseHelp);
        tap(&mut app, KeyCode::Tab);
        assert_eq!(
            action_of(&app),
            HudAction::PreviousHelpPage,
            "wraps within the modal"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        tap(&mut app, KeyCode::Tab);
        assert_eq!(action_of(&app), HudAction::CloseHelp);

        app.world_mut().resource_mut::<InputGate>().open();
        tap(&mut app, KeyCode::Enter);
        assert!(!hud(&app).help_open);
        assert!(app.world().resource::<InputGate>().any_gated());
        assert_eq!(ring(&app), [HudAction::OpenHelp, HudAction::OpenReplays]);
        assert_eq!(
            action_of(&app),
            HudAction::OpenHelp,
            "the ring returns to where it was, not to the first control"
        );

        tap(&mut app, KeyCode::Enter);
        assert!(hud(&app).help_open);
        assert_eq!(
            action_of(&app),
            HudAction::PreviousHelpPage,
            "a reopened modal seeds on Left, not on the Close it was left from"
        );
    }

    /// During a Match nothing of the screen's own is focusable, so the ring
    /// exists exactly while the modal is open: opened as the pointer opens
    /// it, the modal is paged and closed by the D-pad and South, and closed
    /// by East, by Start and by Escape, each time re-arming the gate,
    /// leaving the Return Home confirmation closed and the published focus
    /// cleared; the keyboard's arrows and Enter drive it the same way. While
    /// it is open, gameplay input is blocked by the same state that admits
    /// the ring.
    #[test]
    fn the_modal_is_operable_during_a_match_by_pad_and_keyboard_and_east_closes_it() {
        let mut app = help_modal_app();
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Match);
        app.update();
        assert_eq!(ring(&app), []);
        assert_eq!(focused(&app), None);
        let action_of = |app: &App| *app.world().get::<HudAction>(focused(app).unwrap()).unwrap();
        let closed_cleanly = |app: &mut App| {
            let hud = app.world().resource::<HudState>();
            assert!(!hud.help_open);
            assert!(!hud.return_home_open);
            assert!(app.world().resource::<InputGate>().any_gated());
            app.update();
            assert_eq!(ring(app), []);
            assert_eq!(focused(app), None, "nothing on the Match is left to focus");
            app.world_mut().resource_mut::<InputGate>().open();
        };

        // By pad.
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        open_from_pointer(&mut app);
        assert!(app.world().resource::<GameplayPause>().input_blocked);
        assert_eq!(
            ring(&app),
            [
                HudAction::PreviousHelpPage,
                HudAction::NextHelpPage,
                HudAction::CloseHelp,
            ]
        );
        assert_eq!(action_of(&app), HudAction::PreviousHelpPage);
        pad_tap(&mut app, pad, GamepadButton::DPadRight);
        assert_eq!(
            action_of(&app),
            HudAction::PreviousHelpPage,
            "the first press reveals"
        );
        pad_tap(&mut app, pad, GamepadButton::DPadRight);
        assert_eq!(action_of(&app), HudAction::NextHelpPage);
        pad_tap(&mut app, pad, GamepadButton::South);
        assert_eq!(hud(&app).help_page, 1);
        assert!(app.world().resource::<GameplayPause>().input_blocked);
        pad_tap(&mut app, pad, GamepadButton::DPadRight);
        assert_eq!(action_of(&app), HudAction::CloseHelp);
        app.world_mut().resource_mut::<InputGate>().open();
        pad_tap(&mut app, pad, GamepadButton::South);
        closed_cleanly(&mut app);

        open_from_pointer(&mut app);
        assert_eq!(
            action_of(&app),
            HudAction::PreviousHelpPage,
            "seeded on Left again"
        );
        pad_tap(&mut app, pad, GamepadButton::East);
        closed_cleanly(&mut app);

        open_from_pointer(&mut app);
        pad_tap(&mut app, pad, GamepadButton::Start);
        closed_cleanly(&mut app);

        // By keyboard. The ring has been visible since the pad's first
        // press, so every arrow moves it.
        open_from_pointer(&mut app);
        assert_eq!(action_of(&app), HudAction::PreviousHelpPage);
        tap(&mut app, KeyCode::ArrowRight);
        assert_eq!(action_of(&app), HudAction::NextHelpPage);
        let page = hud(&app).help_page;
        tap(&mut app, KeyCode::Enter);
        assert_eq!(hud(&app).help_page, page + 1);
        tap(&mut app, KeyCode::ArrowLeft);
        assert_eq!(action_of(&app), HudAction::PreviousHelpPage);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(hud(&app).help_page, page);
        tap(&mut app, KeyCode::ArrowLeft);
        assert_eq!(
            action_of(&app),
            HudAction::CloseHelp,
            "wraps within the modal"
        );
        app.world_mut().resource_mut::<InputGate>().open();
        tap(&mut app, KeyCode::Enter);
        closed_cleanly(&mut app);

        open_from_pointer(&mut app);
        tap(&mut app, KeyCode::Escape);
        closed_cleanly(&mut app);
        assert!(!app.world().resource::<GameplayPause>().input_blocked);
    }

    #[test]
    fn help_page_sequence_is_controls_two_terms_pages_five_rules_pages_then_three_controller_pages()
    {
        let bindings = KeyboardBindings::default();
        let controller_bindings = ControllerBindings::default();
        let assignments = ControllerAssignments::default();
        let titles = (0..HELP_PAGE_COUNT)
            .map(|page| help_page(page, &bindings, &controller_bindings, &assignments).0)
            .collect::<Vec<_>>();

        assert_eq!(
            titles,
            [
                "Keyboard Controls",
                "Terms: Points, Groups & Liberties",
                "Terms: Gray, Suicide, Ko & Territory",
                "Round 0: Hidden Opening",
                "Round 1+: Hidden Moves & Timing",
                "Captures, Repetition & Gray",
                "Ending, Territory & Restart",
                "Optional Match Rules",
                "White / Player 2 Controller",
                "Black / Player 1 Controller",
                "Shared Controller Actions",
            ]
        );
    }

    #[test]
    fn controller_help_content_is_strictly_separated_from_rules_and_other_players() {
        let bindings = KeyboardBindings::default();
        let controller_bindings = ControllerBindings::default();
        let assignments = ControllerAssignments::default();

        for page in [
            KEYBOARD_CONTROLS_PAGE,
            BOARD_TERMS_PAGE,
            GAME_TERMS_PAGE,
            ROUND_ZERO_PAGE,
            HIDDEN_MOVES_PAGE,
            CAPTURES_PAGE,
            ENDING_PAGE,
            MATCH_RULES_PAGE,
        ] {
            let (title, body) = help_page(page, &bindings, &controller_bindings, &assignments);
            assert!(!title.to_lowercase().contains("controller"));
            assert!(!body.to_lowercase().contains("controller"));
        }

        let (_, white) = help_page(
            WHITE_CONTROLLER_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        );
        let (_, black) = help_page(
            BLACK_CONTROLLER_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        );
        let (_, shared) = help_page(
            SHARED_CONTROLLER_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        );

        for player_page in [&white, &black] {
            assert!(player_page.contains("Move Up:"));
            assert!(player_page.contains("Select:"));
            assert!(player_page.contains("Set Teleport Marker:"));
            assert!(player_page.contains("Teleport to Marker:"));
            assert!(!player_page.contains("Restart"));
            assert!(!player_page.contains("3 seconds"));
        }
        assert!(!white.contains("Black / Player 1"));
        assert!(!black.contains("White / Player 2"));
        assert!(!shared.contains("Move"));
        assert!(!shared.contains("Teleport Marker"));
        assert!(shared.contains(
            "Starting in Round 1, both players hold their own Select inputs for 3 seconds."
        ));
        assert!(shared.contains(
            "During play or after scoring, either controller holds its mapped Restart input for 3 seconds."
        ));
    }

    #[test]
    fn help_content_uses_beginner_friendly_rule_copy() {
        let bindings = KeyboardBindings::default();
        let controller_bindings = ControllerBindings::default();
        let assignments = ControllerAssignments::default();
        let controls = help_page(
            KEYBOARD_CONTROLS_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;
        let timing = help_page(
            HIDDEN_MOVES_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;
        let captures = help_page(CAPTURES_PAGE, &bindings, &controller_bindings, &assignments).1;
        let ending = help_page(ENDING_PAGE, &bindings, &controller_bindings, &assignments).1;
        let match_rules = help_page(
            MATCH_RULES_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;
        let opening = help_page(
            ROUND_ZERO_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;

        assert_eq!(controls.matches("Select a spot").count(), 2);
        assert_eq!(controls.matches("Set Teleport Marker").count(), 2);
        assert_eq!(controls.matches("Teleport to Marker").count(), 2);
        assert!(timing.contains(
            "Each player privately selects any intersection that is empty on the currently visible board"
        ));
        assert!(timing.contains("including a suicidal placement."));
        assert!(timing.contains(
            "Selecting one of your own stones that borders an opposing group with no liberties instead records an active cleanup: at resolution it removes every such adjacent group at once, places no stone, and leaves your own stone in place."
        ));
        assert!(!timing.contains("choose"));
        assert!(timing.contains("Moves within 0.1 seconds resolve simultaneously."));
        assert!(timing.contains("the point becomes a permanent Gray stone."));
        assert!(timing.contains(
            "a group can end a round with no liberties and stay standing. Nothing removes it automatically - removing such a group is exactly what an active cleanup is for."
        ));
        assert!(captures.contains(
            "Full positional repetition prevents a round from recreating any prior completed board position."
        ));
        assert!(captures.contains("each valid attempted point that was empty becomes Gray."));
        assert!(captures.contains("that entire group self-removes"));
        assert!(
            captures.contains("simultaneous moves are evaluated from the same starting board.")
        );
        assert!(captures.contains(
            "Gray stones are permanent neutral walls, created by a point both players choose in Round 0, by a same-point simultaneous selection, or by the repetition conversion above."
        ));
        assert!(captures.contains(
            "Like the edge of the board, a Gray stone provides no liberties to an adjacent group"
        ));
        assert!(match_rules.contains("Match Time starts in Round 0."));
        assert!(match_rules.contains(
            "That streak only grows when a completed round's board exactly matches the board it started with; a placement, a capture, a suicide, a cleanup removal, or a repetition Gray conversion all reset it."
        ));
        assert!(match_rules.contains("Several limits can end the same Match."));
        assert!(match_rules.contains(
            "Capture Points adds captures, the opponent's self-removed stones, and opposing stones removed by the Round 0 reveal sweep to progressive and final area scores."
        ));
        assert!(ending.ends_with("Completed matches are saved automatically."));
        assert!(opening.contains("filled on the board side of the line"));
        assert!(opening.contains("removes your oldest one"));
        assert!(opening.contains("becomes permanent Gray"));
        assert!(opening.contains(
            "The revealed board is then swept once: every Black or White group with no liberties is removed, both colors judged on the same position, and Capture Points credits each player for the opposing stones removed."
        ));
        assert!(opening.contains("Gray is never removed."));
        assert!(opening.contains("No suicide, ko, or territory rule runs at this reveal."));
        assert!(opening.contains("Ordinary play then starts at Round 1."));
        assert!(opening.contains(
            "changes nothing: the wooden clack sounds and your line shows a cross until you select again"
        ));
        assert!(opening.contains("Nothing further happens until both players hold three choices"));
    }

    /// The words the rules pages lean on - the four the accessibility review
    /// named, and the ones found by reading the pages as a stranger to Go -
    /// each open an entry on one of the two terms pages, and the pages sit
    /// where a reader meets them before any rules page uses one: no page
    /// before them uses a word they define. The module docs say why that
    /// placement was chosen over the back of the book.
    #[test]
    fn the_terms_pages_define_the_rules_words_before_any_page_uses_them() {
        let bindings = KeyboardBindings::default();
        let controller_bindings = ControllerBindings::default();
        let assignments = ControllerAssignments::default();
        let body = |page| help_page(page, &bindings, &controller_bindings, &assignments).1;
        let terms = [body(BOARD_TERMS_PAGE), body(GAME_TERMS_PAGE)].join("\n\n");

        for term in [
            "POINT",
            "STONE",
            "ADJACENT",
            "GROUP",
            "LIBERTY",
            "CAPTURE",
            "SUICIDE",
            "KO",
            "GRAY",
            "TERRITORY",
            "TELEPORT MARKER",
        ] {
            let entry = format!("{term}: ");
            assert!(
                terms.starts_with(&entry) || terms.contains(&format!("\n\n{entry}")),
                "{term} has no entry opening a paragraph on the terms pages"
            );
        }

        const { assert!(BOARD_TERMS_PAGE < GAME_TERMS_PAGE && GAME_TERMS_PAGE < ROUND_ZERO_PAGE) };
        for page in 0..BOARD_TERMS_PAGE {
            let before = body(page).to_lowercase();
            for word in [
                "libert", "group", "suicid", " ko", "territor", "adjacent", "capture",
            ] {
                assert!(
                    !before.contains(word),
                    "page {page} uses {word:?} before the terms pages define it"
                );
            }
        }
        // The words really are used afterwards, or the pages would be
        // defining nothing: the first rules page uses the load-bearing one.
        assert!(body(ROUND_ZERO_PAGE).contains("liberties"));
    }

    /// The claims that make these definitions this game's rather than Go's,
    /// pinned so a rewording cannot quietly turn them back into standard Go:
    /// suicide is allowed, there is no separate ko rule, Gray is permanent
    /// and provides no liberties, territory is filled rather than counted
    /// empty, and the marker is not drawn in play. The first sentence of
    /// each of the four entries the review named is pinned whole, because
    /// that is where a reader stops.
    #[test]
    fn the_terms_pages_describe_this_game_rather_than_standard_go() {
        let bindings = KeyboardBindings::default();
        let controller_bindings = ControllerBindings::default();
        let assignments = ControllerAssignments::default();
        let board = help_page(
            BOARD_TERMS_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;
        let game = help_page(
            GAME_TERMS_PAGE,
            &bindings,
            &controller_bindings,
            &assignments,
        )
        .1;

        assert!(board.contains("LIBERTY: An empty point adjacent to a group."));
        assert!(board.contains("Gray stones and the edge of the board provide none."));
        assert!(board.contains(
            "GROUP: Stones of one color on adjacent points connect, and a chain of connected stones, however long, is one group, removed as a whole."
        ));
        assert!(board.contains("Gray never connects."));
        assert!(board.contains(
            "A group with none is removed, at once in most cases; Round 1+ says how one can stand until a cleanup."
        ));
        assert!(board.contains("CAPTURE: Removing an opposing group that has no liberties"));
        assert!(board.contains("up, down, left or right"));
        assert!(
            game.contains("SUICIDE: Placing a stone that leaves its own group with no liberties.")
        );
        assert!(game.contains("Standard Go forbids this; this game allows it."));
        assert!(game.contains(
            "KO: Standard Go's rule against a capture and recapture that repeat a position."
        ));
        assert!(game.contains("each valid attempted point that was empty becomes Gray."));
        assert!(game.contains("This game has no separate ko rule"));
        assert!(game.contains("GRAY: A permanent neutral stone, also called a Gray wall."));
        assert!(game.contains("filled with that player's color and counted"));
        assert!(game.contains("It is never drawn during play"));
        for (title, body) in [
            ("Terms: Points, Groups & Liberties", &board),
            ("Terms: Gray, Suicide, Ko & Territory", &game),
        ] {
            assert!(
                !body.contains("Go Together"),
                "{title} names the game, which the other pages never do"
            );
            // The only other "Go" a player meets is the countdown card on
            // the Round 0 page, so the board game is always "Standard Go".
            for (index, _) in body.match_indices("Go") {
                assert!(
                    body[..index].ends_with("Standard "),
                    "{title} says Go without saying which: {:?}",
                    &body[index.saturating_sub(12)..(index + 2).min(body.len())]
                );
            }
        }
    }
}
