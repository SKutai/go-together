//! Spawns the settings screen's frame - the title, the shared instruction and
//! error lines, the tab strip, the four panel containers, the footer and the
//! reset-confirmation modal - and keeps the frame where [`super::geometry`]
//! puts it for the window's size.
//!
//! All four panels are spawned once here and then only shown or hidden by
//! [`super::display`], so switching tabs never respawns entities. The
//! transient state does not survive a switch, though: [`super`]'s
//! `switch_settings_tab_with_commit` commits the Game tab's open editor and
//! cancels any binding capture before changing tabs. That is what makes the
//! frame a module of its own: it owns the containers, and each panel's
//! contents come from whichever module owns that tab.
//!
//! Every control spawned here and in the three panel modules is marked
//! [`Focusable`] in the order it is drawn, with the tab bar, each panel and
//! the footer in the bands [`super::ring`] lays out; the reset
//! confirmation's buttons are the modal group, and its full-screen overlay
//! root is a plain node, not a button, there only to block the pointer -
//! which a node does by being under it (`ui::button`) - so there is nothing
//! there to mark.
//!
//! Each panel's contents come from a module of their own. The two tabs that
//! have state of their own are spawned by [`super::game`] and
//! [`super::bindings`], beside the input rules that give their widgets
//! meaning; the Preferences panel, whose plain toggles and volume stops have
//! no state behind them, by [`super::preferences`], which is a module for
//! its size rather than its kind. The frame still places that panel's
//! surface and switches for the window's size, as it places every node
//! whose place depends on it.
//!
//! # One frame, two arrangements
//!
//! The frame is one tree in either of [`super::geometry`]'s arrangements,
//! and changing between them moves its nodes rather than respawning them.
//! The panel holds a head - the title and the notes under it, which are the
//! instruction and the error line - then a body - the tab strip and the
//! contents the four panels hang in - and, on top, the footer with Reset
//! Controls and Back. In the sidebar the head and the body lie on their
//! sides, the tab strip stands up, and the footer joins the end of the head,
//! which is the one change that moves a node to another parent. Every node
//! whose place depends on the window carries a [`SettingsPlacement`], and
//! [`place`] writes what each arrangement owns of it; the spawners place
//! each one as they make it, and [`place_settings_furniture`] places them
//! all again when the window's size changes. Each arrangement writes every
//! field it owns, so a node moved into one arrangement and back is the node
//! it was.
//!
//! Neither pass needs the other first. [`place_settings_furniture`] writes
//! the placed nodes' `Node` and the error line's justification;
//! [`super::display`] writes the text and visibility of the panels, the
//! instruction, the error lines and the reset dialog, none of which carries
//! a placement. Where both touch one node, the error line, they write
//! different components, so the two stages are left unordered, as
//! `AppSystems` leaves every pair with nothing between them. The
//! display pass decides from the window's size itself whether the sidebar
//! is showing, which is the one thing it has to know about the
//! arrangement: there the top row has room for the instruction or the
//! error, so an error stands in the instruction's place while it is shown.

use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::bindings::{spawn_controller_panel, spawn_keyboard_panel};
use super::game::spawn_game_panel;
use super::geometry::{
    BACK_BUTTON_WIDTH, ERROR_SIZE, FOOTER_GAP, INSTRUCTION_SIZE, PANEL_BORDER, PANEL_GAP,
    RESET_BUTTON_WIDTH, SETTINGS_TITLE_SIZE, SIDEBAR_BODY_GAP, SIDEBAR_HEAD_GAP, SettingsLayout,
    TAB_COLUMN_GAP, TAB_HEIGHT, TAB_LABEL_SIZE, TAB_ROW_GAP, settings_layout,
};
use super::preferences::{
    PREFERENCES_GAP, PREFERENCES_MAX_WIDTH, PREFERENCES_WIDTH, SWITCH_CARD_GAP,
    spawn_preferences_panel,
};
use super::ring::{FOOTER_FOCUS_ORDER, RingOrder, TAB_BAR_FOCUS_ORDER, reset_dialog_focusable};
use super::settings_tab_action_and_label;
use super::{
    MappingPanel, MappingTab, MappingUiRole, PREFERENCES_INSTRUCTION, RESET_MAPPINGS_BODY,
    RESET_MAPPINGS_TITLE, ResetMappingsModal, SETTINGS_TAB_ORDER, SETTINGS_TITLE,
    SettingsTabButton, SettingsTabText,
};
use crate::app::AppScreen;
use crate::app::visuals::WindowSize;
use crate::audio::UiSound;
use crate::presentation::{
    BLUE_BORDER, BUTTON_BACKGROUND, DANGER, ORANGE, OVERLAY_BACKGROUND, PANEL_BACKGROUND,
    TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY,
};
use crate::ui::focus::Focusable;
use crate::ui::hud::{HudAction, ScreenStyled, hud_action_ui_cue};
use crate::ui::widgets::spawn_child_button;

/// The gap between the Keyboard tab's `SHARED` heading and its row, when
/// the two stand on one line.
const SHARED_INLINE_GAP: f32 = 18.0;

/// A node of the Settings frame whose place depends on the window; see the
/// module docs. [`place`] says what each arrangement owns of it.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettingsPlacement {
    /// The screen's root, which pads the panel off the window's edges.
    Root,
    Panel,
    /// The title and the notes, with the footer at their end in the sidebar.
    Head,
    /// The instruction, and the error line under it or in its place.
    Notes,
    /// The tab strip and the contents.
    Body,
    TabStrip,
    Tab,
    /// What the four panels hang in.
    Contents,
    /// Reset Controls and Back.
    Footer,
    /// The Preferences tab's surface, its three switches together, and one
    /// switch.
    PreferencesSurface,
    PreferenceSwitches,
    PreferenceSwitch,
    /// The Keyboard and Controller tabs' two player columns together, and
    /// one column.
    PlayerColumns,
    PlayerColumn,
    /// The Keyboard tab's shared Restart Match group, and its heading.
    SharedGroup,
    SharedHeading,
    /// A Game tab row with a limit in it, and the last row.
    GameRow,
    LastGameRow,
}

/// Writes into `node` what `layout`'s arrangement owns of it as `placement`:
/// every field either arrangement sets, in both, so moving between them
/// leaves nothing of the other behind. The spawners give each node the rest
/// of its fields.
pub(super) fn place(layout: &SettingsLayout, placement: SettingsPlacement, node: &mut Node) {
    use SettingsPlacement as P;

    let sidebar = layout.sidebar();
    let either = |on_top: Val, beside: Val| if sidebar { beside } else { on_top };
    match placement {
        P::Root => {
            node.padding = UiRect::axes(px(layout.root_padding.x), px(layout.root_padding.y));
        }
        P::Panel => {
            node.width = px(layout.panel_size.x);
            node.height = px(layout.panel_size.y);
            node.padding = UiRect::axes(px(layout.panel_padding.x), px(layout.panel_padding.y));
        }
        P::Head => {
            node.flex_direction = if sidebar {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            };
            node.row_gap = either(px(PANEL_GAP), px(0));
            node.column_gap = either(px(0), px(SIDEBAR_HEAD_GAP));
        }
        P::Notes => {
            node.width = either(percent(100), Val::Auto);
            node.align_items = if sidebar {
                AlignItems::FlexStart
            } else {
                AlignItems::Center
            };
            node.flex_grow = if sidebar { 1.0 } else { 0.0 };
            node.flex_basis = either(Val::Auto, px(0));
            node.min_width = either(Val::Auto, px(0));
        }
        P::Body => {
            node.flex_direction = if sidebar {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            };
            node.align_items = if sidebar {
                AlignItems::Stretch
            } else {
                AlignItems::Center
            };
            node.row_gap = either(px(PANEL_GAP), px(0));
            node.column_gap = either(px(0), px(SIDEBAR_BODY_GAP));
        }
        P::TabStrip => {
            node.flex_direction = if sidebar {
                FlexDirection::Column
            } else {
                FlexDirection::Row
            };
            node.width = either(percent(100), Val::Auto);
            node.justify_content = if sidebar {
                JustifyContent::FlexStart
            } else {
                JustifyContent::Center
            };
            node.column_gap = either(px(TAB_ROW_GAP), px(0));
            node.row_gap = either(px(0), px(TAB_COLUMN_GAP));
        }
        P::Tab => node.width = px(layout.tab_width),
        P::Contents => {
            node.width = either(percent(100), Val::Auto);
            node.flex_basis = either(Val::Auto, px(0));
            node.min_width = either(Val::Auto, px(0));
        }
        P::Footer => node.width = either(percent(100), Val::Auto),
        P::PreferencesSurface => {
            node.width = either(px(PREFERENCES_WIDTH), percent(100));
            node.max_width = either(Val::Auto, px(PREFERENCES_MAX_WIDTH));
        }
        P::PreferenceSwitches => {
            node.flex_direction = if sidebar {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            };
            node.row_gap = either(px(PREFERENCES_GAP), px(0));
            node.column_gap = either(px(0), px(PREFERENCES_GAP));
        }
        P::PreferenceSwitch => {
            node.flex_direction = if sidebar {
                FlexDirection::Column
            } else {
                FlexDirection::Row
            };
            node.align_items = if sidebar {
                AlignItems::FlexStart
            } else {
                AlignItems::Center
            };
            node.row_gap = either(px(0), px(SWITCH_CARD_GAP));
            node.width = either(percent(100), Val::Auto);
            node.flex_basis = either(Val::Auto, px(0));
            node.flex_grow = if sidebar { 1.0 } else { 0.0 };
        }
        P::PlayerColumns => node.column_gap = px(layout.player_column_gap),
        P::PlayerColumn => node.width = px(layout.player_column_width),
        P::SharedGroup => {
            // In the sidebar its one row stands beside its heading, so it
            // takes both columns' width, which leaves the row's halves room
            // apart while it captures a key.
            node.width = either(px(layout.player_column_width), px(layout.players_width()));
            node.flex_direction = if sidebar {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            };
            node.align_items = if sidebar {
                AlignItems::Center
            } else {
                AlignItems::Stretch
            };
            node.column_gap = either(px(0), px(SHARED_INLINE_GAP));
        }
        P::SharedHeading => {
            node.margin = if sidebar {
                UiRect::ZERO
            } else {
                UiRect::bottom(px(3))
            };
        }
        P::GameRow => node.min_height = px(layout.game_row_heights()[0]),
        P::LastGameRow => node.min_height = px(layout.game_row_heights()[1]),
    }
}

