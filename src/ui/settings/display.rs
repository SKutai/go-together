//! Keeps every settings widget in sync with the live control resources.
//!
//! These systems are pure "render the current state" passes: they never mutate
//! bindings or preferences. Two behaviours are worth calling out.
//!
//! A binding row that is currently capturing shows a prompt instead of its
//! value, so the player can tell the difference between "no binding" and "press
//! something now". Controller rows for a player with no assigned controller are
//! drawn disabled *and* have their click cue muted, so an inert button never
//! sounds like it worked.
//!
//! One write here reaches the focus ring: a numeric input whose limit is off
//! carries `bevy_ui`'s [`InteractionDisabled`], which the ring's candidate
//! collector filters, so the same computation that greys the field also
//! takes it out of the ring and puts it back. The pointer reads the same
//! marker - `ui::button` neither presses nor activates a button that
//! carries it - and the button system ignores a disabled input besides, as
//! it always has. See [`super::ring`] for why the other two disabled looks
//! on this screen do *not* leave the ring.
//!
//! # Why this is one module and not one per tab
//!
//! [`super::game`] and [`super::bindings`] each own a tab end to end apart
//! from this pass. This pass is the exception because it is not per-tab work:
//! it is one chained run in [`AppSystems::SettingsDisplay`](crate::app::AppSystems)
//! over every widget on the screen, hidden panels included, and the tab
//! panels and tab buttons are restyled from the same
//! [`ControlMappingState`] read. Splitting it by tab would scatter one frame's
//! refresh across four files without making any of them easier to read.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use super::ControlMappingState;
use super::bindings::mapping_binding_colors;
use super::game::game_input_text;
use super::geometry::settings_layout;
use super::{
    ControllerMappingBinding, ControllerMappingUiRole, GameInputError, GameInputValue,
    GameNumericInput, GameToggle, GameToggleValue, MappingBinding, MappingPanel, MappingUiRole,
    MatchMinimumLabel, PreferenceToggle, PreferenceValue, ResetMappingsModal, SettingsTabButton,
    SettingsTabText, VolumeStop, VolumeStopLabel, mapping_instruction, preference_enabled,
};
use crate::app::visuals::WindowSize;
use crate::audio::{AudioChannel, UiSound};
use crate::input::controller::ControllerSlotStatus;
use crate::match_settings::{MatchSettings, duration_parts};
use crate::presentation::{
    BLUE_BORDER, BUTTON_DISABLED, BUTTON_DISABLED_BORDER, Highlight, ORANGE, TEXT_HEADING,
    TEXT_MUTED, TEXT_PRIMARY, button_background,
};
use crate::settings::{GamePreferences, VolumeLevel};
use crate::ui::ControlBindings;
use crate::ui::button::PointerHighlight;
use crate::ui::widgets::selected_label;

/// The Keyboard tab's binding rows plus the screen-wide instruction and error
/// lines - every [`MappingUiRole`]; the `Node` is for hiding the error line
/// when it is empty.
type BindingRowTexts<'w, 's> = Query<
    'w,
    's,
    (&'static MappingUiRole, &'static mut Text, &'static mut Node),
    (
        Without<ResetMappingsModal>,
        Without<ControllerMappingUiRole>,
        Without<PreferenceValue>,
    ),
>;

/// Binding rows and connection status for the Controller tab.
type ControllerRowTexts<'w, 's> = Query<
    'w,
    's,
    (&'static ControllerMappingUiRole, &'static mut Text),
    (Without<MappingUiRole>, Without<PreferenceValue>),
>;

/// On/off text for each preference toggle.
type PreferenceTexts<'w, 's> = Query<
    'w,
    's,
    (&'static PreferenceValue, &'static mut Text),
    (
        Without<MappingUiRole>,
        Without<ControllerMappingUiRole>,
        Without<VolumeStopLabel>,
    ),
>;

/// Labels on the volume stops, recoloured and bracketed to show the
/// selected level.
type VolumeStopTexts<'w, 's> = Query<
    'w,
    's,
    (
        &'static VolumeStopLabel,
        &'static mut Text,
        &'static mut TextColor,
    ),
    (
        Without<MappingUiRole>,
        Without<ControllerMappingUiRole>,
        Without<PreferenceValue>,
    ),
>;

/// Every piece of text on the settings screen.
///
/// The `Without` filters are what let these overlapping queries coexist in one
/// system: each text entity carries exactly one role marker, and excluding the
/// others proves to Bevy that the queries are disjoint.
#[derive(SystemParam)]
pub(super) struct MappingTexts<'w, 's> {
    roles: BindingRowTexts<'w, 's>,
    controller_roles: ControllerRowTexts<'w, 's>,
    preferences: PreferenceTexts<'w, 's>,
    volume_labels: VolumeStopTexts<'w, 's>,
}

type GameToggleTexts<'w, 's> = Query<
    'w,
    's,
    (&'static GameToggleValue, &'static mut Text),
    (
        Without<GameInputValue>,
        Without<GameInputError>,
        Without<MatchMinimumLabel>,
    ),
>;

type GameInputTexts<'w, 's> = Query<
    'w,
    's,
    (
        &'static GameInputValue,
        &'static mut Text,
        &'static mut TextColor,
    ),
    (
        Without<GameToggleValue>,
        Without<GameInputError>,
        Without<MatchMinimumLabel>,
    ),
>;

type GameErrorTexts<'w, 's> = Query<
    'w,
    's,
    (
        &'static GameInputError,
        &'static mut Text,
        &'static mut Node,
    ),
    (
        Without<GameToggleValue>,
        Without<GameInputValue>,
        Without<MatchMinimumLabel>,
    ),
>;

type MatchMinimumTexts<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<MatchMinimumLabel>,
        Without<GameToggleValue>,
        Without<GameInputValue>,
        Without<GameInputError>,
    ),
