//! The settings screen: display preferences, match rules, volume, and keyboard
//! and controller remapping.
//!
//! # Responsibility
//!
//! [`ControlMappingState`] is the screen's own transient state: which tab is
//! shown, which binding or number is currently accepting input, and errors. It
//! is reset on every entry so a half-finished edit never survives
//! navigation, but the *error* is not cleared on entry, so a controls file
//! that failed to load at startup is still reported the first time the player
//! opens Settings. It is then dismissed like any other error - by a successful
//! save, a tab switch, or starting or cancelling a capture.
//!
//! # Data flow
//!
//! Every accepted change is written to the resources immediately and then
//! persisted through [`persist_settings`], which writes the whole
//! [`ControlsConfig`] atomically - every tab, no Save button, same policy for
//! local settings and the Match preset alike (see
//! [Configuration ownership](../../../docs/ARCHITECTURE.md#configuration-ownership)
//! for the three-way ownership boundary this rests on). A failed write leaves
//! the in-memory change in place - never rolled back - and shows
//! [`SETTINGS_NOT_SAVED_WARNING`] rather than the raw technical error. The
//! error goes through [`crate::diagnostics::report_recoverable`] like every
//! other recoverable failure that happens once the app is running (the one
//! startup exception is described in [`crate::diagnostics`]), which logs it
//! and pushes the same player-safe warning onto
//! [`crate::diagnostics::Diagnostics`]; this screen's inline text is
//! unaffected by that and renders exactly as before, since the boundary is
//! additive rather than a replacement for a screen's own display. There is no
//! separate retry step: since every persist call writes the *complete* current
//! configuration, the next accepted edit on any tab retries everything still
//! pending, including whatever failed earlier.
//!
//! # Keyboard and controller operation
//!
//! Every control on this screen is in the focus ring [`crate::ui`]'s focus
//! module draws - one `Screen` ring over the tab bar, the panel on show and
//! the footer, with the reset confirmation as the screen's one `Modal` - and
//! the screen raises that module's `FocusYield` for every frame a binding
//! capture or a numeric editor is reading the keys, so the ring neither
//! moves nor presses on the very input a control is waiting for. Where each
//! control sits, which disabled-looking controls stay in the ring and which
//! leave it, and why the ring stops moving as well as pressing while it
//! yields, are all in [`ring`].
//!
//! The screen's back input - Escape, or a pad's Start or East - runs *first*
//! in the input chain, before the button router, so a pointer click and a
//! back press landing in the same frame resolve to one step back rather than
//! two; [`handle_mapping_escape`] names the same-frame pairs whose outcome
//! that ordering decides.
//!
//! # Why this screen has six submodules
//!
//! Most screens divide into a `mod.rs` and a `layout.rs`; this one introduced
//! the `display.rs` third. Four tabs later that was no longer enough, and the
//! extra cuts fall by tab, because that is where the state actually differs:
//!
//! * [`game`] - the Match-rules tab. The only tab whose widgets are typed
//!   into, so it owns an editing session, a commit that can fail, and the row
//!   shapes those rules are expressed in.
//! * [`bindings`] - the Keyboard and Controller tabs. A binding row means "I
//!   am the row now being captured", and the capture systems are the only
//!   readers of that meaning, so the rows and the capture live together.
//! * [`layout`] - the frame those panels hang in, the reset-confirmation
//!   modal, and the Preferences panel, whose toggles have no state to own;
//!   and the pass that keeps the frame where [`geometry`] puts it.
//! * [`geometry`] - where the frame stands for the window's size, as a pure
//!   function of it, which the spawners and that pass both read and the
//!   tests run at every size the layouts are checked at.
//! * [`display`] - the whole screen's per-frame refresh, all four tabs
//!   together, because it runs as one pass in
//!   [`AppSystems::SettingsDisplay`].
//! * [`ring`] - the screen's place in the focus ring: the order bands the
//!   three spawning modules draw from, the yield, and the tests that hold
//!   the reset confirmation's trap. It cuts across the tabs the way
//!   [`display`] does, and for the same reason: one ring, one pass.
//!
//! What stays here is the plugin, [`ControlMappingState`], the copy and tab
//! vocabulary, the component markers the other modules spawn and query, the
//! button router that decides what an activation means, and
//! [`persist_settings`], which every tab ends up calling.

mod bindings;
mod display;
mod game;
mod geometry;
mod layout;
mod ring;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::button::{ButtonActivation, UiButtonPlugin};
use super::hud::HudAction;
use crate::app::visuals::WindowSize;
use crate::app::{AppScreen, AppSystems};
use crate::audio::{AudioChannel, AudioCue, AudioCueQueue, UiCue, volume_preview};
use crate::input::GameAction;
use crate::input::controller::{ControllerAssignments, ControllerBindings, ProfileAction};
use crate::input::keyboard::KeyboardBindings;
use crate::match_settings::{MatchSettingField, MatchSettings};
use crate::rules::Player;
use crate::settings::{
    ControlsConfig, GamePreferences, SettingsStore, VolumeLevel, save_user_controls,
};
use crate::ui::text_input::TextEditor;
use bindings::{capture_controller_mapping, capture_mapping_key};
use display::{
    update_control_mapping_display, update_game_settings_display, update_mapping_tab_panels,
    update_settings_tab_buttons,
};
use game::{GameEditor, capture_game_numeric_input, commit_active_game_editor, game_input_text};
use layout::{place_settings_furniture, spawn_control_mapping_screen};
use ring::yield_focus_while_capturing;

/// Owns the settings screen lifecycle, capture systems and display refresh.
pub(super) struct SettingsPlugin {
    binding_warning: Option<String>,
}

impl SettingsPlugin {
    pub(super) fn new(binding_warning: Option<String>) -> Self {
        Self { binding_warning }
    }
}

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        UiButtonPlugin::require(app);
        app.insert_resource(ControlMappingState::new(self.binding_warning.clone()))
            .add_systems(
                OnEnter(AppScreen::ControlMapping),
                (prepare_control_mapping_screen, spawn_control_mapping_screen).chain(),
            )
            .add_systems(
                Update,
                // Back first, so a same-frame click cannot pop a second
                // layer (see `handle_mapping_escape`); the yield last, after
                // every way a capture or editing session can begin or end.
                (
                    handle_mapping_escape,
                    handle_mapping_buttons,
                    capture_game_numeric_input,
                    capture_mapping_key,
                    capture_controller_mapping,
                    yield_focus_while_capturing,
                )
                    .chain()
                    .run_if(in_state(AppScreen::ControlMapping))
                    .in_set(AppSystems::SettingsInput),
            )
            .add_systems(
                Update,
                (
                    update_control_mapping_display,
                    update_game_settings_display,
                    update_mapping_tab_panels,
                    update_settings_tab_buttons,
                )
                    .chain()
                    .run_if(in_state(AppScreen::ControlMapping))
                    .in_set(AppSystems::SettingsDisplay),
            )
            .add_systems(
                Update,
                place_settings_furniture
                    .run_if(in_state(AppScreen::ControlMapping))
                    .run_if(resource_changed::<WindowSize>)
                    .in_set(AppSystems::ScreenLayout),
            );
    }
}

pub(super) const SETTINGS_TITLE: &str = "Settings";
pub(super) const PREFERENCES_INSTRUCTION: &str =
    "Choose which match details are shown. Changes are saved automatically.";
const KEYBOARD_MAPPING_INSTRUCTION: &str = "Choose a key binding, then press a key to change it.";
const CONTROLLER_MAPPING_INSTRUCTION: &str =
    "Choose a binding. Then use a controller to change it.";
pub(super) const GAME_INSTRUCTION: &str =
    "Choose optional match limits. Changes are saved automatically.";
pub(super) const RESET_MAPPINGS_TITLE: &str = "Reset All Controls?";
pub(super) const RESET_MAPPINGS_BODY: &str =
    "All custom keyboard and controller bindings will be replaced by the defaults.";

pub(super) const SETTINGS_TAB_ORDER: [MappingTab; 4] = [
    MappingTab::Preferences,
    MappingTab::Game,
    MappingTab::Keyboard,
    MappingTab::Controller,
];