/// `node` with its place in `layout` written into it, and the marker that
/// keeps it there.
pub(super) fn placed(
    layout: &SettingsLayout,
    placement: SettingsPlacement,
    mut node: Node,
) -> (Node, SettingsPlacement) {
    place(layout, placement, &mut node);
    (node, placement)
}

/// The error line's justification: centred under the instruction on top,
/// and from the left in the sidebar, where it stands in the instruction's
/// place beside the title.
fn error_justify(layout: &SettingsLayout) -> Justify {
    if layout.sidebar() {
        Justify::Left
    } else {
        Justify::Center
    }
}

/// Keeps the Settings frame where [`super::geometry`] puts it for the
/// window's size: every placed node, the footer's parent - the panel on
/// top, the head in the sidebar - and the error line's justification. It
/// runs on a frame the window's size changed; the spawners place every
/// node already, through the same [`place`].
pub(super) fn place_settings_furniture(
    mut commands: Commands,
    window: Res<WindowSize>,
    mut nodes: Query<(Entity, &SettingsPlacement, &mut Node, Option<&ChildOf>)>,
    mut texts: Query<(&MappingUiRole, &mut TextLayout)>,
) {
    let layout = settings_layout(window.layout());
    let (mut head, mut panel, mut footer) = (None, None, None);
    for (entity, placement, mut node, parent) in &mut nodes {
        let mut placed = node.clone();
        place(&layout, *placement, &mut placed);
        if *node != placed {
            *node = placed;
        }
        match placement {
            SettingsPlacement::Head => head = Some(entity),
            SettingsPlacement::Panel => panel = Some(entity),
            SettingsPlacement::Footer => footer = Some((entity, parent.map(ChildOf::parent))),
            _ => {}
        }
    }
    if let (Some(head), Some(panel), Some((footer, parent))) = (head, panel, footer) {
        // Appended: the footer is the last child of either parent.
        let wanted = if layout.sidebar() { head } else { panel };
        if parent != Some(wanted) {
            commands.entity(footer).insert(ChildOf(wanted));
        }
    }
    let justify = error_justify(&layout);
    for (role, mut text_layout) in &mut texts {
        if matches!(role, MappingUiRole::Error) && text_layout.justify != justify {
            text_layout.justify = justify;
        }
    }
}

/// Spawns the Settings screen for the window's size: the frame in the
/// arrangement [`super::geometry`] gives it, every tab's panel, and the
/// reset confirmation.
pub(super) fn spawn_control_mapping_screen(mut commands: Commands, window: Res<WindowSize>) {
    let layout = settings_layout(window.layout());
    let mut head = None;
    let mut panel_entity = None;
    commands
        .spawn((
            placed(
                &layout,
                SettingsPlacement::Root,
                Node {
                    width: percent(100),
                    height: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ),
            DespawnOnExit(AppScreen::ControlMapping),
        ))
        .with_children(|root| {
            let mut panel = root.spawn((
                placed(
                    &layout,
                    SettingsPlacement::Panel,
                    Node {
                        border: UiRect::all(px(PANEL_BORDER)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(PANEL_GAP),
                        ..default()
                    },
                ),
                BackgroundColor(PANEL_BACKGROUND),
                BorderColor::all(ORANGE),
            ));
            panel_entity = Some(panel.id());
            panel.with_children(|panel| {
                let mut head_commands = panel.spawn(placed(
                    &layout,
                    SettingsPlacement::Head,
                    Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                ));
                head = Some(head_commands.id());
                head_commands.with_children(|head| {
                    head.spawn((
                        Text::new(SETTINGS_TITLE),
                        TextFont {
                            font_size: FontSize::Px(SETTINGS_TITLE_SIZE),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                    ));
                    head.spawn(placed(
                        &layout,
                        SettingsPlacement::Notes,
                        Node {
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            row_gap: px(PANEL_GAP),
                            ..default()
                        },
                    ))
                    .with_children(|notes| {
                        notes.spawn((
                            Text::new(PREFERENCES_INSTRUCTION),
                            TextFont {
                                font_size: FontSize::Px(INSTRUCTION_SIZE),
                                ..default()
                            },
                            TextColor(TEXT_MUTED),
                            MappingUiRole::Instruction,
                        ));
                        notes.spawn((
                            Text::new(""),
                            TextFont {
                                font_size: FontSize::Px(ERROR_SIZE),
                                ..default()
                            },
                            TextColor(DANGER),
                            TextLayout::justify(error_justify(&layout)),
                            mapping_error_node(),
                            MappingUiRole::Error,
                        ));
                    });
                });
                panel
                    .spawn(placed(
                        &layout,
                        SettingsPlacement::Body,
                        Node {
                            width: percent(100),
                            flex_grow: 1.0,
                            ..default()
                        },
                    ))
                    .with_children(|body| {
                        body.spawn(placed(
                            &layout,
                            SettingsPlacement::TabStrip,
                            Node::default(),
                        ))
                        .with_children(|tabs| {
                            let mut ring = RingOrder::starting_at(TAB_BAR_FOCUS_ORDER);
                            for tab in SETTINGS_TAB_ORDER {
                                let (action, label) = settings_tab_action_and_label(tab);
                                spawn_settings_tab_button(
                                    tabs,
                                    &layout,
                                    tab,
                                    action,
                                    label,
                                    ring.next(),
                                );
                            }
                        });
                        body.spawn(placed(
                            &layout,
                            SettingsPlacement::Contents,
                            Node {
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                        ))
                        .with_children(|contents| {
                            spawn_tab_panels(contents, &layout);
                        });
                    });
            });
        });

    // Reset Controls and Back, last in the ring in either arrangement: at
    // the panel's foot on top, and at the end of its top row in the sidebar.
    let parent =
        if layout.sidebar() { head } else { panel_entity }.expect("the frame was spawned above");
    commands
        .spawn((
            placed(
                &layout,
                SettingsPlacement::Footer,
                Node {
                    justify_content: JustifyContent::Center,
                    column_gap: px(FOOTER_GAP),
                    ..default()
                },
            ),
            ChildOf(parent),
        ))
        .with_children(|footer| {
            let mut ring = RingOrder::starting_at(FOOTER_FOCUS_ORDER);
            spawn_child_button(
                footer,
                HudAction::RequestResetMappings,
                "Reset Controls",
                RESET_BUTTON_WIDTH,
                Some(ring.next()),
            );
            spawn_child_button(
                footer,
                HudAction::BackToHome,
                "Back",
                BACK_BUTTON_WIDTH,
                Some(ring.next()),
            );
        });

    spawn_reset_mappings_modal(&mut commands);
}

/// The four tabs' panels, all spawned and only the Preferences one shown.
fn spawn_tab_panels(contents: &mut ChildSpawnerCommands, layout: &SettingsLayout) {
    contents
        .spawn((
            Node {
                width: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(18),
                ..default()
            },
            MappingPanel(MappingTab::Preferences),
        ))
        .with_children(|panel| spawn_preferences_panel(panel, layout));
    contents
        .spawn((
            Node {
                width: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            MappingPanel(MappingTab::Game),
        ))
        .with_children(|panel| spawn_game_panel(panel, layout));
    contents
        .spawn((
            Node {
                width: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                display: Display::None,
                ..default()
            },
            MappingPanel(MappingTab::Keyboard),
        ))
        .with_children(|panel| spawn_keyboard_panel(panel, layout));
    contents
        .spawn((
            Node {
                width: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                display: Display::None,
                ..default()
            },
            MappingPanel(MappingTab::Controller),
        ))
        .with_children(|panel| spawn_controller_panel(panel, layout));
}

fn spawn_settings_tab_button(
    parent: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    tab: MappingTab,
    action: HudAction,
    label: &'static str,
    focus: Focusable,
) {
    parent
        .spawn((
            Button,
            action,
            UiSound::new(hud_action_ui_cue(action)),
            SettingsTabButton(tab),
            // The tab's fill, border and click belong to
            // `update_settings_tab_buttons`, which draws the selected tab
            // inert. Without this the HUD's shared display pass would write
            // the same fill and re-enable the same click every frame.
            ScreenStyled,
            focus,
            placed(
                layout,
                SettingsPlacement::Tab,
                Node {
                    height: px(TAB_HEIGHT),
                    border: UiRect::all(px(1)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ),
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BLUE_BORDER),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(TAB_LABEL_SIZE),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            SettingsTabText(tab),
        ));
}

pub(super) fn mapping_error_node() -> Node {
    Node {
        width: percent(100),
        justify_content: JustifyContent::Center,
        ..default()
    }
}

fn spawn_reset_mappings_modal(commands: &mut Commands) {
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
            ResetMappingsModal,
            DespawnOnExit(AppScreen::ControlMapping),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(520),
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
                        Text::new(RESET_MAPPINGS_TITLE),
                        TextFont {
                            font_size: FontSize::Px(30.0),
                            ..default()
                        },
                        TextColor(TEXT_HEADING),
                    ));
                    modal.spawn((
                        Text::new(RESET_MAPPINGS_BODY),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(TEXT_PRIMARY),
                        TextLayout::justify(Justify::Center),
                    ));
                    modal
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::Center,
                            column_gap: px(16),
                            ..default()
                        })
                        .with_children(|choices| {
                            // Drawn Confirm-then-Cancel; ringed Cancel-first,
                            // so the dialog seeds on the safe button. See
                            // `super::ring`.
                            spawn_child_button(
                                choices,
                                HudAction::ConfirmResetMappings,
                                "Confirm Reset",
                                150.0,
                                Some(reset_dialog_focusable(true)),
                            );
                            spawn_child_button(
                                choices,
                                HudAction::CancelResetMappings,
                                "Cancel",
                                120.0,
                                Some(reset_dialog_focusable(false)),
                            );
                        });
                });
        });
}

