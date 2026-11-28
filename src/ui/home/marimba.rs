//! The home-screen marimba: a hidden playable instrument.
//!
//! # Lifecycle
//!
//! The bars stay invisible until the player has pressed enough note keys
//! ([`HOME_MARIMBA_REVEAL_COUNT`]) in a single visit to the Home screen. After
//! that they fade in, stay while the player keeps playing, and fade out after
//! [`HOME_MARIMBA_IDLE_SECONDS`] of silence. Playing again during the fade-out
//! reverses it *from the current alpha* rather than restarting, so the bars
//! never flash.
//!
//! Once the fade-out completes - or the player leaves Home - the entire state
//! resets, including the press count, so the reveal has to be earned again.
//!
//! # Input
//!
//! Notes are read from raw [`KeyboardInput`] messages rather than
//! `ButtonInput`, because key repeat should count: holding a key trills. The
//! four physical keyboard rows are mapped across the eleven bars by
//! proportional position, so any row plays the full scale.
//!
//! Audio and visuals are deliberately independent: muting the game does not
//! change the reveal progress, and a note always highlights its bar.
//!
//! # Where it stands
//!
//! Under the panel, across the window, where it has always stood beside
//! the panel and the diamond, or upright beside the panel in a window too
//! short for that, as `super::geometry` decides, with the instrument's
//! dimensions. Upright, the instrument is the same one turned on its end:
//! the bars lie across, the longest and lowest at the bottom, and the same
//! key plays the same bar.

use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

use super::geometry::{
    BAR_BREADTH, BAR_GAP, BAR_LENGTH_STEP, HOME_MARIMBA_BAR_COUNT, LOWEST_BAR_LENGTH,
    MARIMBA_DEPTH, MARIMBA_LENGTH, MarimbaStance, PADDING_ACROSS, PADDING_ALONG,
};
use crate::app::{AppScreen, AppSystems};
use crate::presentation::{marimba_bar, marimba_border};

/// Registers the Easter egg's resource and its two systems.
///
/// Note input is *not* gated on the Home screen at registration time: the
/// system runs everywhere and checks the screen itself, so its
/// `MessageReader` cursor is advanced past presses made on other screens and
/// a stale press can never be read as a note on the first frame back on Home.
/// That check alone stopped covering the arrival frame once the Match exit's
/// confirm began transitioning in `PreUpdate` (the screen is already Home by
/// `Update`), so the handler also skips the frame the screen just changed.
/// The reveal count is therefore only ever advanced from Home.
pub(super) struct MarimbaPlugin;

impl Plugin for MarimbaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HomeMarimbaState>().add_systems(
            Update,
            (
                handle_home_marimba_input,
                animate_home_marimba.run_if(in_state(AppScreen::Home)),
            )
                .chain()
                .in_set(AppSystems::HomeEasterEgg),
        );
    }
}
use crate::app::motion::Motion;
use crate::audio::{AudioCueQueue, MarimbaNote, UiCue};
use crate::ui::hud::{HudState, modal_open};

