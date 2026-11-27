//! The replay viewer: reconstructs a saved match and lets the player scrub
//! through it.
//!
//! # Reconstruction, not simulation
//!
//! The viewer never re-runs the rules. [`PlaybackController`] holds the
//! timeline position, `Replay::playback_frame_at` resolves that position to a
//! recorded [`ReplayFrame`](crate::replay::ReplayFrame), and the screen simply
//! draws that frame: stones, both cursors, teleport markers, hidden selections
//! and the score badges.
//!
//! # Seeking is silent
//!
//! Recorded sound events are only played while the timeline advances normally.
//! Jumping, scrubbing or restarting deliberately emits nothing, so seeking
//! cannot produce a burst of stale cues.
//!
//! # Why this screen has four submodules
//!
//! The Match and Replay Library screens each divide into a `mod.rs` and a
//! `layout.rs`, and `ui/settings` is cut into six files - two of them owning
//! tabs (`bindings` the Keyboard and Controller pair, `game` its own), the
//! rest cutting across them. The viewer
//! does not fit in two files either, and the extra cuts fall where the
//! dependencies already do:
//!
//! * [`geometry`] - the fixed window budget, the sizes of the two
//!   Replay-only marks, and the pure functions that place a stone, a hidden
//!   selection or a score badge. Spawning and per-frame drawing both depend
//!   on it and it depends on neither, so it cannot live in either.
//! * [`layout`] - spawns the board, the cursors, the markers, the hidden
//!   selections, the badges and the transport bar, once, on entry.
//! * [`modals`] - the Key and Match Settings overlays, nearly half of the
//!   spawn code between them and involved in nothing else.
//! * [`display`] - the per-frame pass that restyles those entities from the
//!   current frame.
//!
//! What stays here is the plugin, [`ReplayViewerState`], the [`ViewerAction`]
//! a control carries, the component markers the other modules spawn and query,
//! and the transport systems that turn input into playback position.

mod display;
mod geometry;
mod layout;
mod modals;

use bevy::prelude::*;

use super::REPLAY_FIELD_SEPARATOR;
use crate::app::AppScreen;
use crate::audio::{AudioCueQueue, UiAudioSet, UiCue};
use crate::replay::{PlaybackController, Replay};
use crate::rules::{Player, Point, Score};
use crate::ui::button::{Activated, UiButtonPlugin};
use display::update_viewer_display;
use layout::spawn_viewer_screen;
/// Registers the viewer screen's lifecycle and its ordered update pipeline.
///
/// Input is applied before the clock advances and before the board is drawn, so
/// a seek requested this frame is reflected immediately and cannot briefly
/// render the previous timeline position.
pub(super) struct ReplayViewerPlugin;

impl Plugin for ReplayViewerPlugin {
    fn build(&self, app: &mut App) {
        UiButtonPlugin::require(app);
        app.init_resource::<ReplayViewerState>()
            .add_systems(
                OnEnter(AppScreen::ReplayViewer),
                (prepare_viewer_screen, spawn_viewer_screen).chain(),
            )
            .add_systems(
                OnExit(AppScreen::ReplayViewer),
                reset_viewer_session_on_exit,
            )
            .add_systems(
                Update,
                (
                    handle_viewer_keyboard,
                    handle_viewer_buttons,
                    advance_viewer,
                    update_viewer_display,
                )
                    .chain()
                    .after(UiAudioSet)
                    .run_if(in_state(AppScreen::ReplayViewer)),
            );
    }
}