/// The four settings tabs, in the order they are shown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MappingTab {
    Preferences,
    Game,
    Keyboard,
    Controller,
}

/// The [`HudAction`] that selects a tab, plus the label shown on its button.
///
/// Keeping both in one place means the tab strip and the tests cannot disagree
/// about which button does what.
pub(super) const fn settings_tab_action_and_label(tab: MappingTab) -> (HudAction, &'static str) {
    match tab {
        MappingTab::Preferences => (HudAction::SwitchToPreferencesTab, "Preferences"),
        MappingTab::Game => (HudAction::SwitchToGameTab, "Game"),
        MappingTab::Keyboard => (HudAction::SwitchToKeyboardTab, "Keyboard"),
        MappingTab::Controller => (HudAction::SwitchToControllerTab, "Controller"),
    }
}

#[derive(Resource)]
/// Transient state of the settings screen.
///
/// Everything except `error` is reset on entry, so a half-finished capture
/// never survives navigation. `error` is kept so a controls file that failed
/// to load at startup is still reported on the first visit; ordinary
/// interaction - a tab switch, a capture, a successful save - clears it.
pub(super) struct ControlMappingState {
    tab: MappingTab,
    capturing: Option<GameAction>,
    capturing_controller: Option<(Player, ProfileAction)>,
    game_editor: Option<GameEditor>,
    /// The Game tab's validation failure for one row. Keyed by the input that
    /// was being edited, but every reader compares by `GameNumericInput::field`,
    /// so a duration's minutes and seconds inputs share the row's single error
    /// line - spawned tagged with the minutes input in `game`'s row spawner.
    game_error: Option<(GameNumericInput, String)>,
    reset_confirmation_open: bool,
    error: String,
}

impl ControlMappingState {
    fn new(load_warning: Option<String>) -> Self {
        Self {
            tab: MappingTab::Preferences,
            capturing: None,
            capturing_controller: None,
            game_editor: None,
            game_error: None,
            reset_confirmation_open: false,
            error: load_warning.unwrap_or_default(),
        }
    }

    /// Whether one of this screen's controls is reading the raw devices this
    /// frame - a keyboard or controller binding capture listening for its
    /// input, or the Game tab's numeric editor taking keystrokes - which is
    /// when the focus ring yields (see [`ring`]). An open reset confirmation
    /// suspends all three: every capture system returns early under it, and
    /// a numeric editor whose text would not commit is left open *under* the
    /// dialog by a reset request, so the dialog's own ring must not freeze
    /// for a field that is not listening.
    fn owns_raw_input(&self) -> bool {
        !self.reset_confirmation_open
            && (self.capturing.is_some()
                || self.capturing_controller.is_some()
                || self.game_editor.is_some())
    }
}

#[derive(Component)]
pub(super) enum MappingUiRole {
    Instruction,
    Binding(GameAction),
    Error,
}

#[derive(Component)]
pub(super) struct MappingBinding(GameAction);

#[derive(Component)]
pub(super) enum ControllerMappingUiRole {
    Binding(Player, ProfileAction),
    Status(Player),
}

#[derive(Component)]
pub(super) struct ControllerMappingBinding(Player, ProfileAction);

#[derive(Component)]
pub(super) struct MappingPanel(MappingTab);

#[derive(Component)]
pub(super) struct SettingsTabButton(MappingTab);

#[derive(Component)]
pub(super) struct SettingsTabText(MappingTab);

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreferenceToggle {
    MatchTimer,
    RoundNumber,
    ReduceMotion,
}

#[derive(Component)]
pub(super) struct PreferenceValue(PreferenceToggle);

#[derive(Component, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum GameNumericInput {
    RoundLimit,
    RoundTimeMinutes,
    RoundTimeSeconds,
    NoStoneRoundLimit,
    MatchTimeMinutes,
    MatchTimeSeconds,
    PointTarget,
}

impl GameNumericInput {
    const fn field(self) -> MatchSettingField {
        match self {
            Self::RoundLimit => MatchSettingField::RoundLimit,
            Self::RoundTimeMinutes | Self::RoundTimeSeconds => MatchSettingField::RoundTime,
            Self::NoStoneRoundLimit => MatchSettingField::NoStoneRoundLimit,
            Self::MatchTimeMinutes | Self::MatchTimeSeconds => MatchSettingField::MatchTime,
            Self::PointTarget => MatchSettingField::PointTarget,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GameToggle {
    Limit(MatchSettingField),
    CapturePoints,
}

#[derive(Component)]
pub(super) struct GameToggleValue(pub(super) GameToggle);

#[derive(Component)]
pub(super) struct GameInputValue(pub(super) GameNumericInput);

#[derive(Component)]
pub(super) struct GameInputError(pub(super) GameNumericInput);

#[derive(Component)]
/// The `Min M:SS` caption under the Match Time row.
///
/// Only that row has one, because its floor moves:
/// [`MatchSettings::match_time_minimum`] is the Round Time value while Round
/// Time is enabled and one second otherwise, so [`display`] rewrites the
/// caption every frame. The Round Time row shows a static `MM:SS` hint
/// instead.
pub(super) struct MatchMinimumLabel;

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VolumeStop {
    pub(super) channel: AudioChannel,
    pub(super) level: VolumeLevel,
}

#[derive(Component)]
pub(super) struct VolumeStopLabel(VolumeStop);

#[derive(Component)]
pub(super) struct ResetMappingsModal;

fn prepare_control_mapping_screen(mut mapping: ResMut<ControlMappingState>) {
    initialize_settings_state(&mut mapping);
}

fn initialize_settings_state(mapping: &mut ControlMappingState) {
    mapping.tab = MappingTab::Preferences;
    mapping.capturing = None;
    mapping.capturing_controller = None;
    mapping.game_editor = None;
    mapping.game_error = None;
    mapping.reset_confirmation_open = false;
}

/// This frame's button activations, and every kind of button the settings
/// screen reacts to.
///
/// One reader for all seven kinds keeps them - and the order in which they
/// are consumed - in one place: the reset dialog is handled first and
/// swallows everything else, so a confirmation click can never also start a
/// binding capture underneath the modal. The reader is drained every frame,
/// swallowed or not, so nothing a frame ignores is seen again the next.
/// "Everything else" is everything *this* router reads: the footer's Back
/// carries [`HudAction::BackToHome`], which `handle_ui_buttons` reads on
/// every screen with no gate, so what keeps Back out of reach under the
/// dialog is not this router but the overlay (for the pointer) and the modal
/// focus group (for the ring) - see [`ring`].
#[derive(SystemParam)]
struct SettingsButtons<'w, 's> {
    activations: MessageReader<'w, 's, ButtonActivation>,
    bindings: Query<'w, 's, &'static MappingBinding>,
    controller_bindings: Query<'w, 's, &'static ControllerMappingBinding>,
    preferences: Query<'w, 's, &'static PreferenceToggle>,
    game_toggles: Query<'w, 's, &'static GameToggle>,
    game_inputs: Query<'w, 's, &'static GameNumericInput>,
    volumes: Query<'w, 's, &'static VolumeStop>,
    actions: Query<'w, 's, &'static HudAction>,
}

/// The intents, among this frame's `activated` buttons, of those `buttons`
/// matches, in the order they were activated.
fn activated_in<'a, C: Component>(
    buttons: &'a Query<'_, '_, &'static C>,
    activated: &'a [Entity],
) -> impl Iterator<Item = &'a C> {
    activated
        .iter()
        .filter_map(|entity| buttons.get(*entity).ok())
}

#[derive(SystemParam)]
struct SettingsValues<'w> {
    keyboard: ResMut<'w, KeyboardBindings>,
    controller: ResMut<'w, ControllerBindings>,
    preferences: ResMut<'w, GamePreferences>,
    match_settings: ResMut<'w, MatchSettings>,
    store: Res<'w, SettingsStore>,
    diagnostics: ResMut<'w, crate::diagnostics::Diagnostics>,
}

