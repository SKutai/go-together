//! Application bootstrap: resource loading, plugin composition, the
//! [`AppScreen`] state machine and the explicit per-frame system ordering.
//!
//! # Responsibility
//!
//! This module is the map of the program. [`run`] loads persisted controls,
//! inserts the resources those controls produce and then composes the feature
//! plugins. [`AppPlugin`] adds Bevy's `DefaultPlugins`, registers [`AppScreen`]
//! and - most importantly - declares [`AppSystems`], the ordered list of stages
//! that the Home, Settings, device-input, Match and HUD systems schedule into.
//!
//! # Bevy lifecycle
//!
//! Screens are Bevy states. Entering a screen runs its `OnEnter` chain (reset
//! state, then spawn entities). Leaving one despawns every independently
//! spawned root tagged `DespawnOnExit(that screen)` - Bevy's own built-in
//! state-scoped despawn, wired automatically by `init_state::<AppScreen>()` -
//! including every linked descendant, recursively, with no per-screen system
//! to register or forget. A screen's `OnExit` system, if it has one at all,
//! exists only for what despawning does not cover: resetting a resource (Home's
//! marimba, the Replay Viewer's playback session). See [Screen, modal, and
//! persistent entity ownership](../../docs/ARCHITECTURE.md#screen-modal-and-persistent-entity-ownership)
//! for the full ownership contract, including why a screen's modal roots stay
//! spawned for their whole screen's lifetime rather than being despawned when
//! closed, why the Help modal - reachable from two screens - is
//! application-persistent instead, and [`despawn_screen_entities_now`] for
//! the one case - the Replay Library rebuilding its card list - that must
//! despawn before a screen exits.
//!
//! # Ordering
//!
//! `Update` work is a small graph, not one blanket pipeline: [`configure_app_systems`]
//! declares only the [`AppSystems`] edges that are real data or behavioral
//! dependencies (input fused into semantic actions before gameplay reads it;
//! every display refresh after the state it renders), documented on
//! [`AppSystems`] itself. Feature plugins do not order themselves against
//! another plugin's systems directly; they declare which [`AppSystems`] stage
//! they belong to. The two Replay screens are the one exception: each is a
//! single self-contained chain in no [`AppSystems`] stage, gated to its own
//! screen and ordered only after [`UiAudioSet`], because nothing outside the
//! screen reads what it writes in the same frame. Every entry point into the
//! graph - and both Replay chains - runs after [`UiAudioSet`] so a button's
//! hover cue is queued before the systems that may react to that button by
//! despawning it. Its activation cue needs no edge: it is queued earlier
//! still, the moment the button is activated (`crate::audio` says how).
//!
//! # Extending
//!
//! Add a screen by extending [`AppScreen`] and adding a plugin under
//! [`crate::ui`]; add a new frame stage by extending [`AppSystems`] at the
//! position it must run in.

pub(crate) mod motion;
pub(crate) mod startup_log;
pub(crate) mod visuals;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::ui_widgets::ButtonPlugin;
use bevy::window::WindowResolution;

use crate::audio::{GameAudioPlugin, UiAudioSet};
use crate::diagnostics::{self, Diagnostics};
use crate::gameplay::GameplayPlugin;
use crate::input::controller::{
    ControllerAssignments, ControllerCaptureState, ControllerHysteresisState,
    update_controller_assignments,
};
use crate::input::{SemanticInput, update_semantic_input};
use crate::platform::ReplayStorageWarning;
use crate::platform::{WindowPlatformConfig, asset_root, window_platform_config};
use crate::presentation::APP_BACKGROUND;
use crate::replay_library::ReplayLibrary;
use crate::settings::{ControlsConfig, SettingsStore, load_user_controls};
use crate::ui::UiPlugin;
use motion::{MotionPolicy, read_platform_motion_signal};
use startup_log::{DeferredStartupWarnings, emit_deferred_startup_warnings};
use visuals::{WINDOW_HEIGHT, WINDOW_WIDTH, WindowLayoutPlugin, setup_app};

