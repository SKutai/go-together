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
//! once the maximised size has arrived, or [`MAXIMISE_PATIENCE`] later if
//! the platform never sends one, as a desktop with no window manager does
//! not. Either way no frame is drawn at a size the window is about to
//! leave. Centring is Bevy's: it puts the window's frame where its inside
//! would stand centred, so the window stands a title bar below the middle.
//! With no primary monitor known the window is shown where the platform
//! puts it, at the opening size. That is the normal case on Wayland, where
//! winit names no primary monitor and a window cannot be placed; it is the
//! case on an X server whose RandR lists no monitor, where winit's only
//! answer is a 1x1 stand-in it never lists among the monitors; and it is
//! the case on a Mac whose display is asleep as the game starts, when
//! macOS lists no active display at all. A browser's canvas is neither
//! hidden nor placed: the page puts it where it stands.
//!
//! # Fullscreen
//!
//! F11 switches a desktop window to fullscreen and back, on every screen
//! and whatever has the keys: a modal, a binding capture or a text field.
//! It is reserved (`crate::input::keyboard`), so it never binds, types or
//! plays a note, and it is read as `just_pressed`, as F1 is, so a held key
//! switches once. A browser keeps F11 for its own fullscreen: the page
//! hands it to the browser before the game sees it, and the game reads
//! nothing of it there either ([`WindowPlatformConfig::fullscreen_key`]).
//!
//! Fullscreen is borderless, on the monitor the window is on, and going
//! back restores the window as it was; that is the platform's to do, and
//! it does. On macOS borderless fullscreen is the green button's own, a
//! Space of its own, so F11 and the button are one fullscreen and either
//! leaves what the other entered. But Bevy never writes `Window::mode`
//! back from the system, so after the button it would still say windowed,
//! and an F11 that read it would ask for the fullscreen already shown -
//! and, since Bevy applies a mode only when it differs from the last one
//! it applied, nothing at all. So the system's own state is read from
//! winit every frame into [`SystemFullscreen`], `Window::mode` is kept in
//! step with it whenever it changes, and F11 asks for the other of the
//! system's two states ([`mode_after_f11`]), never of `Window::mode`'s.
//! Only a change is followed, not a difference: a compositor that is slow
//! to confirm a change - Wayland's - goes on reporting the old state for
//! a frame or two, and following that would undo what F11 asked for
//! before it took effect.

use std::time::Duration;

use bevy::ecs::system::NonSendMarker;
use bevy::input::InputSystems;
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
/// fewer than 160 px a 1036-px-tall Windows screen put the window's bottom
/// 9 px under the taskbar, as measured.
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

/// Places the window as it opens and shows it, and switches fullscreen on
/// F11, each where the host leaves it to the game: the systems of the
/// module docs, in `PreUpdate`.
pub(crate) struct GameWindowPlugin {
    placed_on_a_monitor: bool,
    fullscreen_key: bool,
}

impl GameWindowPlugin {
    /// The plugin for the host `platform` describes.
    pub(crate) fn new(platform: &WindowPlatformConfig) -> Self {
        Self {
            placed_on_a_monitor: platform.placed_on_a_monitor,
            fullscreen_key: platform.fullscreen_key,
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
                    switch_fullscreen_on_f11,
                )
                    .chain()
                    .after(InputSystems),
            );
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
    /// Placed and shown; nothing more to do.
    Shown,
}

/// Places the hidden window on the first frame, from the primary
/// monitor's logical size, and shows it once it is placed; see the module
/// docs.
pub(crate) fn place_the_opening_window(
    mut opening: ResMut<WindowOpening>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    time: Res<Time<Real>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    match *opening {
        WindowOpening::Unplaced => {
            let monitor = monitors.single().ok().map(|monitor| {
                Vec2::new(
                    monitor.physical_width as f32,
                    monitor.physical_height as f32,
                ) / monitor.scale_factor as f32
            });
            match monitor.map(opening_for) {
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
            let arrived = window.size() != from;
            let late = time.elapsed().saturating_sub(since) >= MAXIMISE_PATIENCE;
            if arrived || late {
                if !arrived {
                    // A platform may ignore a hidden window's request; it
                    // answers a shown one.
                    window.set_maximized(true);
                }
                window.visible = true;
                *opening = WindowOpening::Shown;
            }
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

/// On F11, asks for the mode [`mode_after_f11`] gives from the system's
/// state. A press in the frame the system's state changed waits for the
/// next one: that frame's change has just been written into
/// `Window::mode`, and asking for the other mode on top of it would leave
/// the mode Bevy last applied, and so ask for nothing.
fn switch_fullscreen_on_f11(
    keys: Res<ButtonInput<KeyCode>>,
    system: Res<SystemFullscreen>,
    mut pressed: Local<bool>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    *pressed |= keys.just_pressed(KeyCode::F11);
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
    /// shown once [`MAXIMISE_PATIENCE`] has passed, and asked again then,
    /// shown - and not a frame before.
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
        assert_eq!(late.internal.take_maximize_request(), Some(true));
    }

    /// With no primary monitor - a Wayland compositor's monitors, which
    /// winit names none of primary, or none at all - the window is shown at
    /// once, at the opening size, where the platform puts it. A browser's
    /// canvas is shown from the start and nothing places it.
    #[test]
    fn with_no_primary_monitor_the_window_is_shown_where_the_platform_puts_it() {
        let native = window_platform_config();
        let mut app = opening_app(&native, None);
        // A monitor that is not the primary one, as on Wayland.
        app.world_mut().spawn(Monitor {
            name: None,
            physical_width: 3840,
            physical_height: 2160,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: None,
            scale_factor: 2.0,
            video_modes: Vec::new(),
        });
        app.update();
        let mut shown = window(&mut app);
        assert!(shown.visible);
        assert_eq!(shown.position, WindowPosition::Automatic);
        assert_eq!(shown.size(), Vec2::new(1040.0, 900.0));
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

    /// Through the production plugin and schedule, on every screen, with
    /// nothing open and with either of the HUD's modals open, each press of
    /// F11 asks for fullscreen when the system shows the window windowed
    /// and for the window when it shows it fullscreen, and a held key asks
    /// once. Between presses the test does what the system would: it shows
    /// what was asked. A binding capture and a text field, which hold the
    /// keys on their screens, have tests of their own beside them in
    /// `ui::settings` and `ui::replay::library`.
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
