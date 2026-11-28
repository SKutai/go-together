//! The game's window: how it is made, where and how large it opens, and
//! fullscreen.
//!
//! # The window
//!
//! [`primary_window`] makes the one window the game draws into from what
//! the two hosts do differently ([`WindowPlatformConfig`]) and what they
//! share: the title, that the window can be resized, and its smallest
//! size, [`MIN_WINDOW_WIDTH`] by [`MIN_WINDOW_HEIGHT`] logical pixels,
//! which every screen is laid out for (`app::visuals`). A window can still
//! end up smaller - fullscreen on a screen smaller than that, or under a
//! window manager that ignores the minimum - and `app::visuals` then draws
//! the minimum's layout scaled down to fit it. In a browser the window is
//! the page's canvas, which `web/shell.js` sizes to fill the page and never
//! lays out smaller than the minimum; winit writes the minimum on the
//! canvas too, as its CSS `min-width` and `min-height`. Nothing about the
//! window is remembered from one run to the next.
//!
//! # Where it opens
//!
//! A desktop window opens at [`WINDOW_WIDTH`] by [`WINDOW_HEIGHT`], centred
//! on the primary monitor when that monitor has room for it, and maximised
//! when it has not. Which is [`opening_for`]'s to say, from the monitor's
//! logical size alone, because winit reports nothing about the bars a
//! desktop keeps for itself - a taskbar, a Dock, a menu bar - only each
//! monitor's size and scale factor. So "room" is the window's size plus
//! [`OPENING_ALLOWANCE`] for those bars, an estimate whose sources are on
//! the constant.
//!
//! Bevy makes the window before it knows the monitors, so the window opens
//! hidden, and [`place_the_opening_window`] places it on the first frame,
//! when they are known, and then shows it: centred, at once; maximised,
//! once the maximised size has arrived, so that no frame is drawn at a size
//! the window is about to leave. Where no maximised size comes within
//! [`MAXIMISE_PATIENCE`] - a desktop with no window manager sends none, and
//! an X11 window manager ignores a request from a window not yet mapped -
//! the window is shown at the opening size and asked again on the next
//! frame, once it is shown, and on that path it is drawn at the opening
//! size until the maximised size arrives, if it ever does. Centring is
//! Bevy's: it puts the window's frame where its inside would stand centred,
//! so the window stands a title bar below the middle.
//!
//! With no primary monitor known the window cannot be centred, but it can
//! still be maximised: it is where every monitor listed is short of room,
//! and otherwise it is shown at the opening size where the platform puts
//! it ([`opening_on`]). Wayland is that case: winit names no primary
//! monitor there, though it lists the outputs, and a window cannot be
//! positioned, only maximised - nor hidden, so there its first frames are drawn
//! at the opening size until the maximised size arrives. Wayland has not been
//! tried with this game. An X server whose RandR lists no monitor, where
//! winit's only answer is a 1x1 stand-in it never lists, and a Mac whose
//! display is asleep as the game starts, when macOS lists no active display at
//! all (seen on 2026-11-28), list none, and the window is shown where the
//! platform puts it. A browser's canvas is neither hidden nor placed: the page
//! puts it where it stands.
//!
//! # Fullscreen
//!
//! F11 switches a desktop window to fullscreen and back, on every screen
//! and whatever has the keys: a modal, a binding capture or a text field.
//! It is reserved (`crate::input::keyboard`), so it never binds, types or
//! plays a note, and it is read as `just_pressed`, as F1 is, so a held key
//! switches once. A browser keeps F11 for its own fullscreen, and on a Mac
//! Control-Command-F too: the page hands either to the browser before the game
//! sees it, and the game reads nothing of them there either
//! ([`WindowPlatformConfig::fullscreen_key`]).
//!
//! A Mac with its default settings keeps F11 itself: the key alone turns
//! the volume down, and with fn it is Show Desktop's shortcut, which takes
//! it first (seen on 2026-11-28). So on a Mac, and only there
//! ([`WindowPlatformConfig::fullscreen_chord`]), Control-Command-F -
//! macOS's own fullscreen shortcut - switches fullscreen as F11 does, once
//! a press. F stays an ordinary key: a chord is no binding, and a plain F,
//! or F with other modifiers, is what it always was. But the chord's F is
//! the chord's alone. [`take_the_fullscreen_chord`] takes it out of
//! `ButtonInput` from its press until it is let go, so no binding, capture or
//! ring sees it, and the three that read keys as messages - the Home marimba,
//! the Replay Library's text fields and Settings' numeric field - ask
//! [`FullscreenChord::takes`] first, so neither the chord's F nor its repeats,
//! once Control and Command are let go before it, play or type anything. A loss
//! of the keyboard focus ends the hold. During an armed binding capture the
//! chord's Control is taken as the binding before its F arrives
//! (`crate::input::keyboard` says why that stands).
//!
//! Fullscreen is borderless, on the monitor the window is on, and going back
//! restores the window as it was. That is the platform's to do; on macOS it
//! did, for a 1280x720 window taken to fullscreen and back by F11 and by the
//! green button, and on Windows for a maximised window taken there and back by
//! F11 (both on 2026-11-28). On macOS borderless fullscreen is the green
//! button's own, a Space of its own, so F11 and the button are one fullscreen
//! and either leaves what the other entered. But Bevy never writes
//! `Window::mode` back from the system, so after the button it would still say
//! windowed, and an F11 that read it would ask for the fullscreen already
//! shown - and, since Bevy applies a mode only when it differs from the last
//! one it applied, nothing at all. So the system's own state is read from winit
//! every frame into [`SystemFullscreen`], `Window::mode` is kept in step with
//! it whenever it changes, and F11 asks for the other of the system's two
//! states ([`mode_after_f11`]), never of `Window::mode`'s. Only a change is
//! followed, not a difference: winit reports a platform's fullscreen once the
//! platform has taken it up, which can be a frame or more after it was asked
//! for - on macOS F11's fullscreen was reported a frame late, and the button's
//! way out not until its animation had ended, most of a second - and following
//! the old state meanwhile would undo what F11 asked for before it took effect.

