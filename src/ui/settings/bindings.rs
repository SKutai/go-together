//! The Keyboard and Controller tabs: their panels, and the live capture that
//! rebinds a row while one of them is open.
//!
//! Spawning and capture share a file because they are one concern seen from
//! two sides. A binding row is a button that means "I am the row now being
//! captured", and the capture systems here are the only readers of that
//! meaning. Keeping them in `layout.rs` and `mod.rs` put the row's shape and
//! the rule for what may fill it in different files. What stays in
//! [`super::display`] is only the per-frame text and colour refresh, because
//! that is the whole screen's pass rather than these tabs'.
//!
//! # Player column order
//!
//! [`CONTROL_MAPPING_PLAYER_ORDER`] puts White/Player 2 first so the columns
//! read left to right the way the board header does, and the focus ring
//! walks them the same way: every row of the left column, then every row of
//! the right, then - on the Keyboard tab - the shared row beneath. The two
//! columns stand side by side and centred together, as wide as
//! [`super::geometry`] makes them for the window: their full 420 px
//! wherever there is room. In the sidebar the shared row stands on one line
//! beside its `SHARED` heading, so that the tab fits the smallest window.
//! Controller rows are spawned even
//! when no controller is connected, so the bindings stay discoverable;
//! [`super::display`] draws those rows disabled and mutes their click cue,
//! but they stay in the ring, because pressing one is not a no-op - it tells
//! the player to connect a controller (see [`super::ring`]).
//!
//! # Capture rules
//!
//! Keyboard capture rejects reserved keys silently - Escape still has to
//! cancel the capture and leave the screen, and F1 is the Help key
//! on the screens that have them - and reports unsupported keys, so the
//! player can tell "nothing happened" from "that key will not work".
//! Controller capture only accepts input from the controller *assigned to the
//! player being remapped*, so a second pad cannot silently rebind the first
//! player; if that controller disconnects mid-capture the capture is
//! cancelled with an explanation.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::geometry::{
    BINDING_ROW_BORDER, BINDING_ROW_PADDING, BINDING_TEXT_SIZE, PLAYER_COLUMN_PADDING,
    PLAYER_COLUMN_WIDTH, SettingsLayout,
};
use super::layout::{SettingsPlacement, placed};
use super::ring::{RingOrder, panel_focus_order};
use super::{
    ControlMappingState, ControllerMappingBinding, ControllerMappingUiRole, MappingBinding,
    MappingTab, MappingUiRole, persist_settings,
};
use crate::audio::{AudioCueQueue, UiCue, UiSound};
use crate::input::GameAction;
use crate::input::controller::{
    ControllerAssignments, ControllerBindings, ControllerCaptureState, ProfileAction, capture_step,
};
use crate::input::keyboard::{KeyCaptureError, KeyboardBindings, KeyboardKey};
use crate::match_settings::MatchSettings;
use crate::presentation::{
    BLUE_BORDER, BUTTON_BACKGROUND, Highlight, ORANGE, SURFACE_BACKGROUND, TEXT_HEADING,
    TEXT_MUTED, TEXT_PRIMARY, button_background, player_accent,
};
use crate::rules::Player;
use crate::settings::{GamePreferences, SettingsStore};

/// What the error line says when a keyboard capture is handed a key no
/// binding can hold, and when the controller being captured from goes away.
pub(super) const UNSUPPORTED_KEY_MESSAGE: &str =
    "That key is not supported. Press another key, or Escape to cancel.";
pub(super) const CONTROLLER_DISCONNECTED_MESSAGE: &str =
    "The assigned controller disconnected. Connect a controller and try again.";

/// White/Player 2 first, so the settings columns read left to right the way
/// the board header does.
pub(super) const CONTROL_MAPPING_PLAYER_ORDER: [Player; 2] = [Player::Two, Player::One];

/// Each column's heading: 24px, because a player's column is headed in
/// their accent, and WCAG holds text this size to 3:1 rather than 4.5:1,
/// which Player One's red (3.54:1 on the column) passes. 4.5:1 is out of
/// reach here for any red at any saturation - even pure red is 4.22:1 on
/// this surface - so the size is the fix and not the colour. The Keyboard
/// tab's `SHARED` heading is the same size, so the three headings on that
/// tab still read as one set.
pub(super) const COLUMN_HEADING_SIZE: f32 = 24.0;