>;

type GameToggleButtons<'w, 's> = Query<
    'w,
    's,
    (
        PointerHighlight,
        &'static GameToggle,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
    ),
    Without<GameNumericInput>,
>;

/// Numeric inputs, with whether each currently carries
/// [`InteractionDisabled`] so the marker is written only on change - it is
/// an archetype move.
type GameInputButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        PointerHighlight,
        &'static GameNumericInput,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        &'static mut UiSound,
        Has<InteractionDisabled>,
    ),
    Without<GameToggle>,
>;

#[derive(SystemParam)]
pub(super) struct GameSettingsWidgets<'w, 's> {
    toggle_texts: GameToggleTexts<'w, 's>,
    input_texts: GameInputTexts<'w, 's>,
    error_texts: GameErrorTexts<'w, 's>,
    minimum_texts: MatchMinimumTexts<'w, 's>,
    toggle_buttons: GameToggleButtons<'w, 's>,
    input_buttons: GameInputButtons<'w, 's>,
}

pub(super) fn update_game_settings_display(
    mut commands: Commands,
    settings: Res<MatchSettings>,
    mapping: Res<ControlMappingState>,
    mut widgets: GameSettingsWidgets,
) {
    for (value, mut text) in &mut widgets.toggle_texts {
        let enabled = match value.0 {
            GameToggle::Limit(field) => settings.enabled(field),
            GameToggle::CapturePoints => settings.capture_points(),
        };
        **text = if enabled { "X" } else { "" }.into();
    }

    for (value, mut text, mut color) in &mut widgets.input_texts {
        let enabled = settings.enabled(value.0.field());
        **text = mapping
            .game_editor
            .as_ref()
            .filter(|active| active.input == value.0)
            .map_or_else(
                || game_input_text(value.0, &settings),
                |active| active.editor.display_text(),
            );
        color.0 = if enabled { TEXT_PRIMARY } else { TEXT_MUTED };
    }

    for (role, mut text, mut node) in &mut widgets.error_texts {
        let error = mapping
            .game_error
            .as_ref()
            .filter(|(input, _)| input.field() == role.0.field())
            .map(|(_, message)| message.as_str());
        **text = error.unwrap_or_default().into();
        node.display = if error.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }

    let (minimum_minutes, minimum_seconds) = duration_parts(settings.match_time_minimum());
    for mut text in &mut widgets.minimum_texts {
        **text = format!("Min {minimum_minutes}:{minimum_seconds:02}");
    }

    for (highlight, toggle, mut background, mut border) in &mut widgets.toggle_buttons {
        let selected = match toggle {
            GameToggle::Limit(field) => settings.enabled(*field),
            GameToggle::CapturePoints => settings.capture_points(),
        };
        background.0 = button_background(highlight.get(), selected, false);
        *border = BorderColor::all(if selected { ORANGE } else { BLUE_BORDER });
    }

    for (entity, highlight, input, mut background, mut border, mut sound, was_disabled) in
        &mut widgets.input_buttons
    {
        let enabled = settings.enabled(input.field());
        // A disabled input is not in the focus ring: the collector filters
        // `InteractionDisabled`, so the enabled state greys the field and
        // takes it out of (or puts it back into) the ring in one place.
        // `Commands` apply before `PostUpdate`, where the ring is collected
        // after layout, so the change lands the same frame. `bevy_ui`'s add
        // hook also mirrors the marker onto the button's `AccessibilityNode`,
        // which `Button` requires and so carries from the moment it spawns.
        if enabled == was_disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let editing = mapping
            .game_editor
            .as_ref()
            .is_some_and(|active| active.input == *input);
        background.0 = if enabled {
            button_background(highlight.get(), editing, false)
        } else {
            BUTTON_DISABLED
        };
        *border = BorderColor::all(if !enabled {
            BUTTON_DISABLED_BORDER
        } else if editing {
            ORANGE
        } else {
            BLUE_BORDER
        });
        sound.set_enabled(enabled);
    }
}