/// Loads persisted settings, builds the Bevy app and blocks until the window
/// closes.
///
/// Controls are read *before* the app exists so the loaded bindings can be
/// inserted as resources and any load warning can be handed to the settings
/// screen, which is where the user can act on it. Reading that early is also
/// why the same warning cannot simply be logged where it happens; see
/// `app::startup_log`.
///
/// `ReplayLibrary::load_user` runs synchronously here, but it only reads
/// catalog metadata (`ReplayCatalogEntry`), not every complete Replay's event
/// timeline - see `crate::replay_library::catalog` and `crate::platform::native`'s
/// on-disk catalog cache. In the common case (an up-to-date cache) this is
/// one small cache read plus a directory listing and a `stat` per Replay,
/// with no Replay parsed, so a background task was judged unnecessary
/// complexity; the one case that still scans and parses every Replay file -
/// a missing, corrupt, or incompatible cache - is a one-time cost per
/// environment, self-healing on the next launch.
pub fn run() {
    let settings_store = SettingsStore::user();
    let (controls_config, binding_warning) = load_user_controls(&settings_store);
    let ControlsConfig {
        keyboard: bindings,
        controller: controller_bindings,
        preferences,
        match_settings,
    } = controls_config;
    let replay_library = ReplayLibrary::load_user();
    // Both halves of a startup failure, filled here because both failures
    // happen before any `App` exists. `Diagnostics` starts pre-loaded rather
    // than waiting on a system, because the generic player-facing sentence is
    // just data a screen may read. The technical detail cannot be handled
    // that way: writing it needs a `tracing` subscriber, which only
    // `LogPlugin::build` installs, so it is carried into the app and emitted
    // by `emit_deferred_startup_warnings` at `Startup` - see
    // `startup_diagnostics` and `startup_log`.
    let diagnostics = startup_diagnostics(binding_warning.as_deref());
    let deferred_warnings =
        deferred_startup_warnings(binding_warning.as_deref(), &replay_library.warnings);
    App::new()
        .insert_resource(ClearColor(APP_BACKGROUND))
        .insert_resource(bindings)
        .insert_resource(controller_bindings)
        .insert_resource(preferences)
        .insert_resource(match_settings)
        .insert_resource(settings_store)
        .insert_resource(ControllerAssignments::default())
        .insert_resource(ControllerHysteresisState::default())
        .insert_resource(SemanticInput::default())
        .insert_resource(ControllerCaptureState::default())
        .insert_resource(replay_library)
        .insert_resource(diagnostics)
        .insert_resource(deferred_warnings)
        .add_plugins(AppPlugin)
        .add_plugins(GameAudioPlugin)
        .add_plugins(GameplayPlugin)
        .add_plugins(UiPlugin::new(binding_warning))
        .run();
}

/// The player-safe notice [`startup_diagnostics`] pushes when
/// [`load_user_controls`] reports that some saved settings did not load.
///
/// Deliberately one sentence covering every reason that warning can appear -
/// an unreadable settings file, an unsupported version, a keyboard,
/// controller, preference, or Match-rules section that fell back to
/// defaults - because
/// [`Diagnostics`] is unscoped: any screen could surface this later, out of
/// the context that produced it. The specific cause is on the Settings
/// screen, which is where a player can act on it.
const SETTINGS_LOAD_WARNING: &str = "Some saved settings could not be loaded, so defaults are in use. The Settings screen has \
     the details.";

/// Builds the [`Diagnostics`] resource [`run`] starts the app with, holding
/// [`SETTINGS_LOAD_WARNING`] when `binding_warning` is `Some`.
///
/// `binding_warning` itself never reaches [`Diagnostics`]. It is the detailed
/// string - for a storage failure it carries the settings file path - and it
/// already has its own home: [`run`] hands the same value to
/// [`UiPlugin::new`], which seats it in the Settings screen's inline error
/// text. That display is unchanged by this; only the broadcast copy is
/// generic, exactly as [`crate::diagnostics::Diagnostic`] requires.
///
/// This pushes directly rather than through
/// [`report_recoverable`](crate::diagnostics::report_recoverable), which
/// every other call site uses, for one reason: that function's contract is to
/// do *both* halves of the boundary - log the technical detail and push the
/// safe message - and the logging half cannot happen here. `run` calls this
/// before `App::new()`, so Bevy's `LogPlugin` has not installed a `tracing`
/// subscriber yet and a `warn!` at this point is discarded rather than
/// written anywhere. Routing through `report_recoverable` would therefore
/// pass a technical string to a call that provably drops it, which reads as
/// logging that is not happening. The detail still reaches a log: it goes
/// into [`DeferredStartupWarnings`] alongside this, and
/// [`emit_deferred_startup_warnings`] writes it out at `Startup`, once a
/// subscriber exists.
fn startup_diagnostics(binding_warning: Option<&str>) -> Diagnostics {
    let mut diagnostics = Diagnostics::default();
    if binding_warning.is_some() {
        diagnostics.push(diagnostics::Diagnostic {
            severity: diagnostics::Severity::Warning,
            message: SETTINGS_LOAD_WARNING.into(),
        });
    }
    diagnostics
}