/// The Keyboard tab: one column per player, then the shared actions.
pub(super) fn spawn_keyboard_panel(
    keyboard_panel: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
) {
    let mut ring = RingOrder::starting_at(panel_focus_order(MappingTab::Keyboard));
    keyboard_panel
        .spawn(player_columns_node(layout))
        .with_children(|columns| {
            for player in CONTROL_MAPPING_PLAYER_ORDER {
                let (title, actions, accent) = match player {
                    Player::One => (
                        "PLAYER 1 - BLACK",
                        GameAction::PLAYER_ONE.as_slice(),
                        player_accent(player),
                    ),
                    Player::Two => (
                        "PLAYER 2 - WHITE",
                        GameAction::PLAYER_TWO.as_slice(),
                        player_accent(player),
                    ),
                };
                spawn_mapping_group(
                    columns,
                    placed(
                        layout,
                        SettingsPlacement::PlayerColumn,
                        mapping_group_node(),
                    ),
                    None,
                    title,
                    actions,
                    accent,
                    &mut ring,
                );
            }
        });
    keyboard_panel
        .spawn(Node {
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|shared| {
            spawn_mapping_group(
                shared,
                placed(layout, SettingsPlacement::SharedGroup, mapping_group_node()),
                Some(placed(
                    layout,
                    SettingsPlacement::SharedHeading,
                    Node::default(),
                )),
                "SHARED",
                &GameAction::SHARED,
                TEXT_HEADING,
                &mut ring,
            );
        });
}

/// The Controller tab: one column per player. There is no shared column -
/// every controller action belongs to the profile of the pad that sends it.
pub(super) fn spawn_controller_panel(
    controller_panel: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
) {
    let mut ring = RingOrder::starting_at(panel_focus_order(MappingTab::Controller));
    controller_panel
        .spawn(player_columns_node(layout))
        .with_children(|columns| {
            for player in CONTROL_MAPPING_PLAYER_ORDER {
                let (title, accent) = match player {
                    Player::One => ("PLAYER 1 - BLACK", player_accent(player)),
                    Player::Two => ("PLAYER 2 - WHITE", player_accent(player)),
                };
                spawn_controller_mapping_group(
                    columns,
                    placed(
                        layout,
                        SettingsPlacement::PlayerColumn,
                        mapping_group_node(),
                    ),
                    player,
                    title,
                    accent,
                    &mut ring,
                );
            }
        });
}

/// The row holding the two players' columns, side by side and centred
/// together.
fn player_columns_node(layout: &SettingsLayout) -> (Node, SettingsPlacement) {
    placed(
        layout,
        SettingsPlacement::PlayerColumns,
        Node {
            width: percent(100),
            flex_grow: 1.0,
            justify_content: JustifyContent::Center,
            ..default()
        },
    )
}

/// A column's heading node: 3 px above the first row, as it always was.
fn column_heading_node() -> Node {
    Node {
        margin: UiRect::bottom(px(3)),
        ..default()
    }
}

/// Spawns one group of binding rows under its heading: a player's column,
/// or the shared group, whose heading carries a placement of its own
/// because in the sidebar it stands beside its row rather than over it.
fn spawn_mapping_group(
    parent: &mut ChildSpawnerCommands,
    group: (Node, SettingsPlacement),
    heading: Option<(Node, SettingsPlacement)>,
    title: &'static str,
    actions: &[GameAction],
    accent: Color,
    ring: &mut RingOrder,
) {
    parent
        .spawn((group, BackgroundColor(SURFACE_BACKGROUND)))
        .with_children(|group| {
            let (heading_node, heading_placement) = heading
                .map_or((column_heading_node(), None), |(node, placement)| {
                    (node, Some(placement))
                });
            let mut heading = group.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(COLUMN_HEADING_SIZE),
                    ..default()
                },
                TextColor(accent),
                heading_node,
            ));
            if let Some(placement) = heading_placement {
                heading.insert(placement);
            }
            for action in actions {
                group
                    .spawn((
                        Button,
                        MappingBinding(*action),
                        UiSound::new(UiCue::Neutral),
                        ring.next(),
                        mapping_binding_node(),
                        BackgroundColor(BUTTON_BACKGROUND),
                        BorderColor::all(BLUE_BORDER),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(action.label()),
                            TextFont {
                                font_size: FontSize::Px(BINDING_TEXT_SIZE),
                                ..default()
                            },
                            TextColor(TEXT_PRIMARY),
                        ));
                        row.spawn((
                            Text::new(""),
                            TextFont {
                                font_size: FontSize::Px(BINDING_TEXT_SIZE),
                                ..default()
                            },
                            TextColor(TEXT_HEADING),
                            MappingUiRole::Binding(*action),
                        ));
                    });
            }
        });
}