use std::time::Duration;

use bevy::ecs::system::NonSendMarker;
use bevy::input::keyboard::{KeyboardFocusLost, KeyboardInput};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, PrimaryMonitor, PrimaryWindow, WindowMode, WindowPosition,
    WindowResizeConstraints, WindowResolution,
};

use super::visuals::{
    MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, WINDOW_HEIGHT, WINDOW_WIDTH, WindowSize,
};
use crate::platform::WindowPlatformConfig;

/// How much more than the opening window a monitor needs, in logical
/// pixels, to have room for it: nothing across, and 160 down. Bevy centres
/// a window by its inside's size but places its frame there: the frame's
/// top-left corner, title bar and all, goes where the inside's would be if
/// the inside were centred on the whole screen. So the window stands a
/// title bar below the middle, with half the spare height above it and
/// half less the title bar below it, and it clears a bar along the
/// screen's bottom edge only where the spare height holds twice the title
/// bar and the bar, and one along the top edge where it holds twice the
/// bar. On Windows 11 that is 2 x (32 + 48): its title bar is 32 px
/// (Microsoft's title bar design guidance,
/// <https://learn.microsoft.com/en-us/windows/apps/design/basics/titlebar-design>),
/// and its taskbar 48 px, measured on 2026-11-28 as the difference between
/// a Windows 11 24H2 screen and its work area. On macOS 26 a title bar is
/// 32 pt and the menu bar of a display with a camera housing 33 pt, as
/// measured on a 15-inch MacBook Air the same day, where a centred
/// 1040x900 window's frame stood 104 pt down a 1107 pt screen: so 160 also
/// clears that menu bar above and a Dock up to 48 pt below, and one shown
/// larger was not measured. Across, the bars stand above and below a
/// window unless a dock is moved to a side, and every common screen is
/// wide enough to centre the window clear of one: the narrowest, 1280
/// wide, leaves 120 a side. An estimate, then, from those sources; with
/// an allowance of 100 px instead, a window centred on a 1036-px-tall
/// Windows screen had its bottom 9 px under the taskbar, as measured.
pub(crate) const OPENING_ALLOWANCE: Vec2 = Vec2::new(0.0, 160.0);

/// How long a window that asked to be maximised stays hidden waiting for
/// its new size before it is shown anyway, and asked again: half a second,
/// a guess, far longer than any platform takes to answer and short enough
/// not to be mistaken for a game that failed to start.
pub(crate) const MAXIMISE_PATIENCE: Duration = Duration::from_millis(500);

/// The window the game draws into, as it is created: the host's
/// differences from `platform`, and the title, resizing and minimum both
/// hosts share. A window the game places on its monitor opens hidden; see
/// the module docs.
pub(crate) fn primary_window(platform: &WindowPlatformConfig) -> Window {
    let (width, height) = platform
        .opening_size
        .unwrap_or((WINDOW_WIDTH, WINDOW_HEIGHT));
    Window {
        title: "Go Together".into(),
        resolution: WindowResolution::new(width, height),
        resizable: true,
        resize_constraints: WindowResizeConstraints {
            min_width: MIN_WINDOW_WIDTH as f32,
            min_height: MIN_WINDOW_HEIGHT as f32,
            ..default()
        },
        visible: !platform.placed_on_a_monitor,
        canvas: platform.canvas.clone(),
        fit_canvas_to_parent: platform.fit_canvas_to_parent,
        prevent_default_event_handling: platform.prevent_default_event_handling,
        ..default()
    }
}

/// How a desktop window opens on a monitor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Opening {
    /// At the opening size, centred on the monitor.
    Centred,
    /// Maximised, on a monitor without room for the opening size.
    Maximised,
}

/// How the window opens on a monitor `monitor` logical pixels in size:
/// centred where the monitor is at least the opening size plus
/// [`OPENING_ALLOWANCE`] in both directions, 1040x1060, and maximised
/// where it falls short in either.
pub(crate) fn opening_for(monitor: Vec2) -> Opening {
    let room = WindowSize::OPENING.0 + OPENING_ALLOWANCE;
    if monitor.x >= room.x && monitor.y >= room.y {
        Opening::Centred
    } else {
        Opening::Maximised
    }
}

/// How the window opens from what is known of the monitors, in logical
/// pixels: by the primary monitor where one is named ([`opening_for`]);
/// where none is, maximised if every monitor `listed` is short of room,
/// and otherwise `None`, shown at the opening size where the platform puts
/// it, since a window is centred only on the primary monitor. With no
/// monitor listed either, `None` as well.
pub(crate) fn opening_on(primary: Option<Vec2>, listed: &[Vec2]) -> Option<Opening> {
    match primary {
        Some(primary) => Some(opening_for(primary)),
        None if !listed.is_empty()
            && listed
                .iter()
                .all(|monitor| opening_for(*monitor) == Opening::Maximised) =>
        {
            Some(Opening::Maximised)
        }
        None => None,
    }
}

/// Places the window as it opens and shows it, and switches fullscreen on
/// F11 and, on a Mac, Control-Command-F, each where the host leaves it to
/// the game: the systems of the module docs, in `PreUpdate`.
pub(crate) struct GameWindowPlugin {
    placed_on_a_monitor: bool,
    fullscreen_key: bool,
    fullscreen_chord: bool,
}

impl GameWindowPlugin {
    /// The plugin for the host `platform` describes.
    pub(crate) fn new(platform: &WindowPlatformConfig) -> Self {
        Self {
            placed_on_a_monitor: platform.placed_on_a_monitor,
            fullscreen_key: platform.fullscreen_key,
            fullscreen_chord: platform.fullscreen_chord,
        }
    }
}