#[derive(Resource, Default)]
/// Which replay is open, its complete loaded content, and where playback is.
///
/// `replay` holds the one fully loaded, validated `Replay` the Library
/// handed over when the player opened it - the Viewer never asks
/// `ReplayLibrary` to load a Replay itself, and never touches any other
/// Replay's content. Cleared when the screen is left, so re-opening a replay
/// always starts from the beginning, paused, with both modals closed, and so
/// a complete Replay's event timeline is never retained once the Viewer is
/// not showing it.
pub(super) struct ReplayViewerState {
    pub(super) replay_id: Option<String>,
    pub(super) replay: Option<Replay>,
    pub(super) controller: PlaybackController,
    pub(super) key_open: bool,
    pub(super) match_settings_open: bool,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
/// A transport control. `Speed` carries its multiplier so one component type
/// covers all three speed buttons.
pub(super) enum ViewerAction {
    BackToReplays,
    MatchSettings,
    CloseMatchSettings,
    Key,
    CloseKey,
    PlayPause,
    Rewind,
    JumpBack,
    JumpForward,
    FastForward,
    Speed(f64),
}

impl ViewerAction {
    pub(super) const fn sound(self) -> UiCue {
        match self {
            Self::BackToReplays
            | Self::CloseMatchSettings
            | Self::CloseKey
            | Self::Rewind
            | Self::JumpBack => UiCue::Cancel,
            Self::MatchSettings | Self::Key | Self::Speed(_) => UiCue::Neutral,
            Self::PlayPause | Self::JumpForward | Self::FastForward => UiCue::Confirm,
        }
    }
}

#[derive(Component)]
struct ViewerIntersection(Point);

/// The ring disc behind one intersection, shown only while a white or gray
/// stone stands there; see `stone_geometry` in `app::visuals`.
#[derive(Component)]
struct ViewerStoneRing(Point);

#[derive(Component)]
struct ViewerCursor(Player);

#[derive(Component)]
struct ViewerScoreBadge(Player);

#[derive(Component)]
struct ViewerMarker(Player);

/// One hidden-selection mark. Every slot has two, of which at most one is
/// shown: the ordinary mark, and the smaller `shared` one drawn in its place
/// when the other player selected the same point. See
/// `viewer_selection_position` in [`geometry`].
#[derive(Component)]
struct ViewerSelection {
    player: Player,
    slot: usize,
    shared: bool,
}

#[derive(Component)]
struct ViewerKeyModal;

#[derive(Component)]
struct ViewerMatchSettingsModal;

#[derive(Component)]
struct ViewerCountdown;

#[derive(Component)]
struct ViewerButtonLabel;

#[derive(Component)]
enum ViewerText {
    Label,
    Timer,
    Round,
    Result,
    Countdown,
    PlayPause,
}

fn prepare_viewer_screen(mut viewer: ResMut<ReplayViewerState>) {
    prepare_viewer_session(&mut viewer);
}

fn prepare_viewer_session(viewer: &mut ReplayViewerState) {
    viewer.controller = PlaybackController::default();
    viewer.key_open = false;
    viewer.match_settings_open = false;
}

/// Entity teardown is automatic (`DespawnOnExit`); this only resets the
/// playback session resource despawning cannot touch.
fn reset_viewer_session_on_exit(mut viewer: ResMut<ReplayViewerState>) {
    clear_viewer_session(&mut viewer);
}

fn clear_viewer_session(viewer: &mut ReplayViewerState) {
    *viewer = ReplayViewerState::default();
}

fn format_setting_duration(seconds: u16) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn player_score(score: Score, player: Player) -> usize {
    match player {
        Player::One => score.black,
        Player::Two => score.white,
    }
}

const fn player_index(player: Player) -> usize {
    match player {
        Player::One => 0,
        Player::Two => 1,
    }
}

/// Applies this frame's activations of the transport and header buttons.
///
/// The Replay is borrowed out of the resource alongside the fields an action
/// mutates - the same split borrow [`advance_viewer`] uses - rather than
/// cloned, because cloning here would copy the whole event timeline, boards
/// included, on every activation.
fn handle_viewer_buttons(
    mut activated: Activated<ViewerAction>,
    mut viewer: ResMut<ReplayViewerState>,
    mut next_screen: ResMut<NextState<AppScreen>>,
) {
    for action in activated.read() {
        let ReplayViewerState {
            replay,
            controller,
            key_open,
            match_settings_open,
            ..
        } = &mut *viewer;
        // `continue` rather than `return`, so the reader is still drained.
        let Some(replay) = replay.as_ref() else {
            continue;
        };
        let screen = apply_action_to_controls(
            ViewerControls {
                controller,
                key_open,
                match_settings_open,
            },
            *action,
            replay,
        );
        if let Some(screen) = screen {
            clear_viewer_session(&mut viewer);
            next_screen.set(screen);
        }
    }
}

/// Escape, or a pad's East or Start button, steps back one layer: it closes
/// an open Key or Match Settings panel first, and otherwise returns to the
/// Replay Library - exactly where the header's own Back button goes,
/// session cleared the same way, so this screen still has no route to
/// Home. Start is the pad's back on every screen; East is safe as back here
/// too because this is a menu screen: no gameplay or capture system reads a
/// pad on it.
fn handle_viewer_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut viewer: ResMut<ReplayViewerState>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut audio: ResMut<AudioCueQueue>,
) {
    let back = keys.just_pressed(KeyCode::Escape)
        || gamepads.iter().any(|gamepad| {
            gamepad.just_pressed(GamepadButton::East) || gamepad.just_pressed(GamepadButton::Start)
        });
    if !back {
        return;
    }
    if !close_open_viewer_modal(&mut viewer) {
        clear_viewer_session(&mut viewer);
        next_screen.set(AppScreen::ReplayLibrary);
    }
    audio.ui(UiCue::Cancel);
}