#[cfg(test)]
pub(super) mod tests {
    use bevy::input::keyboard::KeyboardInput;
    use bevy::ui::{ComputedNode, UiGlobalTransform};
    use bevy::window::{PrimaryWindow, WindowResolution};

    use super::super::display::{
        update_control_mapping_display, update_game_settings_display, update_mapping_tab_panels,
        update_settings_tab_buttons,
    };
    use super::super::geometry::{SettingsArrangement, settings_layout};
    use super::super::{ControlMappingState, SettingsPlugin};
    use super::*;
    use crate::app::visuals::{NAMED_WINDOW_SIZES, WindowLayoutPlugin};
    use crate::app::{AppScreen, configure_app_systems};
    use crate::audio::AudioCueQueue;
    use crate::input::controller::{
        ControllerAssignments, ControllerBindings, ControllerCaptureState,
    };
    use crate::input::keyboard::KeyboardBindings;
    use crate::match_settings::MatchSettings;
    use crate::settings::{GamePreferences, SettingsStore};
    use crate::ui::focus::{FocusYield, real_layout_app};

    /// The sizes the real-layout tests run at: every named size, and the
    /// two either side of the change of arrangement.
    pub(in crate::ui::settings) fn real_layout_sizes() -> Vec<Vec2> {
        let mut sizes = NAMED_WINDOW_SIZES.to_vec();
        sizes.extend([Vec2::new(1040.0, 854.0), Vec2::new(1040.0, 853.0)]);
        sizes
    }