const HOME_MARIMBA_REVEAL_COUNT: u8 = 20;
const HOME_MARIMBA_FADE_IN_SECONDS: f32 = 0.6;
const HOME_MARIMBA_IDLE_SECONDS: f32 = 10.0;
const HOME_MARIMBA_FADE_OUT_SECONDS: f32 = 0.8;
const HOME_MARIMBA_HIGHLIGHT_SECONDS: f32 = 0.36;
const HOME_MARIMBA_NOTES: [MarimbaNote; HOME_MARIMBA_BAR_COUNT] = [
    MarimbaNote::A3,
    MarimbaNote::B3,
    MarimbaNote::C4,
    MarimbaNote::D4,
    MarimbaNote::E4,
    MarimbaNote::G4,
    MarimbaNote::A4,
    MarimbaNote::C5,
    MarimbaNote::D5,
    MarimbaNote::E5,
    MarimbaNote::G5,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum HomeMarimbaPhase {
    #[default]
    Hidden,
    FadingIn,
    Visible,
    FadingOut,
}

#[derive(Resource, Debug)]
/// Progress towards - and through - the marimba reveal.
///
/// `note_count` only ever grows within one visit to Home; resetting the state
/// is what makes the Easter egg have to be earned again.
pub(super) struct HomeMarimbaState {
    note_count: u8,
    phase: HomeMarimbaPhase,
    fade_elapsed: f32,
    idle_elapsed: f32,
    highlights: [f32; HOME_MARIMBA_BAR_COUNT],
}

impl Default for HomeMarimbaState {
    fn default() -> Self {
        Self {
            note_count: 0,
            phase: HomeMarimbaPhase::Hidden,
            fade_elapsed: 0.0,
            idle_elapsed: 0.0,
            highlights: [0.0; HOME_MARIMBA_BAR_COUNT],
        }
    }
}

#[derive(Component)]
pub(super) struct HomeMarimbaVisual;

#[derive(Component)]
pub(super) struct HomeMarimbaBar(pub(super) usize);

pub(super) fn spawn_home_marimba(commands: &mut Commands, stance: MarimbaStance) {
    let mut root = Node {
        position_type: PositionType::Absolute,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    place_marimba(stance, &mut root);
    commands
        .spawn((
            root,
            Visibility::Hidden,
            GlobalZIndex(3),
            HomeMarimbaVisual,
            DespawnOnExit(AppScreen::Home),
        ))
        .with_children(|marimba| {
            for index in 0..HOME_MARIMBA_BAR_COUNT {
                let mut bar = Node {
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                };
                place_bar(index, stance, &mut bar);
                marimba.spawn((
                    bar,
                    BackgroundColor(marimba_bar(0.0, 0.0)),
                    BorderColor::all(marimba_border(0.0)),
                    HomeMarimbaBar(index),
                ));
            }
        });
}

/// Writes where the instrument's root stands, and which way its bars run,
/// for `stance`. Each stance sets every field it owns, so a marimba turned
/// upright and back is the one it was.
fn place_marimba(stance: MarimbaStance, node: &mut Node) {
    let top_left = stance.top_left();
    node.left = px(top_left.x);
    node.top = px(top_left.y);
    match stance {
        MarimbaStance::UnderPanel(_) => {
            node.width = px(MARIMBA_LENGTH);
            node.height = px(MARIMBA_DEPTH);
            node.padding = UiRect::axes(px(PADDING_ALONG), px(PADDING_ACROSS));
            node.flex_direction = FlexDirection::Row;
            node.column_gap = px(BAR_GAP);
            node.row_gap = Val::ZERO;
        }
        MarimbaStance::Upright(_) => {
            node.width = px(MARIMBA_DEPTH);
            node.height = px(MARIMBA_LENGTH);
            node.padding = UiRect::axes(px(PADDING_ACROSS), px(PADDING_ALONG));
            // The lowest bar first, at the bottom.
            node.flex_direction = FlexDirection::ColumnReverse;
            node.column_gap = Val::ZERO;
            node.row_gap = px(BAR_GAP);
        }
    }
}

/// Writes one bar's size for `stance`: standing across the instrument
/// under the panel, lying across it upright.
fn place_bar(index: usize, stance: MarimbaStance, node: &mut Node) {
    let length = px(LOWEST_BAR_LENGTH - index as f32 * BAR_LENGTH_STEP);
    let breadth = px(BAR_BREADTH);
    (node.width, node.height) = match stance {
        MarimbaStance::UnderPanel(_) => (breadth, length),
        MarimbaStance::Upright(_) => (length, breadth),
    };
}

/// The instrument's root and its bars, for a pass that moves them when the
/// window's size changes.
#[derive(SystemParam)]
pub(super) struct MarimbaNodes<'w, 's> {
    roots: Query<'w, 's, &'static mut Node, (With<HomeMarimbaVisual>, Without<HomeMarimbaBar>)>,
    bars: Query<'w, 's, (&'static HomeMarimbaBar, &'static mut Node), Without<HomeMarimbaVisual>>,
}

impl MarimbaNodes<'_, '_> {
    /// Stands the instrument as `stance` says, writing only what changes.
    pub(super) fn place(&mut self, stance: MarimbaStance) {
        for mut root in &mut self.roots {
            let mut placed = root.clone();
            place_marimba(stance, &mut placed);
            if *root != placed {
                *root = placed;
            }
        }
        for (bar, mut node) in &mut self.bars {
            let mut placed = node.clone();
            place_bar(bar.0, stance, &mut placed);
            if *node != placed {
                *node = placed;
            }
        }
    }
}

fn handle_home_marimba_input(
    mut keyboard_events: MessageReader<KeyboardInput>,
    screen: Res<State<AppScreen>>,
    hud: Res<HudState>,
    mut marimba: ResMut<HomeMarimbaState>,
    mut audio: ResMut<AudioCueQueue>,
) {
    // The arrival frame is not a note frame: a `PreUpdate` screen-change
    // writer (the Match exit's confirmed return) applies its transition
    // before `Update`, so this frame's messages can carry presses made on
    // the previous screen - a gameplay key held into the confirm would
    // otherwise ring a bar the instant Home appears. `is_changed` marks
    // exactly the transition frame (and the app's first frame, where
    // skipping is harmless); the messages are still drained below.
    let accepts_notes =
        *screen.get() == AppScreen::Home && !modal_open(&hud) && !screen.is_changed();
    for event in keyboard_events.read() {
        if event.state != ButtonState::Pressed || !accepts_notes {
            continue;
        }
        let Some(bar) = home_marimba_bar_for_key(event.key_code) else {
            continue;
        };
        register_home_marimba_note(&mut marimba, bar);
        audio.ui(UiCue::Marimba(HOME_MARIMBA_NOTES[bar]));
    }
}

/// Maps a note key to one of the eleven bars.
///
/// Each physical keyboard row is spread proportionally across the whole
/// scale, so every row plays low-to-high regardless of how many keys it has.
/// Keys outside the four rows are not notes.
fn home_marimba_bar_for_key(key: KeyCode) -> Option<usize> {
    const NUMBER_ROW: [KeyCode; 10] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    const TOP_ROW: [KeyCode; 10] = [
        KeyCode::KeyQ,
        KeyCode::KeyW,
        KeyCode::KeyE,
        KeyCode::KeyR,
        KeyCode::KeyT,
        KeyCode::KeyY,
        KeyCode::KeyU,
        KeyCode::KeyI,
        KeyCode::KeyO,
        KeyCode::KeyP,
    ];
    const HOME_ROW: [KeyCode; 9] = [
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
    ];
    const BOTTOM_ROW: [KeyCode; 7] = [
        KeyCode::KeyZ,
        KeyCode::KeyX,
        KeyCode::KeyC,
        KeyCode::KeyV,
        KeyCode::KeyB,
        KeyCode::KeyN,
        KeyCode::KeyM,
    ];

    [NUMBER_ROW.as_slice(), &TOP_ROW, &HOME_ROW, &BOTTOM_ROW]
        .into_iter()
        .find_map(|row| {
            row.iter()
                .position(|candidate| *candidate == key)
                .map(|position| {
                    let intervals = row.len() - 1;
                    (position * (HOME_MARIMBA_BAR_COUNT - 1) + intervals / 2) / intervals
                })
        })
}

/// Counts a played note and advances the reveal.
///
/// A note during the fade-out reverses it *from the current alpha* instead of
/// restarting the fade-in, so returning to the keyboard never makes the bars
/// flash.
fn register_home_marimba_note(marimba: &mut HomeMarimbaState, bar: usize) {
    marimba.note_count = marimba.note_count.saturating_add(1);
    if marimba.phase == HomeMarimbaPhase::Hidden {
        if marimba.note_count < HOME_MARIMBA_REVEAL_COUNT {
            return;
        }
        marimba.phase = HomeMarimbaPhase::FadingIn;
        marimba.fade_elapsed = 0.0;
    } else if marimba.phase == HomeMarimbaPhase::FadingOut {
        let current_alpha = home_marimba_alpha(marimba);
        marimba.phase = HomeMarimbaPhase::FadingIn;
        marimba.fade_elapsed = current_alpha * HOME_MARIMBA_FADE_IN_SECONDS;
    }
    marimba.idle_elapsed = 0.0;
    marimba.highlights[bar] = HOME_MARIMBA_HIGHLIGHT_SECONDS;
}

pub(super) fn reset_home_marimba(marimba: &mut HomeMarimbaState) {
    *marimba = HomeMarimbaState::default();
}

fn advance_home_marimba_state(marimba: &mut HomeMarimbaState, delta_seconds: f32) {
    let delta_seconds = delta_seconds.max(0.0);
    for highlight in &mut marimba.highlights {
        *highlight = (*highlight - delta_seconds).max(0.0);
    }
    match marimba.phase {
        HomeMarimbaPhase::Hidden => return,
        HomeMarimbaPhase::FadingIn => {
            marimba.fade_elapsed =
                (marimba.fade_elapsed + delta_seconds).min(HOME_MARIMBA_FADE_IN_SECONDS);
            marimba.idle_elapsed += delta_seconds;
            if marimba.fade_elapsed >= HOME_MARIMBA_FADE_IN_SECONDS {
                marimba.phase = HomeMarimbaPhase::Visible;
                marimba.fade_elapsed = 0.0;
            }
        }
        HomeMarimbaPhase::Visible => {
            marimba.idle_elapsed += delta_seconds;
        }
        HomeMarimbaPhase::FadingOut => {
            marimba.fade_elapsed += delta_seconds;
            if marimba.fade_elapsed >= HOME_MARIMBA_FADE_OUT_SECONDS {
                *marimba = HomeMarimbaState::default();
            }
            return;
        }
    }
    if marimba.idle_elapsed >= HOME_MARIMBA_IDLE_SECONDS {
        marimba.phase = HomeMarimbaPhase::FadingOut;
        marimba.fade_elapsed = 0.0;
    }
}

fn home_marimba_alpha(marimba: &HomeMarimbaState) -> f32 {
    match marimba.phase {
        HomeMarimbaPhase::Hidden => 0.0,
        HomeMarimbaPhase::FadingIn => {
            (marimba.fade_elapsed / HOME_MARIMBA_FADE_IN_SECONDS).clamp(0.0, 1.0)
        }
        HomeMarimbaPhase::Visible => 1.0,
        HomeMarimbaPhase::FadingOut => {
            1.0 - (marimba.fade_elapsed / HOME_MARIMBA_FADE_OUT_SECONDS).clamp(0.0, 1.0)
        }
    }
}

/// The alpha the instrument is drawn at. With motion reduced, every change
/// is a step, as `app::motion` asks: the instrument is there or it is not,
/// and a fade-in or fade-out in progress is drawn at its end. The state
/// machine still runs the fades underneath, so a note during a fade-out
/// still reverses it, and the instrument still leaves after its idle time.
fn home_marimba_drawn_alpha(marimba: &HomeMarimbaState, motion_reduced: bool) -> f32 {
    if !motion_reduced {
        return home_marimba_alpha(marimba);
    }
    match marimba.phase {
        HomeMarimbaPhase::Hidden | HomeMarimbaPhase::FadingOut => 0.0,
        HomeMarimbaPhase::FadingIn | HomeMarimbaPhase::Visible => 1.0,
    }
}

/// How brightly a struck bar is drawn, from the time its highlight has
/// left. A step with motion reduced - lit for the whole highlight, then
/// not - and a decay otherwise.
fn home_marimba_drawn_highlight(remaining: f32, motion_reduced: bool) -> f32 {
    if motion_reduced {
        f32::from(remaining > 0.0)
    } else {
        remaining / HOME_MARIMBA_HIGHLIGHT_SECONDS
    }
}

fn animate_home_marimba(
    time: Res<Time>,
    motion: Motion,
    mut marimba: ResMut<HomeMarimbaState>,
    mut visual: Query<&mut Visibility, With<HomeMarimbaVisual>>,
    mut bars: Query<(&HomeMarimbaBar, &mut BackgroundColor, &mut BorderColor)>,
) {
    advance_home_marimba_state(&mut marimba, time.delta_secs());
    let motion_reduced = motion.reduced();
    let alpha = home_marimba_drawn_alpha(&marimba, motion_reduced);
    for mut visibility in &mut visual {
        *visibility = if marimba.phase == HomeMarimbaPhase::Hidden {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
    for (bar, mut background, mut border) in &mut bars {
        let highlight = home_marimba_drawn_highlight(marimba.highlights[bar.0], motion_reduced);
        background.0 = marimba_bar(alpha, highlight);
        *border = BorderColor::all(marimba_border(alpha));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{WindowSize, WorldFraming};
    use crate::audio;
    use crate::gameplay::{GameState, InputGate};
    use crate::settings::{GamePreferences, VolumeLevel};
    use crate::test_support::send_keyboard_event;
    use crate::ui::home::{prepare_home_screen, reset_home_screen_resources, spawn_home_screen};
    use bevy::input::ButtonState;

    fn home_marimba_input_app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .add_message::<KeyboardInput>()
            .init_state::<AppScreen>()
            .insert_resource(HudState::default())
            .insert_resource(HomeMarimbaState::default())
            .insert_resource(AudioCueQueue::default())
            .add_systems(Update, handle_home_marimba_input);
        app
    }

    #[test]
    fn home_marimba_mapping_tracks_physical_rows_and_filters_non_notes() {
        let rows = [
            [
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
                KeyCode::Digit9,
                KeyCode::Digit0,
            ]
            .as_slice(),
            &[
                KeyCode::KeyQ,
                KeyCode::KeyW,
                KeyCode::KeyE,
                KeyCode::KeyR,
                KeyCode::KeyT,
                KeyCode::KeyY,
                KeyCode::KeyU,
                KeyCode::KeyI,
                KeyCode::KeyO,
                KeyCode::KeyP,
            ],
            &[
                KeyCode::KeyA,
                KeyCode::KeyS,
                KeyCode::KeyD,
                KeyCode::KeyF,
                KeyCode::KeyG,
                KeyCode::KeyH,
                KeyCode::KeyJ,
                KeyCode::KeyK,
                KeyCode::KeyL,
            ],
            &[
                KeyCode::KeyZ,
                KeyCode::KeyX,
                KeyCode::KeyC,
                KeyCode::KeyV,
                KeyCode::KeyB,
                KeyCode::KeyN,
                KeyCode::KeyM,
            ],
        ];
        for row in &rows {
            let mapped = row
                .iter()
                .map(|key| home_marimba_bar_for_key(*key).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(mapped.first(), Some(&0));
            assert_eq!(mapped.last(), Some(&(HOME_MARIMBA_BAR_COUNT - 1)));
            assert!(mapped.windows(2).all(|pair| pair[0] <= pair[1]));
        }
        assert_eq!(
            rows.iter()
                .copied()
                .flat_map(|row| row.iter())
                .filter_map(|key| home_marimba_bar_for_key(*key))
                .collect::<std::collections::HashSet<_>>()
                .len(),
            HOME_MARIMBA_BAR_COUNT
        );
        // The first ten are the seven menu focus keys (`crate::ui::focus`
        // reads Enter, Space, Tab and the four arrows raw on Home) plus
        // Escape and F1, which close and open the help modal there, and
        // F11, which switches fullscreen on every screen: none of them may
        // ever become a note.
        for key in [
            KeyCode::Space,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Escape,
            KeyCode::F1,
            KeyCode::F11,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::Backquote,
            KeyCode::Minus,
            KeyCode::ShiftLeft,
            KeyCode::Numpad1,
        ] {
            assert_eq!(home_marimba_bar_for_key(key), None);
        }
    }

    #[test]
    fn home_marimba_uses_pressed_messages_counts_repeats_and_respects_gates() {
        let mut app = home_marimba_input_app();
        // One warm-up frame: the state resource counts as changed on the
        // app's first frame, and a changed frame accepts no notes.
        app.update();
        send_keyboard_event(&mut app, KeyCode::KeyA, ButtonState::Pressed, false);
        app.update();
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 1);
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Ui(UiCue::Marimba(MarimbaNote::A3))]
        );

        send_keyboard_event(&mut app, KeyCode::KeyA, ButtonState::Pressed, true);
        send_keyboard_event(&mut app, KeyCode::KeyA, ButtonState::Released, false);
        send_keyboard_event(&mut app, KeyCode::Space, ButtonState::Pressed, false);
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 2);
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![audio::AudioCue::Ui(UiCue::Marimba(MarimbaNote::A3))]
        );

        app.world_mut().resource_mut::<HudState>().help_open = true;
        send_keyboard_event(&mut app, KeyCode::KeyS, ButtonState::Pressed, false);
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 2);

        app.world_mut().resource_mut::<HudState>().help_open = false;
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ControlMapping);
        app.update();
        send_keyboard_event(&mut app, KeyCode::KeyD, ButtonState::Pressed, false);
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 2);
        assert!(
            app.world_mut()
                .resource_mut::<AudioCueQueue>()
                .drain()
                .is_empty()
        );
    }

    /// The Match exit's confirmed return transitions in `PreUpdate`, so the
    /// arrival frame's messages can hold presses made during the match -
    /// every note row doubles as a gameplay key there. The handler must
    /// drain that frame without counting it, and count again from the next.
    #[test]
    fn notes_landing_on_the_arrival_frame_are_drained_but_not_counted() {
        let mut app = home_marimba_input_app();
        app.update();
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ControlMapping);
        app.update();

        // The transition back to Home and the note key land in the same
        // frame, which is exactly what Update sees after a PreUpdate
        // confirm: the screen already reads Home while the message was
        // pressed elsewhere.
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Home);
        send_keyboard_event(&mut app, KeyCode::KeyE, ButtonState::Pressed, false);
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 0);

        send_keyboard_event(&mut app, KeyCode::KeyE, ButtonState::Pressed, false);
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 1);
    }

    #[test]
    fn home_marimba_reveals_on_twentieth_press_and_mute_does_not_change_progress() {
        let mut marimba = HomeMarimbaState::default();
        let preferences = GamePreferences {
            ui_sound_effects: VolumeLevel::Mute,
            ..default()
        };
        for _ in 0..HOME_MARIMBA_REVEAL_COUNT - 1 {
            register_home_marimba_note(&mut marimba, 4);
        }
        assert_eq!(marimba.note_count, 19);
        assert_eq!(marimba.phase, HomeMarimbaPhase::Hidden);
        assert_eq!(home_marimba_alpha(&marimba), 0.0);

        register_home_marimba_note(&mut marimba, 4);
        assert_eq!(preferences.ui_sound_effects.gain(), 0.0);
        assert_eq!(marimba.note_count, 20);
        assert_eq!(marimba.phase, HomeMarimbaPhase::FadingIn);
        assert_eq!(marimba.highlights[4], HOME_MARIMBA_HIGHLIGHT_SECONDS);
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_FADE_IN_SECONDS / 2.0);
        assert!((home_marimba_alpha(&marimba) - 0.5).abs() < f32::EPSILON);
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_FADE_IN_SECONDS / 2.0);
        assert_eq!(marimba.phase, HomeMarimbaPhase::Visible);
        assert_eq!(home_marimba_alpha(&marimba), 1.0);
    }

    /// With motion reduced the instrument is drawn in steps: a fade in
    /// progress is drawn at its end, and a struck bar is lit for its whole
    /// highlight and then not, while the state machine underneath keeps
    /// the same timing - so the instrument still leaves after its idle
    /// time and a note during a fade-out still brings it back.
    #[test]
    fn reduced_motion_draws_the_marimba_in_steps_without_changing_its_timing() {
        let mut marimba = HomeMarimbaState {
            note_count: HOME_MARIMBA_REVEAL_COUNT - 1,
            ..default()
        };
        assert_eq!(home_marimba_drawn_alpha(&marimba, true), 0.0);
        register_home_marimba_note(&mut marimba, 4);
        assert_eq!(marimba.phase, HomeMarimbaPhase::FadingIn);
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_FADE_IN_SECONDS / 2.0);
        assert!(home_marimba_drawn_alpha(&marimba, false) < 1.0);
        assert_eq!(home_marimba_drawn_alpha(&marimba, true), 1.0);

        assert_eq!(
            home_marimba_drawn_highlight(marimba.highlights[4], true),
            1.0
        );
        assert!(home_marimba_drawn_highlight(marimba.highlights[4], false) < 1.0);
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_HIGHLIGHT_SECONDS);
        assert_eq!(
            home_marimba_drawn_highlight(marimba.highlights[4], true),
            0.0
        );
        assert_eq!(
            home_marimba_drawn_highlight(marimba.highlights[4], false),
            0.0
        );

        marimba.phase = HomeMarimbaPhase::FadingOut;
        marimba.fade_elapsed = HOME_MARIMBA_FADE_OUT_SECONDS / 2.0;
        assert!(home_marimba_drawn_alpha(&marimba, false) > 0.0);
        assert_eq!(home_marimba_drawn_alpha(&marimba, true), 0.0);
        register_home_marimba_note(&mut marimba, 1);
        assert_eq!(marimba.phase, HomeMarimbaPhase::FadingIn);
        assert_eq!(home_marimba_drawn_alpha(&marimba, true), 1.0);
    }

    #[test]
    fn home_marimba_highlights_overlap_retrigger_decay_and_interrupt_fade_out() {
        let mut marimba = HomeMarimbaState {
            note_count: HOME_MARIMBA_REVEAL_COUNT,
            phase: HomeMarimbaPhase::Visible,
            ..default()
        };
        register_home_marimba_note(&mut marimba, 2);
        advance_home_marimba_state(&mut marimba, 0.2);
        let decayed = marimba.highlights[2];
        register_home_marimba_note(&mut marimba, 2);
        register_home_marimba_note(&mut marimba, 8);
        assert!(marimba.highlights[2] > decayed);
        assert_eq!(marimba.highlights[8], HOME_MARIMBA_HIGHLIGHT_SECONDS);
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_HIGHLIGHT_SECONDS);
        assert_eq!(marimba.highlights[2], 0.0);
        assert_eq!(marimba.highlights[8], 0.0);

        marimba.phase = HomeMarimbaPhase::FadingOut;
        marimba.fade_elapsed = HOME_MARIMBA_FADE_OUT_SECONDS / 2.0;
        let fading_alpha = home_marimba_alpha(&marimba);
        register_home_marimba_note(&mut marimba, 5);
        assert_eq!(marimba.phase, HomeMarimbaPhase::FadingIn);
        assert!((home_marimba_alpha(&marimba) - fading_alpha).abs() < f32::EPSILON);
        assert_eq!(marimba.idle_elapsed, 0.0);
    }

    #[test]
    fn home_marimba_idle_and_navigation_reset_the_hidden_threshold() {
        let mut marimba = HomeMarimbaState::default();
        for _ in 0..HOME_MARIMBA_REVEAL_COUNT {
            register_home_marimba_note(&mut marimba, 6);
        }
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_IDLE_SECONDS - 0.1);
        assert_ne!(marimba.phase, HomeMarimbaPhase::FadingOut);
        register_home_marimba_note(&mut marimba, 6);
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_IDLE_SECONDS - 0.1);
        assert_ne!(marimba.phase, HomeMarimbaPhase::FadingOut);
        advance_home_marimba_state(&mut marimba, 0.1);
        assert_eq!(marimba.phase, HomeMarimbaPhase::FadingOut);
        advance_home_marimba_state(&mut marimba, HOME_MARIMBA_FADE_OUT_SECONDS);
        assert_eq!(marimba.phase, HomeMarimbaPhase::Hidden);
        assert_eq!(marimba.note_count, 0);
        assert!(marimba.highlights.iter().all(|highlight| *highlight == 0.0));

        marimba.note_count = 19;
        marimba.phase = HomeMarimbaPhase::FadingIn;
        marimba.idle_elapsed = 4.0;
        reset_home_marimba(&mut marimba);
        assert_eq!(marimba.note_count, 0);
        assert_eq!(marimba.phase, HomeMarimbaPhase::Hidden);
        assert_eq!(marimba.idle_elapsed, 0.0);
    }

    #[test]
    fn home_marimba_visual_has_eleven_textless_bars_and_starts_hidden() {
        let mut app = App::new();
        app.init_resource::<WindowSize>()
            .init_resource::<WorldFraming>()
            .add_systems(Update, spawn_home_screen);
        app.update();

        let world = app.world_mut();
        let mut roots = world.query_filtered::<&Visibility, With<HomeMarimbaVisual>>();
        assert_eq!(roots.iter(world).collect::<Vec<_>>(), [&Visibility::Hidden]);
        let mut bars = world.query::<(&HomeMarimbaBar, Option<&Text>, &Node)>();
        let bars = bars.iter(world).collect::<Vec<_>>();
        assert_eq!(bars.len(), HOME_MARIMBA_BAR_COUNT);
        assert!(bars.iter().all(|(_, text, _)| text.is_none()));
        assert!(bars.windows(2).all(|pair| {
            let Val::Px(left) = pair[0].2.height else {
                return false;
            };
            let Val::Px(right) = pair[1].2.height else {
                return false;
            };
            left > right
        }));
    }

    #[test]
    fn home_marimba_cleans_and_respawns_once_across_navigation() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(GameState::default())
            .insert_resource(HudState::default())
            .insert_resource(InputGate::default())
            .insert_resource(HomeMarimbaState::default())
            .init_resource::<WindowSize>()
            .init_resource::<WorldFraming>()
            .add_systems(
                OnEnter(AppScreen::Home),
                (prepare_home_screen, spawn_home_screen).chain(),
            )
            .add_systems(OnExit(AppScreen::Home), reset_home_screen_resources);
        app.update();

        let visual_count = |world: &mut World| {
            world
                .query_filtered::<Entity, With<HomeMarimbaVisual>>()
                .iter(world)
                .count()
        };
        assert_eq!(visual_count(app.world_mut()), 1);
        app.world_mut()
            .resource_mut::<HomeMarimbaState>()
            .note_count = HOME_MARIMBA_REVEAL_COUNT;

        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ControlMapping);
        app.update();
        assert_eq!(visual_count(app.world_mut()), 0);
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 0);

        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Home);
        app.update();
        assert_eq!(visual_count(app.world_mut()), 1);
        assert_eq!(app.world().resource::<HomeMarimbaState>().note_count, 0);
    }
}
