//! The title screen: the menu panel, the decorative backdrop and the
//! marimba Easter egg.
//!
//! Entering Home resets the whole match state, so the title screen is always a
//! clean slate: any abandoned match is discarded and its recorder dropped
//! without being saved. Leaving Home despawns its entities and resets the
//! marimba, which is why the Easter egg has to be re-earned on every visit.

mod marimba;

use bevy::prelude::*;

use super::focus::{FocusGroup, Focusable};
use super::hud::HudAction;
use super::settings::SETTINGS_TITLE;
use super::widgets::spawn_menu_button;
use crate::app::AppScreen;
use crate::gameplay::{GameState, InputGate, initialize_home_state};
use crate::presentation::{
    HOME_PANEL_BACKGROUND, ORANGE, PANEL_BACKGROUND, TEXT_HEADING, TEXT_MUTED,
};
use marimba::{HomeMarimbaState, MarimbaPlugin, reset_home_marimba, spawn_home_marimba};

/// Owns the Home screen lifecycle and the marimba systems.
pub(super) struct HomePlugin;

impl Plugin for HomePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MarimbaPlugin)
            .add_systems(
                OnEnter(AppScreen::Home),
                (prepare_home_screen, spawn_home_screen).chain(),
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

fn spawn_home_screen(mut commands: Commands) {
    commands.spawn((
        Sprite::from_color(ORANGE, Vec2::splat(470.0)),
        Transform::from_xyz(0.0, 10.0, -2.0)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        DespawnOnExit(AppScreen::Home),
    ));
    commands.spawn((
        Sprite::from_color(PANEL_BACKGROUND, Vec2::splat(430.0)),
        Transform::from_xyz(0.0, 10.0, -1.0)
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
                width: px(590),
                padding: UiRect::axes(px(54), px(46)),
                border: UiRect::all(px(2)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(16),
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
                        font_size: FontSize::Px(68.0),
                        ..default()
                    },
                    TextColor(TEXT_HEADING),
                    TextLayout::justify(Justify::Center),
                ));
                panel.spawn((
                    Text::new("SIMULTANEOUS GO  |  SHARED BOARD"),
                    TextFont {
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    TextLayout::justify(Justify::Center),
                    Node {
                        margin: UiRect::bottom(px(20)),
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
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    // Muted rather than subtle: the panel is translucent,
                    // and where this line crosses the backdrop's orange band
                    // the subtler grey fell to 4.05:1.
                    TextColor(TEXT_MUTED),
                    TextLayout::justify(Justify::Center),
                    Node {
                        margin: UiRect::top(px(12)),
                        ..default()
                    },
                ));
            });
        });

    spawn_home_marimba(&mut commands);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppScreen;
    use crate::gameplay::InputGate;
    use crate::ui::hud::{HudState, apply_hud_action};
    use crate::ui::settings::{SETTINGS_TAB_ORDER, settings_tab_action_and_label};

    /// The pointer reaches each of Home's four menu buttons at its centre,
    /// under real layout and real picking, with everything the screen can
    /// draw shown - the marimba included. The title panel and the marimba
    /// stand above the screen's root on `GlobalZIndex`es of their own, and
    /// picking stops at the first node under the pointer whatever it is.
    #[test]
    fn the_pointer_reaches_every_home_button_with_the_marimba_drawn() {
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::real_layout_app;

        let mut app = real_layout_app();
        app.add_plugins(bevy::state::app::StatesPlugin)
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

        let buttons = reachable_buttons(&mut app);
        assert_eq!(buttons.len(), HOME_ACTIONS.len());
        assert_eq!(covered_at_their_centres(&mut app, &buttons), []);
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
        app.add_systems(Update, spawn_home_screen);
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