/// Collects the technical detail of everything that failed before the `App`
/// existed, for [`emit_deferred_startup_warnings`] to log at `Startup`.
///
/// The mirror image of [`startup_diagnostics`]: that one keeps the generic
/// sentence a player may be shown, this one keeps the strings that name a
/// settings file's path, an unreadable Replay, or an OS error. Nothing moves between them - the Settings screen's inline text and
/// the Replay Library's warning list still receive exactly what they did
/// before.
///
/// `AppPlugin::build` adds one more message to the result: resolving the
/// asset root is the third startup step that runs before `LogPlugin`, and it
/// is not reachable from here because it happens inside the plugin.
fn deferred_startup_warnings(
    binding_warning: Option<&str>,
    replay_warnings: &[ReplayStorageWarning],
) -> DeferredStartupWarnings {
    let mut deferred = DeferredStartupWarnings::default();
    if let Some(warning) = binding_warning {
        deferred.push(warning);
    }
    // `Display` is each warning's full technical message: this is the log,
    // which is exactly where that detail belongs.
    for warning in replay_warnings {
        deferred.push(warning.to_string());
    }
    deferred
}

/// The `Update` stages shared by every feature plugin.
///
/// Unlike an earlier version of this enum, the variants are *not* one blanket
/// chain: `AppPlugin::build` declares only the edges below, each a real data
/// or behavioral dependency, not "the frame order." Systems still keep their
/// own `run_if` screen conditions in addition to these set memberships.
///
/// * [`Self::HudInput`] -> [`Self::MatchUpdate`]: a modal a HUD button opens
///   this frame (which sets [`crate::gameplay::GameplayPause`]) must block
///   gameplay the same frame, not one frame late.
/// * [`Self::HudInput`] -> [`Self::HomeEasterEgg`]: the marimba's input
///   handler reads `HudState` directly to suppress notes while a modal is
///   open, so it needs this frame's modal state, not last frame's.
/// * [`Self::SettingsInput`] -> [`Self::DeviceInput`]: binding capture writes
///   `KeyboardBindings` and `ControllerBindings`, and semantic fusion reads
///   both, so fusion resolves this frame's binding rather than last frame's.
///   Capture cannot consume the physical press - it holds an immutable
///   `Res<ButtonInput<KeyCode>>` - so a newly bound key that is still held is
///   also reported as its semantic action that same frame. That is harmless
///   on the Settings screen, where the only
///   [`crate::input::SemanticInput`] reader not gated to Match is gameplay's
///   input-release gate.
/// * [`Self::SettingsInput`] -> [`Self::SettingsDisplay`]: display reflects
///   this frame's capture/tab changes.
/// * [`Self::DeviceInput`] -> [`Self::MatchUpdate`]: gameplay reads fused
///   semantic input, which must exist before it does.
/// * [`Self::MatchUpdate`] -> [`Self::MatchDisplay`]: the board/HUD render
///   this frame's resolved state.
/// * [`Self::MatchUpdate`] -> [`Self::HudDisplay`]: `update_shared_display`
///   reads `GameState` (for round-visibility text) as well as HUD and
///   binding state, so it needs this frame's Match state too.
/// * [`Self::ScreenLayout`] -> [`Self::MatchDisplay`] and
///   [`Self::ScreenLayout`] -> [`Self::HudDisplay`]: the Match screen's
///   layout pass writes where its nodes stand, and the display passes
///   write other parts of some of the same nodes - the HUD's which of four
///   panels are shown, the Match's what its texts say and which of its
///   hints and dot rows are drawn. Placing first means every display pass
///   sees its nodes already where this frame's window puts them, and the
///   layout pass does not rely on the display passes writing only the
///   fields it leaves alone. The display passes must still keep to their
///   own fields: one that replaced a whole `Node` would undo the placement
///   until the window's size next changed.
///
/// Every other pair is deliberately left unordered - see
/// [`configure_app_systems`]'s doc comment for why that is safe rather than
/// an oversight, and for the one pair that was not safe until it was made
/// so.
#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum AppSystems {
    /// HUD button presses: screen navigation and modal open/close.
    HudInput,
    /// The home-screen marimba Easter egg: note input and fade animation.
    HomeEasterEgg,
    /// Settings screen: tab/preference buttons and binding capture.
    SettingsInput,
    /// Controller assignment plus keyboard+gamepad fusion into semantic input.
    DeviceInput,
    /// Live match: clock, input-release gate, hold gestures, player actions.
    MatchUpdate,
    /// Where each screen's furniture stands for the window's size, and the
    /// main camera's framing of its world-space scene: the Match screen's
    /// layout pass and Home's, each gated to its own screen.
    ScreenLayout,
    /// HUD text and panel visibility shared by several screens.
    HudDisplay,
    /// Settings screen display refresh.
    SettingsDisplay,
    /// Match board, cursors, HUD text, the capture marks and their count
    /// lines, and the two bars below the board - hold progress while a
    /// gesture runs or a released end hold drains, and the round-time bar
    /// otherwise. Where things stand in the window is
    /// [`Self::ScreenLayout`]'s, which runs first; this stage still places
    /// what is drawn on the board, such as the cursors and the capture
    /// marks.
    MatchDisplay,
}

/// Window, rendering and scheduling setup that is not owned by any one feature.
struct AppPlugin;