/// Mirrors [`spawn_mapping_group`] for a controller profile: a connection
/// status line followed by the seven player actions plus the profile-local
/// restart gesture.
fn spawn_controller_mapping_group(
    parent: &mut ChildSpawnerCommands,
    group: (Node, SettingsPlacement),
    player: Player,
    title: &'static str,
    accent: Color,
    ring: &mut RingOrder,
) {
    parent
        .spawn((group, BackgroundColor(SURFACE_BACKGROUND)))
        .with_children(|group| {
            group.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(COLUMN_HEADING_SIZE),
                    ..default()
                },
                TextColor(accent),
                column_heading_node(),
            ));
            group.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    margin: UiRect::bottom(px(3)),
                    ..default()
                },
                ControllerMappingUiRole::Status(player),
            ));
            for action in ProfileAction::ALL {
                group
                    .spawn((
                        Button,
                        ControllerMappingBinding(player, action),
                        UiSound::new(UiCue::Neutral),
                        ring.next(),
                        mapping_binding_node(),
                        BackgroundColor(BUTTON_BACKGROUND),
                        BorderColor::all(BLUE_BORDER),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(action.label()),
                            TextFont {
                                font_size: FontSize::Px(BINDING_TEXT_SIZE),
                                ..default()
                            },
                            TextColor(TEXT_PRIMARY),
                        ));
                        row.spawn((
                            Text::new(""),
                            TextFont {
                                font_size: FontSize::Px(BINDING_TEXT_SIZE),
                                ..default()
                            },
                            TextColor(TEXT_HEADING),
                            ControllerMappingUiRole::Binding(player, action),
                        ));
                    });
            }
        });
}

/// The shape a binding column and its rows are spawned in, and the colours a
/// row takes.
///
/// `mapping_binding_colors` is a function rather than two constants because
/// [`super::display`] has to reproduce the same values every frame - a
/// capturing row is drawn in [`ORANGE`], every other row in [`BLUE_BORDER`] -
/// and neither module should be the one the other copies them from. The two
/// `Node` builders are used only here.
pub(super) fn mapping_group_node() -> Node {
    Node {
        width: px(PLAYER_COLUMN_WIDTH),
        padding: UiRect::all(px(PLAYER_COLUMN_PADDING)),
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        ..default()
    }
}

pub(super) fn mapping_binding_node() -> Node {
    Node {
        width: percent(100),
        height: px(42),
        padding: UiRect::horizontal(px(BINDING_ROW_PADDING)),
        border: UiRect::all(px(BINDING_ROW_BORDER)),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        ..default()
    }
}

pub(super) fn mapping_binding_colors(highlight: Highlight, capturing: bool) -> (Color, Color) {
    (
        button_background(highlight, capturing, false),
        if capturing { ORANGE } else { BLUE_BORDER },
    )
}

/// Binds the next pressed key to the row being captured.
///
/// Reserved keys are ignored silently - Escape still has to cancel the
/// capture and leave the screen, and F1 is the Help key on the
/// screens that have them - while an unsupported key explains itself, so
/// the player can tell "nothing happened" from "that key will not work".
#[derive(SystemParam)]
pub(super) struct MappingKeyCapture<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    bindings: ResMut<'w, KeyboardBindings>,
    controller_bindings: Res<'w, ControllerBindings>,
    preferences: Res<'w, GamePreferences>,
    match_settings: Res<'w, MatchSettings>,
    settings_store: Res<'w, SettingsStore>,
    mapping: ResMut<'w, ControlMappingState>,
    audio: ResMut<'w, AudioCueQueue>,
    diagnostics: ResMut<'w, crate::diagnostics::Diagnostics>,
    /// The row that was already capturing on the previous frame, for the
    /// rising-edge guard below - the same `Local`-held arming step
    /// [`capture_controller_mapping`] keeps and `capture_step` enforces.
    active: Local<'s, Option<GameAction>>,
}