/// Keyboard binding buttons, which also carry their own click cue.
type BindingButtons<'w, 's> = Query<
    'w,
    's,
    (
        PointerHighlight,
        &'static MappingBinding,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        &'static mut UiSound,
    ),
    (
        Without<MappingUiRole>,
        Without<ControllerMappingBinding>,
        Without<PreferenceToggle>,
        Without<VolumeStop>,
    ),
>;

/// Controller binding buttons, which are disabled when no pad is assigned.
type ControllerBindingButtons<'w, 's> = Query<
    'w,
    's,
    (
        PointerHighlight,
        &'static ControllerMappingBinding,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        &'static mut UiSound,
    ),
    (
        Without<MappingBinding>,
        Without<PreferenceToggle>,
        Without<VolumeStop>,
    ),
>;

/// Preference toggle buttons.
type PreferenceButtons<'w, 's> = Query<
    'w,
    's,
    (
        PointerHighlight,
        &'static PreferenceToggle,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
    ),
    (
        Without<MappingBinding>,
        Without<ControllerMappingBinding>,
        Without<VolumeStop>,
    ),
>;

/// Volume stop buttons.
type VolumeButtons<'w, 's> = Query<
    'w,
    's,
    (
        PointerHighlight,
        &'static VolumeStop,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
    ),
    (
        Without<MappingBinding>,
        Without<ControllerMappingBinding>,
        Without<PreferenceToggle>,
    ),
>;

/// Every interactive control on the settings screen.
#[derive(SystemParam)]
pub(super) struct MappingButtons<'w, 's> {
    bindings: BindingButtons<'w, 's>,
    controller_bindings: ControllerBindingButtons<'w, 's>,
    preferences: PreferenceButtons<'w, 's>,
    volumes: VolumeButtons<'w, 's>,
}