impl Plugin for AppPlugin {
    fn build(&self, app: &mut App) {
        let platform_window = window_platform_config();
        // Evaluated here, which is still before `DefaultPlugins` - and so
        // before `LogPlugin` - is added, so this failure gets the same
        // treatment as `run`'s: carried, not logged where it happens. The
        // resource is normally already inserted by `run`; `init_resource`
        // keeps this plugin usable on its own without silently dropping the
        // message.
        let (asset_path, asset_root_warning) = asset_root();
        app.init_resource::<DeferredStartupWarnings>();
        if let Some(warning) = asset_root_warning {
            app.world_mut()
                .resource_mut::<DeferredStartupWarnings>()
                .push(warning);
        }
        app.add_plugins(engine_plugins(asset_path, platform_window))
            .init_state::<AppScreen>()
            .add_plugins(WindowLayoutPlugin)
            .init_resource::<MotionPolicy>();
        configure_app_systems(app);
        app.add_systems(
            Startup,
            (
                setup_app,
                emit_deferred_startup_warnings,
                read_platform_motion_signal,
            ),
        )
        // Re-read on every Match so an operating-system change made
        // between Matches is honoured; see `motion`'s module docs.
        .add_systems(OnEnter(AppScreen::Match), read_platform_motion_signal)
        // Device input has no feature plugin of its own: `input` is a pure
        // translation module, so its two systems are scheduled here, where the
        // rest of the frame order is declared.
        .add_systems(
            Update,
            (update_controller_assignments, update_semantic_input)
                .chain()
                .in_set(AppSystems::DeviceInput),
        );
    }
}

/// Bevy's `DefaultPlugins` as the game runs them: its asset root, its
/// window, and without `bevy_ui_widgets`' own `ButtonPlugin`.
///
/// The crate's buttons are pressed by `ui::button` and the focus ring
/// alone. Bevy's button behaviour would press them on a right click too,
/// and press the focused one again on every Enter or Space, including a
/// Space typed into the Replay Library's search field - see `ui::button`'s
/// module docs, and the canary there that pins why.
fn engine_plugins(asset_path: String, platform_window: WindowPlatformConfig) -> PluginGroupBuilder {
    DefaultPlugins
        .set(AssetPlugin {
            file_path: asset_path,
            ..default()
        })
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Go Together".into(),
                resolution: WindowResolution::new(WINDOW_WIDTH, WINDOW_HEIGHT),
                canvas: platform_window.canvas,
                fit_canvas_to_parent: platform_window.fit_canvas_to_parent,
                prevent_default_event_handling: platform_window.prevent_default_event_handling,
                resizable: platform_window.resizable,
                ..default()
            }),
            ..default()
        })
        .disable::<ButtonPlugin>()
}

