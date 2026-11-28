//! The title screen: the menu panel, the decorative backdrop and the
//! marimba Easter egg.
//!
//! Entering Home resets the whole match state, so the title screen is always a
//! clean slate: any abandoned match is discarded and its recorder dropped
//! without being saved. Leaving Home despawns its entities and resets the
//! marimba, which is why the Easter egg has to be re-earned on every visit.
//!
//! The panel is centred at its own size in a window of any size; the
//! diamond behind it and the marimba answer to the window's size as
//! [`geometry`] says, set at entry and again whenever the size changes.

mod geometry;
mod marimba;

use bevy::prelude::*;

use super::focus::{FocusGroup, Focusable};
use super::hud::HudAction;
use super::settings::SETTINGS_TITLE;
use super::widgets::spawn_menu_button;
use crate::app::visuals::{WindowSize, WorldFraming};
use crate::app::{AppScreen, AppSystems};
use crate::gameplay::{GameState, InputGate, initialize_home_state};
use crate::presentation::{
    HOME_PANEL_BACKGROUND, ORANGE, PANEL_BACKGROUND, TEXT_HEADING, TEXT_MUTED,
};
use geometry::{
    DIAMOND_CENTER_Y, DIAMOND_FACE_SIDE, DIAMOND_SIDE, PANEL_BORDER, PANEL_GAP, PANEL_PADDING,
    PANEL_WIDTH, SUBTITLE_MARGIN_ABOVE, SUBTITLE_SIZE, TAGLINE_MARGIN_BELOW, TAGLINE_SIZE,
    TITLE_SIZE, home_layout,
};
use marimba::{
    HomeMarimbaState, MarimbaNodes, MarimbaPlugin, reset_home_marimba, spawn_home_marimba,
};

/// Owns the Home screen lifecycle and the marimba systems.
pub(super) struct HomePlugin;

impl Plugin for HomePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MarimbaPlugin)
            .add_systems(
                OnEnter(AppScreen::Home),
                (prepare_home_screen, spawn_home_screen).chain(),
            )
            .add_systems(
                Update,
                place_home_furniture
                    .run_if(in_state(AppScreen::Home))
                    .run_if(resource_changed::<WindowSize>)
                    .in_set(AppSystems::ScreenLayout),
            )
            .add_systems(OnExit(AppScreen::Home), reset_home_screen_resources);
    }
}

const HOME_ACTIONS: [(HudAction, &str); 4] = [
    (HudAction::PlayMatch, "Play"),
    (HudAction::OpenHelp, "Help"),
    (HudAction::OpenControlMapping, SETTINGS_TITLE),
    (HudAction::OpenReplays, "Replays"),
];
const HOME_SUBTITLE: &str = "Local two-player strategy | esv.org/ecclesiastes";

/// Resets the Match state and the marimba. The HUD's modal state is not
/// reset here: `ui::hud` closes every modal on the screen change itself,
/// for every screen at once, since the Help modal belongs to no
/// screen.
fn prepare_home_screen(
    mut game: ResMut<GameState>,
    mut input_gate: ResMut<InputGate>,
    mut marimba: ResMut<HomeMarimbaState>,
) {
    initialize_home_state(&mut game, &mut input_gate);
    reset_home_marimba(&mut marimba);
}

/// Entity teardown is automatic (`DespawnOnExit`); this only resets the one
/// resource despawning cannot touch, so the marimba Easter egg is re-earned
/// on every visit.
fn reset_home_screen_resources(mut marimba: ResMut<HomeMarimbaState>) {
    reset_home_marimba(&mut marimba);
}