fn close_open_viewer_modal(viewer: &mut ReplayViewerState) -> bool {
    let was_open = viewer.key_open || viewer.match_settings_open;
    viewer.key_open = false;
    viewer.match_settings_open = false;
    was_open
}

/// The parts of [`ReplayViewerState`] a viewer action can change, borrowed
/// apart from `replay` so a caller can hand [`apply_action_to_controls`] a
/// `&Replay` taken from that very same resource.
struct ViewerControls<'a> {
    controller: &'a mut PlaybackController,
    key_open: &'a mut bool,
    match_settings_open: &'a mut bool,
}

/// Drives one action against a whole [`ReplayViewerState`], mirroring what
/// [`handle_viewer_buttons`] does per press: apply it, and clear the session
/// if it routes away. Only tests hold the resource unsplit like this; the
/// system itself must keep `replay` borrowed apart from the rest.
#[cfg(test)]
fn apply_viewer_action(
    viewer: &mut ReplayViewerState,
    action: ViewerAction,
    replay: &Replay,
) -> Option<AppScreen> {
    let ReplayViewerState {
        controller,
        key_open,
        match_settings_open,
        ..
    } = &mut *viewer;
    let screen = apply_action_to_controls(
        ViewerControls {
            controller,
            key_open,
            match_settings_open,
        },
        action,
        replay,
    );
    if screen.is_some() {
        clear_viewer_session(viewer);
    }
    screen
}

/// Returns the screen to route to, if the action leaves the Viewer. Clearing
/// the session is the caller's job, because the session owns the `Replay` this
/// borrow deliberately does not.
fn apply_action_to_controls(
    controls: ViewerControls<'_>,
    action: ViewerAction,
    replay: &Replay,
) -> Option<AppScreen> {
    let ViewerControls {
        controller,
        key_open,
        match_settings_open,
    } = controls;
    if *key_open && action != ViewerAction::CloseKey {
        return None;
    }
    if *match_settings_open && action != ViewerAction::CloseMatchSettings {
        return None;
    }
    if action_disabled(action, controller, *key_open, *match_settings_open, replay) {
        return None;
    }
    match action {
        ViewerAction::BackToReplays => return Some(AppScreen::ReplayLibrary),
        ViewerAction::MatchSettings => {
            controller.pause();
            *key_open = false;
            *match_settings_open = true;
        }
        ViewerAction::CloseMatchSettings => *match_settings_open = false,
        ViewerAction::Key => {
            controller.pause();
            *match_settings_open = false;
            *key_open = true;
        }
        ViewerAction::CloseKey => *key_open = false,
        ViewerAction::PlayPause => controller.toggle(replay),
        ViewerAction::Rewind => controller.rewind(),
        ViewerAction::JumpBack => controller.jump_back(replay),
        ViewerAction::JumpForward => controller.jump_forward(replay),
        ViewerAction::FastForward => controller.fast_forward(replay),
        ViewerAction::Speed(speed) => controller.set_speed(speed),
    }
    None
}

