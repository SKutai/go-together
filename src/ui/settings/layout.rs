//! Spawns the settings screen's frame - the title, the shared instruction and
//! error lines, the tab strip, the four panel containers, the footer and the
//! reset-confirmation modal - plus the Preferences panel itself.
//!
//! All four panels are spawned once here and then only shown or hidden by
//! [`super::display`], so switching tabs never respawns entities. The
//! transient state does not survive a switch, though: [`super`]'s
//! `switch_settings_tab_with_commit` commits the Game tab's open editor and
//! cancels any binding capture before changing tabs. That is what makes the
//! frame a module of its own: it owns the containers, and each panel's
//! contents come from whichever module owns that tab.
//!
//! Every control spawned here and in the two tab modules is marked
//! [`Focusable`] in the order it is drawn, with the tab bar, each panel and
//! the footer in the bands [`super::ring`] lays out; the reset
//! confirmation's buttons are the modal group, and its full-screen overlay
//! root is a plain node, not a button, there only to block the pointer -
//! which a node does by being under it (`ui::button`) - so there is nothing
//! there to mark.
//!
//! The Preferences panel stays here rather than earning a module of its own.
//! Its widgets are plain toggles and volume stops with no state behind them -
//! a press applies and persists immediately, and there is nothing to capture,
//! parse or commit - so it is furniture in the same sense the frame is. The
//! two tabs that do have state of their own are spawned by
//! [`super::game`] and [`super::bindings`].

use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::bindings::{spawn_controller_panel, spawn_keyboard_panel};
use super::game::spawn_game_panel;
use super::ring::{
    FOOTER_FOCUS_ORDER, RingOrder, TAB_BAR_FOCUS_ORDER, panel_focus_order, reset_dialog_focusable,
};
use super::settings_tab_action_and_label;
use super::{
    MappingPanel, MappingTab, MappingUiRole, PREFERENCES_INSTRUCTION, PreferenceToggle,
    PreferenceValue, RESET_MAPPINGS_BODY, RESET_MAPPINGS_TITLE, ResetMappingsModal,
    SETTINGS_TAB_ORDER, SETTINGS_TITLE, SettingsTabButton, SettingsTabText, VolumeStop,
    VolumeStopLabel,
};
use crate::app::AppScreen;
use crate::audio::{AudioChannel, UiCue, UiSound};
use crate::platform::settings_storage_note;
use crate::presentation::{
    BLUE_BORDER, BUTTON_BACKGROUND, DANGER, ORANGE, OVERLAY_BACKGROUND, PANEL_BACKGROUND,
    SURFACE_BACKGROUND, TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY,
};
use crate::settings::VolumeLevel;
use crate::ui::focus::Focusable;
use crate::ui::hud::{HudAction, ScreenStyled, hud_action_ui_cue};
use crate::ui::widgets::spawn_child_button;

pub(super) fn spawn_control_mapping_screen(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(28)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            DespawnOnExit(AppScreen::ControlMapping),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(930),
                    height: px(830),
                    padding: UiRect::all(px(28)),
                    border: UiRect::all(px(2)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(12),
                    ..default()
                },
                BackgroundColor(PANEL_BACKGROUND),
                BorderColor::all(ORANGE),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(SETTINGS_TITLE),
                    TextFont {
                        font_size: FontSize::Px(38.0),
                        ..default()
                    },
                    TextColor(TEXT_HEADING),
                ));
                panel.spawn((
                    Text::new(PREFERENCES_INSTRUCTION),
                    TextFont {
                        font_size: FontSize::Px(16.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    MappingUiRole::Instruction,
                ));
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(DANGER),
                    TextLayout::justify(Justify::Center),
                    mapping_error_node(),
                    MappingUiRole::Error,
                ));
                panel
                    .spawn(Node {
                        width: percent(100),
                        justify_content: JustifyContent::Center,
                        column_gap: px(18),
                        ..default()
                    })
                    .with_children(|tabs| {
                        let mut ring = RingOrder::starting_at(TAB_BAR_FOCUS_ORDER);
                        for tab in SETTINGS_TAB_ORDER {
                            let (action, label) = settings_tab_action_and_label(tab);
                            spawn_settings_tab_button(tabs, tab, action, label, ring.next());
                        }
                    });
                panel
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
                    .with_children(spawn_preferences_panel);
                panel
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
                    .with_children(spawn_game_panel);
                panel
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
                    .with_children(spawn_keyboard_panel);
                panel
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
                    .with_children(spawn_controller_panel);
                panel
                    .spawn(Node {
                        width: percent(100),
                        justify_content: JustifyContent::Center,
                        column_gap: px(18),
                        ..default()
                    })
                    .with_children(|footer| {
                        let mut ring = RingOrder::starting_at(FOOTER_FOCUS_ORDER);
                        spawn_child_button(
                            footer,
                            HudAction::RequestResetMappings,
                            "Reset Controls",
                            170.0,
                            Some(ring.next()),
                        );
                        spawn_child_button(
                            footer,
                            HudAction::BackToHome,
                            "Back",
                            110.0,
                            Some(ring.next()),
                        );
                    });
            });
        });

    spawn_reset_mappings_modal(&mut commands);
}