impl Plugin for GameWindowPlugin {
    fn build(&self, app: &mut App) {
        if self.placed_on_a_monitor {
            app.init_resource::<WindowOpening>().add_systems(
                PreUpdate,
                place_the_opening_window
                    .run_if(|opening: Res<WindowOpening>| *opening != WindowOpening::Shown),
            );
        }
        if self.fullscreen_key {
            app.init_resource::<SystemFullscreen>().add_systems(
                PreUpdate,
                (
                    read_fullscreen_from_the_system,
                    keep_the_window_mode_with_the_system
                        .run_if(resource_changed::<SystemFullscreen>),
                    take_the_fullscreen_chord.run_if(resource_exists::<FullscreenChord>),
                    switch_fullscreen_on_its_keys,
                )
                    .chain()
                    .after(InputSystems),
            );
            if self.fullscreen_chord {
                app.init_resource::<FullscreenChord>()
                    .add_message::<KeyboardInput>()
                    .add_message::<KeyboardFocusLost>();
            }
        }
    }
}

/// Where the window is in being placed as it opens.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum WindowOpening {
    /// Not yet placed, and hidden.
    #[default]
    Unplaced,
    /// Asked to be maximised `since` that much real time into the run,
    /// when it was `from` logical pixels, and hidden until it is not.
    Maximising { since: Duration, from: Vec2 },
    /// Shown at the size it had once [`MAXIMISE_PATIENCE`] passed with no
    /// maximised size, and to be asked again on the next frame: Bevy
    /// applies a maximise request before it shows a window, so one made
    /// in the frame the window is shown would still reach it hidden.
    AskAgain,
    /// Placed and shown; nothing more to do.
    Shown,
}

/// Places the hidden window on the first frame, from the monitors' logical
/// sizes ([`opening_on`]), and shows it once it is placed; see the module
/// docs.
pub(crate) fn place_the_opening_window(
    mut opening: ResMut<WindowOpening>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    primary: Query<&Monitor, With<PrimaryMonitor>>,
    monitors: Query<&Monitor>,
    time: Res<Time<Real>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let logical = |monitor: &Monitor| {
        Vec2::new(
            monitor.physical_width as f32,
            monitor.physical_height as f32,
        ) / monitor.scale_factor as f32
    };
    match *opening {
        WindowOpening::Unplaced => {
            let listed: Vec<Vec2> = monitors.iter().map(logical).collect();
            match opening_on(primary.single().ok().map(logical), &listed) {
                Some(Opening::Centred) => {
                    window.position = WindowPosition::Centered(MonitorSelection::Primary);
                    window.visible = true;
                    *opening = WindowOpening::Shown;
                }
                Some(Opening::Maximised) => {
                    window.set_maximized(true);
                    *opening = WindowOpening::Maximising {
                        since: time.elapsed(),
                        from: window.size(),
                    };
                }
                None => {
                    window.visible = true;
                    *opening = WindowOpening::Shown;
                }
            }
        }
        WindowOpening::Maximising { since, from } => {
            if window.size() != from {
                window.visible = true;
                *opening = WindowOpening::Shown;
            } else if time.elapsed().saturating_sub(since) >= MAXIMISE_PATIENCE {
                window.visible = true;
                *opening = WindowOpening::AskAgain;
            }
        }
        WindowOpening::AskAgain => {
            window.set_maximized(true);
            *opening = WindowOpening::Shown;
        }
        WindowOpening::Shown => {}
    }
}

/// Whether the system shows the primary window fullscreen, as winit last
/// reported it: by F11 or, on macOS, by the green button, which Bevy's own
/// `Window::mode` never hears of. Tests, which run without winit, set it
/// as the system would.
#[derive(Resource, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SystemFullscreen(pub(crate) bool);

/// The mode F11 asks for, from whether the system shows the window
/// fullscreen: the other one. Fullscreen is borderless, on the monitor the
/// window is on.
pub(crate) fn mode_after_f11(system_fullscreen: bool) -> WindowMode {
    if system_fullscreen {
        WindowMode::Windowed
    } else {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    }
}

/// Whether `mode` is one of the fullscreen modes.
fn is_fullscreen(mode: WindowMode) -> bool {
    !matches!(mode, WindowMode::Windowed)
}

/// Reads winit's own fullscreen state for the primary window into
/// [`SystemFullscreen`], writing only a change. winit's windows live on the
/// main thread, so this runs there. It leaves the resource alone where
/// there is no winit window, as in a test.
fn read_fullscreen_from_the_system(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut system: ResMut<SystemFullscreen>,
    _main_thread: NonSendMarker,
) {
    let Ok(entity) = windows.single() else {
        return;
    };
    let fullscreen = bevy::winit::WINIT_WINDOWS.with_borrow(|winit| {
        winit
            .get_window(entity)
            .map(|window| window.fullscreen().is_some())
    });
    if let Some(fullscreen) = fullscreen {
        system.set_if_neq(SystemFullscreen(fullscreen));
    }
}

/// When the system's fullscreen state changes, writes `Window::mode` to
/// match it, so that the next mode F11 asks for differs from the last one
/// Bevy applied. Run only on a change; see the module docs for why a
/// difference alone is not followed.
fn keep_the_window_mode_with_the_system(
    system: Res<SystemFullscreen>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    for mut window in &mut windows {
        if is_fullscreen(window.mode) != system.0 {
            window.mode = if system.0 {
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            } else {
                WindowMode::Windowed
            };
        }
    }
}

/// Whether the keys held make an F the fullscreen chord's: Control and
/// Command both, and neither Shift nor Option, as in macOS's own shortcut,
/// Control-Command-F. Command is Bevy's Super key.
pub(crate) fn chord_modifiers_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        && keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight])
        && !keys.any_pressed([
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ])
}