    /// The Settings screen as spawned in a window of `window` logical
    /// pixels, under real layout, in `mapping`'s state and with
    /// `match_settings`, with its placement pass and its whole display
    /// chain, run for two frames. The pass runs before the chain here; the
    /// game leaves the two unordered, which it can because they write
    /// different components (see `place_settings_furniture`).
    pub(in crate::ui::settings) fn laid_out_settings(
        window: Vec2,
        mapping: ControlMappingState,
        match_settings: MatchSettings,
    ) -> App {
        let mut app = real_layout_app(window);
        app.insert_resource(mapping)
            .insert_resource(match_settings)
            .insert_resource(GamePreferences::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .add_systems(Startup, spawn_control_mapping_screen)
            .add_systems(
                Update,
                (
                    place_settings_furniture,
                    (
                        update_control_mapping_display,
                        update_game_settings_display,
                        update_mapping_tab_panels,
                        update_settings_tab_buttons,
                    )
                        .chain(),
                )
                    .chain(),
            );
        app.update();
        app.update();
        app
    }

    /// A node's box as `bevy_ui` laid it out, in logical pixels from the
    /// window's top-left.
    pub(in crate::ui::settings) fn node_rect(app: &App, entity: Entity) -> Rect {
        let node = app.world().get::<ComputedNode>(entity).unwrap();
        let transform = app.world().get::<UiGlobalTransform>(entity).unwrap();
        let scale = node.inverse_scale_factor;
        Rect::from_center_size(transform.translation * scale, node.size * scale)
    }

    /// Every entity carrying `placement`.
    fn placed_entities(app: &mut App, placement: SettingsPlacement) -> Vec<Entity> {
        let world = app.world_mut();
        world
            .query::<(Entity, &SettingsPlacement)>()
            .iter(world)
            .filter(|(_, found)| **found == placement)
            .map(|(entity, _)| entity)
            .collect()
    }

    /// The one entity carrying `placement`.
    pub(in crate::ui::settings) fn placed_entity(
        app: &mut App,
        placement: SettingsPlacement,
    ) -> Entity {
        let found = placed_entities(app, placement);
        assert_eq!(found.len(), 1, "one {placement:?}");
        found[0]
    }

    fn assert_near(actual: Rect, expected: Rect, what: &str) {
        assert!(
            (actual.min - expected.min).abs().max_element() <= 1.0
                && (actual.max - expected.max).abs().max_element() <= 1.0,
            "{what}: laid out at {actual:?}, the layout puts it at {expected:?}"
        );
    }

    /// The room the frame gives a tab's contents: what every tab must fit,
    /// since a tab taller than that grows the contents past it rather than
    /// being clipped. In the sidebar the layout's own rect; on top the
    /// opening window's, moved with the panel.
    pub(in crate::ui::settings) fn contents_room(layout: &SettingsLayout) -> Rect {
        match layout.arrangement {
            SettingsArrangement::Sidebar => layout.sidebar_contents_rect(),
            SettingsArrangement::TabsOnTop => {
                let offset = layout.panel_rect().min - Vec2::new(55.0, 35.0);
                Rect::from_corners(
                    Vec2::new(85.0, 209.0) + offset,
                    Vec2::new(955.0, 787.0) + offset,
                )
            }
        }
    }

    /// At every real-layout size the frame stands where the layout says,
    /// within the pixel `bevy_ui` rounds to: the panel, and in the sidebar
    /// the title and the two buttons in its top row, each tab in its column
    /// and the contents beside them. In the opening window the frame is
    /// exactly where the screen always had it - the panel at 55,35, the
    /// tabs in a row 155 px down, the tab's contents 870x578 from 85,209
    /// and the two buttons at the panel's foot - and in a window of the
    /// same arrangement it is the same frame, moved to the window's middle.
    #[test]
    fn the_frame_stands_where_the_layout_puts_it_at_every_size() {
        use super::super::SettingsTabButton;

        for window in real_layout_sizes() {
            let layout = settings_layout(window);
            let mut app = laid_out_settings(
                window,
                ControlMappingState::new(None),
                MatchSettings::default(),
            );
            let panel = placed_entity(&mut app, SettingsPlacement::Panel);
            assert_near(node_rect(&app, panel), layout.panel_rect(), "the panel");
            let contents = placed_entity(&mut app, SettingsPlacement::Contents);
            let contents = node_rect(&app, contents);
            let footer = placed_entity(&mut app, SettingsPlacement::Footer);
            let footer = node_rect(&app, footer);
            let tab_entities: Vec<(MappingTab, Entity)> = {
                let world = app.world_mut();
                world
                    .query::<(Entity, &SettingsTabButton)>()
                    .iter(world)
                    .map(|(entity, tab)| (tab.0, entity))
                    .collect()
            };
            let mut tabs: Vec<(MappingTab, Rect)> = tab_entities
                .into_iter()
                .map(|(tab, entity)| (tab, node_rect(&app, entity)))
                .collect();
            tabs.sort_by_key(|(tab, _)| SETTINGS_TAB_ORDER.iter().position(|t| t == tab));
            match layout.arrangement {
                SettingsArrangement::Sidebar => {
                    let title = crate::ui::focus::laid_out_texts(&mut app)
                        .into_iter()
                        .find(|(text, _)| text == super::super::SETTINGS_TITLE)
                        .map(|(_, rect)| rect)
                        .expect("the title is shown");
                    assert_near(title, layout.sidebar_title_rect(), "the title");
                    assert_near(contents, layout.sidebar_contents_rect(), "the contents");
                    assert_near(footer, layout.sidebar_footer_rect(), "the two buttons");
                    for ((tab, rect), expected) in tabs.iter().zip(layout.sidebar_tab_rects()) {
                        assert_near(*rect, expected, &format!("{window}: {tab:?}"));
                    }
                }
                SettingsArrangement::TabsOnTop => {
                    // The opening window's numbers, moved with the panel.
                    let offset = layout.panel_rect().min - Vec2::new(55.0, 35.0);
                    let at = |x0: f32, y0: f32, x1: f32, y1: f32| {
                        Rect::new(x0 + offset.x, y0 + offset.y, x1 + offset.x, y1 + offset.y)
                    };
                    assert_eq!(contents, at(85.0, 209.0, 955.0, 787.0), "{window}");
                    assert_eq!(footer, at(85.0, 799.0, 955.0, 835.0), "{window}");
                    for (index, (tab, rect)) in tabs.iter().enumerate() {
                        let x = 183.0 + index as f32 * 173.0;
                        assert_eq!(*rect, at(x, 155.0, x + 155.0, 197.0), "{window}: {tab:?}");
                    }
                }
            }
        }
    }

    /// At every real-layout size, on both binding tabs, the two players'
    /// columns stand side by side as wide as the layout makes them, the
    /// layout's gap apart, and centred together in the tab's contents -
    /// not pushed to its edges, and not one of them left at the opening
    /// window's width.
    #[test]
    fn the_player_columns_stand_together_in_the_middle_at_every_size() {
        for window in real_layout_sizes() {
            let layout = settings_layout(window);
            for tab in [MappingTab::Keyboard, MappingTab::Controller] {
                let context = format!("{window}, {tab:?}");
                let mut mapping = ControlMappingState::new(None);
                mapping.tab = tab;
                let mut app = laid_out_settings(window, mapping, MatchSettings::default());
                let contents = placed_entity(&mut app, SettingsPlacement::Contents);
                let contents = node_rect(&app, contents);
                let mut columns: Vec<Rect> =
                    placed_entities(&mut app, SettingsPlacement::PlayerColumn)
                        .into_iter()
                        .filter(|entity| {
                            app.world()
                                .get::<ComputedNode>(*entity)
                                .is_some_and(|node| node.size() != Vec2::ZERO)
                        })
                        .map(|entity| node_rect(&app, entity))
                        .collect();
                columns.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
                assert_eq!(columns.len(), 2, "{context}: two columns shown");
                for (column, (left, right)) in
                    columns.iter().zip(layout.player_column_spans(contents))
                {
                    assert!(
                        (column.min.x - left).abs() <= 1.0 && (column.max.x - right).abs() <= 1.0,
                        "{context}: a column at {column:?}, not {left}..{right}"
                    );
                }
                let gap = columns[1].min.x - columns[0].max.x;
                assert!(
                    (gap - layout.player_column_gap).abs() <= 1.0,
                    "{context}: the columns are {gap} apart"
                );
            }
        }
    }

    /// The Keyboard tab's shared Restart Match binding stands under its
    /// heading on top, as it always has, and in the sidebar beside it, on
    /// one line: the heading's line and the row's share a middle, the
    /// heading clear of the row to its left, the group as wide as the two
    /// players' columns over it. While the row captures a key, its halves -
    /// `Restart Match` and `Press a key...` - keep at least 12 px apart by
    /// their glyphs in both arrangements.
    #[test]
    fn the_shared_binding_stands_beside_its_heading_in_the_sidebar() {
        use crate::input::GameAction;

        for window in real_layout_sizes() {
            let mut mapping = ControlMappingState::new(None);
            mapping.tab = MappingTab::Keyboard;
            mapping.capturing = Some(GameAction::RestartMatch);
            let mut app = laid_out_settings(window, mapping, MatchSettings::default());
            let rows = row_halves_drawn(&mut app, "Restart Match");
            assert_eq!(rows.len(), 1, "{window}: one shared row");
            let (label, binding) = rows[0];
            assert!(
                (label.center().y - binding.center().y).abs() < 2.0,
                "{window}: one line each"
            );
            let gap = binding.min.x - label.max.x;
            assert!(gap >= 12.0, "{window}: {gap} px between the halves");
            let drawn = crate::ui::focus::drawn_texts(&mut app);
            assert!(
                drawn.iter().any(|(_, text, _)| text == "Press a key..."),
                "{window}: the row is capturing"
            );
            let heading = placed_entity(&mut app, SettingsPlacement::SharedHeading);
            let group = placed_entity(&mut app, SettingsPlacement::SharedGroup);
            let children: Vec<Entity> =
                app.world().get::<Children>(group).unwrap().iter().collect();
            assert_eq!(children.len(), 2, "{window}: the heading and its row");
            assert_eq!(children[0], heading, "{window}");
            let heading = node_rect(&app, heading);
            let row = node_rect(&app, children[1]);
            let layout = settings_layout(window);
            if layout.sidebar() {
                let group = node_rect(&app, group);
                assert!(
                    (group.width() - layout.players_width()).abs() <= 1.0,
                    "{window}: the group is {} wide of {}",
                    group.width(),
                    layout.players_width()
                );
                assert!(heading.max.x <= row.min.x, "{window}: {heading:?} {row:?}");
                assert!(
                    (heading.center().y - row.center().y).abs() <= 1.0,
                    "{window}: {heading:?} {row:?}"
                );
            } else {
                assert!(heading.max.y <= row.min.y, "{window}: {heading:?} {row:?}");
            }
        }
    }

    /// The entity of the one panel on show.
    fn shown_panel(app: &mut App) -> Entity {
        let world = app.world_mut();
        let shown: Vec<Entity> = world
            .query::<(Entity, &MappingPanel, &Node)>()
            .iter(world)
            .filter(|(_, _, node)| node.display != Display::None)
            .map(|(entity, ..)| entity)
            .collect();
        assert_eq!(shown.len(), 1, "one panel shown");
        shown[0]
    }

    /// Every laid-out node under `entity`, with its box.
    fn laid_out_descendants(app: &App, entity: Entity) -> Vec<(Entity, Rect)> {
        let mut found = Vec::new();
        let mut stack = vec![entity];
        while let Some(parent) = stack.pop() {
            let Some(children) = app.world().get::<Children>(parent) else {
                continue;
            };
            for child in children.iter() {
                let Some(node) = app.world().get::<ComputedNode>(child) else {
                    continue;
                };
                if node.size() == Vec2::ZERO {
                    continue;
                }
                found.push((child, node_rect(app, child)));
                stack.push(child);
            }
        }
        found
    }

    /// Fails unless no two texts drawn in `app` meet, by their glyphs.
    fn assert_no_texts_meet(app: &mut App, context: &str) {
        use crate::ui::focus::drawn_texts;

        let drawn = drawn_texts(app);
        assert!(drawn.len() > 10, "{context}: the screen's texts are drawn");
        for (index, (_, a_text, a)) in drawn.iter().enumerate() {
            for (_, b_text, b) in &drawn[index + 1..] {
                assert!(
                    a.intersect(*b).is_empty(),
                    "{context}: {a_text:?} at {a:?} meets {b_text:?} at {b:?}"
                );
            }
        }
    }

    /// At every real-layout size and on every tab, under real layout and
    /// real picking: everything the tab draws stands inside its panel, and
    /// the panel inside the contents; every text stands inside the window
    /// and no two meet; and the pointer reaches every control shown at its
    /// centre. With the reset confirmation open, its two buttons are
    /// reached and every control beneath it is covered.
    #[test]
    fn every_tab_fits_and_every_control_is_reached_at_every_size() {
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::laid_out_texts;

        for window in real_layout_sizes() {
            let inside = Rect::from_corners(Vec2::ZERO, window);
            for tab in SETTINGS_TAB_ORDER {
                let context = format!("{window}, {tab:?}");
                let mut mapping = ControlMappingState::new(None);
                mapping.tab = tab;
                let mut app = laid_out_settings(window, mapping, MatchSettings::default());
                let panel = shown_panel(&mut app);
                let panel_rect = node_rect(&app, panel);
                let contents = placed_entity(&mut app, SettingsPlacement::Contents);
                let contents = node_rect(&app, contents);
                assert_near(
                    contents,
                    contents_room(&settings_layout(window)),
                    &format!("{context}: the contents"),
                );
                assert!(
                    contents.inflate(0.5).contains(panel_rect.min)
                        && contents.inflate(0.5).contains(panel_rect.max),
                    "{context}: the panel {panel_rect:?} leaves the contents {contents:?}"
                );
                let drawn = laid_out_descendants(&app, panel);
                assert!(drawn.len() > 10, "{context}");
                for (entity, rect) in drawn {
                    assert!(
                        panel_rect.inflate(0.5).contains(rect.min)
                            && panel_rect.inflate(0.5).contains(rect.max),
                        "{context}: {entity:?} at {rect:?} leaves the panel {panel_rect:?}"
                    );
                }
                for (text, rect) in laid_out_texts(&mut app) {
                    assert!(
                        inside.contains(rect.min) && inside.contains(rect.max),
                        "{context}: {text:?} at {rect:?}"
                    );
                }
                assert_no_texts_meet(&mut app, &context);
                let buttons = reachable_buttons(&mut app);
                assert!(buttons.len() >= 10, "{context}");
                assert_eq!(
                    covered_at_their_centres(&mut app, &buttons),
                    [],
                    "{context}"
                );

                if tab == MappingTab::Controller {
                    let under = reachable_buttons(&mut app);
                    app.world_mut()
                        .resource_mut::<ControlMappingState>()
                        .reset_confirmation_open = true;
                    app.update();
                    app.update();
                    let dialog: Vec<Entity> = {
                        let world = app.world_mut();
                        world
                            .query::<(Entity, &HudAction)>()
                            .iter(world)
                            .filter(|(_, action)| {
                                matches!(
                                    action,
                                    HudAction::CancelResetMappings
                                        | HudAction::ConfirmResetMappings
                                )
                            })
                            .map(|(entity, _)| entity)
                            .collect()
                    };
                    assert_eq!(dialog.len(), 2, "{context}");
                    assert_eq!(covered_at_their_centres(&mut app, &dialog), [], "{context}");
                    assert_eq!(
                        covered_at_their_centres(&mut app, &under),
                        under,
                        "{context}: under the dialog every control is covered"
                    );
                    for (text, rect) in laid_out_texts(&mut app) {
                        assert!(
                            inside.contains(rect.min) && inside.contains(rect.max),
                            "{context}, the dialog open: {text:?} at {rect:?}"
                        );
                    }
                }
            }
        }
    }

    /// For each binding row whose label reads `label`, the glyphs of its
    /// two halves as drawn: the label's, and its binding's, in the same row.
    fn row_halves_drawn(app: &mut App, label: &str) -> Vec<(Rect, Rect)> {
        use crate::ui::focus::drawn_texts;

        let rows: Vec<(Entity, Entity)> = {
            let world = app.world_mut();
            world
                .query::<(Entity, &Text, &ChildOf)>()
                .iter(world)
                .filter(|(_, text, _)| text.0 == label)
                .map(|(entity, _, child_of)| (entity, child_of.parent()))
                .collect()
        };
        let drawn = drawn_texts(app);
        let ink = |entity: Entity| {
            drawn
                .iter()
                .find(|(drawn, ..)| *drawn == entity)
                .map(|(_, _, rect)| *rect)
        };
        rows.into_iter()
            .filter_map(|(label, row)| {
                let children = app.world().get::<Children>(row).unwrap();
                assert_eq!(children.len(), 2, "a row is its label and its binding");
                Some((ink(label)?, ink(children[1])?))
            })
            .collect()
    }

    /// The Controller tab's longest row with the default binding, `Select /
    /// Revise Selection` beside `South Button`, keeps its halves' glyphs
    /// at least 12 px apart at every real-layout size: 14.6 on top, as they
    /// always were, and 12.6 at the smallest width, where the margins have
    /// narrowed for it - and 12.6 at 1043, 1059 and 1084 px wide too, where
    /// the contents' width is odd and a column's half pixel is rounded away.
    /// The layout reckons the gap from the font's advance: 12.8 at 1040 px
    /// wide, and 12.3 at those three, from a column of 417.5 px. `bevy_ui`
    /// lays each text box out up to a pixel wider than its advance, and the
    /// glyphs stand inside their boxes.
    #[test]
    fn the_longest_default_row_keeps_its_halves_apart_at_every_size() {
        let odd = [
            Vec2::new(1043.0, 600.0),
            Vec2::new(1059.0, 600.0),
            Vec2::new(1084.0, 600.0),
        ];
        for window in real_layout_sizes().into_iter().chain(odd) {
            let mut mapping = ControlMappingState::new(None);
            mapping.tab = MappingTab::Controller;
            let mut app = laid_out_settings(window, mapping, MatchSettings::default());
            let rows = row_halves_drawn(&mut app, "Select / Revise Selection");
            assert_eq!(rows.len(), 2, "{window}: one row a player");
            for (label, binding) in rows {
                assert!(
                    (label.center().y - binding.center().y).abs() < 2.0,
                    "{window}: one line each"
                );
                let gap = binding.min.x - label.max.x;
                assert!(gap >= 12.0, "{window}: {gap} px between the halves");
                if odd.contains(&window) {
                    assert!((12.5..12.7).contains(&gap), "{window}: {gap}");
                }
                if !settings_layout(window).sidebar() {
                    assert!((14.5..14.7).contains(&gap), "{window}: {gap}");
                }
                if window == WindowSize::MINIMUM.0 {
                    assert!((12.5..12.7).contains(&gap), "{window}: {gap}");
                }
            }
        }
    }

    /// What a binding longer than the default's does - `Right Stick Right`,
    /// bound to Select, which makes the row wider than its column on top as
    /// in the sidebar: the label and the binding each wrap onto two lines,
    /// side by side inside the row, as they always have on top, and neither
    /// meets the other nor leaves the row.
    #[test]
    fn a_long_binding_wraps_both_halves_of_its_row_inside_it() {
        use crate::input::controller::{ControllerInput, ProfileAction};
        use crate::rules::Player;

        for window in [WindowSize::OPENING.0, WindowSize::MINIMUM.0] {
            let mut mapping = ControlMappingState::new(None);
            mapping.tab = MappingTab::Controller;
            let mut app = real_layout_app(window);
            let mut bindings = ControllerBindings::default();
            bindings.assign(
                Player::Two,
                ProfileAction::Select,
                ControllerInput::RightStickRight,
            );
            app.insert_resource(mapping)
                .insert_resource(MatchSettings::default())
                .insert_resource(GamePreferences::default())
                .insert_resource(KeyboardBindings::default())
                .insert_resource(bindings)
                .insert_resource(ControllerAssignments::default())
                .add_systems(Startup, spawn_control_mapping_screen)
                .add_systems(
                    Update,
                    (update_control_mapping_display, update_mapping_tab_panels).chain(),
                );
            app.update();
            app.update();
            let long_row: Vec<Entity> = {
                let world = app.world_mut();
                world
                    .query::<(&Text, &ChildOf)>()
                    .iter(world)
                    .filter(|(text, _)| text.0 == "Right Stick Right")
                    .map(|(_, child_of)| child_of.parent())
                    .collect()
            };
            assert_eq!(long_row.len(), 1, "{window}");
            let row = node_rect(&app, long_row[0]);
            let halves: Vec<Entity> = app.world().get::<Children>(long_row[0]).unwrap().to_vec();
            let lines = |app: &App, entity: Entity| {
                app.world()
                    .get::<bevy::text::TextLayoutInfo>(entity)
                    .unwrap()
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.line_index + 1)
                    .max()
                    .unwrap_or(0)
            };
            assert_eq!(
                [lines(&app, halves[0]), lines(&app, halves[1])],
                [2, 2],
                "{window}: both halves wrap"
            );
            let drawn = row_halves_drawn(&mut app, "Select / Revise Selection");
            let (label, binding) = drawn
                .into_iter()
                .find(|(label, _)| row.contains(label.center()))
                .expect("the long row's halves are drawn");
            assert!(
                binding.min.x > label.max.x,
                "{window}: {label:?} {binding:?}"
            );
            for half in [label, binding] {
                assert!(
                    row.contains(half.min) && row.contains(half.max),
                    "{window}: {half:?} leaves the row {row:?}"
                );
            }
        }
    }