/// Spawns Home for the window's size: the diamond, framed by the main
/// camera at the zoom [`geometry`] gives it, the panel, and the marimba in
/// its stance.
fn spawn_home_screen(
    mut commands: Commands,
    window: Res<WindowSize>,
    mut framing: ResMut<WorldFraming>,
) {
    let layout = home_layout(window.layout());
    framing.set_if_neq(layout.framing());
    commands.spawn((
        Sprite::from_color(ORANGE, Vec2::splat(DIAMOND_SIDE)),
        Transform::from_xyz(0.0, DIAMOND_CENTER_Y, -2.0)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        DespawnOnExit(AppScreen::Home),
    ));
    commands.spawn((
        Sprite::from_color(PANEL_BACKGROUND, Vec2::splat(DIAMOND_FACE_SIDE)),
        Transform::from_xyz(0.0, DIAMOND_CENTER_Y, -1.0)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        DespawnOnExit(AppScreen::Home),
    ));
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            DespawnOnExit(AppScreen::Home),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: px(PANEL_WIDTH),
                padding: UiRect::axes(px(PANEL_PADDING.x), px(PANEL_PADDING.y)),
                border: UiRect::all(px(PANEL_BORDER)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(PANEL_GAP),
                ..default()
            })
            .insert((
                BackgroundColor(HOME_PANEL_BACKGROUND),
                BorderColor::all(ORANGE),
                GlobalZIndex(2),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Go Together"),
                    TextFont {
                        font_size: FontSize::Px(TITLE_SIZE),
                        ..default()
                    },
                    TextColor(TEXT_HEADING),
                    TextLayout::justify(Justify::Center),
                ));
                panel.spawn((
                    Text::new("SIMULTANEOUS GO  |  SHARED BOARD"),
                    TextFont {
                        font_size: FontSize::Px(TAGLINE_SIZE),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    TextLayout::justify(Justify::Center),
                    Node {
                        margin: UiRect::bottom(px(TAGLINE_MARGIN_BELOW)),
                        ..default()
                    },
                ));
                for (order, (action, label)) in HOME_ACTIONS.into_iter().enumerate() {
                    spawn_menu_button(
                        panel,
                        Some(action),
                        label,
                        true,
                        Some(Focusable {
                            group: FocusGroup::Screen,
                            order: order as u32,
                        }),
                    );
                }
                panel.spawn((
                    Text::new(HOME_SUBTITLE),
                    TextFont {
                        font_size: FontSize::Px(SUBTITLE_SIZE),
                        ..default()
                    },
                    // Muted rather than subtle: the panel is translucent,
                    // and where this line crosses the backdrop's orange band
                    // the subtler grey fell to 4.05:1.
                    TextColor(TEXT_MUTED),
                    TextLayout::justify(Justify::Center),
                    Node {
                        margin: UiRect::top(px(SUBTITLE_MARGIN_ABOVE)),
                        ..default()
                    },
                ));
            });
        });

    spawn_home_marimba(&mut commands, layout.marimba);
}