/// Control-Command-F, where it is the game's: whether the F now held went
/// down as the chord, so that nothing else takes it until it is let go, and
/// whether a press of the chord has asked for the other mode. See the
/// module docs.
#[derive(Resource, Debug, Default)]
pub(crate) struct FullscreenChord {
    f_held: bool,
    asked: bool,
}

impl FullscreenChord {
    /// Whether a press of `key`, read as a message, is the chord's and so
    /// for nothing else: an F while the chord's F is held, or an F with
    /// the chord's modifiers held.
    pub(crate) fn takes(&self, key: KeyCode, keys: &ButtonInput<KeyCode>) -> bool {
        key == KeyCode::KeyF && (self.f_held || chord_modifiers_held(keys))
    }

    /// The state the chord leaves while its F is still held, for a test of
    /// a system that reads keys as messages.
    #[cfg(test)]
    pub(crate) fn holding_f() -> Self {
        Self {
            f_held: true,
            asked: false,
        }
    }
}

/// Reads the chord from this frame's key messages, after Bevy's input systems
/// have pressed its F: an F pressed - not a repeat - with the chord's modifiers
/// held asks for the switch, once, and from that press until F is let go
/// `ButtonInput` holds no F, so no binding, capture or ring sees it, not even
/// for a press and release within one frame. A window that loses the keyboard
/// may never hear F let go, so a loss of focus ends the hold too, as it
/// releases every key in `ButtonInput`.
fn take_the_fullscreen_chord(
    mut chord: ResMut<FullscreenChord>,
    mut messages: MessageReader<KeyboardInput>,
    mut focus_lost: MessageReader<KeyboardFocusLost>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    let mut pressed_now = false;
    for message in messages.read() {
        if message.key_code != KeyCode::KeyF {
            continue;
        }
        match message.state {
            ButtonState::Pressed if !message.repeat && chord_modifiers_held(&keys) => {
                chord.f_held = true;
                chord.asked = true;
                pressed_now = true;
            }
            ButtonState::Released => chord.f_held = false,
            ButtonState::Pressed => {}
        }
    }
    if focus_lost.read().count() > 0 {
        chord.f_held = false;
    }
    if chord.f_held || pressed_now {
        keys.reset(KeyCode::KeyF);
    }
}