fn spawn_settings_tab_button(
    parent: &mut ChildSpawnerCommands,
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
            Node {
                width: px(155),
                height: px(42),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BLUE_BORDER),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(17.0),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            SettingsTabText(tab),
        ));
}

fn spawn_preferences_panel(panel: &mut ChildSpawnerCommands) {
    panel
        .spawn((
            Node {
                width: px(650),
                padding: UiRect::all(px(24)),
                flex_direction: FlexDirection::Column,
                row_gap: px(14),
                ..default()
            },
            BackgroundColor(SURFACE_BACKGROUND),
        ))
        .with_children(|preferences| {
            let mut ring = RingOrder::starting_at(panel_focus_order(MappingTab::Preferences));
            spawn_preference_toggle(
                preferences,
                PreferenceToggle::MatchTimer,
                "Show Match Timer",
                "Display elapsed match time as MM:SS.",
                ring.next(),
            );
            spawn_preference_toggle(
                preferences,
                PreferenceToggle::RoundNumber,
                "Show Round Number",
                "Display the current round below the timer area.",
                ring.next(),
            );
            spawn_preference_toggle(
                preferences,
                PreferenceToggle::ReduceMotion,
                "Reduce Motion",
                "Show captures and other changes without animation.",
                ring.next(),
            );
            spawn_volume_slider(
                preferences,
                AudioChannel::Game,
                "Game Sound Effects",
                "Countdown, captures, teleports, Match events, and Replays.",
                &mut ring,
            );
            spawn_volume_slider(
                preferences,
                AudioChannel::Ui,
                "UI Sound Effects",
                "Buttons, navigation, dialogs, and Settings previews.",
                &mut ring,
            );
            preferences.spawn((
                Text::new(settings_storage_note()),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                TextLayout::justify(Justify::Center),
                Node {
                    width: percent(100),
                    margin: UiRect::top(px(4)),
                    ..default()
                },
            ));
        });
}

fn spawn_preference_toggle(
    parent: &mut ChildSpawnerCommands,
    preference: PreferenceToggle,
    label: &'static str,
    description: &'static str,
    focus: Focusable,
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_height: px(72),
            padding: UiRect::axes(px(16), px(10)),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(19.0),
                        ..default()
                    },
                    TextColor(TEXT_PRIMARY),
                ));
                copy.spawn((
                    Text::new(description),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                ));
            });
            row.spawn((
                Button,
                preference,
                UiSound::new(UiCue::Neutral),
                focus,
                Node {
                    width: px(90),
                    height: px(42),
                    border: UiRect::all(px(1)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(BUTTON_BACKGROUND),
                BorderColor::all(BLUE_BORDER),
            ))
            .with_child((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(TEXT_PRIMARY),
                PreferenceValue(preference),
            ));
        });
}

fn spawn_volume_slider(
    parent: &mut ChildSpawnerCommands,
    channel: AudioChannel,
    label: &'static str,
    description: &'static str,
    ring: &mut RingOrder,
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_height: px(94),
            padding: UiRect::axes(px(16), px(8)),
            flex_direction: FlexDirection::Column,
            row_gap: px(7),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(19.0),
                    ..default()
                },
                TextColor(TEXT_PRIMARY),
            ));
            row.spawn((
                Text::new(description),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            row.spawn(Node {
                width: percent(100),
                height: px(38),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|track| {
                for level in VolumeLevel::ALL {
                    let stop = VolumeStop { channel, level };
                    track
                        .spawn((
                            Button,
                            stop,
                            UiSound::preview_only(),
                            ring.next(),
                            Node {
                                width: px(120),
                                height: px(32),
                                border: UiRect::all(px(2)),
                                border_radius: BorderRadius::all(px(16)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(BUTTON_BACKGROUND),
                            BorderColor::all(BLUE_BORDER),
                        ))
                        .with_child((
                            Text::new(level.label()),
                            TextFont {
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(TEXT_PRIMARY),
                            VolumeStopLabel(stop),
                        ));
                }
            });
        });
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