    struct NoSettingsStorage;

    impl crate::platform::SettingsStorage for NoSettingsStorage {
        fn load(&self) -> Result<Option<String>, crate::platform::SettingsStorageError> {
            Ok(None)
        }

        fn save(&self, _json: &str) -> Result<(), crate::platform::SettingsStorageError> {
            Ok(())
        }
    }

    /// Settings entered from Home the way the game enters it - its own
    /// plugin, the game's window layout and its stage graph - in a primary
    /// window of `window` logical pixels. No `bevy_ui` runs: what is
    /// compared is the frame as placed, not as laid out.
    fn entered_settings(window: Vec2) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .add_message::<KeyboardInput>()
            .insert_state(AppScreen::Home)
            .add_plugins(WindowLayoutPlugin)
            .add_plugins(SettingsPlugin::new(None))
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(SettingsStore::from_storage(NoSettingsStorage))
            .insert_resource(ControllerAssignments::default())
            .insert_resource(ControllerCaptureState::default())
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<FocusYield>()
            .init_resource::<crate::diagnostics::Diagnostics>();
        configure_app_systems(&mut app);
        let physical = window.as_uvec2();
        app.world_mut().spawn((
            Window {
                resolution: WindowResolution::new(physical.x, physical.y)
                    .with_scale_factor_override(1.0),
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ControlMapping);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::ControlMapping
        );
        app
    }