/// Fills every text on the screen but the Game tab's and styles every
/// control but the tabs.
///
/// The instruction and the error line are the one pair whose showing
/// depends on the arrangement. On top both are shown, the error under the
/// instruction, as they always were. In the sidebar they share the top row
/// with the title and the two buttons, which has room for one of them, so
/// an error stands in the instruction's place while it is shown and the
/// instruction comes back when it clears; the error says what to do, and the
/// instruction would only repeat the tab's purpose beside it.
pub(super) fn update_control_mapping_display(
    controls: ControlBindings,
    preferences: Res<GamePreferences>,
    mapping: Res<ControlMappingState>,
    window: Res<WindowSize>,
    mut texts: MappingTexts,
    mut buttons: MappingButtons,
    mut reset_modal: Query<&mut Node, (With<ResetMappingsModal>, Without<MappingUiRole>)>,
) {
    let error_in_the_instructions_place =
        settings_layout(window.layout()).sidebar() && !mapping.error.is_empty();
    for (role, mut text, mut node) in &mut texts.roles {
        **text = match role {
            MappingUiRole::Instruction => mapping_instruction(mapping.tab).into(),
            MappingUiRole::Binding(action) if mapping.capturing == Some(*action) => {
                "Press a key...".into()
            }
            MappingUiRole::Binding(action) => controls.keyboard.key(*action).display_name().into(),
            MappingUiRole::Error => mapping.error.clone(),
        };
        match role {
            MappingUiRole::Error => {
                node.display = if mapping.error.is_empty() {
                    Display::None
                } else {
                    Display::Flex
                };
            }
            MappingUiRole::Instruction => {
                let display = if error_in_the_instructions_place {
                    Display::None
                } else {
                    Display::Flex
                };
                if node.display != display {
                    node.display = display;
                }
            }
            MappingUiRole::Binding(_) => {}
        }
    }

    for (role, mut text) in &mut texts.controller_roles {
        **text = match role {
            ControllerMappingUiRole::Binding(player, action)
                if mapping.capturing_controller == Some((*player, *action)) =>
            {
                "Press a button...".into()
            }
            ControllerMappingUiRole::Binding(player, action) => {
                let family = controls.assignments.family(*player);
                controls
                    .controller
                    .input(*player, *action)
                    .display_name(family)
                    .into()
            }
            ControllerMappingUiRole::Status(player) => match controls.assignments.status(*player) {
                ControllerSlotStatus::Unassigned => "No controller assigned".into(),
                ControllerSlotStatus::Connected { family, name } => {
                    format!("{name} ({})", family.label())
                }
            },
        };
    }

    for (role, mut text) in &mut texts.preferences {
        let enabled = preference_enabled(&preferences, role.0);
        **text = if enabled { "On" } else { "Off" }.into();
    }

    for (highlight, binding, mut background, mut border, mut sound) in &mut buttons.bindings {
        let capturing = mapping.capturing == Some(binding.0);
        let (button_background, button_border) = mapping_binding_colors(highlight.get(), capturing);
        background.0 = button_background;
        *border = BorderColor::all(button_border);
        sound.set_enabled(true);
    }

    for (highlight, binding, mut background, mut border, mut sound) in
        &mut buttons.controller_bindings
    {
        let capturing = mapping.capturing_controller == Some((binding.0, binding.1));
        let enabled = controls.assignments.assigned_entity(binding.0).is_some();
        let (button_background, button_border) = mapping_binding_colors(highlight.get(), capturing);
        background.0 = if enabled {
            button_background
        } else {
            BUTTON_DISABLED
        };
        *border = BorderColor::all(if enabled {
            button_border
        } else {
            BUTTON_DISABLED_BORDER
        });
        sound.set_enabled(enabled);
    }

    for (highlight, preference, mut background, mut border) in &mut buttons.preferences {
        let enabled = preference_enabled(&preferences, *preference);
        background.0 = button_background(highlight.get(), enabled, false);
        *border = BorderColor::all(if enabled { ORANGE } else { BLUE_BORDER });
    }

    for (highlight, stop, mut background, mut border) in &mut buttons.volumes {
        let selected = volume_level(&preferences, stop.channel) == stop.level;
        background.0 = button_background(highlight.get(), selected, false);
        *border = BorderColor::all(if selected { ORANGE } else { BLUE_BORDER });
    }

    // The fill, border and label colour all say which stop is selected, and
    // all of it is hue; the brackets are what says it without.
    for (label, mut text, mut color) in &mut texts.volume_labels {
        let selected = volume_level(&preferences, label.0.channel) == label.0.level;
        color.0 = if selected { TEXT_HEADING } else { TEXT_PRIMARY };
        let words = selected_label(label.0.level.label(), selected);
        if **text != words {
            **text = words;
        }
    }

    for mut node in &mut reset_modal {
        node.display = if mapping.reset_confirmation_open {
            Display::Flex
        } else {
            Display::None
        };
    }

    const fn volume_level(preferences: &GamePreferences, channel: AudioChannel) -> VolumeLevel {
        match channel {
            AudioChannel::Game => preferences.game_sound_effects,
            AudioChannel::Ui => preferences.ui_sound_effects,
        }
    }
}

/// Shows only the panel matching the active mapping tab.
pub(super) fn update_mapping_tab_panels(
    mapping: Res<ControlMappingState>,
    mut panels: Query<(&MappingPanel, &mut Node)>,
) {
    for (panel, mut node) in &mut panels {
        node.display = if panel.0 == mapping.tab {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub(super) fn settings_tab_colors(highlight: Highlight, selected: bool) -> (Color, Color, Color) {
    if selected {
        (BUTTON_DISABLED, BUTTON_DISABLED_BORDER, TEXT_MUTED)
    } else {
        (
            button_background(highlight, false, false),
            BLUE_BORDER,
            TEXT_PRIMARY,
        )
    }
}

/// Restyles the four tab buttons: the selected tab inert - a disabled fill
/// and border, a muted label, no click - and the others as ordinary buttons.
///
/// This is the only system that writes a tab button's fill and click cue.
/// The tabs carry a `HudAction`, which would otherwise hand both to the
/// HUD's `update_shared_display`, and the two disagree about the selected
/// tab; `spawn_settings_tab_button` marks each tab `ScreenStyled` so that
/// pass skips them. Do not remove the marker without ordering the two.
pub(super) fn update_settings_tab_buttons(
    mapping: Res<ControlMappingState>,
    mut buttons: Query<(
        PointerHighlight,
        &SettingsTabButton,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut UiSound,
    )>,
    mut texts: Query<(&SettingsTabText, &mut TextColor)>,
) {
    for (highlight, tab, mut background, mut border, mut sound) in &mut buttons {
        let (button_background, button_border, _) =
            settings_tab_colors(highlight.get(), tab.0 == mapping.tab);
        background.0 = button_background;
        *border = BorderColor::all(button_border);
        sound.set_enabled(tab.0 != mapping.tab);
    }
    for (tab, mut text) in &mut texts {
        let (_, _, text_color) = settings_tab_colors(Highlight::None, tab.0 == mapping.tab);
        text.0 = text_color;
    }
}
