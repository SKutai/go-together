//! The Preferences tab's panel: the three switches - the Match timer, the
//! round number and reduced motion - the two volume rows, and the note on
//! where settings are stored.
//!
//! Its widgets are plain toggles and volume stops with no state behind
//! them: a press applies and persists at once, and there is nothing to
//! capture, parse or commit, so what a press means is [`super`]'s and what
//! a switch shows is [`super::display`]'s. The panel is a module of its own
//! for its size rather than its kind, as the Replay Viewer's overlays are:
//! it was a quarter of the frame's module. Where its surface and switches
//! stand in each arrangement is the frame's to say: [`super::layout`]'s
//! placement pass places them, from the sizes here.

use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::geometry::SettingsLayout;
use super::layout::{SettingsPlacement, placed};
use super::ring::{RingOrder, panel_focus_order};
use super::{MappingTab, PreferenceToggle, PreferenceValue, VolumeStop, VolumeStopLabel};
use crate::audio::{AudioChannel, UiCue, UiSound};
use crate::platform::settings_storage_note;
use crate::presentation::{
    BLUE_BORDER, BUTTON_BACKGROUND, SURFACE_BACKGROUND, TEXT_MUTED, TEXT_PRIMARY,
};
use crate::settings::VolumeLevel;
use crate::ui::focus::Focusable;

/// The Preferences surface's width on top, and the most it takes in the
/// sidebar, where it is otherwise as wide as the tab's contents.
pub(super) const PREFERENCES_WIDTH: f32 = 650.0;
pub(super) const PREFERENCES_MAX_WIDTH: f32 = 900.0;
/// The gap between the Preferences surface's rows, and between its three
/// switches when they stand side by side.
pub(super) const PREFERENCES_GAP: f32 = 14.0;
/// The gap between a switch's words and its button, when the two are
/// stacked in a card.
pub(super) const SWITCH_CARD_GAP: f32 = 8.0;
pub(super) fn spawn_preferences_panel(panel: &mut ChildSpawnerCommands, layout: &SettingsLayout) {
    panel
        .spawn((
            placed(
                layout,
                SettingsPlacement::PreferencesSurface,
                Node {
                    padding: UiRect::all(px(24)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(PREFERENCES_GAP),
                    ..default()
                },
            ),
            BackgroundColor(SURFACE_BACKGROUND),
        ))
        .with_children(|preferences| {
            let mut ring = RingOrder::starting_at(panel_focus_order(MappingTab::Preferences));
            preferences
                .spawn(placed(
                    layout,
                    SettingsPlacement::PreferenceSwitches,
                    Node {
                        width: percent(100),
                        ..default()
                    },
                ))
                .with_children(|switches| {
                    spawn_preference_toggle(
                        switches,
                        layout,
                        PreferenceToggle::MatchTimer,
                        "Show Match Timer",
                        "Display elapsed match time as MM:SS.",
                        ring.next(),
                    );
                    spawn_preference_toggle(
                        switches,
                        layout,
                        PreferenceToggle::RoundNumber,
                        "Show Round Number",
                        "Display the current round below the timer area.",
                        ring.next(),
                    );
                    spawn_preference_toggle(
                        switches,
                        layout,
                        PreferenceToggle::ReduceMotion,
                        "Reduce Motion",
                        "Show captures and other changes without animation.",
                        ring.next(),
                    );
                });
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

/// One Preferences switch: its words and its On/Off button, side by side
/// in a row on top, and stacked in a card beside the other two in the
/// sidebar.
fn spawn_preference_toggle(
    parent: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    preference: PreferenceToggle,
    label: &'static str,
    description: &'static str,
    focus: Focusable,
) {
    parent
        .spawn(placed(
            layout,
            SettingsPlacement::PreferenceSwitch,
            Node {
                min_height: px(72),
                padding: UiRect::axes(px(16), px(10)),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
        ))
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