pub(super) fn capture_mapping_key(mut capture: MappingKeyCapture) {
    // No capture is live under the reset confirmation (requesting it cleared
    // both), so the arming memory is dropped too: a row re-armed in the very
    // frame the dialog closes must arm afresh, not inherit a guard that
    // still names it.
    if capture.mapping.reset_confirmation_open {
        *capture.active = None;
        return;
    }
    let Some(action) = capture.mapping.capturing else {
        *capture.active = None;
        return;
    };
    // The arming frame reads nothing: `ButtonInput::just_pressed` is not
    // cleared until the next frame's `PreUpdate`, so on the frame a key
    // press activates this row (a keyboard-focus activation runs in the
    // same `Update` as this system), that key is still in `just_pressed`
    // and would silently become the row's new binding, persisted to disk.
    // A pointer click leaves no key behind; the one pointer-visible shift
    // is that a key landing in the click's own ~16ms frame now binds on
    // the next press instead of instantly.
    let just_armed = *capture.active != Some(action);
    *capture.active = Some(action);
    if just_armed {
        return;
    }
    let Some(pressed_key) = capture.keys.get_just_pressed().copied().next() else {
        return;
    };

    let key = match KeyboardKey::from_key_code(pressed_key) {
        Ok(key) => key,
        Err(KeyCaptureError::Reserved) => return,
        Err(KeyCaptureError::Unsupported) => {
            capture.mapping.error = UNSUPPORTED_KEY_MESSAGE.into();
            return;
        }
    };
    capture.bindings.assign(action, key);
    capture.mapping.capturing = None;
    *capture.active = None;
    persist_settings(
        &capture.settings_store,
        &capture.bindings,
        &capture.controller_bindings,
        &capture.preferences,
        &capture.match_settings,
        &mut capture.mapping,
        &mut capture.diagnostics,
    );
    capture.audio.ui(UiCue::Confirm);
}

/// Resolves a player to the gamepad currently assigned to them.
///
/// Capture must only ever accept input from the *assigned* pad, so that a
/// second connected controller cannot silently rebind the first player.
#[derive(SystemParam)]
pub(super) struct AssignedGamepads<'w, 's> {
    assignments: Res<'w, ControllerAssignments>,
    gamepads: Query<'w, 's, &'static Gamepad>,
}

impl AssignedGamepads<'_, '_> {
    fn assigned(&self, player: Player) -> Option<&Gamepad> {
        self.assignments
            .assigned_entity(player)
            .and_then(|entity| self.gamepads.get(entity).ok())
    }
}

/// The in-progress controller capture.
///
/// `state` accumulates the analog baselines that let an axis push be told apart
/// from resting drift; `active` remembers which binding the baselines belong
/// to, so switching rows restarts the measurement instead of inheriting it.
#[derive(SystemParam)]
pub(super) struct ControllerCapture<'w, 's> {
    state: ResMut<'w, ControllerCaptureState>,
    active: Local<'s, Option<(Player, ProfileAction)>>,
}

#[derive(SystemParam)]
pub(super) struct PersistedSettings<'w> {
    keyboard: Res<'w, KeyboardBindings>,
    controller: ResMut<'w, ControllerBindings>,
    preferences: Res<'w, GamePreferences>,
    match_settings: Res<'w, MatchSettings>,
    store: Res<'w, SettingsStore>,
    diagnostics: ResMut<'w, crate::diagnostics::Diagnostics>,
}