    /// Resizes the primary window to `window` logical pixels and runs a
    /// frame, as a player dragging its edge would.
    fn resize(app: &mut App, window: Vec2) {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(world)
            .unwrap()
            .resolution
            .set(window.x, window.y);
        app.update();
    }

    /// The Settings frame as one list, depth first in child order: every
    /// node under the screen's root with its depth, its placement, its text,
    /// its text's justification and its whole `Node`.
    fn frame_tree(app: &mut App) -> Vec<String> {
        fn walk(world: &World, entity: Entity, depth: usize, out: &mut Vec<String>) {
            out.push(format!(
                "{depth} {:?} {:?} {:?} {:?}",
                world.get::<SettingsPlacement>(entity),
                world.get::<Text>(entity).map(|text| text.0.clone()),
                world.get::<TextLayout>(entity),
                world.get::<Node>(entity),
            ));
            if let Some(children) = world.get::<Children>(entity) {
                for child in children.iter() {
                    walk(world, child, depth + 1, out);
                }
            }
        }
        let root = placed_entity(app, SettingsPlacement::Root);
        let mut out = Vec::new();
        walk(app.world(), root, 0, &mut out);
        out
    }

    /// A window smaller than the minimum - a browser's page can be - gets
    /// the frame the minimum gets: at 800x400 the screen's own readings of
    /// the window raise it to 1040x600, so every node of the frame, its text
    /// and its place are what they are at 1040x600 - entered there, where the
    /// frame is the spawner's own, and resized to it from the opening window
    /// with a long warning on the error line, where the placement pass moves
    /// the frame and the display pass cuts the warning. As the game enters
    /// and resizes it, through the screen's own plugin.
    #[test]
    fn a_window_below_the_minimum_gets_the_minimums_frame() {
        let below = Vec2::new(800.0, 400.0);
        let mut entered = entered_settings(below);
        let mut minimum = entered_settings(WindowSize::MINIMUM.0);
        assert_eq!(frame_tree(&mut entered), frame_tree(&mut minimum));
        // A window resized below the minimum, the error line showing a
        // message its place cuts: the placement pass, which runs on the
        // change of size, and the display pass, which cuts the message to
        // the place's width, both read the window raised to the minimum.
        let warning = longest_load_warnings()[1].clone();
        let mut resized = entered_settings(WindowSize::OPENING.0);
        resized
            .world_mut()
            .resource_mut::<ControlMappingState>()
            .error = warning.clone();
        resized.update();
        resize(&mut resized, below);
        resized.update();
        minimum
            .world_mut()
            .resource_mut::<ControlMappingState>()
            .error = warning;
        minimum.update();
        minimum.update();
        assert_eq!(frame_tree(&mut resized), frame_tree(&mut minimum));
    }

    /// When the window's size changes while Settings is shown, the frame
    /// becomes exactly what entering it at the new size makes - every node's
    /// place, the footer's parent and the error line's justification - into
    /// the sidebar, within it, back to tabs on top at the switch, and back
    /// in the opening window to the frame that was first spawned there.
    /// Through the game's own plugin, window layout and stage graph, from
    /// the window's own size: the pass that does it runs on a change of the
    /// window's size, and a pass that never ran, or ran before the size was
    /// read, would leave the frame as it was.
    #[test]
    fn a_change_of_window_size_rearranges_settings_as_a_fresh_entry_would() {
        let mut app = entered_settings(WindowSize::OPENING.0);
        let opening = frame_tree(&mut app);
        assert!(opening.len() > 100, "the whole frame is walked");
        let mut seen_sidebar = false;
        for size in [
            WindowSize::MINIMUM.0,
            Vec2::new(1440.0, 848.0),
            Vec2::new(1040.0, 853.0),
            Vec2::new(1040.0, 854.0),
            Vec2::new(1920.0, 1080.0),
        ] {
            resize(&mut app, size);
            let moved = frame_tree(&mut app);
            let fresh = frame_tree(&mut entered_settings(size));
            assert_eq!(moved, fresh, "{size}");
            if settings_layout(size).sidebar() {
                seen_sidebar = true;
                assert_ne!(moved, opening, "{size}: the sidebar is another frame");
            }
        }
        assert!(seen_sidebar);
        resize(&mut app, WindowSize::OPENING.0);
        assert_eq!(frame_tree(&mut app), opening);
    }

    /// In the sidebar the top row has room for the instruction or the
    /// error, and the error stands in the instruction's place while it is
    /// shown: from the left of the room between the title and the buttons,
    /// within the title's line, and the instruction comes back when the
    /// error clears. On top both are shown, as they always were: the error
    /// centred under the instruction.
    #[test]
    fn in_the_sidebar_an_error_stands_in_the_instructions_place() {
        use super::super::bindings::UNSUPPORTED_KEY_MESSAGE;
        use crate::ui::focus::laid_out_texts;

        const INSTRUCTION: &str = "Choose a key binding, then press a key to change it.";
        for window in [WindowSize::MINIMUM.0, WindowSize::OPENING.0] {
            let sidebar = settings_layout(window).sidebar();
            let mut mapping = ControlMappingState::new(None);
            mapping.tab = MappingTab::Keyboard;
            mapping.error = UNSUPPORTED_KEY_MESSAGE.into();
            let mut app = laid_out_settings(window, mapping, MatchSettings::default());
            let find = |app: &mut App, wanted: &str| {
                laid_out_texts(app)
                    .into_iter()
                    .find(|(text, _)| text == wanted)
                    .map(|(_, rect)| rect)
            };
            let error = find(&mut app, UNSUPPORTED_KEY_MESSAGE).expect("the error is shown");
            let instruction = find(&mut app, INSTRUCTION);
            let head = placed_entity(&mut app, SettingsPlacement::Head);
            let head = node_rect(&app, head);
            if sidebar {
                assert_eq!(instruction, None, "{window}: the error takes its place");
                let notes = settings_layout(window).sidebar_notes_rect();
                assert!(
                    (error.min.x - notes.min.x).abs() <= 1.0,
                    "{window}: {error:?}"
                );
                assert!(
                    head.contains(error.min) && head.contains(error.max),
                    "{window}"
                );
                assert!((head.height() - 46.0).abs() < 0.5, "{window}: {head:?}");
            } else {
                let instruction = instruction.expect("both are shown on top");
                assert!(error.min.y >= instruction.max.y + 11.5, "{window}");
                assert!(
                    (error.center().x - instruction.center().x).abs() <= 1.0,
                    "{window}"
                );
            }
            app.world_mut()
                .resource_mut::<ControlMappingState>()
                .error
                .clear();
            app.update();
            app.update();
            assert_eq!(find(&mut app, UNSUPPORTED_KEY_MESSAGE), None, "{window}");
            let instruction = find(&mut app, INSTRUCTION).expect("the instruction is back");
            if sidebar {
                let notes = settings_layout(window).sidebar_notes_rect();
                assert!((instruction.min.x - notes.min.x).abs() <= 1.0, "{window}");
            }
        }
    }