fn handle_mapping_buttons(
    mut buttons: SettingsButtons,
    mut settings: SettingsValues,
    assignments: Res<ControllerAssignments>,
    mut mapping: ResMut<ControlMappingState>,
    mut audio: ResMut<AudioCueQueue>,
) {
    let activated: Vec<Entity> = buttons
        .activations
        .read()
        .map(|activation| activation.entity)
        .collect();
    if mapping.reset_confirmation_open {
        for action in activated_in(&buttons.actions, &activated) {
            match action {
                HudAction::CancelResetMappings => {
                    mapping.reset_confirmation_open = false;
                    mapping.error.clear();
                }
                HudAction::ConfirmResetMappings => {
                    settings.keyboard.reset_to_defaults();
                    settings.controller.reset_to_defaults();
                    mapping.reset_confirmation_open = false;
                    persist_settings(
                        &settings.store,
                        &settings.keyboard,
                        &settings.controller,
                        &settings.preferences,
                        &settings.match_settings,
                        &mut mapping,
                        &mut settings.diagnostics,
                    );
                }
                _ => {}
            }
        }
        return;
    }

    for action in activated_in(&buttons.actions, &activated) {
        match action {
            HudAction::RequestResetMappings => {
                mapping.error.clear();
                commit_active_game_editor(&mut settings, &mut mapping);
                let commit_error = std::mem::take(&mut mapping.error);
                mapping.capturing = None;
                mapping.capturing_controller = None;
                mapping.reset_confirmation_open = true;
                mapping.error = commit_error;
            }
            HudAction::SwitchToPreferencesTab => {
                switch_settings_tab_with_commit(
                    &mut settings,
                    &mut mapping,
                    MappingTab::Preferences,
                );
            }
            HudAction::SwitchToGameTab => {
                switch_settings_tab_with_commit(&mut settings, &mut mapping, MappingTab::Game);
            }
            HudAction::SwitchToKeyboardTab => {
                switch_settings_tab_with_commit(&mut settings, &mut mapping, MappingTab::Keyboard);
            }
            HudAction::SwitchToControllerTab => {
                switch_settings_tab_with_commit(
                    &mut settings,
                    &mut mapping,
                    MappingTab::Controller,
                );
            }
            _ => commit_active_game_editor(&mut settings, &mut mapping),
        }
    }

    for preference in activated_in(&buttons.preferences, &activated) {
        toggle_preference(&mut settings.preferences, *preference);
        persist_settings(
            &settings.store,
            &settings.keyboard,
            &settings.controller,
            &settings.preferences,
            &settings.match_settings,
            &mut mapping,
            &mut settings.diagnostics,
        );
    }

    for stop in activated_in(&buttons.volumes, &activated) {
        if set_volume_level(&mut settings.preferences, *stop) {
            persist_settings(
                &settings.store,
                &settings.keyboard,
                &settings.controller,
                &settings.preferences,
                &settings.match_settings,
                &mut mapping,
                &mut settings.diagnostics,
            );
            if let Some(preview) = volume_preview(stop.channel, stop.level) {
                match preview {
                    AudioCue::Game(cue) => audio.game(cue),
                    AudioCue::Ui(cue) => audio.ui(cue),
                }
            }
        }
    }

    for toggle in activated_in(&buttons.game_toggles, &activated) {
        commit_active_game_editor(&mut settings, &mut mapping);
        apply_game_toggle(
            &mut settings.match_settings,
            &mut settings.preferences,
            *toggle,
        );
        persist_current_settings(&mut settings, &mut mapping);
    }

    for input in activated_in(&buttons.game_inputs, &activated) {
        if !settings.match_settings.enabled(input.field()) {
            continue;
        }
        if mapping
            .game_editor
            .as_ref()
            .is_some_and(|editor| editor.input == *input)
        {
            continue;
        }
        commit_active_game_editor(&mut settings, &mut mapping);
        mapping.game_editor = Some(GameEditor {
            input: *input,
            editor: TextEditor::new(game_input_text(*input, &settings.match_settings)),
        });
    }

    for binding in activated_in(&buttons.bindings, &activated) {
        mapping.capturing = Some(binding.0);
        mapping.capturing_controller = None;
        mapping.error.clear();
    }

    for binding in activated_in(&buttons.controller_bindings, &activated) {
        if assignments.assigned_entity(binding.0).is_some() {
            mapping.capturing_controller = Some((binding.0, binding.1));
            mapping.capturing = None;
            mapping.error.clear();
        } else {
            mapping.capturing_controller = None;
            mapping.error = CONNECT_A_CONTROLLER_MESSAGE.into();
        }
    }
}

fn set_volume_level(preferences: &mut GamePreferences, stop: VolumeStop) -> bool {
    let current = match stop.channel {
        AudioChannel::Game => &mut preferences.game_sound_effects,
        AudioChannel::Ui => &mut preferences.ui_sound_effects,
    };
    let changed = *current != stop.level;
    *current = stop.level;
    changed
}

/// Switches tabs, cancelling any capture in progress. Returns `false` when
/// the tab was already selected, so a redundant click changes nothing.
fn switch_settings_tab(mapping: &mut ControlMappingState, tab: MappingTab) -> bool {
    if mapping.tab == tab {
        return false;
    }
    mapping.tab = tab;
    mapping.capturing = None;
    mapping.capturing_controller = None;
    mapping.game_editor = None;
    mapping.game_error = None;
    mapping.error.clear();
    true
}

fn switch_settings_tab_with_commit(
    settings: &mut SettingsValues,
    mapping: &mut ControlMappingState,
    tab: MappingTab,
) -> bool {
    if mapping.tab == tab {
        return false;
    }
    mapping.error.clear();
    commit_active_game_editor(settings, mapping);
    let commit_error = std::mem::take(&mut mapping.error);
    let switched = switch_settings_tab(mapping, tab);
    mapping.error = commit_error;
    switched
}

pub(super) fn toggle_preference(preferences: &mut GamePreferences, preference: PreferenceToggle) {
    match preference {
        PreferenceToggle::MatchTimer => {
            preferences.show_match_timer = !preferences.show_match_timer;
        }
        PreferenceToggle::RoundNumber => {
            preferences.show_round_number = !preferences.show_round_number;
        }
        PreferenceToggle::ReduceMotion => {
            preferences.reduce_motion = !preferences.reduce_motion;
        }
    }
}

/// Flips a Game-tab On/Off rule and, when that turns a timed rule *on*,
/// reveals its clock by switching the matching Preferences-tab readout on
/// too: Round Time reveals Show Round Number, Match Time reveals Show Match
/// Timer.
///
/// That is a Game-tab edit writing a Preferences-tab value, which no other
/// edit on this screen does, and it is deliberate. Both readouts are player
/// preferences, and Show Match Timer is off by default, so without this the
/// most common way to play a timed Match was with no clock on screen: a
/// player could start a Match that ends on a deadline they were never shown.
/// It *reveals* rather than forces, so the readout stays a preference - the
/// player may turn it off again afterwards and it stays off, and turning the
/// rule back off leaves the preference wherever the player last put it. Both
/// of those hold only because this runs on the enable edge of a toggle press
/// and nowhere else: never on a settings save, which would re-enable a
/// readout the player had just hidden, and never on load. The edge is
/// knowable here precisely because the rule is a toggle - the field's state
/// immediately after flipping it says which way it went.
///
/// The round clock's indication that is *not* a preference - the bar below
/// the board - is drawn by `ui::match_screen` from the Match's own state and
/// needs nothing from here.
fn apply_game_toggle(
    match_settings: &mut MatchSettings,
    preferences: &mut GamePreferences,
    toggle: GameToggle,
) {
    match toggle {
        GameToggle::Limit(field) => {
            match_settings.toggle_enabled(field);
            if match_settings.enabled(field) {
                reveal_clock(preferences, field);
            }
        }
        GameToggle::CapturePoints => match_settings.toggle_capture_points(),
    }
}