/// Moves Home's diamond and marimba to where a window of the new size puts
/// them; runs only on a frame the window's size changed.
fn place_home_furniture(
    window: Res<WindowSize>,
    mut framing: ResMut<WorldFraming>,
    mut marimba: MarimbaNodes,
) {
    let layout = home_layout(window.layout());
    framing.set_if_neq(layout.framing());
    marimba.place(layout.marimba);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppScreen;
    use crate::gameplay::InputGate;
    use crate::ui::hud::{HudState, apply_hud_action};
    use crate::ui::settings::{SETTINGS_TAB_ORDER, settings_tab_action_and_label};

    /// Home laid out by `bevy_ui` in a window of `window` logical pixels,
    /// entered as the game enters it, with the one thing it hides - the
    /// marimba - shown.
    fn home_layout_app(window: Vec2) -> App {
        use crate::ui::focus::real_layout_app;

        let mut app = real_layout_app(window);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_resource::<WorldFraming>()
            .insert_state(AppScreen::Home)
            .add_systems(OnEnter(AppScreen::Home), spawn_home_screen);
        app.update();
        let world = app.world_mut();
        let mut hidden = 0;
        for mut visibility in world.query::<&mut Visibility>().iter_mut(world) {
            if *visibility == Visibility::Hidden {
                *visibility = Visibility::Visible;
                hidden += 1;
            }
        }
        assert_eq!(hidden, 1, "the marimba is the one thing Home hides");
        app.update();
        app
    }

    /// The boxes of the panel and of each marimba bar, lowest note first, in
    /// logical pixels from the window's top-left, as `bevy_ui` laid them
    /// out.
    fn panel_and_bars(app: &mut App) -> (Rect, Vec<Rect>) {
        use bevy::ui::{ComputedNode, UiGlobalTransform};
        use marimba::HomeMarimbaBar;

        let world = app.world_mut();
        let rect = |node: &ComputedNode, transform: &UiGlobalTransform| {
            let scale = node.inverse_scale_factor;
            Rect::from_center_size(transform.translation * scale, node.size * scale)
        };
        let panels: Vec<Rect> = world
            .query::<(&ComputedNode, &UiGlobalTransform, &BackgroundColor)>()
            .iter(world)
            .filter(|(.., fill)| fill.0 == HOME_PANEL_BACKGROUND)
            .map(|(node, transform, _)| rect(node, transform))
            .collect();
        assert_eq!(panels.len(), 1, "one panel");
        let mut bars: Vec<(usize, Rect)> = world
            .query::<(&HomeMarimbaBar, &ComputedNode, &UiGlobalTransform)>()
            .iter(world)
            .map(|(bar, node, transform)| (bar.0, rect(node, transform)))
            .collect();
        bars.sort_by_key(|(index, _)| *index);
        (panels[0], bars.into_iter().map(|(_, rect)| rect).collect())
    }

    /// At every named size, under real layout and real picking: the pointer
    /// reaches each of Home's four menu buttons at its centre with
    /// everything the screen can draw shown, the marimba included; every
    /// text stands inside the window; the panel is as tall as `geometry`
    /// reckons, and the marimba's bars are drawn where it reckons them, which
    /// are what the marimba's stance is worked out from; the camera frames
    /// the diamond as `geometry` says; and the marimba's bars stand inside
    /// the window, clear of the panel, running across under it or up beside
    /// it. The title panel and the marimba stand above the
    /// screen's root on `GlobalZIndex`es of their own, and picking stops at
    /// the first node under the pointer whatever it is.
    #[test]
    fn the_pointer_reaches_every_home_button_with_the_marimba_drawn() {
        use crate::app::visuals::NAMED_WINDOW_SIZES;
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::laid_out_texts;
        use geometry::{MarimbaStance, PANEL_HEIGHT};

        let mut stances = Vec::new();
        for window in NAMED_WINDOW_SIZES {
            let mut app = home_layout_app(window);
            let buttons = reachable_buttons(&mut app);
            assert_eq!(buttons.len(), HOME_ACTIONS.len(), "{window}");
            assert_eq!(covered_at_their_centres(&mut app, &buttons), [], "{window}");
            let inside = Rect::from_corners(Vec2::ZERO, window);
            for (text, rect) in laid_out_texts(&mut app) {
                assert!(
                    inside.contains(rect.min) && inside.contains(rect.max),
                    "{window}: {text:?} at {rect:?}"
                );
            }

            let layout = home_layout(window);
            assert_eq!(*app.world().resource::<WorldFraming>(), layout.framing());
            let (panel, bars) = panel_and_bars(&mut app);
            assert!(
                (panel.height() - PANEL_HEIGHT).abs() < 1.0,
                "{window}: the panel is {} tall, reckoned {PANEL_HEIGHT}",
                panel.height()
            );
            assert_eq!(bars.len(), 11);
            for bar in &bars {
                assert!(
                    inside.contains(bar.min) && inside.contains(bar.max),
                    "{window}: a bar at {bar:?} leaves the window"
                );
                assert!(
                    bar.intersect(panel).is_empty(),
                    "{window}: a bar at {bar:?} covers the panel {panel:?}"
                );
            }
            // The bars stand where the layout reckons they are drawn, within
            // a pixel: `bevy_ui` rounds the node and each bar in it to whole
            // pixels, and an odd window puts both on a half.
            let drawn = bars.iter().fold(bars[0], |all, bar| all.union(*bar));
            let reckoned = layout.marimba_bars_rect();
            assert!(
                (drawn.min - reckoned.min).abs().max_element() <= 1.0
                    && (drawn.max - reckoned.max).abs().max_element() <= 1.0,
                "{window}: the bars are drawn at {drawn:?}, reckoned {reckoned:?}"
            );
            // The lowest bar is the longest: across the window it stands at
            // the left, upright it lies at the bottom.
            let (lowest, highest) = (bars[0], bars[10]);
            match layout.marimba {
                MarimbaStance::UnderPanel(_) => {
                    assert!(lowest.height() > highest.height(), "{window}");
                    assert!(lowest.max.x < highest.min.x, "{window}");
                    assert!(bars.iter().all(|bar| bar.min.y > panel.max.y), "{window}");
                }
                MarimbaStance::Upright(_) => {
                    assert!(lowest.width() > highest.width(), "{window}");
                    assert!(lowest.min.y > highest.max.y, "{window}");
                    assert!(bars.iter().all(|bar| bar.min.x > panel.max.x), "{window}");
                }
            }
            stances.push(matches!(layout.marimba, MarimbaStance::Upright(_)));
        }
        assert!(stances.contains(&true) && stances.contains(&false));
    }

    /// When the window's size changes, Home moves with it: the diamond's
    /// framing and the marimba's stance follow, and back in the opening
    /// window every node is again the one a fresh entry spawns, so turning
    /// the marimba upright and back leaves nothing of the turn behind.
    #[test]
    fn a_change_of_window_size_moves_home_and_moving_back_restores_it() {
        use crate::app::visuals::WindowSize;
        use marimba::{HomeMarimbaBar, HomeMarimbaVisual};

        fn marimba_nodes(app: &mut App) -> Vec<Node> {
            let world = app.world_mut();
            let mut nodes: Vec<Node> = world
                .query_filtered::<&Node, With<HomeMarimbaVisual>>()
                .iter(world)
                .cloned()
                .collect();
            nodes.extend(
                world
                    .query_filtered::<&Node, With<HomeMarimbaBar>>()
                    .iter(world)
                    .cloned(),
            );
            nodes
        }

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .init_resource::<WindowSize>()
            .init_resource::<WorldFraming>()
            .add_systems(OnEnter(AppScreen::Home), spawn_home_screen)
            .add_systems(
                Update,
                place_home_furniture
                    .run_if(in_state(AppScreen::Home))
                    .run_if(resource_changed::<WindowSize>),
            );
        app.update();
        let opening = marimba_nodes(&mut app);
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            WorldFraming::IDENTITY
        );

        app.insert_resource(WindowSize::MINIMUM);
        app.update();
        let smallest = home_layout(WindowSize::MINIMUM.0);
        assert_eq!(*app.world().resource::<WorldFraming>(), smallest.framing());
        let moved = marimba_nodes(&mut app);
        assert_ne!(moved, opening, "the marimba turned upright");
        let geometry::MarimbaStance::Upright(top_left) = smallest.marimba else {
            panic!("the smallest window stands the marimba upright");
        };
        assert_eq!(moved[0].left, px(top_left.x));

        app.insert_resource(WindowSize::OPENING);
        app.update();
        assert_eq!(marimba_nodes(&mut app), opening);
        assert_eq!(
            *app.world().resource::<WorldFraming>(),
            WorldFraming::IDENTITY
        );
    }

    /// Home entered through its own plugin and the game's window layout,
    /// from another screen, in the smallest window, twice: each entry
    /// frames the diamond at its zoom, after the change of screen has put
    /// the framing back, so the main camera draws it shrunk; and leaving
    /// puts the camera back to a scale of 1 over the origin. Home frames
    /// the diamond on entry and again only when the window's size changes,
    /// so a reset that ran after the screens' enter schedules would leave
    /// the diamond drawn at its full size, here on the second entry above
    /// all, where the size has not changed since the first.
    #[test]
    fn entering_home_in_a_short_window_frames_the_diamond_shrunk_every_time() {
        use crate::app::motion::MotionPolicy;
        use crate::app::visuals::{MainCamera, WindowLayoutPlugin, WindowSize};
        use crate::audio::AudioCueQueue;
        use crate::settings::GamePreferences;
        use bevy::input::keyboard::KeyboardInput;

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .add_message::<KeyboardInput>()
            .insert_state(AppScreen::ControlMapping)
            .add_plugins(WindowLayoutPlugin)
            .add_plugins(HomePlugin)
            .insert_resource(WindowSize::MINIMUM)
            .insert_resource(GameState::default())
            .insert_resource(InputGate::default())
            .insert_resource(HudState::default())
            .insert_resource(AudioCueQueue::default())
            .insert_resource(GamePreferences::default())
            .init_resource::<MotionPolicy>()
            .init_resource::<Time>();
        let camera = app
            .world_mut()
            .spawn((
                MainCamera,
                Projection::Orthographic(OrthographicProjection::default_2d()),
                Transform::default(),
            ))
            .id();
        let drawn = |app: &App| {
            let Projection::Orthographic(projection) =
                app.world().get::<Projection>(camera).unwrap()
            else {
                panic!("an orthographic camera");
            };
            (*app.world().resource::<WorldFraming>(), projection.scale)
        };
        let go_to = |app: &mut App, screen: AppScreen| {
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(screen);
            app.update();
        };
        app.update();
        assert_eq!(drawn(&app), (WorldFraming::IDENTITY, 1.0));

        let shrunk = home_layout(WindowSize::MINIMUM.0).framing();
        assert!(
            shrunk.zoom < 0.85,
            "the diamond shrinks in the smallest window"
        );
        for entry in ["first", "second"] {
            go_to(&mut app, AppScreen::Home);
            assert_eq!(drawn(&app), (shrunk, 1.0 / shrunk.zoom), "{entry} entry");
            go_to(&mut app, AppScreen::ControlMapping);
            assert_eq!(drawn(&app), (WorldFraming::IDENTITY, 1.0), "{entry} exit");
        }
    }

    #[test]
    fn home_settings_navigation_and_tab_labels_are_exact() {
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();
        assert_eq!(SETTINGS_TITLE, "Settings");
        assert_eq!(
            apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenControlMapping),
            Some(AppScreen::ControlMapping)
        );
        assert_eq!(
            SETTINGS_TAB_ORDER.map(|tab| settings_tab_action_and_label(tab).1),
            ["Preferences", "Game", "Keyboard", "Controller"]
        );
    }

    #[test]
    fn home_primary_actions_have_exact_one_word_labels() {
        assert_eq!(
            HOME_ACTIONS,
            [
                (HudAction::PlayMatch, "Play"),
                (HudAction::OpenHelp, "Help"),
                (HudAction::OpenControlMapping, "Settings"),
                (HudAction::OpenReplays, "Replays"),
            ]
        );
        assert!(
            HOME_ACTIONS
                .iter()
                .all(|(_, label)| label.split_whitespace().count() == 1)
        );
    }

    /// Marking is authored, not derived: exactly the four menu buttons are
    /// in the focus ring, in the order they are drawn, and nothing else this
    /// screen spawns is focusable. The Help modal's buttons are not
    /// this screen's to spawn - the modal is the HUD's, application-
    /// persistent, and its ring is pinned in `ui::help`.
    #[test]
    fn home_menu_buttons_are_focusable_in_drawn_order_and_nothing_else_is() {
        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>()
            .init_resource::<WorldFraming>()
            .add_systems(Update, spawn_home_screen);
        app.update();

        let world = app.world_mut();
        let mut focusables = world.query::<(&Focusable, Option<&HudAction>)>();
        let mut marked: Vec<_> = focusables
            .iter(world)
            .map(|(focusable, action)| {
                (
                    focusable.group,
                    focusable.order,
                    *action.expect("a focusable button must carry an intent"),
                )
            })
            .collect();
        marked.sort_by_key(|(group, order, _)| (*group, *order));
        assert_eq!(
            marked,
            [
                (FocusGroup::Screen, 0, HudAction::PlayMatch),
                (FocusGroup::Screen, 1, HudAction::OpenHelp),
                (FocusGroup::Screen, 2, HudAction::OpenControlMapping),
                (FocusGroup::Screen, 3, HudAction::OpenReplays),
            ]
        );
    }

    #[test]
    fn home_subtitle_is_exact_font_safe_ascii() {
        assert_eq!(
            HOME_SUBTITLE,
            "Local two-player strategy | esv.org/ecclesiastes"
        );
        assert!(HOME_SUBTITLE.is_ascii());
    }
}