    /// A settings storage whose every load fails with `message`, as the
    /// platform's does when the file cannot be read.
    struct FailingSettingsStorage(String);

    impl crate::platform::SettingsStorage for FailingSettingsStorage {
        fn load(&self) -> Result<Option<String>, crate::platform::SettingsStorageError> {
            Err(crate::platform::SettingsStorageError::Failed(
                self.0.clone(),
            ))
        }

        fn save(&self, _json: &str) -> Result<(), crate::platform::SettingsStorageError> {
            Ok(())
        }
    }

    /// The longest warnings the settings loader writes, as it writes them:
    /// every section of a file that fell back, five preferences each quoting
    /// a value of 10,000 characters, and a file that could not be read from
    /// a path with a home folder of 120 characters.
    fn longest_load_warnings() -> [String; 2] {
        use crate::settings::{ControlsConfig, SettingsStore, load_user_controls};

        let long = "x".repeat(10_000);
        let json = format!(
            r#"{{"version": 6, "keyboard": "k", "controller": "c", "game": "g",
                "preferences": {{"show_match_timer": "{long}", "show_round_number": "{long}",
                "game_sound_effects": "{long}", "ui_sound_effects": "{long}",
                "reduce_motion": "{long}"}}}}"#
        );
        let (_, sections) = ControlsConfig::from_json(&json).unwrap();
        let path = format!(
            "/Users/{}/Library/Application Support/go-together/controls.json",
            "a".repeat(120)
        );
        let storage = FailingSettingsStorage(format!(
            "Could not read settings from {path}: Permission denied (os error 13)"
        ));
        let (_, unread) = load_user_controls(&SettingsStore::from_storage(storage));
        [sections.unwrap(), unread.unwrap()]
    }

    /// The error line held to its place: a message that fits is shown
    /// whole, the screen's own messages among them in both arrangements, and
    /// one line with more than printable ASCII in it as it is; a
    /// longer one keeps the lines there is room for, wrapped at the
    /// characters a line holds a pixel short of the place, the last ending
    /// with "..." - also where a run of spaces is what was cut away; a word
    /// longer than a line is split; and characters are counted, not bytes.
    #[test]
    fn the_error_line_is_cut_to_the_lines_it_has_room_for() {
        use super::super::bindings::{CONTROLLER_DISCONNECTED_MESSAGE, UNSUPPORTED_KEY_MESSAGE};
        use super::super::display::shown_error;
        use super::super::geometry::ERROR_SIZE;
        use super::super::{CONNECT_A_CONTROLLER_MESSAGE, SETTINGS_NOT_SAVED_WARNING};
        use crate::ui::line_breaks::chars_per_line;

        for window in [WindowSize::MINIMUM.0, WindowSize::OPENING.0] {
            let layout = settings_layout(window);
            let (width, room) = layout.error_line();
            assert_eq!(room, if layout.sidebar() { 2 } else { 1 }, "{window}");
            let chars = chars_per_line(width - 1.0, ERROR_SIZE);
            let short = "Keyboard bindings were missing; using keyboard defaults.";
            assert_eq!(shown_error(short, &layout), short, "{window}");
            // One line with more than printable ASCII in it is drawn as it
            // is, its run of spaces too, which the game's own lines close.
            let accented = "Could not read /home/jos\u{e9}/settings.json:  denied.";
            assert_eq!(shown_error(accented, &layout), accented, "{window}");
            // The screen's own messages, the longest of them 103 characters
            // and filling the line on top, are all shown whole.
            for message in [
                SETTINGS_NOT_SAVED_WARNING,
                UNSUPPORTED_KEY_MESSAGE,
                CONTROLLER_DISCONNECTED_MESSAGE,
                CONNECT_A_CONTROLLER_MESSAGE,
            ] {
                assert_eq!(shown_error(message, &layout), message, "{window}");
            }
            assert_eq!(SETTINGS_NOT_SAVED_WARNING.len(), 103);
            // A value of 200 spaces, which take characters but make no
            // line: cut, and marked as cut.
            let spaces = format!(
                "Preference show_match_timer was invalid (invalid type: string \"{}\", \
                 expected a boolean); using its default.",
                " ".repeat(200)
            );
            let shown = shown_error(&spaces, &layout);
            assert!(shown.ends_with("..."), "{window}: {shown:?}");
            for long in longest_load_warnings()
                .into_iter()
                .chain(["\u{e9}".repeat(5_000), "word ".repeat(5_000)])
            {
                let shown = shown_error(&long, &layout);
                let lines: Vec<&str> = shown.lines().collect();
                assert_eq!(lines.len(), room, "{window}: {shown:?}");
                assert!(shown.ends_with("..."), "{window}: {shown:?}");
                for line in &lines {
                    assert!(line.chars().count() <= chars, "{window}: {line:?}");
                }
                let kept: String = lines.concat();
                assert!(
                    long.replace(' ', "")
                        .starts_with(&kept.trim_end_matches('.').replace(' ', "")),
                    "{window}: {shown:?} is not the message's start"
                );
            }
        }
    }

    /// The error line, drawn as `shown` in `window` under real layout, held
    /// to its place: on the lines the place has room for, two beside the
    /// title in the sidebar, which stays one title tall, and one under the
    /// instruction on top; every text inside the window and no two meeting;
    /// the tab's contents clear of Reset Controls and Back; and every
    /// control reached by the pointer.
    fn assert_the_error_keeps_to_its_place(
        app: &mut App,
        window: Vec2,
        shown: &str,
        context: &str,
    ) {
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::{drawn_texts, laid_out_texts, measure_drawn_lines};

        let layout = settings_layout(window);
        let lines = measure_drawn_lines(app, shown);
        let room = if layout.sidebar() { 2 } else { 1 };
        assert!(
            (1..=room).contains(&lines.len()),
            "{context}: {} lines",
            lines.len()
        );
        let inside = Rect::from_corners(Vec2::ZERO, window);
        for (text, rect) in laid_out_texts(app) {
            assert!(
                inside.contains(rect.min) && inside.contains(rect.max),
                "{context}: {text:?} at {rect:?}"
            );
        }
        let drawn = drawn_texts(app);
        for (index, (_, a_text, a)) in drawn.iter().enumerate() {
            for (_, b_text, b) in &drawn[index + 1..] {
                assert!(
                    a.intersect(*b).is_empty(),
                    "{context}: {a_text:?} meets {b_text:?}"
                );
            }
        }
        let contents = placed_entity(app, SettingsPlacement::Contents);
        let contents = node_rect(app, contents);
        let footer = placed_entity(app, SettingsPlacement::Footer);
        let footer = node_rect(app, footer);
        assert!(contents.intersect(footer).is_empty(), "{context}");
        assert!(
            inside.contains(footer.min) && inside.contains(footer.max),
            "{context}: {footer:?}"
        );
        if layout.sidebar() {
            let head = placed_entity(app, SettingsPlacement::Head);
            let head = node_rect(app, head);
            assert!((head.height() - 46.0).abs() < 0.5, "{context}: {head:?}");
        }
        let buttons = reachable_buttons(app);
        assert_eq!(covered_at_their_centres(app, &buttons), [], "{context}");
    }

    /// The longest warning the settings loader writes, on Preferences, the
    /// tab Settings opens on, under real layout at 1040x600, either side of
    /// 854 px and in the opening window, in both arrangements: cut to two
    /// lines beside the title in the sidebar and to one under the
    /// instruction on top, and keeping to its place
    /// ([`assert_the_error_keeps_to_its_place`]). A warning that fits is
    /// drawn as it is, so the opening window shows it as it always did.
    #[test]
    fn the_longest_load_warning_keeps_to_its_place_in_both_arrangements() {
        let short = "Keyboard bindings were missing; using keyboard defaults.".to_string();
        let mut warnings = longest_load_warnings().to_vec();
        warnings.push(short.clone());
        for window in [
            WindowSize::MINIMUM.0,
            Vec2::new(1040.0, 853.0),
            Vec2::new(1040.0, 854.0),
            WindowSize::OPENING.0,
        ] {
            let layout = settings_layout(window);
            for warning in &warnings {
                let context = format!("{window}, a warning of {} characters", warning.len());
                let mut app = laid_out_settings(
                    window,
                    ControlMappingState::new(Some(warning.clone())),
                    MatchSettings::default(),
                );
                let shown = super::super::display::shown_error(warning, &layout);
                if *warning == short {
                    assert_eq!(shown, short, "{context}: shown as it is");
                }
                assert_the_error_keeps_to_its_place(&mut app, window, &shown, &context);
            }
        }
    }

    /// Messages that break where `bevy_ui` breaks them, on the error line
    /// under real layout in the sidebar at 1040x600, where a line holds 56
    /// characters, and at 1150x768, where it holds 67, and on top in the
    /// opening window, where it holds 103: one whose longest word breaks
    /// after its hyphens and one whose longest word breaks after its
    /// slashes, each word longer than the narrowest sidebar's line, and one
    /// with a space before "!" as the last character that line holds, where
    /// no line may break, so that the word before it moves down and
    /// `bevy_ui` would set the message on a third line there. At 1040x600
    /// the first two are drawn as they are, on two lines, and the third in
    /// the game's own two lines, broken at its spaces. Elsewhere each is
    /// drawn as it is: on two lines in the sidebar at 1150x768, and on one
    /// line on top, as it always was. Only the 1040x600 states tell this fit
    /// from a wrap at spaces alone, which draws all three whole at the other
    /// two sizes as well. Each keeps to its place as the longest warning
    /// does.
    #[test]
    fn the_error_line_fits_a_message_where_bevy_ui_breaks_it() {
        use super::super::display::shown_error;
        use super::super::geometry::ERROR_SIZE;
        use crate::ui::line_breaks::chars_per_line;

        const HYPHENS: &str = concat!(
            "Could not save settings to ",
            "go-together-keyboard-and-controller-bindings-backup.json."
        );
        const SLASHES: &str = concat!(
            "Could not save settings to ",
            "~/.config/games/settings/keyboard/controller/controls.json."
        );
        const SPACED_BANG: &str = concat!(
            "The controller was unplugged while you were reassigning ! ",
            "Plug it back in and press a button to go on."
        );
        let narrowest = settings_layout(WindowSize::MINIMUM.0);
        assert_eq!(
            chars_per_line(narrowest.error_line().0 - 1.0, ERROR_SIZE),
            56
        );
        let game_lines = SPACED_BANG.replacen(" ! ", "\n! ", 1);
        // The message, and how it is drawn at 1040x600.
        let messages = [
            (HYPHENS, HYPHENS),
            (SLASHES, SLASHES),
            (SPACED_BANG, game_lines.as_str()),
        ];
        for window in [
            WindowSize::MINIMUM.0,
            Vec2::new(1150.0, 768.0),
            WindowSize::OPENING.0,
        ] {
            let layout = settings_layout(window);
            assert_eq!(layout.sidebar(), window != WindowSize::OPENING.0);
            for (message, at_the_minimum) in messages {
                let context = format!("{window}, {message:?}");
                let mut app = laid_out_settings(
                    window,
                    ControlMappingState::new(Some(message.into())),
                    MatchSettings::default(),
                );
                let shown = shown_error(message, &layout);
                if window == WindowSize::MINIMUM.0 {
                    assert_eq!(shown, at_the_minimum, "{context}");
                } else {
                    assert_eq!(shown, message, "{context}: shown as it is");
                }
                assert_the_error_keeps_to_its_place(&mut app, window, &shown, &context);
            }
        }
    }

    /// The error line's text node is its place with the place's edges
    /// rounded to the display's pixels, so a line of it holds the characters
    /// of a pixel less than the place or those of a pixel more, and a
    /// message is drawn as it is only where it fits at both. In the sidebar
    /// the place follows the window's width, and at some widths the two
    /// counts are one apart: at 1064x600 the place is 479.6 px, a pixel less
    /// holds 56 characters and a pixel more 57, and the node `bevy_ui` lays
    /// the text out in is 479 px, which holds 57. A message with a space
    /// before "!" as the 57th character of its first line takes two lines at
    /// 56 and three at 57: at 1064x600 it is set in the game's own two
    /// lines, and at 1040x600, where both counts are 56, it is drawn as it
    /// is. At 1081x600 the place less a pixel is an exact number of advances,
    /// 59, which a line holds whole: a message whose first line is 59
    /// characters is drawn as it is there, on two lines. In each window the
    /// node holds one of the two counts, and the message keeps to its place
    /// as the longest warning does. Under real layout.
    #[test]
    fn the_error_line_fits_a_message_at_both_counts_its_place_can_hold() {
        use super::super::display::shown_error;
        use super::super::geometry::ERROR_SIZE;
        use crate::ui::line_breaks::chars_per_line;

        const WIDER_BANG: &str = concat!(
            "The controller was disconnected while you were remapping ! ",
            "Plug it back in and press any button to go on mapping."
        );
        const EXACT_FIT: &str = concat!(
            "Could not load settings: the keyboard section was stored by ",
            "a newer version of the game, which this one cannot read."
        );
        let game_lines = WIDER_BANG.replacen(" ! ", "\n! ", 1);
        // The window, the characters a pixel less and a pixel more than its
        // place hold, the message, and how it is drawn.
        let cases = [
            (Vec2::new(1040.0, 600.0), [56, 56], WIDER_BANG, WIDER_BANG),
            (
                Vec2::new(1064.0, 600.0),
                [56, 57],
                WIDER_BANG,
                game_lines.as_str(),
            ),
            (Vec2::new(1081.0, 600.0), [59, 59], EXACT_FIT, EXACT_FIT),
        ];
        for (window, counts, message, drawn) in cases {
            let context = format!("{window}, {message:?}");
            let layout = settings_layout(window);
            assert!(layout.sidebar(), "{context}");
            let (width, room) = layout.error_line();
            assert_eq!(room, 2, "{context}");
            assert_eq!(
                [width - 1.0, width + 1.0].map(|width| chars_per_line(width, ERROR_SIZE)),
                counts,
                "{context}: {width} px"
            );
            let mut app = laid_out_settings(
                window,
                ControlMappingState::new(Some(message.into())),
                MatchSettings::default(),
            );
            let shown = shown_error(message, &layout);
            assert_eq!(shown, drawn, "{context}");
            let world = app.world_mut();
            let node = world
                .query::<(&MappingUiRole, &ComputedNode)>()
                .iter(world)
                .find(|(role, _)| matches!(role, MappingUiRole::Error))
                .map(|(_, node)| node.size.x * node.inverse_scale_factor)
                .expect("the error line is laid out");
            assert!(
                counts.contains(&chars_per_line(node, ERROR_SIZE)),
                "{context}: a node {node} px wide"
            );
            assert_the_error_keeps_to_its_place(&mut app, window, &shown, &context);
        }
        // At 1081x600 a hair less than the place less a pixel holds 58.
        let (width, _) = settings_layout(Vec2::new(1081.0, 600.0)).error_line();
        assert_eq!(chars_per_line(width - 1.01, ERROR_SIZE), 58);
    }

    /// Every message the screen itself puts on its error line, and every
    /// tab's instruction, fits the sidebar's top row in the narrowest window
    /// without making it taller than the title's line: two lines at most,
    /// beside the title and clear of the buttons.
    #[test]
    fn the_screens_own_messages_keep_the_sidebars_top_row_one_title_tall() {
        use super::super::bindings::{CONTROLLER_DISCONNECTED_MESSAGE, UNSUPPORTED_KEY_MESSAGE};
        use super::super::{
            CONNECT_A_CONTROLLER_MESSAGE, SETTINGS_NOT_SAVED_WARNING, mapping_instruction,
        };

        let window = WindowSize::MINIMUM.0;
        let mut messages: Vec<(MappingTab, &str)> = [
            SETTINGS_NOT_SAVED_WARNING,
            UNSUPPORTED_KEY_MESSAGE,
            CONTROLLER_DISCONNECTED_MESSAGE,
            CONNECT_A_CONTROLLER_MESSAGE,
        ]
        .into_iter()
        .map(|message| (MappingTab::Keyboard, message))
        .collect();
        messages.extend(SETTINGS_TAB_ORDER.map(|tab| (tab, "")));
        for (tab, message) in messages {
            let mut mapping = ControlMappingState::new(None);
            mapping.tab = tab;
            mapping.error = message.into();
            let mut app = laid_out_settings(window, mapping, MatchSettings::default());
            let shown = if message.is_empty() {
                mapping_instruction(tab)
            } else {
                message
            };
            let head = placed_entity(&mut app, SettingsPlacement::Head);
            let head = node_rect(&app, head);
            assert!(
                (head.height() - 46.0).abs() < 0.5,
                "{shown:?} makes the top row {}",
                head.height()
            );
            let lines = crate::ui::focus::measure_drawn_lines(&mut app, shown);
            assert!(
                (1..=2).contains(&lines.len()),
                "{shown:?}: {} lines",
                lines.len()
            );
            let footer = placed_entity(&mut app, SettingsPlacement::Footer);
            let footer = node_rect(&app, footer);
            for line in lines {
                assert!(line.max.x < footer.min.x, "{shown:?} reaches the buttons");
            }
        }
    }
}