/// Switches on the readout a timed rule shows its clock in. The three
/// untimed rules have no clock to reveal and leave preferences alone.
fn reveal_clock(preferences: &mut GamePreferences, field: MatchSettingField) {
    match field {
        MatchSettingField::RoundTime => preferences.show_round_number = true,
        MatchSettingField::MatchTime => preferences.show_match_timer = true,
        MatchSettingField::RoundLimit
        | MatchSettingField::NoStoneRoundLimit
        | MatchSettingField::PointTarget => {}
    }
}

fn persist_current_settings(settings: &mut SettingsValues, mapping: &mut ControlMappingState) {
    persist_settings(
        &settings.store,
        &settings.keyboard,
        &settings.controller,
        &settings.preferences,
        &settings.match_settings,
        mapping,
        &mut settings.diagnostics,
    );
}

/// Escape, or a pad's Start or East button, steps back one layer: it closes
/// the reset confirmation, else abandons the Game tab's editing session,
/// else cancels a binding capture, else returns to Home - the same cue every
/// step.
///
/// Runs *first* in the settings input chain, the shape the Replay Library's
/// back handler has, so a pointer click landing in the same frame as a back
/// press cannot make the pair pop two layers: back reads the layer stack as
/// the frame began, and the click's own handler then runs against whatever
/// back left. Before this ordering, a Cancel click plus Escape closed the
/// dialog and then left Settings, and a virtual Enter on Confirm Reset plus
/// Escape reset every binding *and* left. Now the first pops one layer and
/// the second resets nothing: with the dialog already closed, the router
/// treats the confirmation's buttons as inert, so the destructive read of a
/// same-frame pair loses the tie, as it does on the Match screen.
///
/// The same rule decides every other same-frame pair of a back press and a
/// press, and its consequences are named here rather than left to be
/// discovered. Each needs two inputs inside one frame, and each resolves to
/// the step back asked for, applied *before* the press:
///
/// * From idle, back plus a press that would open a layer - arm a binding
///   capture, open a numeric editor, request the reset confirmation - used
///   to open it and close it in one frame; it now leaves Settings, and the
///   layer opens on a screen that is already leaving (re-entry resets it).
/// * With a numeric editor open, back plus an input that would commit it -
///   a tab switch, a toggle, Back, another field, Reset Controls, or the
///   Enter keystroke itself - used to commit the value and then step back;
///   it now abandons the typed value first, and the press applies to the
///   screen as back left it. Escape plus Enter is the one pure-keyboard pair
///   here, and `escape_and_enter_in_one_frame_abandon_an_open_number` pins
///   it: the value is discarded and the player stays.
/// * With a capture armed, back plus a press that would have cleared it
///   used to clear it and then step back; it now cancels the capture, and
///   the press applies: a tab switches and the player stays, another row
///   arms afresh, Reset Controls opens its dialog.
/// * With a controller capture armed, East plus a press that would have
///   cleared the capture used to read as back, because back ran after the
///   clearing; it now reads as the input being bound, and is dropped.
fn handle_mapping_escape(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut mapping: ResMut<ControlMappingState>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut audio: ResMut<AudioCueQueue>,
) {
    // A pad's Start and East buttons are back on the menu screens, with one
    // carve-out for East that neither Escape nor Start needs: East is itself
    // a bindable controller input, so while a controller capture is armed, a
    // pressed East is the player *binding* East and must reach
    // `capture_controller_mapping` instead of cancelling it. Escape can never
    // be bound (reserved, as F1 is) and Start is absent from
    // `ControllerInput` altogether, so both still cancel a controller
    // capture - Start being a pad-only player's only way to do so.
    let pad_back = gamepads.iter().any(|gamepad| {
        gamepad.just_pressed(GamepadButton::Start)
            || (mapping.capturing_controller.is_none() && gamepad.just_pressed(GamepadButton::East))
    });
    if !keys.just_pressed(KeyCode::Escape) && !pad_back {
        return;
    }
    if mapping.reset_confirmation_open {
        mapping.reset_confirmation_open = false;
        mapping.error.clear();
    } else if let Some(active) = mapping.game_editor.take() {
        if mapping
            .game_error
            .as_ref()
            .is_some_and(|(input, _)| input.field() == active.input.field())
        {
            mapping.game_error = None;
        }
    } else if mapping.capturing.is_some() || mapping.capturing_controller.is_some() {
        mapping.capturing = None;
        mapping.capturing_controller = None;
        mapping.error.clear();
    } else {
        next_screen.set(AppScreen::Home);
    }
    audio.ui(UiCue::Cancel);
}

/// What the error line says when a controller row is pressed for a player
/// with no controller assigned.
pub(super) const CONNECT_A_CONTROLLER_MESSAGE: &str =
    "Connect a controller for this player before changing its bindings.";

/// Shown in place of the raw storage error whenever a settings write fails,
/// across every tab. The wording is deliberately reassuring rather than
/// technical - a player does not need to know *why* saving failed, only that
/// their change is still in effect and what they stand to lose - and
/// deliberately identical every time, so the same failure does not read as a
/// new, distinct problem on every subsequent edit.
pub(super) const SETTINGS_NOT_SAVED_WARNING: &str = "Your changes are active for this session but could not be saved. They may be lost when the game closes.";

/// Writes the whole control configuration to the platform settings store - an
/// atomic `controls.json` natively, a `localStorage` key in a browser.
///
/// The in-memory change is already applied when this runs (see this
/// module's doc comment for the full apply-then-persist policy), so a
/// failed write surfaces [`SETTINGS_NOT_SAVED_WARNING`] but never rolls the
/// change back. One failure produces two records of that warning:
/// [`crate::diagnostics::report_recoverable`] logs the
/// [`SettingsStorageError`](crate::platform::SettingsStorageError) and pushes
/// the warning onto [`crate::diagnostics::Diagnostics`], while `mapping.error`
/// puts the same text inline on this screen, which is where a player actually
/// reads it today. The next accepted edit on any tab calls this again with
/// the complete, current configuration - not just what changed since the
/// failure - so a transient failure (a full disk, a momentarily denied
/// browser storage grant) self-heals on the next successful write without
/// needing any separate retry mechanism: whatever was different at failure
/// time is still different now, and gets persisted along with everything
/// else.
fn persist_settings(
    store: &SettingsStore,
    keyboard: &KeyboardBindings,
    controller: &ControllerBindings,
    preferences: &GamePreferences,
    match_settings: &MatchSettings,
    mapping: &mut ControlMappingState,
    diagnostics: &mut crate::diagnostics::Diagnostics,
) {
    let config = ControlsConfig {
        keyboard: keyboard.clone(),
        controller: controller.clone(),
        preferences: *preferences,
        match_settings: match_settings.clone(),
    };
    match save_user_controls(store, &config) {
        Ok(()) => mapping.error.clear(),
        Err(error) => {
            crate::diagnostics::report_recoverable(diagnostics, &error, SETTINGS_NOT_SAVED_WARNING);
            mapping.error = SETTINGS_NOT_SAVED_WARNING.into();
        }
    }
}

pub(super) const fn mapping_instruction(tab: MappingTab) -> &'static str {
    match tab {
        MappingTab::Preferences => PREFERENCES_INSTRUCTION,
        MappingTab::Game => GAME_INSTRUCTION,
        MappingTab::Keyboard => KEYBOARD_MAPPING_INSTRUCTION,
        MappingTab::Controller => CONTROLLER_MAPPING_INSTRUCTION,
    }
}