fn viewer_action_disabled(
    action: ViewerAction,
    viewer: &ReplayViewerState,
    replay: &Replay,
) -> bool {
    action_disabled(
        action,
        &viewer.controller,
        viewer.key_open,
        viewer.match_settings_open,
        replay,
    )
}

fn action_disabled(
    action: ViewerAction,
    controller: &PlaybackController,
    key_open: bool,
    match_settings_open: bool,
    replay: &Replay,
) -> bool {
    if key_open {
        return action != ViewerAction::CloseKey;
    }
    if match_settings_open {
        return action != ViewerAction::CloseMatchSettings;
    }
    match action {
        ViewerAction::Rewind | ViewerAction::JumpBack => controller.at_start(),
        ViewerAction::PlayPause if !controller.playing => controller.at_end(replay),
        ViewerAction::JumpForward | ViewerAction::FastForward => controller.at_end(replay),
        ViewerAction::BackToReplays
        | ViewerAction::MatchSettings
        | ViewerAction::CloseMatchSettings
        | ViewerAction::Key
        | ViewerAction::CloseKey
        | ViewerAction::PlayPause
        | ViewerAction::Speed(_) => false,
    }
}

fn advance_viewer(
    time: Res<Time>,
    mut viewer: ResMut<ReplayViewerState>,
    mut audio: ResMut<AudioCueQueue>,
) {
    if viewer.key_open || viewer.match_settings_open {
        return;
    }
    let ReplayViewerState {
        replay, controller, ..
    } = &mut *viewer;
    let Some(replay) = replay else {
        return;
    };
    for cue in controller.advance_with_cues(time.delta_secs_f64(), replay) {
        audio.game(cue);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fixed_metadata_replay;
    use super::*;
    use crate::presentation::BUTTON_DISABLED_BORDER;
    use crate::presentation::{Highlight, button_background};

    /// Escape (or a pad's East or Start button) steps back one layer: an
    /// open panel closes first, and the next press leaves for the Library
    /// with the session cleared, exactly as the header's Back button does -
    /// never straight to Home.
    #[test]
    fn escape_or_east_or_start_closes_a_panel_then_returns_to_the_library() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(ReplayViewerState {
                replay_id: Some("id".into()),
                replay: Some(fixed_metadata_replay()),
                key_open: true,
                ..default()
            })
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, handle_viewer_keyboard);
        let press_escape = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Escape);
            keys.clear();
            keys.press(KeyCode::Escape);
        };

        press_escape(&mut app);
        app.update();
        let viewer = app.world().resource::<ReplayViewerState>();
        assert!(!viewer.key_open, "the first back closes the open panel");
        assert!(viewer.replay.is_some(), "and only the panel");
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));

        press_escape(&mut app);
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::ReplayLibrary)
        ));
        let viewer = app.world().resource::<ReplayViewerState>();
        assert!(
            viewer.replay.is_none() && viewer.replay_id.is_none(),
            "leaving clears the session the way the Back button does"
        );

        // The pad's East button is the same back.
        *app.world_mut().resource_mut::<NextState<AppScreen>>() = NextState::Unchanged;
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Escape);
            keys.clear();
        }
        app.world_mut()
            .resource_mut::<ReplayViewerState>()
            .match_settings_open = true;
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::East);
        let pad = app.world_mut().spawn(gamepad).id();
        app.update();
        assert!(
            !app.world()
                .resource::<ReplayViewerState>()
                .match_settings_open
        );
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));

        // And so is Start, the pad's back on every screen.
        app.world_mut().resource_mut::<ReplayViewerState>().key_open = true;
        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::East);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::Start);
        }
        app.update();
        assert!(!app.world().resource::<ReplayViewerState>().key_open);
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));
    }

    /// The whole Viewer over `app`'s layout - one of the real-layout apps in
    /// `crate::ui::focus` - with the production `Update` chain, opened on
    /// `replay` at its start. Spawned on the first update.
    fn viewer_screen_app(mut app: App, replay: Replay) -> App {
        use crate::app::visuals::StoneMesh;
        use crate::gameplay::InputGate;
        use crate::ui::focus::UiFocusPlugin;
        use crate::ui::hud::HudState;

        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(AppScreen::ReplayViewer)
            .add_plugins(UiFocusPlugin)
            .init_resource::<Assets<ColorMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(HudState::default())
            .insert_resource(InputGate::default())
            .insert_resource(AudioCueQueue::default())
            .insert_resource(ReplayViewerState {
                replay_id: Some(replay.id.clone()),
                replay: Some(replay),
                ..default()
            })
            .add_systems(
                Update,
                (
                    handle_viewer_keyboard,
                    handle_viewer_buttons,
                    advance_viewer,
                    update_viewer_display,
                )
                    .chain(),
            );
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
            .add_systems(Startup, spawn_viewer_screen);
        app
    }

    /// The Round 0 countdown, laid out with its text by `bevy_ui` itself
    /// and read back: the digit is drawn in the middle of its card, and the
    /// card stands over the same point of this board as the Match screen's
    /// over the live one. Until 2026-10-05 the card was itself the `Text`
    /// node, and its `3` sat in the card's top-left corner while the card
    /// was exactly where it was meant to be - `crate::ui::countdown_card`
    /// says why. A test of node rects alone cannot see that, which is why
    /// the digit is measured by its glyphs.
    #[test]
    fn the_countdown_digit_is_drawn_centred_on_a_card_over_the_board() {
        use super::geometry::{VIEWER_BOARD_CENTER_Y, VIEWER_GRID_SPACING};
        use crate::ui::countdown_card::{
            assert_countdown_card_lands_on_board, measure_countdown_card,
        };
        use crate::ui::focus::real_layout_app;

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/replays/v6-round-zero.json");
        let json = std::fs::read_to_string(fixture).expect("the Round 0 fixture is checked in");
        let replay = Replay::from_json(&json).expect("the fixture is a valid Replay");
        let mut app = viewer_screen_app(real_layout_app(), replay);
        app.update();
        // Half a second in, the fixture's countdown is showing its 3. The
        // display pass shows the card on the next update, and the layout
        // after it places the digit.
        app.world_mut()
            .resource_mut::<ReplayViewerState>()
            .controller
            .position_seconds = 0.5;
        app.update();
        app.update();

        let landing = measure_countdown_card(&mut app, "3");
        assert_countdown_card_lands_on_board(&landing, VIEWER_BOARD_CENTER_Y, VIEWER_GRID_SPACING);
    }

    /// The pointer reaches every control the Viewer shows, at its centre,
    /// under real layout and real picking: nothing drawn over the board,
    /// the header or the transport stops it, which matters because picking
    /// stops at the first node under the pointer whatever that node is.
    /// With either overlay open the pointer reaches its Close and nothing
    /// beneath it.
    #[test]
    fn the_pointer_reaches_every_viewer_control_and_an_overlay_covers_the_rest() {
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::{FocusGroup, Focusable, real_layout_app};

        let mut app = viewer_screen_app(real_layout_app(), fixed_metadata_replay());
        app.update();
        app.update();
        let controls = reachable_buttons(&mut app);
        let screen_controls = {
            let world = app.world_mut();
            world
                .query::<&Focusable>()
                .iter(world)
                .filter(|focusable| focusable.group == FocusGroup::Screen)
                .count()
        };
        assert_eq!(
            controls.len(),
            screen_controls,
            "every header and transport control, and neither overlay's Close"
        );
        assert_eq!(covered_at_their_centres(&mut app, &controls), []);

        for (open, close) in [
            (
                (|viewer: &mut ReplayViewerState| viewer.key_open = true)
                    as fn(&mut ReplayViewerState),
                ViewerAction::CloseKey,
            ),
            (
                |viewer: &mut ReplayViewerState| viewer.match_settings_open = true,
                ViewerAction::CloseMatchSettings,
            ),
        ] {
            open(&mut app.world_mut().resource_mut::<ReplayViewerState>());
            app.update();
            app.update();
            let shown = reachable_buttons(&mut app);
            let closes: Vec<_> = shown
                .iter()
                .copied()
                .filter(|entity| app.world().get::<ViewerAction>(*entity) == Some(&close))
                .collect();
            assert_eq!(closes.len(), 1, "{close:?} is shown");
            assert_eq!(covered_at_their_centres(&mut app, &closes), [], "{close:?}");
            assert_eq!(
                covered_at_their_centres(&mut app, &controls),
                controls,
                "under the overlay {close:?} closes, every control is covered"
            );
            close_open_viewer_modal(&mut app.world_mut().resource_mut::<ReplayViewerState>());
            app.update();
            app.update();
        }
        assert_eq!(covered_at_their_centres(&mut app, &controls), []);
    }

    /// The whole screen driven without a pointer, over real `bevy_ui`
    /// layout and the production `Update` chain: the ring walks the header
    /// and transport in drawn order, skipping the controls disabled at the
    /// start of a replay; Enter on Key opens the overlay and the ring moves
    /// to its Close; closing it - by Enter on Close, or by Escape or Start -
    /// puts the ring back on the control it left rather than on Back; a pad
    /// does the same with the D-pad and South; and Play really plays.
    #[test]
    fn the_viewer_is_operable_without_a_pointer_and_the_ring_survives_an_overlay() {
        use crate::ui::focus::real_layout_app;
        use bevy::input_focus::{InputFocus, InputFocusVisible};

        let mut app = viewer_screen_app(real_layout_app(), fixed_metadata_replay());
        app.update();

        let control = |app: &mut App, wanted: ViewerAction| -> Entity {
            app.world_mut()
                .query::<(Entity, &ViewerAction)>()
                .iter(app.world())
                .find_map(|(entity, action)| (*action == wanted).then_some(entity))
                .expect("the viewer spawns every control")
        };
        let focused = |app: &App| app.world().resource::<InputFocus>().get();
        let press_key = |app: &mut App, key: KeyCode| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
            keys.press(key);
        };
        let release_keys = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            let held: Vec<KeyCode> = keys.get_pressed().copied().collect();
            for key in held {
                keys.release(key);
            }
            keys.clear();
        };
        fn viewer(app: &App) -> &ReplayViewerState {
            app.world().resource::<ReplayViewerState>()
        }

        // The first Tab reveals the ring on Back; then the header and the
        // transport in drawn order, with Rewind and Jump Back - disabled at
        // the start of a replay - skipped.
        assert!(!app.world().resource::<InputFocusVisible>().0);
        let walk = [
            ViewerAction::BackToReplays,
            ViewerAction::MatchSettings,
            ViewerAction::Key,
            ViewerAction::PlayPause,
            ViewerAction::JumpForward,
        ];
        for action in walk {
            press_key(&mut app, KeyCode::Tab);
            app.update();
            assert_eq!(focused(&app), Some(control(&mut app, action)), "{action:?}");
        }
        assert!(app.world().resource::<InputFocusVisible>().0);
        press_key(&mut app, KeyCode::ArrowLeft);
        app.update();
        press_key(&mut app, KeyCode::ArrowLeft);
        app.update();
        assert_eq!(focused(&app), Some(control(&mut app, ViewerAction::Key)));

        // Enter on Key opens the overlay; the ring moves to its Close.
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(viewer(&app).key_open);
        assert_eq!(
            focused(&app),
            Some(control(&mut app, ViewerAction::CloseKey))
        );
        // Movement stays inside the overlay: one entry, so Tab stays put.
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(
            focused(&app),
            Some(control(&mut app, ViewerAction::CloseKey))
        );

        // Enter on Close closes it, and the ring is back on Key - not Back.
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(!viewer(&app).key_open);
        assert_eq!(focused(&app), Some(control(&mut app, ViewerAction::Key)));

        // Escape closes an overlay opened the same way, with the same return.
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(viewer(&app).key_open);
        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(!viewer(&app).key_open);
        assert_eq!(focused(&app), Some(control(&mut app, ViewerAction::Key)));
        release_keys(&mut app);

        // A pad: D-pad left to Match Settings, South opens it, Start closes
        // it, and the ring returns to Match Settings.
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::DPadLeft);
        let pad = app.world_mut().spawn(gamepad).id();
        app.update();
        assert_eq!(
            focused(&app),
            Some(control(&mut app, ViewerAction::MatchSettings))
        );
        let pad_press = |app: &mut App, button: GamepadButton| {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            let held: Vec<GamepadButton> = pad_state.digital().get_pressed().copied().collect();
            for held_button in held {
                pad_state.digital_mut().release(held_button);
            }
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(button);
        };
        pad_press(&mut app, GamepadButton::South);
        app.update();
        assert!(viewer(&app).match_settings_open);
        assert_eq!(
            focused(&app),
            Some(control(&mut app, ViewerAction::CloseMatchSettings))
        );
        pad_press(&mut app, GamepadButton::Start);
        app.update();
        assert!(!viewer(&app).match_settings_open);
        assert_eq!(
            focused(&app),
            Some(control(&mut app, ViewerAction::MatchSettings))
        );
        app.world_mut().entity_mut(pad).despawn();

        // Play really plays through the unmodified button reader.
        for _ in 0..2 {
            press_key(&mut app, KeyCode::Tab);
            app.update();
        }
        assert_eq!(
            focused(&app),
            Some(control(&mut app, ViewerAction::PlayPause))
        );
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(viewer(&app).controller.playing);
        let play_pause = control(&mut app, ViewerAction::PlayPause);
        let label = app.world().get::<Children>(play_pause).unwrap()[0];
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "Pause");

        // With nothing open, Escape leaves for the Library.
        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::ReplayLibrary)
        ));
    }

    #[test]
    fn replay_transport_actions_are_boundary_aware_and_inert_when_disabled() {
        let replay = fixed_metadata_replay();
        let mut viewer = ReplayViewerState {
            replay_id: Some(replay.id.clone()),
            ..default()
        };

        for action in [ViewerAction::Rewind, ViewerAction::JumpBack] {
            assert!(viewer_action_disabled(action, &viewer, &replay));
            let before = viewer.controller;
            apply_viewer_action(&mut viewer, action, &replay);
            assert_eq!(viewer.controller, before);
        }
        for action in [
            ViewerAction::PlayPause,
            ViewerAction::JumpForward,
            ViewerAction::FastForward,
        ] {
            assert!(!viewer_action_disabled(action, &viewer, &replay));
        }

        viewer.controller.position_seconds = 0.5;
        for action in [
            ViewerAction::Rewind,
            ViewerAction::JumpBack,
            ViewerAction::PlayPause,
            ViewerAction::JumpForward,
            ViewerAction::FastForward,
        ] {
            assert!(!viewer_action_disabled(action, &viewer, &replay));
        }

        viewer.controller.position_seconds = replay.duration_seconds - 0.000_000_5;
        for action in [
            ViewerAction::PlayPause,
            ViewerAction::JumpForward,
            ViewerAction::FastForward,
        ] {
            assert!(viewer_action_disabled(action, &viewer, &replay));
            let before = viewer.controller;
            apply_viewer_action(&mut viewer, action, &replay);
            assert_eq!(viewer.controller, before);
        }
        assert!(!viewer_action_disabled(
            ViewerAction::Rewind,
            &viewer,
            &replay
        ));
        assert_eq!(
            button_background(Highlight::Hovered, false, true),
            crate::presentation::BUTTON_DISABLED
        );
        assert_eq!(BUTTON_DISABLED_BORDER, Color::srgb(0.20, 0.23, 0.26));
    }

    #[test]
    fn natural_replay_end_pauses_and_disables_forward_and_play() {
        let replay = fixed_metadata_replay();
        let mut controller = PlaybackController {
            position_seconds: replay.duration_seconds - 0.25,
            speed: 1.0,
            playing: true,
        };

        controller.advance(1.0, replay.duration_seconds);

        assert_eq!(controller.position_seconds, replay.duration_seconds);
        assert!(!controller.playing);
        for action in [
            ViewerAction::PlayPause,
            ViewerAction::JumpForward,
            ViewerAction::FastForward,
        ] {
            let viewer = ReplayViewerState {
                controller,
                ..default()
            };
            assert!(viewer_action_disabled(action, &viewer, &replay));
        }
    }

    #[test]
    fn key_modal_pauses_blocks_controls_and_stays_paused_after_close() {
        let replay = fixed_metadata_replay();
        let mut viewer = ReplayViewerState {
            replay_id: Some(replay.id.clone()),
            replay: Some(replay.clone()),
            controller: PlaybackController {
                position_seconds: 0.5,
                speed: 2.0,
                playing: true,
            },
            key_open: false,
            match_settings_open: false,
        };

        assert_eq!(
            apply_viewer_action(&mut viewer, ViewerAction::Key, &replay),
            None
        );
        assert!(viewer.key_open);
        assert!(!viewer.controller.playing);
        assert_eq!(viewer.controller.position_seconds, 0.5);
        assert_eq!(viewer.controller.speed, 2.0);

        apply_viewer_action(&mut viewer, ViewerAction::Rewind, &replay);
        apply_viewer_action(&mut viewer, ViewerAction::Speed(0.5), &replay);
        assert_eq!(viewer.controller.position_seconds, 0.5);
        assert_eq!(viewer.controller.speed, 2.0);

        apply_viewer_action(&mut viewer, ViewerAction::CloseKey, &replay);
        assert!(!viewer.key_open);
        assert!(!viewer.controller.playing);
        assert_eq!(viewer.controller.position_seconds, 0.5);
        assert_eq!(viewer.controller.speed, 2.0);
    }

    #[test]
    fn match_settings_modal_pauses_blocks_controls_and_is_mutually_exclusive() {
        let replay = fixed_metadata_replay();
        let mut viewer = ReplayViewerState {
            replay_id: Some(replay.id.clone()),
            controller: PlaybackController {
                position_seconds: 0.5,
                speed: 2.0,
                playing: true,
            },
            ..default()
        };

        apply_viewer_action(&mut viewer, ViewerAction::MatchSettings, &replay);
        assert!(viewer.match_settings_open);
        assert!(!viewer.key_open);
        assert!(!viewer.controller.playing);

        for action in [
            ViewerAction::Key,
            ViewerAction::Rewind,
            ViewerAction::Speed(0.5),
            ViewerAction::BackToReplays,
        ] {
            assert!(viewer_action_disabled(action, &viewer, &replay));
            assert_eq!(apply_viewer_action(&mut viewer, action, &replay), None);
        }
        assert_eq!(viewer.controller.position_seconds, 0.5);
        assert_eq!(viewer.controller.speed, 2.0);

        apply_viewer_action(&mut viewer, ViewerAction::CloseMatchSettings, &replay);
        assert!(!viewer.match_settings_open);
        assert!(!viewer.controller.playing);
        apply_viewer_action(&mut viewer, ViewerAction::Key, &replay);
        assert!(viewer.key_open);
        assert!(!viewer.match_settings_open);
        assert_eq!(
            apply_viewer_action(&mut viewer, ViewerAction::MatchSettings, &replay),
            None
        );
        assert!(close_open_viewer_modal(&mut viewer));
        assert!(!viewer.key_open);
        assert!(!viewer.match_settings_open);
        assert!(!viewer.controller.playing);
    }

    #[test]
    fn leaving_replay_viewer_clears_modal_and_playback_state() {
        let replay = fixed_metadata_replay();
        let mut viewer = ReplayViewerState {
            replay_id: Some(replay.id.clone()),
            replay: Some(replay),
            controller: PlaybackController {
                position_seconds: 0.5,
                speed: 2.0,
                playing: false,
            },
            key_open: true,
            match_settings_open: false,
        };

        clear_viewer_session(&mut viewer);

        assert!(viewer.replay_id.is_none());
        assert_eq!(viewer.controller, PlaybackController::default());
        assert!(!viewer.key_open);
        assert!(!viewer.match_settings_open);
    }

    #[test]
    fn opening_another_replay_resets_playback_and_both_modals() {
        let replay = fixed_metadata_replay();
        let mut viewer = ReplayViewerState {
            replay_id: Some(replay.id.clone()),
            replay: Some(replay.clone()),
            controller: PlaybackController {
                position_seconds: 0.5,
                speed: 2.0,
                playing: true,
            },
            key_open: true,
            match_settings_open: true,
        };

        prepare_viewer_session(&mut viewer);

        assert_eq!(viewer.replay_id.as_deref(), Some(replay.id.as_str()));
        assert_eq!(viewer.controller, PlaybackController::default());
        assert!(!viewer.key_open);
        assert!(!viewer.match_settings_open);
    }
}