/// On F11, or the chord where it is the game's, asks for the mode
/// [`mode_after_f11`] gives from the system's state. A press in the frame
/// the system's state changed waits for the next one: that frame's change
/// has just been written into `Window::mode`, and asking for the other mode
/// on top of it would leave the mode Bevy last applied, and so ask for
/// nothing.
fn switch_fullscreen_on_its_keys(
    keys: Res<ButtonInput<KeyCode>>,
    system: Res<SystemFullscreen>,
    chord: Option<ResMut<FullscreenChord>>,
    mut pressed: Local<bool>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    *pressed |= keys.just_pressed(KeyCode::F11);
    if let Some(mut chord) = chord
        && chord.asked
    {
        chord.asked = false;
        *pressed = true;
    }
    if !*pressed || system.is_changed() {
        return;
    }
    *pressed = false;
    for mut window in &mut windows {
        window.mode = mode_after_f11(system.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppScreen;
    use crate::platform::{browser_window_platform_config, window_platform_config};

    /// Both hosts' windows can be resized and are held to the minimum the
    /// layouts are made for, 1040x600, under the game's title. A desktop
    /// window opens at 1040x900, hidden until the game places it; the
    /// browser's opens at the size the page gave its canvas, shown, drawn
    /// into the page's canvas with the browser's default actions kept off
    /// it.
    #[test]
    fn both_hosts_windows_resize_down_to_the_minimum() {
        let native = primary_window(&window_platform_config());
        let browser = primary_window(&browser_window_platform_config(Some((1300, 700))));
        for (host, window) in [("native", &native), ("browser", &browser)] {
            assert!(window.resizable, "{host}");
            assert_eq!(window.resize_constraints.min_width, 1040.0, "{host}");
            assert_eq!(window.resize_constraints.min_height, 600.0, "{host}");
            assert_eq!(window.resize_constraints.max_width, f32::INFINITY, "{host}");
            assert_eq!(
                window.resize_constraints.max_height,
                f32::INFINITY,
                "{host}"
            );
            assert_eq!(window.title, "Go Together", "{host}");
            assert_eq!(window.mode, WindowMode::Windowed, "{host}");
        }
        assert_eq!(native.size(), Vec2::new(1040.0, 900.0));
        assert!(!native.visible, "hidden until it is placed");
        assert_eq!(native.canvas, None);
        assert!(!native.prevent_default_event_handling);
        assert_eq!(browser.size(), Vec2::new(1300.0, 700.0));
        assert!(browser.visible);
        assert_eq!(browser.canvas.as_deref(), Some("#go-together"));
        assert!(browser.prevent_default_event_handling);
        assert!(!browser.fit_canvas_to_parent);
        // A canvas the page could not measure opens at the game's own size.
        assert_eq!(
            primary_window(&browser_window_platform_config(None)).size(),
            Vec2::new(1040.0, 900.0)
        );
    }

    /// Centred on a monitor at least 1040x1060 logical pixels, maximised on
    /// one short of that either way: each threshold on both sides, each
    /// direction on its own, and the common screens.
    #[test]
    fn the_window_opens_centred_where_the_monitor_has_room_and_maximised_where_not() {
        use Opening::{Centred, Maximised};

        assert_eq!(OPENING_ALLOWANCE, Vec2::new(0.0, 160.0));
        for (monitor, expected) in [
            // The thresholds, 1040 across and 1060 down.
            (Vec2::new(1040.0, 1060.0), Centred),
            (Vec2::new(1039.9, 1060.0), Maximised),
            (Vec2::new(1040.0, 1059.9), Maximised),
            (Vec2::new(1039.0, 1440.0), Maximised),
            (Vec2::new(2560.0, 1059.0), Maximised),
            // Common screens: a 1920x1080 one at Windows' 150%, a 1366x768
            // one, a 1440x900 Mac, a 1920x1080 one at 125%, a 1920x1080
            // one, and a 2560x1440 one.
            (Vec2::new(1280.0, 720.0), Maximised),
            (Vec2::new(1366.0, 768.0), Maximised),
            (Vec2::new(1440.0, 900.0), Maximised),
            (Vec2::new(1536.0, 864.0), Maximised),
            (Vec2::new(1920.0, 1080.0), Centred),
            (Vec2::new(2560.0, 1440.0), Centred),
            // Screens just short of the threshold: a 14-inch MacBook
            // Pro's default 1512x982, a 1280x1024 monitor, a Windows
            // screen 1036 tall, where a centred window's bottom would be
            // under the taskbar, and a 1680x1050 one; and a 15-inch
            // MacBook Air's 1710x1107, with room.
            (Vec2::new(1512.0, 982.0), Maximised),
            (Vec2::new(1280.0, 1024.0), Maximised),
            (Vec2::new(1710.0, 1036.0), Maximised),
            (Vec2::new(1680.0, 1050.0), Maximised),
            (Vec2::new(1710.0, 1107.0), Centred),
            // A 2560x1600 laptop at 175% gives a size in fractions.
            (Vec2::new(2560.0, 1600.0) / 1.75, Maximised),
        ] {
            assert_eq!(opening_for(monitor), expected, "{monitor}");
        }
    }

    /// An app with the game's window plugin for `platform` and a primary
    /// window as `primary_window` makes it, with `monitor` - its physical
    /// size and scale factor - as the primary monitor, or none.
    fn opening_app(platform: &WindowPlatformConfig, monitor: Option<(UVec2, f64)>) -> App {
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(GameWindowPlugin::new(platform));
        app.world_mut()
            .spawn((primary_window(platform), PrimaryWindow));
        if let Some((size, scale_factor)) = monitor {
            app.world_mut().spawn((
                Monitor {
                    name: None,
                    physical_width: size.x,
                    physical_height: size.y,
                    physical_position: IVec2::ZERO,
                    refresh_rate_millihertz: None,
                    scale_factor,
                    video_modes: Vec::new(),
                },
                PrimaryMonitor,
            ));
        }
        app
    }

    fn window(app: &mut App) -> Mut<'_, Window> {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows.single_mut(world).unwrap()
    }

    /// On a monitor with room, the window is centred on the primary
    /// monitor and shown on the first frame, at the opening size and not
    /// maximised; where the primary monitor's logical size, at its scale
    /// factor, has no room, it asks to be maximised and stays hidden.
    #[test]
    fn the_first_frame_centres_and_shows_the_window_or_maximises_it_hidden() {
        let native = window_platform_config();
        // 3840x2160 at a scale factor of 2 is 1920x1080: room.
        let mut app = opening_app(&native, Some((UVec2::new(3840, 2160), 2.0)));
        assert!(!window(&mut app).visible);
        app.update();
        let mut shown = window(&mut app);
        assert!(shown.visible);
        assert_eq!(
            shown.position,
            WindowPosition::Centered(MonitorSelection::Primary)
        );
        assert_eq!(shown.size(), Vec2::new(1040.0, 900.0));
        assert_eq!(shown.internal.take_maximize_request(), None);
        assert_eq!(
            *app.world().resource::<WindowOpening>(),
            WindowOpening::Shown
        );

        // 3840x2160 at a scale factor of 3 is 1280x720: no room.
        let mut app = opening_app(&native, Some((UVec2::new(3840, 2160), 3.0)));
        app.update();
        let mut asked = window(&mut app);
        assert!(!asked.visible, "hidden until the maximised size arrives");
        assert_eq!(asked.position, WindowPosition::Automatic);
        assert_eq!(asked.internal.take_maximize_request(), Some(true));
    }

    /// A window waiting to be maximised is shown on the frame its new size
    /// has arrived and is not asked again; one whose size never changes is
    /// shown once [`MAXIMISE_PATIENCE`] has passed - and not a frame
    /// before - and asked again on the frame after, when it has been shown,
    /// never on the frame it is shown in.
    #[test]
    fn a_maximised_window_is_shown_when_its_size_arrives_or_after_a_wait() {
        let native = window_platform_config();
        let small = Some((UVec2::new(1366, 768), 1.0));
        let advance = |app: &mut App, by: Duration| {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(by);
        };

        let mut app = opening_app(&native, small);
        app.update();
        assert_eq!(
            window(&mut app).internal.take_maximize_request(),
            Some(true)
        );
        app.update();
        assert!(!window(&mut app).visible, "nothing has arrived yet");
        // The platform maximises it: 1366x689 under a taskbar and a title
        // bar.
        window(&mut app).resolution.set(1366.0, 689.0);
        app.update();
        let mut arrived = window(&mut app);
        assert!(arrived.visible);
        assert_eq!(arrived.internal.take_maximize_request(), None);
        assert_eq!(
            *app.world().resource::<WindowOpening>(),
            WindowOpening::Shown
        );

        let mut app = opening_app(&native, small);
        app.update();
        window(&mut app).internal.take_maximize_request();
        advance(&mut app, MAXIMISE_PATIENCE - Duration::from_millis(1));
        app.update();
        assert!(!window(&mut app).visible, "still waiting");
        advance(&mut app, Duration::from_millis(1));
        app.update();
        let mut late = window(&mut app);
        assert!(late.visible);
        assert_eq!(late.size(), Vec2::new(1040.0, 900.0));
        assert_eq!(
            late.internal.take_maximize_request(),
            None,
            "not in the frame it is shown in"
        );
        app.update();
        let mut asked = window(&mut app);
        assert!(asked.visible);
        assert_eq!(asked.internal.take_maximize_request(), Some(true));
        assert_eq!(
            *app.world().resource::<WindowOpening>(),
            WindowOpening::Shown
        );
        app.update();
        assert_eq!(
            window(&mut app).internal.take_maximize_request(),
            None,
            "asked once"
        );
    }

    /// With no primary monitor the opening follows every monitor listed:
    /// maximised where all are short of room, as a Wayland laptop's one
    /// output is, and otherwise shown at once, at the opening size, where
    /// the platform puts it - with one that has room, or with none listed
    /// at all, as on a Mac whose display is asleep. Nothing is centred. A
    /// browser's canvas is shown from the start and nothing places it.
    #[test]
    fn with_no_primary_monitor_the_window_follows_every_monitor_listed() {
        let native = window_platform_config();
        let unnamed = |width: u32, height: u32, scale_factor: f64| Monitor {
            name: None,
            physical_width: width,
            physical_height: height,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: None,
            scale_factor,
            video_modes: Vec::new(),
        };

        // Monitors that are not the primary one, as on Wayland: 1366x768
        // and 2560x1600 at 2, 1280x800, both short.
        let mut app = opening_app(&native, None);
        app.world_mut().spawn(unnamed(1366, 768, 1.0));
        app.world_mut().spawn(unnamed(2560, 1600, 2.0));
        app.update();
        let mut asked = window(&mut app);
        assert!(!asked.visible, "hidden until the maximised size arrives");
        assert_eq!(asked.position, WindowPosition::Automatic);
        assert_eq!(asked.internal.take_maximize_request(), Some(true));

        // One of them has room: 3840x2160 at 2 is 1920x1080.
        let mut app = opening_app(&native, None);
        app.world_mut().spawn(unnamed(1366, 768, 1.0));
        app.world_mut().spawn(unnamed(3840, 2160, 2.0));
        app.update();
        let mut shown = window(&mut app);
        assert!(shown.visible);
        assert_eq!(shown.position, WindowPosition::Automatic);
        assert_eq!(shown.size(), Vec2::new(1040.0, 900.0));
        assert_eq!(shown.internal.take_maximize_request(), None);

        // None listed at all.
        let mut app = opening_app(&native, None);
        app.update();
        let mut shown = window(&mut app);
        assert!(shown.visible);
        assert_eq!(shown.position, WindowPosition::Automatic);
        assert_eq!(shown.internal.take_maximize_request(), None);

        let browser = browser_window_platform_config(Some((1300, 700)));
        let mut app = opening_app(&browser, Some((UVec2::new(1366, 768), 1.0)));
        assert!(window(&mut app).visible);
        app.update();
        let mut canvas = window(&mut app);
        assert_eq!(canvas.position, WindowPosition::Automatic);
        assert_eq!(canvas.internal.take_maximize_request(), None);
        assert!(!app.world().contains_resource::<WindowOpening>());
    }

    /// The opening from what is known of the monitors: the primary one
    /// decides wherever it is named, even beside others all short of room;
    /// without it, every monitor listed short of room maximises, and one with
    /// room, or none listed, leaves the window where the platform puts it.
    #[test]
    fn the_opening_follows_the_primary_monitor_or_else_every_monitor() {
        use Opening::{Centred, Maximised};

        let short = Vec2::new(1366.0, 768.0);
        let room = Vec2::new(1920.0, 1080.0);
        assert_eq!(opening_on(Some(room), &[short, room]), Some(Centred));
        assert_eq!(opening_on(Some(room), &[short, short]), Some(Centred));
        assert_eq!(opening_on(Some(short), &[short, room]), Some(Maximised));
        assert_eq!(opening_on(Some(short), &[]), Some(Maximised));
        assert_eq!(opening_on(None, &[short]), Some(Maximised));
        assert_eq!(
            opening_on(None, &[short, Vec2::new(1280.0, 800.0)]),
            Some(Maximised)
        );
        assert_eq!(opening_on(None, &[short, room]), None);
        assert_eq!(opening_on(None, &[room]), None);
        assert_eq!(opening_on(None, &[]), None);
    }

    /// F11 asks for the other of the system's two states, never of
    /// `Window::mode`'s.
    #[test]
    fn f11_asks_for_the_other_of_the_systems_states() {
        assert_eq!(
            mode_after_f11(false),
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
        assert_eq!(mode_after_f11(true), WindowMode::Windowed);
    }

    /// The game's window plugin for `platform`, with the screen state and
    /// the input the screens hold while one of them has the keys, and a
    /// shown, placed primary window.
    fn fullscreen_app(platform: &WindowPlatformConfig) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::ui::hud::HudState>()
            .add_plugins(GameWindowPlugin::new(platform));
        let mut window = primary_window(platform);
        window.visible = true;
        app.world_mut().spawn((window, PrimaryWindow));
        app.insert_resource(WindowOpening::Shown);
        app.update();
        app
    }

    fn press(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.press(key);
    }

    fn release(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.release(key);
    }

    /// What the system shows, as winit would report it after Bevy applied
    /// the window's mode.
    fn system_shows(app: &mut App, fullscreen: bool) {
        app.world_mut()
            .resource_mut::<SystemFullscreen>()
            .set_if_neq(SystemFullscreen(fullscreen));
    }

    fn mode(app: &mut App) -> WindowMode {
        window(app).mode
    }

    /// Through the production plugin and schedule, in every `AppScreen`
    /// state, with neither and with each of the HUD's modal flags set,
    /// each press of F11 asks for fullscreen when the system shows the
    /// window windowed and for the window when it shows it fullscreen, and
    /// a held key asks once. Between presses the test does what the system
    /// would: it shows what was asked. No screen's own plugin is loaded, so
    /// this holds what the window plugin does whatever the state, not what
    /// a screen does with keys: the plugin reads F11 in `PreUpdate`, before
    /// any screen's systems run, and the two places that hold the keys - a
    /// binding capture and a text field - have tests of their own beside
    /// them, with their screens loaded, in `ui::settings` and
    /// `ui::replay::library`.
    #[test]
    fn each_f11_press_asks_for_the_other_mode_on_every_screen() {
        use crate::ui::hud::HudState;

        let fullscreen = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        let mut app = fullscreen_app(&window_platform_config());
        assert_eq!(mode(&mut app), WindowMode::Windowed);
        let mut expected_system = false;
        for screen in AppScreen::ALL {
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(screen);
            app.update();
            for open in ["nothing", "Help", "Return Home"] {
                {
                    let mut hud = app.world_mut().resource_mut::<HudState>();
                    hud.help_open = open == "Help";
                    hud.return_home_open = open == "Return Home";
                }
                let context = format!("{screen:?} with {open} open");
                press(&mut app, KeyCode::F11);
                app.update();
                let asked = mode(&mut app);
                assert_eq!(
                    asked,
                    if expected_system {
                        WindowMode::Windowed
                    } else {
                        fullscreen
                    },
                    "{context}"
                );
                // Held, with the system showing what was asked: no second
                // request, which would be the mode before.
                expected_system = !expected_system;
                system_shows(&mut app, expected_system);
                for _ in 0..4 {
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .clear();
                    app.update();
                    assert_eq!(mode(&mut app), asked, "{context}, held");
                }
                release(&mut app, KeyCode::F11);
                app.update();
            }
        }
    }

    /// The state a green-button entry leaves on macOS: the system shows the
    /// window fullscreen while `Window::mode` still says windowed. The mode
    /// is brought into step, and F11 then asks for the window - and asks
    /// again for fullscreen once the system has left it by the button. A
    /// press in the frame the system's state changes asks a frame later,
    /// for the other of the new state. And a press the system has not yet
    /// followed, as a compositor slow to confirm leaves it, asks for the
    /// other of what the system shows, not of `Window::mode`: fullscreen
    /// again, where the mode already asks for it.
    #[test]
    fn f11_follows_a_fullscreen_the_system_entered_or_left_by_itself() {
        let fullscreen = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        let mut app = fullscreen_app(&window_platform_config());

        // The green button enters fullscreen.
        system_shows(&mut app, true);
        app.update();
        assert_eq!(mode(&mut app), fullscreen, "in step with the system");
        press(&mut app, KeyCode::F11);
        app.update();
        assert_eq!(mode(&mut app), WindowMode::Windowed);
        release(&mut app, KeyCode::F11);
        system_shows(&mut app, false);
        app.update();

        // The green button enters again, and leaves; F11 enters.
        system_shows(&mut app, true);
        app.update();
        system_shows(&mut app, false);
        app.update();
        assert_eq!(mode(&mut app), WindowMode::Windowed, "in step again");
        press(&mut app, KeyCode::F11);
        app.update();
        assert_eq!(mode(&mut app), fullscreen);
        release(&mut app, KeyCode::F11);
        system_shows(&mut app, true);
        app.update();

        // The button leaves in the very frame F11 is pressed: that frame
        // follows the system, and the next asks for the other of it.
        system_shows(&mut app, false);
        press(&mut app, KeyCode::F11);
        app.update();
        assert_eq!(mode(&mut app), WindowMode::Windowed, "the system's");
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert_eq!(mode(&mut app), fullscreen, "the press, a frame later");

        // The system has not followed that press: it still shows the
        // window, so the next press asks for fullscreen once more.
        release(&mut app, KeyCode::F11);
        app.update();
        press(&mut app, KeyCode::F11);
        app.update();
        assert_eq!(
            mode(&mut app),
            fullscreen,
            "the other of what the system shows, the window"
        );
        assert!(!app.world().resource::<SystemFullscreen>().0);
    }

    /// The desktop's window config with the chord on, as on a Mac, or off.
    fn with_the_chord(on: bool) -> WindowPlatformConfig {
        WindowPlatformConfig {
            fullscreen_chord: on,
            ..window_platform_config()
        }
    }

    fn keys(app: &App) -> &ButtonInput<KeyCode> {
        app.world().resource::<ButtonInput<KeyCode>>()
    }

    /// An F going down or up as the keyboard delivers it: its message, and
    /// the `ButtonInput` state Bevy's input systems make of it, which a
    /// repeat presses again.
    fn f_key(app: &mut App, state: ButtonState, repeat: bool) {
        let window = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .single(world)
                .unwrap()
        };
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyF,
            logical_key: bevy::input::keyboard::Key::Character("f".into()),
            state,
            text: None,
            repeat,
            window,
        });
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        match state {
            ButtonState::Pressed => keys.press(KeyCode::KeyF),
            ButtonState::Released => keys.release(KeyCode::KeyF),
        }
    }

    fn hold(app: &mut App, held: &[KeyCode]) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        for key in held {
            keys.press(*key);
        }
    }

    /// Control-Command-F switches fullscreen as F11 does where the chord is
    /// the game's - the desktop on a Mac, and nowhere else - and asks for
    /// the other of the system's states at each press. Without both
    /// Control and Command, or with Shift or Option as well, or where the
    /// chord is not the game's, F is only F: nothing switches, and F stays
    /// pressed for whatever reads it.
    #[test]
    fn control_command_f_switches_fullscreen_on_a_mac_alone() {
        use KeyCode::{AltLeft, ControlLeft, ShiftRight, SuperLeft, SuperRight};

        assert_eq!(
            window_platform_config().fullscreen_chord,
            cfg!(target_os = "macos")
        );
        assert!(!browser_window_platform_config(None).fullscreen_chord);
        let fullscreen = WindowMode::BorderlessFullscreen(MonitorSelection::Current);

        let mut app = fullscreen_app(&with_the_chord(true));
        hold(&mut app, &[ControlLeft, SuperLeft]);
        f_key(&mut app, ButtonState::Pressed, false);
        app.update();
        assert_eq!(mode(&mut app), fullscreen);
        system_shows(&mut app, true);
        f_key(&mut app, ButtonState::Released, false);
        app.update();
        f_key(&mut app, ButtonState::Pressed, false);
        app.update();
        assert_eq!(mode(&mut app), WindowMode::Windowed, "and back");

        for (on, held) in [
            (false, &[ControlLeft, SuperLeft][..]),
            (true, &[ControlLeft][..]),
            (true, &[SuperRight][..]),
            (true, &[ControlLeft, SuperLeft, ShiftRight][..]),
            (true, &[ControlLeft, SuperLeft, AltLeft][..]),
        ] {
            let mut app = fullscreen_app(&with_the_chord(on));
            hold(&mut app, held);
            f_key(&mut app, ButtonState::Pressed, false);
            app.update();
            let context = format!("the chord {on}, with {held:?}");
            assert_eq!(mode(&mut app), WindowMode::Windowed, "{context}");
            assert!(keys(&app).just_pressed(KeyCode::KeyF), "{context}");
        }
    }

    /// A held chord asks once, however long the keyboard repeats its F,
    /// and keeps its F from `ButtonInput` from its press until it is let go,
    /// even once Control and Command are let go first, and no release of it
    /// reaches anything. The next F alone is F again.
    #[test]
    fn a_held_chord_asks_once_and_keeps_its_f_until_it_is_let_go() {
        let fullscreen = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        let mut app = fullscreen_app(&with_the_chord(true));
        hold(&mut app, &[KeyCode::ControlLeft, KeyCode::SuperLeft]);
        f_key(&mut app, ButtonState::Pressed, false);
        app.update();
        assert_eq!(mode(&mut app), fullscreen);
        assert!(!keys(&app).pressed(KeyCode::KeyF));
        system_shows(&mut app, true);
        for _ in 0..4 {
            f_key(&mut app, ButtonState::Pressed, true);
            app.update();
            assert_eq!(mode(&mut app), fullscreen, "asked once");
            assert!(!keys(&app).pressed(KeyCode::KeyF), "held, repeated");
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::ControlLeft);
            keys.release(KeyCode::SuperLeft);
        }
        f_key(&mut app, ButtonState::Pressed, true);
        app.update();
        assert!(!keys(&app).pressed(KeyCode::KeyF), "still the chord's");
        f_key(&mut app, ButtonState::Released, false);
        app.update();
        assert!(!keys(&app).just_released(KeyCode::KeyF), "let go unheard");
        f_key(&mut app, ButtonState::Pressed, false);
        app.update();
        assert!(keys(&app).just_pressed(KeyCode::KeyF), "F alone is F");
        assert_eq!(mode(&mut app), fullscreen, "and switches nothing");
    }

    /// A window that loses the keyboard mid-chord may never hear its F let go.
    /// The loss ends the chord's hold, as it releases `ButtonInput`'s keys, so
    /// the next F, once the window has the keyboard again, is an F.
    #[test]
    fn a_loss_of_focus_ends_the_chords_hold() {
        let mut app = fullscreen_app(&with_the_chord(true));
        hold(&mut app, &[KeyCode::ControlLeft, KeyCode::SuperLeft]);
        f_key(&mut app, ButtonState::Pressed, false);
        app.update();
        assert!(!keys(&app).pressed(KeyCode::KeyF), "the chord's");
        app.world_mut().write_message(KeyboardFocusLost);
        // What Bevy's input systems do with the same message.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release_all();
        app.update();
        f_key(&mut app, ButtonState::Pressed, false);
        app.update();
        assert!(keys(&app).just_pressed(KeyCode::KeyF), "an F again");
    }

    /// The chord's F never reaches the action bound to F - Player Two's
    /// Right by default - through the game's own reading of the bindings,
    /// while an F alone, or with Control alone, does.
    #[test]
    fn the_chords_f_reaches_no_binding() {
        use crate::input::controller::{
            ControllerAssignments, ControllerBindings, ControllerHysteresisState,
        };
        use crate::input::keyboard::{KeyboardBindings, KeyboardKey};
        use crate::input::{GameAction, SemanticInput, update_semantic_input};

        let bindings = KeyboardBindings::default();
        assert_eq!(bindings.key(GameAction::PlayerTwoRight), KeyboardKey::F);
        let right = |app: &App| {
            app.world()
                .resource::<SemanticInput>()
                .pressed(GameAction::PlayerTwoRight)
        };
        for (held, reaches) in [
            (&[KeyCode::ControlLeft, KeyCode::SuperLeft][..], false),
            (&[][..], true),
            (&[KeyCode::ControlLeft][..], true),
        ] {
            let mut app = fullscreen_app(&with_the_chord(true));
            app.insert_resource(KeyboardBindings::default())
                .insert_resource(ControllerBindings::default())
                .insert_resource(ControllerAssignments::default())
                .insert_resource(ControllerHysteresisState::default())
                .insert_resource(SemanticInput::default())
                .add_systems(Update, update_semantic_input);
            hold(&mut app, held);
            f_key(&mut app, ButtonState::Pressed, false);
            app.update();
            assert_eq!(right(&app), reaches, "with {held:?}");
            f_key(&mut app, ButtonState::Pressed, true);
            app.update();
            assert_eq!(right(&app), reaches, "with {held:?}, held");
        }
    }

    /// A browser keeps F11: with the browser's window the plugin reads
    /// nothing of it, and the window keeps its mode however the key is
    /// pressed, where the desktop's switches.
    #[test]
    fn in_a_browser_f11_does_nothing() {
        for (host, platform, switches) in [
            ("desktop", window_platform_config(), true),
            ("browser", browser_window_platform_config(None), false),
        ] {
            let mut app = fullscreen_app(&platform);
            press(&mut app, KeyCode::F11);
            app.update();
            assert_eq!(mode(&mut app) != WindowMode::Windowed, switches, "{host}");
            assert_eq!(
                app.world().contains_resource::<SystemFullscreen>(),
                switches,
                "{host}"
            );
        }
    }
}
