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
//! # Why this screen has five submodules
//!
//! The Match and Replay Library screens each divide into a `mod.rs` and a
//! `layout.rs`, and `ui/settings` is cut into six files - two of them owning
//! tabs (`bindings` the Keyboard and Controller pair, `game` its own), the
//! rest cutting across them. The viewer
//! does not fit in two files either, and the extra cuts fall where the
//! dependencies already do:
//!
//! * [`geometry`] - where everything stands in a window of any size, the
//!   sizes of the two Replay-only marks, and the pure functions that place a
//!   stone, a hidden selection or a score badge. Spawning, placing and
//!   per-frame drawing all depend on it and it depends on none of them, so
//!   it cannot live in any.
//! * [`wrap`] - where `bevy_ui` breaks a line of text, which [`geometry`]
//!   reads to know how many lines the Replay's label takes. It depends on
//!   nothing in the game, which lets it be checked against the text layout
//!   engine's own line breaker.
//! * [`layout`] - spawns the board, the cursors, the markers, the hidden
//!   selections, the badges and the transport bar, once, on entry, and the
//!   pass that keeps them where the window's size puts them.
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
mod wrap;

use bevy::prelude::*;

use super::REPLAY_FIELD_SEPARATOR;
use crate::app::AppScreen;
use crate::audio::{AudioCueQueue, UiAudioSet, UiCue};
use crate::replay::{PlaybackController, Replay};
use crate::rules::{Player, Point, Score};
use crate::ui::button::{Activated, UiButtonPlugin};
use display::update_viewer_display;
use layout::{place_viewer_furniture, spawn_viewer_screen};
/// Registers the viewer screen's lifecycle and its ordered update pipeline.
///
/// Where things stand comes first: the placement pass moves the furniture
/// for the window's size before the display pass writes the parts of it that
/// follow playback, one of them on a node the placement pass places too.
/// Then input is applied before the clock advances and before the board is
/// drawn, so a seek requested this frame is reflected immediately and cannot
/// briefly render the previous timeline position.
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
                    place_viewer_furniture,
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

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
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

    /// The whole Viewer under real `bevy_ui` layout and the game's own
    /// plugins and schedule, in a window of `window` logical pixels: the
    /// window's layout and the main camera's framing, the focus core and
    /// this screen's plugin, with the production stages, entered from the
    /// Replay Library by a real state transition, as the Library's Play
    /// opens a Replay: on `replay`, at its start. The entering frame has
    /// run, so the screen is spawned, placed, framed and laid out.
    fn viewer_app(window: Vec2, replay: Replay) -> App {
        use crate::app::configure_app_systems;
        use crate::app::visuals::{MainCamera, StoneMesh, WindowLayoutPlugin};
        use crate::gameplay::InputGate;
        use crate::ui::focus::{UiFocusPlugin, real_layout_app};
        use crate::ui::hud::HudState;

        let mut app = real_layout_app(window);
        app.init_resource::<Assets<ColorMaterial>>();
        let palette = {
            let mut materials = app.world_mut().resource_mut::<Assets<ColorMaterial>>();
            crate::app::visuals::test_stone_palette(&mut materials)
        };
        let stone_mesh = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            StoneMesh(meshes.add(Circle::new(1.0)))
        };
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(AppScreen::ReplayLibrary)
            .add_plugins(WindowLayoutPlugin)
            .add_plugins(UiFocusPlugin)
            .add_plugins(ReplayViewerPlugin)
            .insert_resource(palette)
            .insert_resource(stone_mesh)
            .insert_resource(HudState::default())
            .insert_resource(InputGate::default())
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>();
        configure_app_systems(&mut app);
        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<Camera2d>>()
            .single(app.world())
            .expect("the real-layout app has one camera");
        app.world_mut().entity_mut(camera).insert(MainCamera);
        app.update();
        open_from_the_library(&mut app, replay);
        app
    }

    /// Opens `replay` as the Library's Play does - the session's Replay,
    /// then the change of screen - and runs the frame that enters the
    /// Viewer.
    fn open_from_the_library(app: &mut App, replay: Replay) {
        {
            let mut viewer = app.world_mut().resource_mut::<ReplayViewerState>();
            viewer.replay_id = Some(replay.id.clone());
            viewer.replay = Some(replay);
        }
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ReplayViewer);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::ReplayViewer
        );
    }

    /// The layout the Viewer in `app` is laid out by: its window's, for its
    /// Replay's label.
    fn layout_of(app: &App) -> geometry::ViewerLayout {
        use crate::app::visuals::WindowSize;

        let label = &app
            .world()
            .resource::<ReplayViewerState>()
            .replay
            .as_ref()
            .expect("a Replay is open")
            .label;
        geometry::viewer_layout(
            app.world().resource::<WindowSize>().layout(),
            geometry::TallLabelRoom::for_label(label),
        )
    }

    /// The camera's framing in `app` is the one the Viewer's layout gives
    /// its board, and the main camera draws with it.
    fn assert_frames_its_board(app: &mut App, context: &str) {
        use crate::app::visuals::{MainCamera, WorldFraming};

        let framing = layout_of(app).framing();
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            framing,
            "{context}"
        );
        let (projection, transform) = app
            .world_mut()
            .query_filtered::<(&Projection, &Transform), With<MainCamera>>()
            .single(app.world())
            .unwrap();
        let Projection::Orthographic(projection) = projection else {
            panic!("the main camera is orthographic");
        };
        assert_eq!(projection.scale, 1.0 / framing.zoom, "{context}");
        assert_eq!(
            transform.translation.truncate(),
            framing.center,
            "{context}"
        );
    }

    /// The Round 0 fixture, whose countdown shows its 3 half a second in.
    fn round_zero_fixture() -> Replay {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/replays/v6-round-zero.json");
        let json = std::fs::read_to_string(fixture).expect("the Round 0 fixture is checked in");
        Replay::from_json(&json).expect("the fixture is a valid Replay")
    }

    /// The Round 0 countdown at every named size, laid out with its text by
    /// `bevy_ui` itself and read back: the digit is drawn in the middle of
    /// its card, and the card stands over the same point of this board as
    /// the Match screen's over the live one, at the zoom the board is drawn
    /// at - where the board's centre really is drawn, by the camera's
    /// framing. Until 2026-10-05 the card was itself the `Text` node, and
    /// its `3` sat in the card's top-left corner while the card was exactly
    /// where it was meant to be - `crate::ui::countdown_card` says why. A
    /// test of node rects alone cannot see that, which is why the digit is
    /// measured by its glyphs.
    #[test]
    fn the_countdown_digit_is_drawn_centred_on_a_card_over_the_board() {
        use super::geometry::{VIEWER_BOARD_CENTER_Y, VIEWER_GRID_SPACING};
        use crate::app::visuals::{NAMED_WINDOW_SIZES, WorldFraming};
        use crate::ui::countdown_card::{
            assert_countdown_card_lands_on_board, measure_countdown_card,
        };

        for window in NAMED_WINDOW_SIZES {
            let mut app = viewer_app(window, round_zero_fixture());
            // Half a second in, the fixture's countdown is showing its 3.
            // The display pass shows the card on the next update, and the
            // layout after it places the digit.
            app.world_mut()
                .resource_mut::<ReplayViewerState>()
                .controller
                .position_seconds = 0.5;
            app.update();
            app.update();

            // Where the camera draws the board's centre, worked out from its
            // world position rather than by the helper that places the card.
            let landing = measure_countdown_card(&mut app, "3");
            let framing = *app.world().resource::<WorldFraming>();
            let board_center = framing.window_point(Vec2::new(0.0, VIEWER_BOARD_CENTER_Y), window);
            let top = assert_countdown_card_lands_on_board(
                &landing,
                board_center,
                VIEWER_GRID_SPACING,
                framing.zoom,
            );
            if window == crate::app::visuals::WindowSize::OPENING.0 {
                // 297 px down: the 268 px it stood at before 2026-10-05,
                // 29 px too high, and those 29, to the half pixel the
                // layout rounds to.
                assert!((top - 296.9).abs() <= 0.5, "{top}");
            }
        }
    }

    /// A Replay that ended by every automatic limit at once, the most end
    /// reasons the Match Settings modal lists.
    fn every_end_reason() -> Replay {
        use crate::replay::EndReason;

        let mut replay = fixed_metadata_replay();
        replay.end_reasons = vec![
            EndReason::MatchTimeLimit,
            EndReason::RoundLimit,
            EndReason::NoStoneRoundLimit,
            EndReason::PointTarget,
        ];
        replay
    }

    /// Every text `app` shows lies inside a window of `window` logical
    /// pixels, and there is some to show.
    fn assert_every_text_is_inside(app: &mut App, window: Vec2, context: &str) {
        use crate::ui::focus::laid_out_texts;

        let texts = laid_out_texts(app);
        assert!(texts.len() > 10, "{context}: {texts:?}");
        let inside = Rect::from_corners(Vec2::ZERO, window);
        for (text, rect) in texts {
            assert!(
                inside.contains(rect.min) && inside.contains(rect.max),
                "{context}: {text:?} at {rect:?} leaves the {window} window"
            );
        }
    }

    /// At every named size, the pointer reaches every control the Viewer
    /// shows, at its centre, under real layout and real picking: nothing
    /// drawn over the board, the header or the transport stops it, which
    /// matters because picking stops at the first node under the pointer
    /// whatever that node is. Every text stands inside the window. With
    /// either overlay open - Match Settings for a Replay with every end
    /// reason it can list - the pointer reaches its Close and nothing
    /// beneath it, and every text still stands inside the window.
    #[test]
    fn the_pointer_reaches_every_viewer_control_and_an_overlay_covers_the_rest() {
        use crate::app::visuals::NAMED_WINDOW_SIZES;
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::{FocusGroup, Focusable};

        for window in NAMED_WINDOW_SIZES {
            let mut app = viewer_app(window, every_end_reason());
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
                "{window}: every header and transport control, and neither overlay's Close"
            );
            assert_eq!(
                covered_at_their_centres(&mut app, &controls),
                [],
                "{window}"
            );
            assert_every_text_is_inside(&mut app, window, &format!("{window}"));

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
                assert_eq!(closes.len(), 1, "{window}: {close:?} is shown");
                assert_eq!(
                    covered_at_their_centres(&mut app, &closes),
                    [],
                    "{window}: {close:?}"
                );
                assert_eq!(
                    covered_at_their_centres(&mut app, &controls),
                    controls,
                    "{window}: under the overlay {close:?} closes, every control is covered"
                );
                assert_every_text_is_inside(&mut app, window, &format!("{window} {close:?}"));
                close_open_viewer_modal(&mut app.world_mut().resource_mut::<ReplayViewerState>());
                app.update();
                app.update();
            }
            assert_eq!(
                covered_at_their_centres(&mut app, &controls),
                [],
                "{window}"
            );
        }
    }

    /// Through the game's own schedule, at every named size: the Viewer
    /// frames its board on entry, a change of screen puts the framing back,
    /// and coming back frames the board again. The layout pass writes the
    /// framing on every frame of the Viewer, so it holds even on an entry
    /// where the window's size has not changed since the last; a pass run
    /// only when the size changes, or not at all, would leave the board
    /// drawn at its own size over the origin. And the same Replay opened
    /// again, at the start where it was left, has everything placed where
    /// a first entry places it - every placed node, both cursors and both
    /// score badges, which the new board spawns at the world's origin and
    /// the display pass has to place even though the Replay, the event in
    /// effect and the window's size are all as they were.
    #[test]
    fn leaving_the_viewer_and_coming_back_frames_the_board_again() {
        use crate::app::visuals::{NAMED_WINDOW_SIZES, WorldFraming};

        for window in NAMED_WINDOW_SIZES {
            let mut app = viewer_app(window, fixed_metadata_replay());
            assert_frames_its_board(&mut app, &format!("{window} on entry"));
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(AppScreen::ReplayLibrary);
            app.update();
            assert_eq!(
                *app.world().resource::<WorldFraming>(),
                WorldFraming::IDENTITY,
                "{window} back in the Library"
            );
            open_from_the_library(&mut app, fixed_metadata_replay());
            assert_frames_its_board(&mut app, &format!("{window} back in the Viewer"));
            let again = placed_state(&mut app);
            let first = placed_state(&mut viewer_app(window, fixed_metadata_replay()));
            assert!(
                first
                    .2
                    .iter()
                    .all(|cursor| cursor.translation != Vec3::ZERO),
                "{window}: a first entry places the cursors away from the origin"
            );
            assert_eq!(again, first, "{window}: opened again");
        }
        let mut wide = viewer_app(Vec2::new(1280.0, 641.0), fixed_metadata_replay());
        assert_ne!(
            *wide.world().resource::<WorldFraming>(),
            WorldFraming::IDENTITY,
            "the laptop's framing is not the identity, so the check can fail"
        );
        assert_frames_its_board(&mut wide, "the laptop");
    }

    /// What the Viewer's passes write where things stand: every placed
    /// node's `Node`, keyed by its placement, in the order it was spawned;
    /// both score badges' transforms; and both cursors', Player One's first
    /// of each.
    type PlacedState = (
        Vec<(layout::ViewerPlacement, Node)>,
        Vec<Transform>,
        Vec<Transform>,
    );

    /// The [`PlacedState`] of the Viewer in `app`.
    fn placed_state(app: &mut App) -> PlacedState {
        let world = app.world_mut();
        let mut nodes: Vec<_> = world
            .query::<(Entity, &layout::ViewerPlacement, &Node)>()
            .iter(world)
            .map(|(entity, placement, node)| (entity, *placement, node.clone()))
            .collect();
        nodes.sort_by_key(|(entity, ..)| *entity);
        let mut badges: Vec<_> = world
            .query::<(&ViewerScoreBadge, &Transform)>()
            .iter(world)
            .map(|(badge, transform)| (player_index(badge.0), *transform))
            .collect();
        badges.sort_by_key(|(player, _)| *player);
        let mut cursors: Vec<_> = world
            .query::<(&ViewerCursor, &Transform)>()
            .iter(world)
            .map(|(cursor, transform)| (player_index(cursor.0), *transform))
            .collect();
        cursors.sort_by_key(|(player, _)| *player);
        (
            nodes
                .into_iter()
                .map(|(_, placement, node)| (placement, node))
                .collect(),
            badges.into_iter().map(|(_, transform)| transform).collect(),
            cursors
                .into_iter()
                .map(|(_, transform)| transform)
                .collect(),
        )
    }

    /// Makes the primary window `size` logical pixels, as a player's
    /// resize does, and runs a frame; the window's size, which every layout
    /// pass reads, follows it in that frame. The test app's camera keeps the
    /// viewport it was made with, so `bevy_ui` goes on laying the screen out
    /// at that size: what follows a resize here is what the passes write.
    fn resize(app: &mut App, size: Vec2) {
        use crate::app::visuals::WindowSize;
        use bevy::window::PrimaryWindow;

        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(size.x, size.y);
        app.update();
        assert_eq!(app.world().resource::<WindowSize>().0, size);
    }

    /// A resize of the window while the Viewer is open writes every placed
    /// node, the framing and the score badges where a fresh entry at the
    /// new size writes them, in the frame of the resize - and for a window
    /// smaller than the minimum, where a fresh entry at the minimum writes
    /// them - and resizing back restores each exactly. With the countdown
    /// showing, so the one node the display pass shows and hides as well is
    /// moved and stays shown. This holds what the passes write, not what
    /// `bevy_ui` lays out from it, which the harness does at the size it
    /// was made at ([`resize`]).
    #[test]
    fn a_change_of_window_size_moves_the_viewer_and_moving_back_restores_it() {
        use crate::app::visuals::WindowSize;

        let laptop = Vec2::new(1280.0, 641.0);
        let at_countdown = |app: &mut App| {
            app.world_mut()
                .resource_mut::<ReplayViewerState>()
                .controller
                .position_seconds = 0.5;
            app.update();
        };
        let countdown_shown = |app: &mut App| {
            let world = app.world_mut();
            world
                .query_filtered::<&Node, With<ViewerCountdown>>()
                .single(world)
                .unwrap()
                .display
                == Display::Flex
        };
        let mut app = viewer_app(WindowSize::OPENING.0, round_zero_fixture());
        at_countdown(&mut app);
        assert!(countdown_shown(&mut app));
        let opening = placed_state(&mut app);
        // The header row, tall's label and its three lines, wide's column,
        // the countdown, the transport row, the Key's panel, body and two
        // columns, and the two wordings of the Key's badge row.
        assert_eq!(opening.0.len(), 14);

        resize(&mut app, laptop);
        let moved = placed_state(&mut app);
        assert_ne!(moved, opening);
        let mut fresh = viewer_app(laptop, round_zero_fixture());
        at_countdown(&mut fresh);
        assert_eq!(moved, placed_state(&mut fresh));
        assert!(countdown_shown(&mut app), "moved, and still shown");
        assert_frames_its_board(&mut app, "at the laptop's size");

        // A window smaller than the minimum is laid out as the minimum.
        resize(&mut app, Vec2::new(800.0, 500.0));
        let mut minimum = viewer_app(WindowSize::MINIMUM.0, round_zero_fixture());
        at_countdown(&mut minimum);
        assert_eq!(placed_state(&mut app), placed_state(&mut minimum));

        resize(&mut app, WindowSize::OPENING.0);
        assert_eq!(placed_state(&mut app), opening);
        assert_frames_its_board(&mut app, "back at the opening size");
    }

    /// Where `entity`, drawn in world space, is drawn in the window by the
    /// main camera, worked out from the camera's own projection and place
    /// rather than from the layout: its centre in window pixels, and how
    /// many window pixels one of its own units is drawn at.
    fn drawn_by_the_camera(app: &mut App, entity: Entity) -> (Vec2, f32) {
        use crate::app::visuals::{MainCamera, WindowSize};

        let window = app.world().resource::<WindowSize>().0;
        let (projection, camera) = app
            .world_mut()
            .query_filtered::<(&Projection, &GlobalTransform), With<MainCamera>>()
            .single(app.world())
            .unwrap();
        let Projection::Orthographic(projection) = projection else {
            panic!("the main camera is orthographic");
        };
        let (scale, camera) = (projection.scale, camera.translation());
        let drawn = app.world().get::<GlobalTransform>(entity).unwrap();
        let offset = (drawn.translation() - camera).truncate() / scale;
        (
            Vec2::new(window.x / 2.0 + offset.x, window.y / 2.0 - offset.y),
            drawn.scale().x / scale,
        )
    }

    /// At every named size, both score badges with both cursors on one
    /// point, as the main camera draws them: each 42 by 28 window pixels,
    /// its score set at 15 px - whatever the board's zoom, which would
    /// otherwise draw them at 12 px in the smallest window - the two side
    /// by side, 5 px apart, Player One's on the left; and each 4 px off the
    /// corner of the larger cursor, as drawn. And once more in the opening
    /// window for a Replay whose label takes three lines there, the one
    /// case in which that window's board is not drawn at its own size; in
    /// each, the camera frames the board where the layout puts it.
    #[test]
    fn the_badges_are_drawn_at_their_own_size_side_by_side_at_every_size() {
        use super::geometry::SCORE_BADGE_SIZE;
        use crate::app::visuals::{NAMED_WINDOW_SIZES, WindowSize, cursor_visual_style};
        use crate::replay::MatchRecorder;
        use crate::replay::test_support::frame;

        let shared = Point::new(3, 5);
        let start = crate::replay::ReplayFrame {
            cursors: [shared, shared],
            ..frame()
        };
        let mut recorder = MatchRecorder::new(1, start.clone());
        let replay = recorder.complete(2, 1.0, start).unwrap();
        let mut three_lines = replay.clone();
        three_lines.label = "Saturday afternoon rematch against my brother, best of three".into();
        let mut zooms = Vec::new();
        let cases = NAMED_WINDOW_SIZES
            .into_iter()
            .map(|window| (window, replay.clone()))
            .chain([(WindowSize::OPENING.0, three_lines)]);
        for (window, replay) in cases {
            let label_lines = geometry::TallLabelRoom::for_label(&replay.label);
            let mut app = viewer_app(window, replay);
            app.update();
            assert_frames_its_board(&mut app, &format!("{window} {label_lines:?}"));
            let mut badges: Vec<_> = app
                .world_mut()
                .query::<(Entity, &ViewerScoreBadge, &Children)>()
                .iter(app.world())
                .map(|(entity, badge, children)| (player_index(badge.0), entity, children.to_vec()))
                .collect();
            badges.sort_by_key(|(player, ..)| *player);
            assert_eq!(badges.len(), 2);
            let mut drawn = Vec::new();
            for (_, badge, children) in badges {
                let (centre, per_unit) = drawn_by_the_camera(&mut app, badge);
                let size = SCORE_BADGE_SIZE * per_unit;
                assert!(
                    (size - Vec2::new(42.0, 28.0)).abs().max_element() < 1e-3,
                    "{window}: a badge drawn {size}"
                );
                let score = children
                    .iter()
                    .copied()
                    .find(|child| app.world().get::<Text2d>(*child).is_some())
                    .expect("a badge holds its score");
                let FontSize::Px(font_px) = app.world().get::<TextFont>(score).unwrap().font_size
                else {
                    panic!("the score is sized in pixels");
                };
                let (_, score_per_unit) = drawn_by_the_camera(&mut app, score);
                assert!(
                    (font_px * score_per_unit - 15.0).abs() < 1e-3,
                    "{window}: the score drawn at {} px",
                    font_px * score_per_unit
                );
                drawn.push(Rect::from_center_size(centre, size));
            }
            let (one, two) = (drawn[0], drawn[1]);
            assert!(
                (two.min.x - one.max.x - 5.0).abs() < 1e-3,
                "{window}: {one:?} and {two:?} are not 5 px apart"
            );
            assert!(
                one.max.x < two.min.x,
                "{window}: Player One's is the left one"
            );
            assert!((one.min.y - two.min.y).abs() < 1e-3, "{window}");

            let cursor = app
                .world_mut()
                .query::<(Entity, &ViewerCursor)>()
                .iter(app.world())
                .find(|(_, cursor)| cursor.0 == Player::One)
                .map(|(entity, _)| entity)
                .unwrap();
            let (cursor, per_unit) = drawn_by_the_camera(&mut app, cursor);
            let half = cursor_visual_style(Player::One).size * per_unit / 2.0;
            assert!(
                (one.min.x - (cursor.x + half) - 4.0).abs() < 1e-3
                    && ((cursor.y - half) - one.max.y - 4.0).abs() < 1e-3,
                "{window}: {one:?} is not 4 px off the cursor's corner"
            );
            zooms.push(per_unit);
        }
        assert!(
            zooms.iter().any(|zoom| *zoom < 0.8) && zooms.iter().any(|zoom| *zoom > 1.9),
            "the sizes reach small and large boards: {zooms:?}"
        );
        let three_lines = zooms.last().unwrap();
        assert!(
            (three_lines - 0.949).abs() < 0.001,
            "a label on three lines draws the opening window's board at 0.949: {three_lines}"
        );
    }

    /// A Replay whose header texts are as long as the screen makes room
    /// for - `label`, the timer at 999:59 of 999:59, round 9,999 with Round
    /// Time on, a three-digit score each and a four-digit year - or, `past`
    /// them, a digit longer: 1,000 minutes, round 10,000 and a score of
    /// 1,000. Its start, in that round and playing, is in effect until its
    /// end, which is a little past the last second the timer shows, so the
    /// tests look half a second before the end ([`longest_moment`]).
    fn longest_texts(label: &str, past: bool) -> Replay {
        use crate::match_settings::{MatchSettingField, MatchSettings};
        use crate::replay::{MatchRecorder, ReplayFrame, ReplayPhase};
        use crate::rules::Score;

        let mut settings = MatchSettings::default();
        settings.set_value(MatchSettingField::RoundTime, 3_600);
        settings.set_enabled(MatchSettingField::RoundTime, true);
        let start = ReplayFrame {
            phase: ReplayPhase::Playing,
            current_round: if past { 10_000 } else { 9_999 },
            ..crate::replay::test_support::frame()
        };
        let mut recorder = MatchRecorder::for_test(1, start.clone(), settings);
        let duration = if past { 60_000.9 } else { 59_999.9 };
        let mut replay = recorder.complete(2, duration, start).unwrap();
        replay.label = label.into();
        let score = if past { 1_000 } else { 999 };
        replay.score = Score {
            black: score,
            white: score - 1,
        };
        // 2026-11-18, in any time zone.
        replay.completed_at_ms = 1_795_000_000_000;
        replay
    }

    /// Where the tests of [`longest_texts`] look: half a second before its
    /// end, where its start is still in effect and the timer reads 999:59
    /// of 999:59, or 1000:00 of 1000:00 past them.
    fn longest_moment(replay: &Replay) -> f64 {
        replay.duration_seconds - 0.5
    }

    /// `text` with every digit a 9: what a line of it breaks like, since a
    /// line breaks alike whichever digits a number has.
    fn as_nines(text: &str) -> String {
        text.chars()
            .map(|character| {
                if character.is_ascii_digit() {
                    '9'
                } else {
                    character
                }
            })
            .collect()
    }

    /// The words of the one shown text in `role`.
    fn shown_text(app: &mut App, role: ViewerText) -> String {
        use bevy::ui::ComputedNode;

        let world = app.world_mut();
        let shown: Vec<String> = world
            .query::<(&ViewerText, &Text, &ComputedNode, &InheritedVisibility)>()
            .iter(world)
            .filter(|(text, _, node, visibility)| {
                **text == role && visibility.get() && node.size() != Vec2::ZERO
            })
            .map(|(_, text, ..)| text.0.clone())
            .collect();
        assert_eq!(shown.len(), 1, "one {role:?} is shown: {shown:?}");
        shown[0].clone()
    }

    /// Labels of 60 characters, the most a Replay holds, that wrap badly: one
    /// word to a line at the column's narrowest, alternating a letter and a
    /// word that fills what is left; one that takes three lines in tall's
    /// header; and one with nowhere to break but within a word.
    const LONGEST_LABELS: [&str; 3] = [
        "A bbbbbbbbbbbbbb c dddddddddddddd e ffffffffffffff g hhhhhhh",
        "Saturday afternoon rematch against my brother, best of three",
        "Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ];

    /// The texts every button stands over, and every visible text drawn
    /// elsewhere, and the board and the score badges as the camera draws
    /// them, each labelled: what the overlap test below checks.
    fn drawn_items(app: &mut App) -> Vec<(String, Rect)> {
        use super::geometry::{SCORE_BADGE_SIZE, VIEWER_BOARD_CENTER_Y, VIEWER_BOARD_OUTER_SIZE};
        use crate::app::visuals::{WindowSize, WorldFraming};
        use crate::ui::focus::drawn_texts;
        use bevy::ui::{ComputedNode, UiGlobalTransform};

        let window = app.world().resource::<WindowSize>().0;
        let mut buttons: Vec<(String, Entity, Rect)> = Vec::new();
        {
            let world = app.world_mut();
            let mut nodes = world.query::<(
                Entity,
                &ViewerAction,
                &ComputedNode,
                &UiGlobalTransform,
                &InheritedVisibility,
            )>();
            for (entity, action, node, transform, visibility) in nodes.iter(world) {
                if !visibility.get() || node.size() == Vec2::ZERO {
                    continue;
                }
                let scale = node.inverse_scale_factor;
                let rect = Rect::from_center_size(transform.translation * scale, node.size * scale);
                buttons.push((format!("{action:?}"), entity, rect));
            }
        }
        let in_a_button = |app: &App, mut entity: Entity| loop {
            if buttons.iter().any(|(_, button, _)| *button == entity) {
                break true;
            }
            match app.world().get::<ChildOf>(entity) {
                Some(parent) => entity = parent.parent(),
                None => break false,
            }
        };
        let mut items: Vec<(String, Rect)> = buttons
            .iter()
            .map(|(label, _, rect)| (label.clone(), *rect))
            .collect();
        for (entity, text, ink) in drawn_texts(app) {
            if !in_a_button(app, entity) {
                items.push((format!("{text:?}"), ink));
            }
        }
        let framing = *app.world().resource::<WorldFraming>();
        let (centre, half) = (
            Vec2::new(0.0, VIEWER_BOARD_CENTER_Y),
            Vec2::splat(VIEWER_BOARD_OUTER_SIZE / 2.0),
        );
        items.push((
            "board".into(),
            Rect::from_corners(
                framing.window_point(centre - half, window),
                framing.window_point(centre + half, window),
            ),
        ));
        let badges: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<ViewerScoreBadge>>()
            .iter(app.world())
            .collect();
        for badge in badges {
            let (centre, per_unit) = drawn_by_the_camera(app, badge);
            items.push((
                "score badge".into(),
                Rect::from_center_size(centre, SCORE_BADGE_SIZE * per_unit),
            ));
        }
        items
    }

    /// At every named size, with the header's texts at their longest - each
    /// of three badly wrapping 60-character labels, and the longest timer,
    /// round and result lines the layout makes room for, which the screen
    /// is seen to draw - and then a digit past every assumption the layout
    /// makes about those three, nothing the Viewer draws meets anything
    /// else and nothing leaves the window: measured from what `bevy_ui`
    /// laid out, glyphs for text and boxes for buttons, and the board and
    /// the score badges where the camera draws them. A text inside a button
    /// is that button's. The score badges are the one thing meant to stand
    /// over another, the board.
    #[test]
    fn the_longest_texts_stand_clear_of_everything_at_every_size() {
        use super::geometry::{LONGEST_RESULT, LONGEST_ROUND, LONGEST_TIMER};
        use crate::app::visuals::NAMED_WINDOW_SIZES;

        for window in NAMED_WINDOW_SIZES {
            for label in LONGEST_LABELS {
                for past in [false, true] {
                    let replay = longest_texts(label, past);
                    let moment = longest_moment(&replay);
                    let mut app = viewer_app(window, replay);
                    app.world_mut()
                        .resource_mut::<ReplayViewerState>()
                        .controller
                        .position_seconds = moment;
                    app.update();
                    app.update();
                    let context = format!("{window} {label:?} past: {past}");
                    for (role, longest) in [
                        (ViewerText::Timer, LONGEST_TIMER),
                        (ViewerText::Round, LONGEST_ROUND),
                        (ViewerText::Result, LONGEST_RESULT),
                    ] {
                        let drawn = shown_text(&mut app, role);
                        if past {
                            assert!(drawn.len() > longest.len(), "{context}: {drawn:?}");
                        } else {
                            assert_eq!(as_nines(&drawn), as_nines(longest), "{context}");
                        }
                    }
                    let items = drawn_items(&mut app);
                    // Back, Match Settings, the nine transport controls, the
                    // label, the timer, the round, the result, the board and
                    // two badges.
                    assert_eq!(items.len(), 18, "{context}: {items:#?}");
                    let inside = Rect::from_corners(Vec2::ZERO, window);
                    for (name, rect) in &items {
                        assert!(
                            inside.contains(rect.min) && inside.contains(rect.max),
                            "{context}: {name} at {rect:?} leaves the window"
                        );
                    }
                    for (index, (name, rect)) in items.iter().enumerate() {
                        for (other, other_rect) in &items[index + 1..] {
                            let pair = [name.as_str(), other.as_str()];
                            if pair.contains(&"board") && pair.contains(&"score badge") {
                                continue;
                            }
                            assert!(
                                rect.intersect(*other_rect).is_empty(),
                                "{context}: {name} {rect:?} meets {other} {other_rect:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// How many lines `bevy_ui` set a label of 26 px on: its layout's
    /// height in lines. A line with nothing drawn on it - spaces at the end
    /// of a label, or the empty line after one past the end of a full line
    /// - counts, as it does in the height the label takes.
    fn lines_set(layout: &bevy::text::TextLayoutInfo) -> usize {
        use crate::ui::text_metrics::text_height;

        let line = text_height(1, geometry::VIEWER_LABEL_SIZE) * layout.scale_factor;
        (layout.size.y / line).round() as usize
    }

    /// How many lines the one shown label is set on.
    fn label_lines(app: &mut App) -> usize {
        use bevy::text::TextLayoutInfo;
        use bevy::ui::ComputedNode;

        let world = app.world_mut();
        let shown: Vec<usize> = world
            .query::<(
                &ViewerText,
                &ComputedNode,
                &TextLayoutInfo,
                &InheritedVisibility,
            )>()
            .iter(world)
            .filter(|(text, node, _, visibility)| {
                **text == ViewerText::Label && visibility.get() && node.size() != Vec2::ZERO
            })
            .map(|(_, _, layout, _)| lines_set(layout))
            .collect();
        assert_eq!(shown.len(), 1, "one label is shown");
        shown[0]
    }

    /// Labels a player might write, among them the ones that tell a line
    /// count that follows `bevy_ui` from one that does not: a bracket, a
    /// quotation mark or a hyphen where a line has to break, a word as
    /// long as a label, runs of spaces at a break and at the end.
    const REALISTIC_LABELS: [&str; 17] = [
        "Round zero",
        "Saturday rematch with my sister best of three, second game",
        "Friday night game with Alex (rematch after the tournament)",
        "Practice game vs. Jordan (handicap, teaching game)",
        "Rematch: best-of-three after the long-awaited semi-final",
        "Club night 'friendly' game -- \"no takebacks\" says Sam",
        "Club night / table 4 / round 2 of the knock-out stage",
        "Game (rematch) for the title: who wins? Nobody knows yet!",
        "Final [game 3 of 5] {sudden death} - played 2026-11-27",
        "Thursday evening game with      lots of spaces here",
        "Thursday   evening   game   with   lots   of   spaces   here",
        "Saturday afternoon rematch against my brother, best of three",
        "A bbbbbbbbbbbbbb c dddddddddddddd e ffffffffffffff g hhhhhhh",
        "Aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "Ends with spaces where the line is already full   ",
        "Short, with spaces at the end   ",
        // A space typed after a full second line: an empty third.
        "Saturday game with my aunt Lucy, best of three, round one. ",
    ];

    /// The judgement of which labels tall gives a third line, against what
    /// `bevy_ui` does with them in the Viewer itself, in the narrowest
    /// window's header, where labels wrap the most: every label given two
    /// lines is drawn on two at most, and every label drawn on two is given
    /// two, among the labels a player might write and those the game writes
    /// by itself.
    #[test]
    fn tall_draws_a_label_it_gives_two_lines_on_two_at_most() {
        use super::geometry::TallLabelRoom;
        use crate::app::visuals::WindowSize;
        use crate::replay::default_replay_label;
        use crate::rules::Score;

        let mut labels: Vec<String> = REALISTIC_LABELS.map(String::from).into();
        labels.push(default_replay_label(
            Score {
                black: 999,
                white: 998,
            },
            "2026-11-27 23:59",
        ));
        let mut seen = [0; 2];
        for label in labels {
            let mut replay = fixed_metadata_replay();
            replay.label = label.clone();
            let mut app = viewer_app(WindowSize::OPENING.0, replay);
            app.update();
            let lines = label_lines(&mut app);
            let given_two = TallLabelRoom::for_label(&label) == TallLabelRoom::TwoLines;
            assert_eq!(given_two, lines <= 2, "{label:?} is drawn on {lines} lines");
            seen[usize::from(given_two)] += 1;
        }
        assert!(seen[0] >= 4 && seen[1] >= 10, "{seen:?}");
    }

    /// Labels of many kinds, for [`the_label_check_counts_the_lines_bevy_ui_sets`]:
    /// the realistic ones; the labels the game writes by itself, at small
    /// and large scores; each printable punctuation mark at and around the
    /// place where a 31-character line has to break, after a space, after
    /// a word, inside a word and before a space; and some hundreds
    /// generated from words, punctuation and runs of spaces. Every one is a
    /// label a Replay may have.
    fn label_corpus() -> Vec<String> {
        use crate::replay::{default_replay_label, validate_label};
        use crate::rules::Score;

        let mut labels: Vec<String> = REALISTIC_LABELS.map(String::from).into();
        for (black, white) in [
            (0, 0),
            (3, 1),
            (42, 38),
            (81, 80),
            (999, 998),
            (99_999, 99_998),
        ] {
            for started in ["2026-01-01 00:00", "2026-11-27 23:59"] {
                labels.push(default_replay_label(Score { black, white }, started));
            }
        }
        for mark in (b'!'..=b'~').filter(|byte| !byte.is_ascii_alphanumeric()) {
            let mark = char::from(mark);
            for at in 27..=32 {
                let word = "a".repeat(at - 1);
                for label in [
                    format!("A{} {mark}bbbbbbbbbb cc", &word[1..]),
                    format!("A{word}{mark} bbbbbbbbbb cc"),
                    format!("A{word}{mark}bbbbbbbbbb cc"),
                    format!("A{} {mark} bbbbbbbbbb cc", &word[1..]),
                ] {
                    labels.push(label);
                }
            }
        }
        // A small generator, so the corpus is the same on every run.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = |below: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % below as u64) as usize
        };
        let words = [
            "a",
            "of",
            "the",
            "game",
            "match",
            "rematch",
            "Saturday",
            "tournament",
            "semi-final",
            "best-of-three",
            "(rematch)",
            "\"quoted\"",
            "'single'",
            "-",
            "/",
            "|",
            "vs.",
            "1/2",
            "3:45",
            "2026-11-27",
            "#4",
            "$5",
            "50%",
            "!",
            "?",
            "a,b",
            "(",
            ")",
            "[x]",
            "{y}",
            "e.g.",
            "...",
            "--",
            "x-",
            "-y",
            "Bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ];
        for _ in 0..400 {
            let mut label = String::from("A");
            let length = 20 + next(41);
            while label.len() < length {
                let spaces = [1, 1, 1, 1, 2, 3, 0][next(7)];
                label.push_str(&" ".repeat(spaces));
                label.push_str(words[next(words.len())]);
            }
            label.truncate(length);
            labels.push(label);
        }
        for label in &labels {
            assert!(validate_label(label).is_ok(), "{label:?}");
        }
        labels
    }

    /// Every label of [`label_corpus`], laid out by `bevy_ui` 498 px wide,
    /// as tall sets its label in the narrowest window's header: the lines
    /// each is set on are the lines [`wrap`] counts for 31 characters of
    /// 26 px to a line, label for label, so tall gives a third line to
    /// exactly the labels set on three.
    #[test]
    fn the_label_check_counts_the_lines_bevy_ui_sets() {
        use super::geometry::{TallLabelRoom, VIEWER_HEADER_INSET, VIEWER_LABEL_SIZE};
        use super::layout::header_text;
        use crate::app::visuals::{MIN_WINDOW_WIDTH, WindowSize};
        use crate::ui::focus::real_layout_app;
        use crate::ui::text_metrics::line_width;
        use bevy::text::TextLayoutInfo;

        let width = (MIN_WINDOW_WIDTH as f32 - 2.0 * VIEWER_HEADER_INSET) / 2.0;
        assert_eq!(width, 498.0);
        assert!(line_width(31, VIEWER_LABEL_SIZE) <= width);
        assert!(line_width(32, VIEWER_LABEL_SIZE) > width);
        let labels = label_corpus();
        let mut app = real_layout_app(WindowSize::OPENING.0);
        let root = app
            .world_mut()
            .spawn(Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .id();
        let nodes: Vec<Entity> = labels
            .iter()
            .map(|label| {
                app.world_mut()
                    .spawn((
                        header_text(ViewerText::Label, label.clone()),
                        Node {
                            width: px(width),
                            ..default()
                        },
                        ChildOf(root),
                    ))
                    .id()
            })
            .collect();
        app.update();
        app.update();
        let mut seen = [0; 4];
        for (label, node) in labels.iter().zip(nodes) {
            let set = lines_set(app.world().get::<TextLayoutInfo>(node).unwrap());
            assert_eq!(wrap::wrapped_lines(label, 31, true), set, "{label:?}");
            assert_eq!(
                TallLabelRoom::for_label(label) == TallLabelRoom::TwoLines,
                set <= 2,
                "{label:?}"
            );
            seen[set.min(3)] += 1;
        }
        assert!(
            seen[1] >= 10 && seen[2] >= 100 && seen[3] >= 20,
            "the corpus reaches one, two and three lines: {seen:?}"
        );
    }

    /// The node box of the one shown node carrying `marker`, in logical
    /// pixels from the window's top-left.
    fn shown_box<M: Component>(app: &mut App) -> Rect {
        use bevy::ui::{ComputedNode, UiGlobalTransform};

        let world = app.world_mut();
        let shown: Vec<Rect> = world
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<M>>()
            .iter(world)
            .filter(|(node, _)| node.size() != Vec2::ZERO)
            .map(|(node, transform)| {
                let scale = node.inverse_scale_factor;
                Rect::from_center_size(transform.translation * scale, node.size * scale)
            })
            .collect();
        assert_eq!(shown.len(), 1);
        shown[0]
    }

    /// The Key's panel and the Match Settings panel, as `bevy_ui` lays them
    /// out, stand inside the window with 12 px to spare at every named size
    /// and at either side of the Key's switch to two columns, 721 and 722
    /// px tall: Match Settings for a Replay with every end reason it lists,
    /// and the Key in one column from 722 px and in two below. The Key's
    /// height in each form is what the layout reckons with, measured here
    /// at a scale factor of 1.
    #[test]
    fn the_key_stands_inside_the_window_in_either_form() {
        use super::geometry::{
            KEY_ONE_COLUMN_HEIGHT, KEY_ONE_COLUMN_WIDTH, KEY_TWO_COLUMN_HEIGHT,
            KEY_TWO_COLUMN_WIDTH,
        };
        use crate::app::visuals::NAMED_WINDOW_SIZES;

        let mut sizes = NAMED_WINDOW_SIZES.to_vec();
        sizes.extend([Vec2::new(1040.0, 721.0), Vec2::new(1040.0, 722.0)]);
        let mut forms = Vec::new();
        for window in sizes {
            let mut app = viewer_app(window, every_end_reason());
            let inside = Rect::from_corners(Vec2::splat(12.0), window - Vec2::splat(12.0));
            app.world_mut().resource_mut::<ReplayViewerState>().key_open = true;
            app.update();
            app.update();
            let panel = {
                let world = app.world_mut();
                let panels: Vec<Entity> = world
                    .query::<(Entity, &layout::ViewerPlacement)>()
                    .iter(world)
                    .filter(|(_, placement)| **placement == layout::ViewerPlacement::KeyPanel)
                    .map(|(entity, _)| entity)
                    .collect();
                panels[0]
            };
            let key = {
                use bevy::ui::{ComputedNode, UiGlobalTransform};
                let node = app.world().get::<ComputedNode>(panel).unwrap();
                let transform = app.world().get::<UiGlobalTransform>(panel).unwrap();
                let scale = node.inverse_scale_factor;
                Rect::from_center_size(transform.translation * scale, node.size * scale)
            };
            assert!(
                inside.contains(key.min) && inside.contains(key.max),
                "{window}: the Key {key:?}"
            );
            let one_column = window.y >= 722.0;
            let expected = if one_column {
                Vec2::new(KEY_ONE_COLUMN_WIDTH, KEY_ONE_COLUMN_HEIGHT)
            } else {
                Vec2::new(KEY_TWO_COLUMN_WIDTH, KEY_TWO_COLUMN_HEIGHT)
            };
            assert!(
                (key.size() - expected).abs().max_element() < 0.5,
                "{window}: the Key is {} against {expected}",
                key.size()
            );
            forms.push(one_column);
            close_open_viewer_modal(&mut app.world_mut().resource_mut::<ReplayViewerState>());
            app.world_mut()
                .resource_mut::<ReplayViewerState>()
                .match_settings_open = true;
            app.update();
            app.update();
            let settings = shown_box::<ViewerMatchSettingsModal>(&mut app);
            assert_eq!(settings, Rect::from_corners(Vec2::ZERO, window), "{window}");
            let panel = {
                use bevy::ui::{ComputedNode, UiGlobalTransform};
                let world = app.world_mut();
                let root = world
                    .query_filtered::<Entity, With<ViewerMatchSettingsModal>>()
                    .single(world)
                    .unwrap();
                let panel = world.get::<Children>(root).unwrap()[0];
                let node = world.get::<ComputedNode>(panel).unwrap();
                let transform = world.get::<UiGlobalTransform>(panel).unwrap();
                let scale = node.inverse_scale_factor;
                Rect::from_center_size(transform.translation * scale, node.size * scale)
            };
            assert!(
                inside.contains(panel.min) && inside.contains(panel.max),
                "{window}: Match Settings {panel:?}"
            );
        }
        assert!(forms.contains(&true) && forms.contains(&false));
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
        use bevy::input_focus::{InputFocus, InputFocusVisible};

        let mut app = viewer_app(
            crate::app::visuals::WindowSize::OPENING.0,
            fixed_metadata_replay(),
        );
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

    /// The opening window as `bevy_ui` lays it out through the game's own
    /// plugins, written out as numbers: Back and Match Settings in the
    /// header row, the label's node between them and the three lines under
    /// it, and every control of the transport row. They are the numbers the
    /// screen was laid out at before it was laid out for other sizes,
    /// measured on `main` on 2026-11-28 with the same reading. A slip in an
    /// arm of the placement that the opening window reaches shows here, and
    /// not only in the pure layout's arithmetic.
    #[test]
    fn the_opening_window_lays_the_viewer_out_where_it_always_did() {
        use crate::app::visuals::WindowSize;
        use bevy::ui::{ComputedNode, UiGlobalTransform};

        let mut app = viewer_app(WindowSize::OPENING.0, fixed_metadata_replay());
        app.update();
        let rect = |left: f32, top: f32, width: f32, height: f32| {
            Rect::new(left, top, left + width, top + height)
        };
        let shown_rect = |node: &ComputedNode, transform: &UiGlobalTransform| {
            let scale = node.inverse_scale_factor;
            Rect::from_center_size(transform.translation * scale, node.size * scale)
        };
        let close =
            |a: Rect, b: Rect| (a.min - b.min).abs().max(a.max - b.max).max_element() < 1e-3;
        let world = app.world_mut();
        let buttons: Vec<(ViewerAction, Rect)> = world
            .query::<(
                &ViewerAction,
                &ComputedNode,
                &UiGlobalTransform,
                &InheritedVisibility,
            )>()
            .iter(world)
            .filter(|(_, node, _, visibility)| visibility.get() && node.size() != Vec2::ZERO)
            .map(|(action, node, transform, _)| (*action, shown_rect(node, transform)))
            .collect();
        for (action, expected) in [
            (ViewerAction::BackToReplays, rect(22.0, 12.0, 86.0, 38.0)),
            (ViewerAction::MatchSettings, rect(873.0, 12.0, 145.0, 38.0)),
            (ViewerAction::Key, rect(83.0, 840.0, 70.0, 38.0)),
            (ViewerAction::Rewind, rect(181.0, 840.0, 90.0, 38.0)),
            (ViewerAction::JumpBack, rect(279.0, 840.0, 105.0, 38.0)),
            (ViewerAction::PlayPause, rect(392.0, 840.0, 90.0, 38.0)),
            (ViewerAction::JumpForward, rect(490.0, 840.0, 115.0, 38.0)),
            (ViewerAction::FastForward, rect(613.0, 840.0, 115.0, 38.0)),
            (ViewerAction::Speed(0.5), rect(756.0, 840.0, 62.0, 38.0)),
            (ViewerAction::Speed(1.0), rect(826.0, 840.0, 62.0, 38.0)),
            (ViewerAction::Speed(2.0), rect(896.0, 840.0, 62.0, 38.0)),
        ] {
            let found: Vec<Rect> = buttons
                .iter()
                .filter(|(shown, _)| *shown == action)
                .map(|(_, rect)| *rect)
                .collect();
            assert_eq!(found.len(), 1, "{action:?} is shown once");
            assert!(close(found[0], expected), "{action:?} at {:?}", found[0]);
        }
        assert_eq!(buttons.len(), 11, "and nothing else: {buttons:?}");
        let texts: Vec<(ViewerText, Rect)> = world
            .query::<(
                &ViewerText,
                &ComputedNode,
                &UiGlobalTransform,
                &InheritedVisibility,
            )>()
            .iter(world)
            .filter(|(_, node, _, visibility)| visibility.get() && node.size() != Vec2::ZERO)
            .map(|(text, node, transform, _)| (*text, shown_rect(node, transform)))
            .collect();
        for (role, expected) in [
            (ViewerText::Label, rect(271.0, 15.0, 498.0, 32.0)),
            (ViewerText::Timer, rect(0.0, 80.0, 1040.0, 22.0)),
            (ViewerText::Round, rect(0.0, 104.0, 1040.0, 20.0)),
            (ViewerText::Result, rect(0.0, 128.0, 1040.0, 17.0)),
        ] {
            let found: Vec<Rect> = texts
                .iter()
                .filter(|(shown, _)| *shown == role)
                .map(|(_, rect)| *rect)
                .collect();
            assert_eq!(found.len(), 1, "one {role:?} is shown");
            assert!(close(found[0], expected), "{role:?} at {:?}", found[0]);
        }
    }
}