/// Declares every real cross-set dependency in the `Update` schedule.
///
/// Factored out of [`AppPlugin::build`] so a test can exercise the exact
/// production scheduling graph - not a hand-rolled approximation of it - on
/// a minimal headless `App` that never touches `DefaultPlugins` or a window.
///
/// Each edge below is a real data or behavioral dependency, not a blanket
/// pipeline order - see [`AppSystems`]'s own doc comment for the reasoning
/// behind each one and `docs/ARCHITECTURE.md` for the full graph.
/// [`AppSystems::HudInput`], [`AppSystems::HomeEasterEgg`],
/// [`AppSystems::SettingsInput`], [`AppSystems::DeviceInput`],
/// [`AppSystems::MatchUpdate`], [`AppSystems::ScreenLayout`],
/// [`AppSystems::SettingsDisplay`], and [`AppSystems::MatchDisplay`] are
/// otherwise unordered relative to each other: several are gated to
/// mutually exclusive screens ([`AppSystems::HomeEasterEgg`] to Home;
/// [`AppSystems::SettingsInput`] and [`AppSystems::SettingsDisplay`] to
/// Control Mapping; [`AppSystems::MatchUpdate`] and
/// [`AppSystems::MatchDisplay`] to Match; each layout pass in
/// [`AppSystems::ScreenLayout`] to its own screen) and never do real work in
/// the same frame, so no edge between them would express anything true.
/// Within a screen, a layout pass shares nothing with that screen's update
/// or input systems. It reads [`visuals::WindowSize`], which furniture is
/// new, each placed node's placement and current value, and the round
/// text's `UiRole`, and writes only nodes' places, that text's
/// justification and the camera's [`visuals::WorldFraming`] - none of which
/// gameplay, or the marimba's input and animation, writes. Two systems are registered without a screen gate
/// and are harmless anyway: gameplay's input-release gate runs on every
/// screen but touches only `InputGate`, [`crate::input::SemanticInput`] and
/// `GameplayPause`, none of which any Settings system reads or writes, and
/// the marimba's input handler checks the screen itself and only drains
/// keyboard messages elsewhere. This crate does not enable Bevy's
/// system-ordering-ambiguity diagnostic, so an unordered pair here is a
/// deliberate "no dependency exists," not an oversight.
///
/// For [`AppSystems::HudDisplay`] that claim has to be argued rather than
/// read off a screen gate. Its one system, `ui::hud`'s
/// `update_shared_display`, runs on every screen, so it is live alongside
/// [`AppSystems::SettingsDisplay`] and [`AppSystems::MatchDisplay`] and
/// ordered against neither. (The Match screen's layout pass also writes
/// nodes it writes - the four HUD panels on that screen - which is why
/// [`AppSystems::ScreenLayout`] runs before it.) It shares two kinds of
/// entity with them.
/// Text carrying a `UiRole`, which it and the Match display both query: the
/// two divide the roles between them, each skipping the other's, so no text
/// has two writers. And buttons carrying a `HudAction`, whose fill it sets
/// every frame along with whether they click: the Match screen leaves those
/// to it, but the Settings tabs carry a `HudAction` *and* are restyled by
/// `update_settings_tab_buttons`, and the two disagree about the selected
/// tab. Until 2026-09-30 both wrote it, and which the player saw was whichever
/// ran last. The tabs are now marked `ScreenStyled`, which takes them out of
/// the HUD pass's query, so the pair is disjoint and needs no edge. A screen
/// that restyles a `HudAction` button of its own must mark it the same way -
/// `the_selected_tab_stays_inert_in_either_display_order` in `ui::settings`
/// is the test that runs both systems in both orders.
///
/// The claims above cover only `Update`, because that is the only schedule
/// this graph orders. Setting aside one-shots (`Startup` systems and the
/// screens' `OnEnter`/`OnExit` chains), the crate's per-frame work outside
/// `Update` is four things: `audio`'s playback chain in `PostUpdate`
/// (spawning the one-shot audio entities the `Update` systems queued); the
/// two resets in `StateTransition`, `ui::hud`'s of `HudState` and
/// [`visuals`]' of the camera's `WorldFraming`
/// (`reset_world_framing_on_screen_change`), each placed by
/// `StateTransitionSystems` between the transition being applied and the
/// exit and enter schedules, so a screen change closes every modal and
/// puts the camera back before either screen's chain runs; [`visuals`]'
/// window and camera pair, `track_window_size` in `PreUpdate`, so every
/// `Update` layout pass reads this frame's window, and `frame_main_camera`
/// in `PostUpdate`, before the camera's projection is recomputed and
/// transforms are propagated, so a framing written in `Update` is drawn
/// that frame; and `ui`'s button and focus modules. A
/// pointer presses a button inside `bevy_picking`'s own `PreUpdate`
/// systems, where its events are triggered, and `ui::button` activates it
/// on the release just after them, reading picking's hover map. Two of the
/// focus module's positions are forced by those - the ring's activation in
/// `PreUpdate` after the pointer's (so the frame's pointer activations come
/// first, and the Help modal's keys see both before deciding) - and by
/// `bevy_ui`'s layout: candidate collection in `PostUpdate` after
/// `UiSystems::Layout` (or it
/// reads last frame's `ComputedNode`). Either way an activation is a
/// message by `Update`, which is where every button handler reads it. The
/// back and help-modal key handlers ride the same `PreUpdate` anchor by
/// `.chain()` rather than needing it themselves, which has one visible
/// consequence worth knowing: a
/// `PreUpdate` system writes `NextState<AppScreen>`, so a confirmed Match
/// exit applies its transition at `StateTransition` a frame earlier than
/// the pointer path does. `ui::home`'s marimba skips that arrival frame
/// because of it. These
/// take their ordering from Bevy's schedule boundaries (and, for focus,
/// picking and `UiSystems::Layout`), not from [`AppSystems`], so nothing here needs
/// an edge for them: their pairwise ordering with everything in `Update` is
/// total by schedule position alone.
pub(crate) fn configure_app_systems(app: &mut App) {
    app.configure_sets(
        Update,
        (AppSystems::HudInput, AppSystems::MatchUpdate)
            .chain()
            .after(UiAudioSet),
    )
    .configure_sets(
        Update,
        (AppSystems::HudInput, AppSystems::HomeEasterEgg)
            .chain()
            .after(UiAudioSet),
    )
    .configure_sets(
        Update,
        (AppSystems::SettingsInput, AppSystems::DeviceInput)
            .chain()
            .after(UiAudioSet),
    )
    .configure_sets(
        Update,
        (AppSystems::SettingsInput, AppSystems::SettingsDisplay)
            .chain()
            .after(UiAudioSet),
    )
    .configure_sets(
        Update,
        (AppSystems::DeviceInput, AppSystems::MatchUpdate).chain(),
    )
    .configure_sets(
        Update,
        (AppSystems::MatchUpdate, AppSystems::MatchDisplay).chain(),
    )
    .configure_sets(
        Update,
        (AppSystems::MatchUpdate, AppSystems::HudDisplay).chain(),
    )
    .configure_sets(
        Update,
        AppSystems::ScreenLayout
            .before(AppSystems::MatchDisplay)
            .before(AppSystems::HudDisplay)
            .after(UiAudioSet),
    );
}