pub(super) fn preference_enabled(
    preferences: &GamePreferences,
    preference: PreferenceToggle,
) -> bool {
    match preference {
        PreferenceToggle::MatchTimer => preferences.show_match_timer,
        PreferenceToggle::RoundNumber => preferences.show_round_number,
        PreferenceToggle::ReduceMotion => preferences.reduce_motion,
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::button::{UiButtonPlugin, trigger_activation};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::display::settings_tab_colors;
    use super::layout::mapping_error_node;
    use super::*;
    use crate::input::controller;
    use crate::input::keyboard::KeyboardKey;
    use crate::presentation;
    use crate::presentation::Highlight;
    use crate::presentation::{
        BLUE_BORDER, BUTTON_DISABLED, BUTTON_DISABLED_BORDER, TEXT_MUTED, TEXT_PRIMARY,
    };
    use crate::ui::hud::match_hud_visibility;

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

    #[derive(Clone, Copy)]
    struct FailingSettingsStorage;

    impl crate::platform::SettingsStorage for FailingSettingsStorage {
        fn load(&self) -> Result<Option<String>, crate::platform::SettingsStorageError> {
            Ok(None)
        }

        fn save(&self, _json: &str) -> Result<(), crate::platform::SettingsStorageError> {
            Err(crate::platform::SettingsStorageError::Failed(
                "mock settings quota exceeded".into(),
            ))
        }
    }

    /// Fails saves with the shape `platform::native` actually produces - a
    /// full path plus a `std::io::Error` - so a test can tell a genuinely
    /// generic player-facing message apart from one that only looks clean
    /// because the mock's own text happened to be clean.
    const LEAKED_SETTINGS_ERROR: &str = "Could not save settings to /Users/name/Library/Application Support/go-together/settings.json: No space left on device (os error 28)";

    #[derive(Clone, Copy)]
    struct PathLeakingSettingsStorage;

    impl crate::platform::SettingsStorage for PathLeakingSettingsStorage {
        fn load(&self) -> Result<Option<String>, crate::platform::SettingsStorageError> {
            Ok(None)
        }

        fn save(&self, _json: &str) -> Result<(), crate::platform::SettingsStorageError> {
            Err(crate::platform::SettingsStorageError::Failed(
                LEAKED_SETTINGS_ERROR.into(),
            ))
        }
    }

    /// Fails saves while `fail` is `true`, otherwise records the JSON like
    /// [`RecordingSettingsStorage`] - lets a test flip a storage backend from
    /// failing to working mid-session, the way a full disk or a denied
    /// browser storage grant might resolve itself between one edit and the
    /// next.
    #[derive(Clone)]
    struct FlakySettingsStorage {
        fail: Arc<AtomicBool>,
        saved: Arc<Mutex<Option<String>>>,
    }

    impl crate::platform::SettingsStorage for FlakySettingsStorage {
        fn load(&self) -> Result<Option<String>, crate::platform::SettingsStorageError> {
            Ok(self.saved.lock().unwrap().clone())
        }

        fn save(&self, json: &str) -> Result<(), crate::platform::SettingsStorageError> {
            if self.fail.load(Ordering::Relaxed) {
                return Err(crate::platform::SettingsStorageError::Failed(
                    "mock quota exceeded".into(),
                ));
            }
            *self.saved.lock().unwrap() = Some(json.into());
            Ok(())
        }
    }

    fn failing_numeric_commit_app(action: HudAction) -> App {
        let mut mapping = ControlMappingState::new(Some("stale transient error".into()));
        mapping.tab = MappingTab::Game;
        mapping.game_editor = Some(GameEditor {
            input: GameNumericInput::RoundLimit,
            editor: TextEditor::new("7".into()),
        });
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(SettingsStore::from_storage(FailingSettingsStorage))
            .insert_resource(ControllerAssignments::default())
            .insert_resource(mapping)
            .insert_resource(AudioCueQueue::default())
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_mapping_buttons);
        let pressed = app.world_mut().spawn(action).id();
        trigger_activation(&mut app, pressed);
        app
    }

    #[test]
    fn settings_copy_is_focused_and_only_errors_are_retained() {
        assert_eq!(SETTINGS_TITLE, "Settings");
        assert_eq!(
            PREFERENCES_INSTRUCTION,
            "Choose which match details are shown. Changes are saved automatically."
        );
        // Device-neutral on purpose: a keyboard or pad player highlights a
        // binding with the focus ring and presses it, and never clicks.
        assert_eq!(
            KEYBOARD_MAPPING_INSTRUCTION,
            "Choose a key binding, then press a key to change it."
        );
        assert_eq!(
            CONTROLLER_MAPPING_INSTRUCTION,
            "Choose a binding. Then use a controller to change it."
        );
        assert!(ControlMappingState::new(None).error.is_empty());
        assert_eq!(
            ControlMappingState::new(Some("Could not load mappings.".into())).error,
            "Could not load mappings."
        );
        let error_node = mapping_error_node();
        assert_eq!(error_node.min_height, Val::Auto);
        assert_eq!(error_node.width, percent(100));
    }

    #[test]
    fn settings_state_defaults_to_preferences_tab_with_no_active_capture() {
        let mut mapping = ControlMappingState::new(None);
        assert_eq!(mapping.tab, MappingTab::Preferences);
        assert_eq!(mapping.capturing, None);
        assert_eq!(mapping.capturing_controller, None);
        assert!(!mapping.reset_confirmation_open);
        assert_eq!(
            SETTINGS_TAB_ORDER,
            [
                MappingTab::Preferences,
                MappingTab::Game,
                MappingTab::Keyboard,
                MappingTab::Controller
            ]
        );

        mapping.tab = MappingTab::Controller;
        mapping.capturing_controller = Some((Player::One, ProfileAction::Select));
        mapping.reset_confirmation_open = true;
        initialize_settings_state(&mut mapping);
        assert_eq!(mapping.tab, MappingTab::Preferences);
        assert_eq!(mapping.capturing_controller, None);
        assert!(!mapping.reset_confirmation_open);
    }

    /// The Settings screen as spawned, under real layout and a real
    /// pointer, with the two display systems that both wrote a tab button
    /// before the tabs were marked `ScreenStyled` - the HUD's shared pass and
    /// this screen's own - run in the order given, after the audio pass that
    /// sounds a hover, and with the observer that sounds an activation.
    fn tab_display_app(hud_pass_runs_last: bool) -> App {
        use super::display::update_settings_tab_buttons;
        use super::layout::spawn_control_mapping_screen;
        use crate::audio::{AudioCueQueue, emit_hover_cues, queue_activation_cue};
        use crate::gameplay::GameState;
        use crate::ui::focus::real_layout_app;
        use crate::ui::hud::{HudState, update_shared_display};

        let mut app = real_layout_app(crate::app::visuals::WindowSize::OPENING.0);
        app.insert_resource(ControlMappingState::new(None))
            .insert_resource(GameState::default())
            .insert_resource(HudState::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .insert_resource(AudioCueQueue::default())
            .add_observer(queue_activation_cue)
            .add_systems(Startup, spawn_control_mapping_screen);
        if hud_pass_runs_last {
            app.add_systems(
                Update,
                (
                    emit_hover_cues,
                    update_settings_tab_buttons,
                    update_shared_display,
                )
                    .chain(),
            );
        } else {
            app.add_systems(
                Update,
                (
                    emit_hover_cues,
                    update_shared_display,
                    update_settings_tab_buttons,
                )
                    .chain(),
            );
        }
        app.update();
        app
    }

    fn tab_entity(app: &mut App, tab: MappingTab) -> Entity {
        let world = app.world_mut();
        let found: Vec<Entity> = world
            .query::<(Entity, &SettingsTabButton)>()
            .iter(world)
            .filter(|(_, button)| button.0 == tab)
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(found.len(), 1, "one button per tab");
        found[0]
    }

    /// The selected tab is drawn inert and makes no click, whichever of the
    /// HUD's display pass and this screen's happens to run last.
    ///
    /// Nothing orders the HUD's display stage against this screen's, and
    /// before the tabs were marked `ScreenStyled` both wrote the tab's fill
    /// and click cue and disagreed about the selected one. So this runs the
    /// real systems over the real layout in both orders, with a real
    /// pointer. Each negative claim has its positive beside it, so the
    /// selected tab's stillness is not a system having failed to run: the
    /// unselected tab under the same pointer does take the hover fill and
    /// does click, which is this screen's pass and the audio at work, and
    /// the Back button takes the hover fill too, which only the HUD's pass
    /// gives it.
    #[test]
    fn the_selected_tab_stays_inert_in_either_display_order() {
        use crate::audio::{AudioCue, AudioCueQueue, UiCue};
        use crate::presentation::{BUTTON_BACKGROUND, BUTTON_HOVERED};
        use crate::ui::button::{click, hover, mouse_button};
        use bevy::input::ButtonState;

        for hud_pass_runs_last in [true, false] {
            let order = if hud_pass_runs_last {
                "the HUD pass last"
            } else {
                "the Settings pass last"
            };
            let mut app = tab_display_app(hud_pass_runs_last);
            let selected = tab_entity(&mut app, MappingTab::Preferences);
            let other = tab_entity(&mut app, MappingTab::Game);
            assert_eq!(
                app.world().resource::<ControlMappingState>().tab,
                MappingTab::Preferences
            );

            let fill = |app: &App, entity| app.world().get::<BackgroundColor>(entity).unwrap().0;
            let border = |app: &App, entity| *app.world().get::<BorderColor>(entity).unwrap();
            assert_eq!(fill(&app, selected), BUTTON_DISABLED, "{order}");
            assert_eq!(fill(&app, other), BUTTON_BACKGROUND, "{order}");

            // The pointer over each in turn, and over Back, which no screen
            // system styles: only the unselected tab and Back answer.
            let back = {
                let world = app.world_mut();
                let found: Vec<Entity> = world
                    .query::<(Entity, &HudAction)>()
                    .iter(world)
                    .filter(|(_, action)| **action == HudAction::BackToHome)
                    .map(|(entity, _)| entity)
                    .collect();
                assert_eq!(found.len(), 1, "one Back button");
                found[0]
            };
            assert_eq!(fill(&app, back), BUTTON_BACKGROUND, "{order}");
            hover(&mut app, back);
            assert_eq!(
                fill(&app, back),
                BUTTON_HOVERED,
                "{order}: the HUD's pass ran, for it alone styles Back"
            );
            hover(&mut app, selected);
            assert_eq!(fill(&app, selected), BUTTON_DISABLED, "{order}, hovered");
            assert_eq!(
                border(&app, selected),
                BorderColor::all(BUTTON_DISABLED_BORDER),
                "{order}, hovered"
            );
            hover(&mut app, other);
            assert_eq!(fill(&app, other), BUTTON_HOVERED, "{order}, hovered");
            app.world_mut().resource_mut::<AudioCueQueue>().drain();

            // A click on each: only the unselected tab sounds.
            hover(&mut app, selected);
            app.world_mut().resource_mut::<AudioCueQueue>().drain();
            mouse_button(&mut app, MouseButton::Left, ButtonState::Pressed);
            assert_eq!(fill(&app, selected), BUTTON_DISABLED, "{order}, pressed");
            mouse_button(&mut app, MouseButton::Left, ButtonState::Released);
            assert_eq!(
                app.world_mut().resource_mut::<AudioCueQueue>().drain(),
                Vec::<AudioCue>::new(),
                "{order}: the selected tab is silent"
            );

            hover(&mut app, other);
            app.world_mut().resource_mut::<AudioCueQueue>().drain();
            click(&mut app, other);
            assert_eq!(
                app.world_mut().resource_mut::<AudioCueQueue>().drain(),
                vec![AudioCue::Ui(UiCue::Neutral)],
                "{order}: an unselected tab clicks"
            );
        }
    }

    #[test]
    fn selected_settings_tabs_are_dimmed_and_redundant_activation_is_ignored() {
        assert_eq!(
            settings_tab_colors(Highlight::Pressed, true),
            (BUTTON_DISABLED, BUTTON_DISABLED_BORDER, TEXT_MUTED)
        );
        assert_eq!(
            settings_tab_colors(Highlight::Hovered, false),
            (presentation::BUTTON_HOVERED, BLUE_BORDER, TEXT_PRIMARY)
        );

        let mut mapping = ControlMappingState::new(None);
        mapping.error = "keep this".into();
        assert!(!switch_settings_tab(&mut mapping, MappingTab::Preferences));
        assert_eq!(mapping.error, "keep this");
        assert!(switch_settings_tab(&mut mapping, MappingTab::Keyboard));
        assert!(mapping.error.is_empty());
    }

    /// One of the five `report_recoverable` call sites - the others are
    /// `gameplay`'s Replay save and the Replay Library's open, rename and
    /// delete - and the one `crate::diagnostics`'s module doc names as its
    /// settings example.
    /// Nothing reads `Diagnostics` back in production, so this test is what
    /// keeps the message honest; it also pins that the boundary is additive
    /// - this screen's own inline text is unchanged by routing through it.
    #[test]
    fn a_failed_settings_write_reports_a_player_safe_message_to_the_boundary() {
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(SettingsStore::from_storage(PathLeakingSettingsStorage))
            .insert_resource(ControllerAssignments::default())
            .insert_resource(ControlMappingState::new(None))
            .insert_resource(AudioCueQueue::default())
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_mapping_buttons);
        let pressed = app.world_mut().spawn(GameToggle::CapturePoints).id();
        trigger_activation(&mut app, pressed);

        app.update();

        let diagnostics = app.world().resource::<crate::diagnostics::Diagnostics>();
        assert_eq!(
            diagnostics.active(),
            [crate::diagnostics::Diagnostic {
                severity: crate::diagnostics::Severity::Warning,
                message: SETTINGS_NOT_SAVED_WARNING.into(),
            }]
        );
        crate::diagnostics::assert_carries_no_internal_detail(&diagnostics.active()[0].message);
        assert_eq!(
            app.world().resource::<ControlMappingState>().error,
            SETTINGS_NOT_SAVED_WARNING,
            "the boundary is additive - the screen's own inline text is unaffected"
        );
        assert!(
            crate::diagnostics::carries_internal_detail(LEAKED_SETTINGS_ERROR),
            "the technical string this failure produced is what must not reach the player"
        );
    }

    #[test]
    fn tab_switch_preserves_a_numeric_commit_persistence_failure() {
        let mut app = failing_numeric_commit_app(HudAction::SwitchToKeyboardTab);

        app.update();

        let mapping = app.world().resource::<ControlMappingState>();
        assert_eq!(mapping.tab, MappingTab::Keyboard);
        assert_eq!(mapping.error, SETTINGS_NOT_SAVED_WARNING);
        assert!(mapping.game_editor.is_none());
        assert_eq!(
            app.world()
                .resource::<MatchSettings>()
                .value(MatchSettingField::RoundLimit),
            7
        );
    }

    #[test]
    fn reset_request_preserves_a_numeric_commit_persistence_failure() {
        let mut app = failing_numeric_commit_app(HudAction::RequestResetMappings);

        app.update();

        let mapping = app.world().resource::<ControlMappingState>();
        assert!(mapping.reset_confirmation_open);
        assert_eq!(mapping.error, SETTINGS_NOT_SAVED_WARNING);
        assert_eq!(
            app.world()
                .resource::<MatchSettings>()
                .value(MatchSettingField::RoundLimit),
            7
        );
    }

    /// The pad's back buttons on this screen, and the one carve-out: East
    /// and Start both step back exactly like Escape - until a controller
    /// capture is listening, when a pressed East is the player binding East
    /// and must reach the capture instead of cancelling it, while Start,
    /// which no binding can hold, cancels it as Escape does.
    #[test]
    fn east_and_start_go_back_but_only_start_cancels_a_controller_capture() {
        use crate::input::controller::{ControllerCaptureState, ControllerInput, DeviceFamily};

        let build = |capturing: Option<(Player, ProfileAction)>, button: GamepadButton| {
            let mut mapping = ControlMappingState::new(None);
            mapping.capturing_controller = capturing;
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin)
                .init_state::<AppScreen>()
                .insert_resource(KeyboardBindings::default())
                .insert_resource(ControllerBindings::default())
                .insert_resource(GamePreferences::default())
                .insert_resource(MatchSettings::default())
                .insert_resource(SettingsStore::from_storage(RecordingSettingsStorage(
                    Arc::new(Mutex::new(None)),
                )))
                .insert_resource(ControllerAssignments::default())
                .insert_resource(ControllerCaptureState::default())
                .insert_resource(mapping)
                .insert_resource(AudioCueQueue::default())
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<crate::diagnostics::Diagnostics>()
                .add_systems(
                    Update,
                    (handle_mapping_escape, capture_controller_mapping).chain(),
                );
            let mut gamepad = Gamepad::default();
            gamepad.digital_mut().press(button);
            let pad = app.world_mut().spawn(gamepad).id();
            app.world_mut()
                .resource_mut::<ControllerAssignments>()
                .connect(pad, DeviceFamily::Xbox, "Pad".into());
            app
        };

        // Idle: East and Start are both back, straight to Home.
        for button in [GamepadButton::East, GamepadButton::Start] {
            let mut app = build(None, button);
            app.update();
            assert!(matches!(
                *app.world().resource::<NextState<AppScreen>>(),
                NextState::Pending(AppScreen::Home)
            ));
        }

        // Capturing: Start cancels the capture and binds nothing, and the
        // screen stays - the one pad-only way out of a capture.
        let capture = Some((Player::Two, ProfileAction::Select));
        let mut app = build(capture, GamepadButton::Start);
        app.update();
        let mapping = app.world().resource::<ControlMappingState>();
        assert_eq!(mapping.capturing_controller, None);
        assert_eq!(
            app.world()
                .resource::<ControllerBindings>()
                .input(Player::Two, ProfileAction::Select),
            ControllerBindings::default().input(Player::Two, ProfileAction::Select),
            "cancelling a capture with Start must bind nothing"
        );
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));

        // Capturing: East is the input being bound. The first frame arms
        // the capture baseline; the still-held press registers on the next.
        let mut app = build(capture, GamepadButton::East);
        app.update();
        assert_eq!(
            app.world()
                .resource::<ControlMappingState>()
                .capturing_controller,
            Some((Player::Two, ProfileAction::Select)),
            "East must not cancel the capture it is about to fill"
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<ControllerBindings>()
                .input(Player::Two, ProfileAction::Select),
            ControllerInput::East
        );
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));
    }

    #[test]
    fn preferences_toggle_independently_and_control_reset_leaves_them_unchanged() {
        let mut preferences = GamePreferences::default();
        let mut match_settings = MatchSettings::default();
        match_settings.set_enabled(MatchSettingField::PointTarget, true);
        match_settings.set_value(MatchSettingField::PointTarget, 21);
        let expected_match_settings = match_settings.clone();
        let original_keyboard = KeyboardBindings::default();
        let original_controller = ControllerBindings::default();
        let mut keyboard = original_keyboard.clone();
        let mut controller = original_controller.clone();

        toggle_preference(&mut preferences, PreferenceToggle::MatchTimer);
        assert_eq!(match_hud_visibility(&preferences), (true, true));
        toggle_preference(&mut preferences, PreferenceToggle::RoundNumber);
        assert_eq!(match_hud_visibility(&preferences), (true, false));
        assert!(!preferences.reduce_motion);
        toggle_preference(&mut preferences, PreferenceToggle::ReduceMotion);
        assert!(preferences.reduce_motion);
        assert_eq!(
            match_hud_visibility(&preferences),
            (true, false),
            "motion is its own preference and touches neither readout"
        );
        toggle_preference(&mut preferences, PreferenceToggle::ReduceMotion);
        assert!(!preferences.reduce_motion);

        keyboard.assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        controller.assign(
            Player::One,
            ProfileAction::SetMarker,
            controller::ControllerInput::RightBumper,
        );
        keyboard.reset_to_defaults();
        controller.reset_to_defaults();

        assert_eq!(keyboard, original_keyboard);
        assert_eq!(controller, original_controller);
        assert_eq!(keyboard.key(GameAction::PlayerTwoUp), KeyboardKey::E);
        assert_eq!(
            keyboard.key(GameAction::PlayerTwoSelect),
            KeyboardKey::Digit2
        );
        assert_eq!(
            keyboard.key(GameAction::PlayerTwoSetMarker),
            KeyboardKey::Backtick
        );
        assert_eq!(
            keyboard.key(GameAction::PlayerTwoJumpToMarker),
            KeyboardKey::Digit1
        );
        assert_eq!(match_hud_visibility(&preferences), (true, false));
        assert_eq!(match_settings, expected_match_settings);
    }

    /// Turning a timed rule on reveals its clock, and that is the whole of
    /// what the Game tab decides about a readout: the player can hide it
    /// again and it stays hidden, turning the rule off leaves the preference
    /// as the player last put it, and the untimed rules never touch one.
    #[test]
    fn enabling_a_timed_rule_reveals_its_clock_and_only_on_that_edge() {
        let mut preferences = GamePreferences::default();
        let mut match_settings = MatchSettings::default();
        let match_time = GameToggle::Limit(MatchSettingField::MatchTime);
        let round_time = GameToggle::Limit(MatchSettingField::RoundTime);
        assert!(!preferences.show_match_timer);

        apply_game_toggle(&mut match_settings, &mut preferences, match_time);
        assert!(match_settings.enabled(MatchSettingField::MatchTime));
        assert!(preferences.show_match_timer);

        // Hidden again by the player. Nothing short of a fresh enable brings
        // it back - certainly not disabling the rule.
        toggle_preference(&mut preferences, PreferenceToggle::MatchTimer);
        assert!(!preferences.show_match_timer);
        apply_game_toggle(&mut match_settings, &mut preferences, match_time);
        assert!(!match_settings.enabled(MatchSettingField::MatchTime));
        assert!(!preferences.show_match_timer);
        apply_game_toggle(&mut match_settings, &mut preferences, match_time);
        assert!(preferences.show_match_timer);

        // Round Time reveals the round panel the same way, and turning the
        // rule off leaves a shown panel shown just as it leaves a hidden one
        // hidden.
        preferences.show_round_number = false;
        apply_game_toggle(&mut match_settings, &mut preferences, round_time);
        assert!(match_settings.enabled(MatchSettingField::RoundTime));
        assert!(preferences.show_round_number);
        apply_game_toggle(&mut match_settings, &mut preferences, round_time);
        assert!(!match_settings.enabled(MatchSettingField::RoundTime));
        assert!(preferences.show_round_number);

        // The untimed rules and Capture Points have no clock to reveal.
        let hidden = GamePreferences {
            show_match_timer: false,
            show_round_number: false,
            ..default()
        };
        preferences = hidden;
        for toggle in [
            GameToggle::Limit(MatchSettingField::RoundLimit),
            GameToggle::Limit(MatchSettingField::NoStoneRoundLimit),
            GameToggle::Limit(MatchSettingField::PointTarget),
            GameToggle::CapturePoints,
        ] {
            apply_game_toggle(&mut match_settings, &mut preferences, toggle);
            assert_eq!(preferences, hidden, "{toggle:?}");
        }
        assert!(match_settings.enabled(MatchSettingField::RoundLimit));
        assert!(match_settings.capture_points());
    }

    /// The reveal rides the same write as the rule it belongs to, and no
    /// later save re-reveals a readout the player has since hidden: only the
    /// toggle's enable edge writes the preference, never `persist_settings`.
    #[test]
    fn a_pressed_timed_rule_persists_its_revealed_clock_and_saves_never_re_reveal_it() {
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
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_mapping_buttons);
        let loaded = |saved: &Arc<Mutex<Option<String>>>| {
            let json = saved.lock().unwrap().clone().unwrap();
            ControlsConfig::from_json(&json).unwrap().0
        };

        let pressed = app
            .world_mut()
            .spawn(GameToggle::Limit(MatchSettingField::MatchTime))
            .id();
        trigger_activation(&mut app, pressed);
        app.update();
        assert!(app.world().resource::<GamePreferences>().show_match_timer);
        let config = loaded(&saved);
        assert!(config.match_settings.enabled(MatchSettingField::MatchTime));
        assert!(config.preferences.show_match_timer);

        // The player hides the timer again from the Preferences tab.
        let pressed = app.world_mut().spawn(PreferenceToggle::MatchTimer).id();
        trigger_activation(&mut app, pressed);
        app.update();
        assert!(!app.world().resource::<GamePreferences>().show_match_timer);
        assert!(!loaded(&saved).preferences.show_match_timer);

        // An unrelated Game-tab edit saves the whole document again, with
        // Match Time still enabled - and the timer stays hidden.
        let pressed = app.world_mut().spawn(GameToggle::CapturePoints).id();
        trigger_activation(&mut app, pressed);
        app.update();
        let config = loaded(&saved);
        assert!(config.match_settings.enabled(MatchSettingField::MatchTime));
        assert!(config.match_settings.capture_points());
        assert!(!config.preferences.show_match_timer);
        assert!(!app.world().resource::<GamePreferences>().show_match_timer);

        // Reduce Motion is a plain toggle: a press flips the resource and
        // the saved document carries it, and nothing else moves with it.
        let pressed = app.world_mut().spawn(PreferenceToggle::ReduceMotion).id();
        trigger_activation(&mut app, pressed);
        app.update();
        assert!(app.world().resource::<GamePreferences>().reduce_motion);
        let config = loaded(&saved);
        assert!(config.preferences.reduce_motion);
        assert!(!config.preferences.show_match_timer);
        assert!(config.match_settings.enabled(MatchSettingField::MatchTime));
    }

    #[test]
    fn reset_confirmation_copy_mentions_both_keyboard_and_controller() {
        assert_eq!(RESET_MAPPINGS_TITLE, "Reset All Controls?");
        assert!(RESET_MAPPINGS_BODY.to_lowercase().contains("keyboard"));
        assert!(RESET_MAPPINGS_BODY.to_lowercase().contains("controller"));
    }

    #[test]
    fn mapping_instruction_follows_the_selected_device_tab() {
        assert_eq!(
            mapping_instruction(MappingTab::Preferences),
            PREFERENCES_INSTRUCTION
        );
        assert_eq!(
            mapping_instruction(MappingTab::Game),
            "Choose optional match limits. Changes are saved automatically."
        );
        assert_eq!(
            mapping_instruction(MappingTab::Keyboard),
            "Choose a key binding, then press a key to change it."
        );
        assert_eq!(
            mapping_instruction(MappingTab::Controller),
            "Choose a binding. Then use a controller to change it."
        );
    }

    #[test]
    fn volume_stops_update_only_the_selected_channel_immediately() {
        let mut preferences = GamePreferences::default();
        assert!(set_volume_level(
            &mut preferences,
            VolumeStop {
                channel: AudioChannel::Game,
                level: VolumeLevel::High,
            }
        ));
        assert_eq!(preferences.game_sound_effects, VolumeLevel::High);
        assert_eq!(preferences.ui_sound_effects, VolumeLevel::Medium);
        assert!(!set_volume_level(
            &mut preferences,
            VolumeStop {
                channel: AudioChannel::Game,
                level: VolumeLevel::High,
            }
        ));
        assert!(set_volume_level(
            &mut preferences,
            VolumeStop {
                channel: AudioChannel::Ui,
                level: VolumeLevel::Mute,
            }
        ));
        assert_eq!(preferences.game_sound_effects, VolumeLevel::High);
        assert_eq!(preferences.ui_sound_effects, VolumeLevel::Mute);
    }

    /// The selected volume stop's fill, border and label colour are all
    /// hue, so its label is bracketed as well - `[Medium]` - and the
    /// brackets follow the level when it changes, on that channel's row
    /// alone.
    #[test]
    fn the_selected_volume_stop_is_bracketed_in_its_label() {
        use super::display::update_control_mapping_display;
        use super::layout::spawn_control_mapping_screen;

        let mut app = App::new();
        app.insert_resource(ControlMappingState::new(None))
            .insert_resource(GamePreferences::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .init_resource::<WindowSize>()
            .add_systems(Startup, spawn_control_mapping_screen)
            .add_systems(Update, update_control_mapping_display);
        app.update();

        let row = |app: &mut App, channel: AudioChannel| -> Vec<String> {
            let world = app.world_mut();
            let mut labels: Vec<(VolumeLevel, String)> = world
                .query::<(&VolumeStopLabel, &Text)>()
                .iter(world)
                .filter(|(label, _)| label.0.channel == channel)
                .map(|(label, text)| (label.0.level, text.0.clone()))
                .collect();
            labels.sort_by_key(|(level, _)| VolumeLevel::ALL.iter().position(|l| l == level));
            labels.into_iter().map(|(_, text)| text).collect()
        };
        for channel in [AudioChannel::Game, AudioChannel::Ui] {
            assert_eq!(row(&mut app, channel), ["Mute", "Low", "[Medium]", "High"]);
        }

        app.world_mut()
            .resource_mut::<GamePreferences>()
            .game_sound_effects = VolumeLevel::Mute;
        app.update();
        assert_eq!(
            row(&mut app, AudioChannel::Game),
            ["[Mute]", "Low", "Medium", "High"]
        );
        assert_eq!(
            row(&mut app, AudioChannel::Ui),
            ["Mute", "Low", "[Medium]", "High"]
        );
    }

    #[test]
    fn game_toggle_ecs_interaction_persists_the_full_document_immediately() {
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
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_mapping_buttons);
        let pressed = app
            .world_mut()
            .spawn(GameToggle::Limit(MatchSettingField::RoundLimit))
            .id();
        trigger_activation(&mut app, pressed);

        app.update();

        assert!(
            app.world()
                .resource::<MatchSettings>()
                .enabled(MatchSettingField::RoundLimit)
        );
        let json = saved.lock().unwrap().clone().unwrap();
        let (loaded, warning) = ControlsConfig::from_json(&json).unwrap();
        assert!(warning.is_none());
        assert!(loaded.match_settings.enabled(MatchSettingField::RoundLimit));
        assert_eq!(loaded.keyboard, KeyboardBindings::default());
        assert_eq!(loaded.preferences, GamePreferences::default());
    }

    /// The retry policy this module's doc comment on `persist_settings`
    /// describes: no dedicated retry mechanism exists because every
    /// subsequent accepted edit already retries the complete configuration.
    /// A later successful save must both clear the warning and persist the
    /// change that failed earlier alongside the new one - not just the new
    /// one alone.
    #[test]
    fn a_later_successful_save_clears_the_warning_and_persists_every_accumulated_change() {
        let fail = Arc::new(AtomicBool::new(true));
        let saved = Arc::new(Mutex::new(None));
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(SettingsStore::from_storage(FlakySettingsStorage {
                fail: Arc::clone(&fail),
                saved: Arc::clone(&saved),
            }))
            .insert_resource(ControllerAssignments::default())
            .insert_resource(ControlMappingState::new(None))
            .insert_resource(AudioCueQueue::default())
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_mapping_buttons);

        let pressed = app
            .world_mut()
            .spawn(GameToggle::Limit(MatchSettingField::RoundLimit))
            .id();
        trigger_activation(&mut app, pressed);
        app.update();
        assert_eq!(
            app.world().resource::<ControlMappingState>().error,
            SETTINGS_NOT_SAVED_WARNING
        );
        assert!(
            saved.lock().unwrap().is_none(),
            "a failed save persists nothing"
        );
        assert!(
            app.world()
                .resource::<MatchSettings>()
                .enabled(MatchSettingField::RoundLimit),
            "the in-memory change stays active despite the failed save"
        );

        fail.store(false, Ordering::Relaxed);
        let pressed = app.world_mut().spawn(GameToggle::CapturePoints).id();
        trigger_activation(&mut app, pressed);
        app.update();

        assert!(
            app.world()
                .resource::<ControlMappingState>()
                .error
                .is_empty(),
            "a successful save clears the warning"
        );
        let json = saved
            .lock()
            .unwrap()
            .clone()
            .expect("the retry must persist");
        let (loaded, warning) = ControlsConfig::from_json(&json).unwrap();
        assert!(warning.is_none());
        assert!(
            loaded.match_settings.enabled(MatchSettingField::RoundLimit),
            "the change made while saving was failing must still reach disk on retry"
        );
        assert!(loaded.match_settings.capture_points());
    }
}