/// Advances controller-binding capture while the Controller tab is
/// listening for a new physical input. Only the *assigned* controller for
/// the player being remapped is eligible - inputs from any other connected
/// gamepad are ignored. If the assigned controller becomes unavailable
/// mid-capture (disconnects), capture is cancelled with a concise prompt.
pub(super) fn capture_controller_mapping(
    mut settings: PersistedSettings,
    gamepads: AssignedGamepads,
    mut capture: ControllerCapture,
    mut mapping: ResMut<ControlMappingState>,
    mut audio: ResMut<AudioCueQueue>,
) {
    // As in `capture_mapping_key`: nothing is captured under the dialog, so
    // the arming memory and the analog baseline go too, or a row re-armed
    // in the frame the dialog closes would skip its arming step and match a
    // button held since before the dialog opened against a stale snapshot.
    if mapping.reset_confirmation_open {
        capture.state.reset();
        *capture.active = None;
        return;
    }
    let Some((player, action)) = mapping.capturing_controller else {
        capture.state.reset();
        *capture.active = None;
        return;
    };

    let Some(gamepad) = gamepads.assigned(player) else {
        mapping.capturing_controller = None;
        mapping.error = CONTROLLER_DISCONNECTED_MESSAGE.into();
        *capture.active = None;
        capture.state.reset();
        return;
    };

    let just_entered = *capture.active != Some((player, action));
    *capture.active = Some((player, action));
    if let Some(input) = capture_step(&mut capture.state, gamepad, just_entered) {
        settings.controller.assign(player, action, input);
        mapping.capturing_controller = None;
        *capture.active = None;
        persist_settings(
            &settings.store,
            &settings.keyboard,
            &settings.controller,
            &settings.preferences,
            &settings.match_settings,
            &mut mapping,
            &mut settings.diagnostics,
        );
        audio.ui(UiCue::Confirm);
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::button::{UiButtonPlugin, trigger_activation};
    use std::sync::{Arc, Mutex};

    use super::super::handle_mapping_buttons;
    use super::*;
    use crate::presentation;
    use crate::presentation::{BLACK_P1_RED, WHITE_P2_BLUE};

    #[derive(Clone)]
    struct RecordingSettingsStorage(Arc<Mutex<Option<String>>>);

    impl crate::platform::SettingsStorage for RecordingSettingsStorage {
        fn load(&self) -> Result<Option<String>, crate::platform::SettingsStorageError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn save(&self, json: &str) -> Result<(), crate::platform::SettingsStorageError> {
            *self.0.lock().unwrap() = Some(json.into());
            Ok(())
        }
    }

    /// The frame-sharing defect the rising-edge guard exists for: the
    /// Settings chain runs `handle_mapping_buttons` and `capture_mapping_key`
    /// in one `Update`, and `ButtonInput::just_pressed` is not cleared until
    /// the next frame's `PreUpdate` - so the moment a key press can activate
    /// a binding row (the focus ring's activation makes it reachable),
    /// that same press is still in `just_pressed` when capture reads it and
    /// would silently become the row's binding, persisted to disk with a
    /// success sound. Only Escape and F1 are reserved, so Enter is even
    /// bindable.
    #[test]
    fn the_key_press_that_starts_a_capture_is_never_captured_itself() {
        let saved = Arc::new(Mutex::new(None));
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(SettingsStore::from_storage(RecordingSettingsStorage(
                Arc::clone(&saved),
            )))
            .insert_resource(ControllerAssignments::default())
            .insert_resource(ControlMappingState::new(None))
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(
                Update,
                (handle_mapping_buttons, capture_mapping_key).chain(),
            );
        let pressed = app
            .world_mut()
            .spawn(MappingBinding(GameAction::PlayerOneSelect))
            .id();
        trigger_activation(&mut app, pressed);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);

        app.update();

        let mapping = app.world().resource::<ControlMappingState>();
        assert_eq!(mapping.capturing, Some(GameAction::PlayerOneSelect));
        assert_eq!(
            app.world()
                .resource::<KeyboardBindings>()
                .key(GameAction::PlayerOneSelect),
            KeyboardKey::M,
            "the press that started the capture must not become the binding"
        );
        assert!(
            saved.lock().unwrap().is_none(),
            "the arming frame must write nothing to disk"
        );

        // Production clears `just_pressed` each `PreUpdate`; no `InputPlugin`
        // runs here, so mimic it, then capture a key for real.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Enter);
            keys.clear();
            keys.press(KeyCode::F8);
        }
        app.update();

        assert_eq!(
            app.world()
                .resource::<KeyboardBindings>()
                .key(GameAction::PlayerOneSelect),
            KeyboardKey::F8,
            "the armed capture must still accept the next fresh press"
        );
        assert_eq!(
            app.world().resource::<ControlMappingState>().capturing,
            None
        );
        assert!(saved.lock().unwrap().is_some(), "a real capture persists");
    }

    /// The two reserved keys during an armed capture: neither binds, neither
    /// is reported, nothing is written, and the capture stays armed for the
    /// next fresh press. Escape also cancels the capture, but from
    /// `handle_mapping_escape`, which is not in this schedule; F1 does
    /// nothing on this screen at all, which is why it must at least not
    /// bind.
    #[test]
    fn reserved_keys_are_ignored_silently_by_an_armed_capture() {
        let saved = Arc::new(Mutex::new(None));
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(SettingsStore::from_storage(RecordingSettingsStorage(
                Arc::clone(&saved),
            )))
            .insert_resource(ControllerAssignments::default())
            .insert_resource(ControlMappingState::new(None))
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(
                Update,
                (handle_mapping_buttons, capture_mapping_key).chain(),
            );
        let pressed = app
            .world_mut()
            .spawn(MappingBinding(GameAction::PlayerOneSelect))
            .id();
        trigger_activation(&mut app, pressed);
        app.update();
        assert_eq!(
            app.world().resource::<ControlMappingState>().capturing,
            Some(GameAction::PlayerOneSelect)
        );

        for reserved in [KeyCode::F1, KeyCode::Escape] {
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.clear();
                keys.press(reserved);
            }
            app.update();
            let mapping = app.world().resource::<ControlMappingState>();
            assert_eq!(
                mapping.capturing,
                Some(GameAction::PlayerOneSelect),
                "{reserved:?} must leave the capture armed"
            );
            assert!(
                mapping.error.is_empty(),
                "{reserved:?} must not be reported as unsupported"
            );
            assert_eq!(
                app.world()
                    .resource::<KeyboardBindings>()
                    .key(GameAction::PlayerOneSelect),
                KeyboardKey::M,
                "{reserved:?} must not bind"
            );
            assert!(
                saved.lock().unwrap().is_none(),
                "{reserved:?} wrote to disk"
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(reserved);
        }

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::F8);
        }
        app.update();
        assert_eq!(
            app.world()
                .resource::<KeyboardBindings>()
                .key(GameAction::PlayerOneSelect),
            KeyboardKey::F8,
            "the armed capture still accepts the next real key"
        );
        assert!(saved.lock().unwrap().is_some());
    }

    #[test]
    fn control_mapping_orders_white_p2_left_and_black_p1_right() {
        assert_eq!(CONTROL_MAPPING_PLAYER_ORDER, [Player::Two, Player::One]);
        assert_eq!(
            player_accent(CONTROL_MAPPING_PLAYER_ORDER[0]),
            WHITE_P2_BLUE
        );
        assert_eq!(player_accent(CONTROL_MAPPING_PLAYER_ORDER[1]), BLACK_P1_RED);
    }

    /// Every column heading on both binding tabs is 24px: the two players'
    /// are in their accents, and text that size owes its background 3:1,
    /// which Player One's red on the column meets and which no red could
    /// meet at 4.5:1. `SHARED` matches them so the three read as one set.
    #[test]
    fn binding_column_headings_are_large_text() {
        let mut world = World::new();
        world
            .commands()
            .spawn(Node::default())
            .with_children(|panel| {
                let layout = super::super::geometry::settings_layout(
                    crate::app::visuals::WindowSize::OPENING.0,
                );
                spawn_keyboard_panel(panel, &layout);
                spawn_controller_panel(panel, &layout);
            });
        world.flush();

        let headings: Vec<(String, f32, Color)> = world
            .query::<(&Text, &TextFont, &TextColor)>()
            .iter(&world)
            .filter(|(text, ..)| {
                ["PLAYER 1 - BLACK", "PLAYER 2 - WHITE", "SHARED"].contains(&text.0.as_str())
            })
            .map(|(text, font, color)| {
                let FontSize::Px(size) = font.font_size else {
                    panic!("{:?} is not sized in pixels", text.0);
                };
                (text.0.clone(), size, color.0)
            })
            .collect();
        // Two players on each of two tabs, and the Keyboard tab's shared one.
        assert_eq!(headings.len(), 5, "{headings:?}");
        for (title, size, color) in headings {
            assert_eq!(size, COLUMN_HEADING_SIZE, "{title}");
            let expected = match title.as_str() {
                "PLAYER 1 - BLACK" => BLACK_P1_RED,
                "PLAYER 2 - WHITE" => WHITE_P2_BLUE,
                _ => TEXT_HEADING,
            };
            assert_eq!(color, expected, "{title}");
        }
        assert_eq!(COLUMN_HEADING_SIZE, 24.0);
    }

    #[test]
    fn control_mapping_restores_neutral_cards_and_button_states() {
        let group = mapping_group_node();
        let binding = mapping_binding_node();

        assert_eq!(group.width, px(420));
        assert_eq!(group.padding, UiRect::all(px(12)));
        assert_eq!(group.border, UiRect::default());
        assert_eq!(group.row_gap, px(6));
        assert_eq!(binding.height, px(42));
        assert_eq!(binding.border, UiRect::all(px(1)));
        assert_eq!(
            mapping_binding_colors(Highlight::None, false),
            (BUTTON_BACKGROUND, BLUE_BORDER)
        );
        assert_eq!(
            mapping_binding_colors(Highlight::Hovered, false),
            (presentation::BUTTON_HOVERED, BLUE_BORDER)
        );
        assert_eq!(
            mapping_binding_colors(Highlight::Pressed, false),
            (presentation::BUTTON_PRESSED, BLUE_BORDER)
        );
        assert_eq!(
            mapping_binding_colors(Highlight::None, true),
            (presentation::ORANGE_ACTIVE, ORANGE)
        );
    }
}