/// The screen the player is on.
///
/// Exactly one screen is active. Entering one runs its `OnEnter` chain;
/// leaving it despawns everything tagged `DespawnOnExit(that screen)` -
/// Bevy's own built-in state-scoped despawn - so screens cannot leak entities
/// into each other. `#[states(scoped_entities = false)]` is deliberately
/// absent: every screen relies on `DespawnOnExit` being wired automatically
/// by `init_state::<AppScreen>()` in [`AppPlugin::build`].
#[derive(States, Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum AppScreen {
    #[default]
    Home,
    ControlMapping,
    Match,
    ReplayLibrary,
    ReplayViewer,
}

impl AppScreen {
    /// Every screen, for tests that must hold a claim on all of them at
    /// once. Test-only because no running system enumerates screens - each
    /// gates itself with `in_state`. `every_screen_is_listed_once` fails to
    /// compile, then to pass, when a variant is added without a row here.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 5] = [
        Self::Home,
        Self::ControlMapping,
        Self::Match,
        Self::ReplayLibrary,
        Self::ReplayViewer,
    ];
}

/// Despawns every entity tagged `DespawnOnExit(screen)`, right now, without
/// waiting for an actual `AppScreen` transition.
///
/// Every screen's teardown on a genuine state change is automatic - see the
/// module docs - so the only remaining caller is the Replay Library rebuilding
/// its card list in place after a search, sort, rename, or delete, which does
/// not change `AppScreen` and therefore never fires the built-in despawn.
pub(crate) fn despawn_screen_entities_now(
    commands: &mut Commands,
    entities: &Query<(Entity, &DespawnOnExit<AppScreen>)>,
    screen: AppScreen,
) {
    for (entity, marker) in entities {
        if marker.0 == screen {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CapturedLog, capture_log_events};
    use crate::ui::button::{UiButtonPlugin, trigger_activation};
    use bevy::log::Level;

    /// The game's `DefaultPlugins` leave out exactly Bevy's own button
    /// behaviour: `ButtonPlugin` is in the group and disabled, while its
    /// sibling widget plugins and the input-focus dispatch that a screen
    /// reader will need stay enabled. The canary in `ui::button` pins why.
    #[test]
    fn the_engine_plugins_leave_out_bevys_button_plugin_alone() {
        use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
        use bevy::ui_widgets::{CheckboxPlugin, SliderPlugin};

        let plugins = engine_plugins("assets".into(), window_platform_config());
        assert!(plugins.contains::<ButtonPlugin>(), "it is in the group");
        assert!(!plugins.enabled::<ButtonPlugin>(), "and it is disabled");
        assert!(plugins.enabled::<CheckboxPlugin>());
        assert!(plugins.enabled::<SliderPlugin>());
        assert!(plugins.enabled::<InputFocusPlugin>());
        assert!(plugins.enabled::<InputDispatchPlugin>());
    }

    /// [`AppScreen::ALL`] is a mirror of the enum with no compiler-visible
    /// seam, so it gets the two-layer guard `docs/ARCHITECTURE.md` describes
    /// for `ALLOWED_EDGES`: the exhaustive `match` fails to compile until a
    /// new variant is given an index, and the length check then fails until
    /// the array gains its row.
    #[test]
    fn every_screen_is_listed_once() {
        const fn index(screen: AppScreen) -> usize {
            match screen {
                AppScreen::Home => 0,
                AppScreen::ControlMapping => 1,
                AppScreen::Match => 2,
                AppScreen::ReplayLibrary => 3,
                AppScreen::ReplayViewer => 4,
            }
        }
        assert_eq!(AppScreen::ALL.len(), 5);
        for (position, screen) in AppScreen::ALL.into_iter().enumerate() {
            assert_eq!(index(screen), position, "{screen:?}");
        }
    }

    /// [`despawn_screen_entities_now`] is a manual, immediate despawn used
    /// outside a real state transition - it has to despawn exactly the
    /// targeted screen's entities and leave every other screen's alone.
    #[test]
    fn despawn_screen_entities_now_despawns_only_the_targeted_screen() {
        let mut app = App::new();
        let home = app.world_mut().spawn(DespawnOnExit(AppScreen::Home)).id();
        let library = app
            .world_mut()
            .spawn(DespawnOnExit(AppScreen::ReplayLibrary))
            .id();
        let untagged = app.world_mut().spawn_empty().id();
        app.add_systems(
            Update,
            |mut commands: Commands, entities: Query<(Entity, &DespawnOnExit<AppScreen>)>| {
                despawn_screen_entities_now(&mut commands, &entities, AppScreen::Home);
            },
        );

        app.update();

        assert!(!app.world().entities().contains(home));
        assert!(app.world().entities().contains(library));
        assert!(app.world().entities().contains(untagged));
    }

    /// The built-in mechanism the whole crate now relies on: leaving a screen
    /// must despawn every `DespawnOnExit(that screen)` root and its
    /// descendants, on a real `AppScreen` transition, with no per-screen
    /// system registered for despawning at all.
    #[test]
    fn despawn_on_exit_recursively_despawns_a_screens_roots_and_descendants() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>();

        let root = app.world_mut().spawn(DespawnOnExit(AppScreen::Home)).id();
        let child = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(root).add_child(child);
        let other_screen_root = app.world_mut().spawn(DespawnOnExit(AppScreen::Match)).id();
        let persistent = app.world_mut().spawn_empty().id();
        app.update();

        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Match);
        app.update();

        assert!(!app.world().entities().contains(root));
        assert!(
            !app.world().entities().contains(child),
            "a descendant must be despawned with its screen root"
        );
        assert!(app.world().entities().contains(other_screen_root));
        assert!(app.world().entities().contains(persistent));
    }

    /// Every screen ultimately relies on this same generic mechanism, so a
    /// regression here would silently reappear as entity growth on every
    /// screen at once: repeatedly entering and leaving a screen must always
    /// settle back to exactly the entities that screen spawns once, never
    /// accumulating a copy per visit.
    #[test]
    fn repeated_visits_never_accumulate_a_screens_entities() {
        fn spawn_two_tagged(mut commands: Commands) {
            commands.spawn(DespawnOnExit(AppScreen::Match));
            commands.spawn(DespawnOnExit(AppScreen::Match));
        }

        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_systems(OnEnter(AppScreen::Match), spawn_two_tagged);
        app.update();

        let count = |app: &mut App| {
            app.world_mut()
                .query::<&DespawnOnExit<AppScreen>>()
                .iter(app.world())
                .count()
        };

        for _ in 0..5 {
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(AppScreen::Match);
            app.update();
            assert_eq!(
                count(&mut app),
                2,
                "a visit must not leak the previous one's entities"
            );

            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(AppScreen::Home);
            app.update();
            assert_eq!(
                count(&mut app),
                0,
                "leaving must despawn everything spawned on entry"
            );
        }
    }

    /// The one genuinely cross-cutting edge in the whole graph: a modal a
    /// HUD button opens must block the authoritative Match clock the exact
    /// same frame, not one frame late. Exercises the real
    /// `handle_ui_buttons`/`sync_gameplay_pause`/`advance_match_clock`
    /// systems under the real [`configure_app_systems`] graph, not a
    /// hand-rolled approximation of the ordering.
    #[test]
    fn opening_a_modal_blocks_the_match_clock_the_same_frame() {
        use crate::gameplay::countdown::{advance_match_clock, begin_active_match};
        use crate::gameplay::{GameState, GameplayPause, InputGate, MatchRules};
        use crate::ui::hud::{HudAction, HudState, handle_ui_buttons, sync_gameplay_pause};

        let mut game = GameState::with_match_rules(MatchRules::snapshot(
            &crate::match_settings::MatchSettings::default(),
        ));
        begin_active_match(&mut game);
        let elapsed_before = game.elapsed;

        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(Time::<()>::default())
            .insert_resource(game)
            .insert_resource(GameplayPause::default())
            .insert_resource(InputGate::default())
            .insert_resource(HudState::default())
            .insert_resource(crate::audio::AudioCueQueue::default())
            .add_systems(
                Update,
                (
                    (handle_ui_buttons, sync_gameplay_pause)
                        .chain()
                        .in_set(AppSystems::HudInput),
                    advance_match_clock.in_set(AppSystems::MatchUpdate),
                ),
            );
        configure_app_systems(&mut app);
        // A HUD button opening the Help modal, pressed this same frame.
        let pressed = app.world_mut().spawn(HudAction::OpenHelp).id();
        trigger_activation(&mut app, pressed);

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();

        assert!(app.world().resource::<HudState>().help_open);
        assert!(app.world().resource::<GameplayPause>().match_paused);
        assert_eq!(
            app.world().resource::<GameState>().elapsed,
            elapsed_before,
            "the clock must not advance in the same frame a modal opens"
        );
    }

    /// `SettingsInput` and `MatchUpdate` are deliberately left unordered
    /// against each other in [`configure_app_systems`] - the doc comment on
    /// [`AppSystems`] argues this is safe because the two sets are always
    /// screen-exclusive, never running in the same frame. This test proves
    /// that directly: a system in `SettingsInput`, gated to a screen that is
    /// not active, must not run at all while `MatchUpdate` is - so no
    /// ordering edge between the two sets could ever matter in practice.
    #[test]
    fn a_system_on_an_inactive_screen_never_runs_and_cannot_affect_the_active_screen() {
        #[derive(Resource, Default)]
        struct SettingsInputRanCount(u32);

        fn poison_if_it_ever_runs(mut count: ResMut<SettingsInputRanCount>) {
            count.0 += 1;
        }

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(SettingsInputRanCount::default())
            .add_systems(
                Update,
                poison_if_it_ever_runs
                    .run_if(in_state(AppScreen::ControlMapping))
                    .in_set(AppSystems::SettingsInput),
            );
        configure_app_systems(&mut app);

        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Match);
        app.update();
        app.update();

        assert_eq!(
            app.world().resource::<SettingsInputRanCount>().0,
            0,
            "a SettingsInput system gated to ControlMapping must not run while Match is active"
        );
    }

    /// The shape `platform::native` produces: a full path plus a
    /// `std::io::Error`. A mock with clean text would let the tests below
    /// pass for the wrong reason.
    const LEAKED_SETTINGS_ERROR: &str = "Could not read settings from /Users/name/Library/Application Support/go-together/controls.json: Permission denied (os error 13)";

    struct PathLeakingSettingsStorage;

    impl crate::platform::SettingsStorage for PathLeakingSettingsStorage {
        fn load(&self) -> Result<Option<String>, crate::platform::SettingsStorageError> {
            Err(crate::platform::SettingsStorageError::Failed(
                LEAKED_SETTINGS_ERROR.into(),
            ))
        }

        fn save(&self, _json: &str) -> Result<(), crate::platform::SettingsStorageError> {
            Ok(())
        }
    }

    /// The real startup failure both halves of the boundary are built from:
    /// `load_user_controls` against a backend that fails the way
    /// `platform::native` actually does.
    fn failed_settings_load_warning() -> String {
        let store = SettingsStore::from_storage(PathLeakingSettingsStorage);
        let (_, binding_warning) = load_user_controls(&store);
        binding_warning.expect("a failed load must produce a warning")
    }

    /// A settings-load failure at startup is the one `Diagnostic` pushed
    /// outside `report_recoverable`, so nothing else keeps its message
    /// honest. This pins that the broadcast copy is the generic sentence, not
    /// the detailed string the Settings screen receives.
    #[test]
    fn a_failed_settings_load_pre_loads_a_player_safe_startup_diagnostic() {
        let binding_warning = failed_settings_load_warning();

        let diagnostics = startup_diagnostics(Some(&binding_warning));

        assert_eq!(
            diagnostics.active(),
            [diagnostics::Diagnostic {
                severity: diagnostics::Severity::Warning,
                message: SETTINGS_LOAD_WARNING.into(),
            }]
        );
        diagnostics::assert_carries_no_internal_detail(&diagnostics.active()[0].message);
        assert!(
            diagnostics::carries_internal_detail(&binding_warning),
            "the string the Settings screen still receives is what must not reach the broadcast log"
        );
    }

    /// A clean load leaves the startup log empty - the notice is tied to an
    /// actual failure, not pushed unconditionally.
    #[test]
    fn a_clean_settings_load_pre_loads_no_diagnostic() {
        assert!(startup_diagnostics(None).active().is_empty());
    }

    /// The counterpart to the `Diagnostic` test: the detail that notice
    /// deliberately drops must not be lost altogether. This drives the real
    /// `Startup` system on the real resource `run` builds and asserts the
    /// `tracing` event it emits - that the line is written, not merely that
    /// the code path producing it ran.
    #[test]
    fn a_failed_settings_loads_detail_is_logged_by_the_startup_system() {
        let binding_warning = failed_settings_load_warning();

        let mut app = App::new();
        app.insert_resource(deferred_startup_warnings(Some(&binding_warning), &[]))
            .add_systems(Startup, emit_deferred_startup_warnings);
        let captured = capture_log_events(&mut app, 1);

        assert_eq!(
            captured,
            [CapturedLog {
                level: Level::WARN,
                message: binding_warning.clone(),
            }]
        );
        assert!(
            diagnostics::carries_internal_detail(&binding_warning),
            "the log line is where the path and OS error are supposed to survive"
        );
    }

    /// Every startup step that runs before `LogPlugin` feeds the same
    /// resource, in the order the failures happened.
    #[test]
    fn each_pre_subscriber_failure_is_collected_for_the_startup_log() {
        let deferred = deferred_startup_warnings(
            Some("Could not load settings: ... Using defaults."),
            &[ReplayStorageWarning::SkippedReplay(
                "Could not read Replay abc.json: unexpected end of input".into(),
            )],
        );

        assert_eq!(
            deferred.messages(),
            [
                "Could not load settings: ... Using defaults.",
                "Could not read Replay abc.json: unexpected end of input",
            ]
        );
    }

    /// A startup with nothing to report logs nothing at all.
    #[test]
    fn a_clean_startup_collects_no_deferred_warnings() {
        assert!(deferred_startup_warnings(None, &[]).messages().is_empty());
    }
}
