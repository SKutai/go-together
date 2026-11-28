//! How the settings screen joins the keyboard and controller focus ring:
//! where each control sits in it, and when the ring stands down because one
//! of this screen's own controls is reading the keys.
//!
//! # One ring, not four
//!
//! Every control here is in a single [`FocusGroup::Screen`] ring - the tab
//! bar, whichever panel is shown, and the footer - and the reset
//! confirmation's two buttons are the screen's one [`FocusGroup::Modal`].
//! A panel does not get a group of its own, because the core has no
//! cross-group movement: [`apply_focus_navigation`] wraps within the
//! contiguous run of the resolved candidate's group, so a panel in its own
//! group would be a ring a player could enter and never leave back to the
//! tab bar. The hidden panels drop out on their own instead: they are
//! `Display::None`, which zeroes every node beneath them, and the candidate
//! collector discards zero-size nodes. `hidden_panels_leave_the_ring_and_the_shown_one_joins_it_under_real_layout`
//! pins that against real `bevy_ui` layout on this screen's own tree, three
//! levels of nesting included.
//!
//! # Bands
//!
//! Orders are banded rather than consecutive, because gaps cost nothing
//! and a band leaves room to add a control without renumbering its
//! neighbours: the tab bar from [`TAB_BAR_FOCUS_ORDER`], each panel from its
//! own [`panel_focus_order`], the footer from [`FOOTER_FOCUS_ORDER`]. Within
//! a band a [`RingOrder`] hands out consecutive positions in the order the
//! spawner draws its controls, so the ring walks the screen the way the eye
//! reads it. The Keyboard and Controller tabs are read column by column -
//! all of White/Player 2, then all of Black/Player 1, then on the Keyboard
//! tab the shared row - because a binding column is one list a player works
//! down.
//!
//! The Preferences band sits *above* the other three panels' bands, out of
//! tab order, for the sake of re-entry. Focus is remembered by order and
//! outlives a screen visit, but every entry resets this screen to the
//! Preferences tab, so a position remembered inside another panel resolves
//! against a ring that no longer shows it and clamps to the nearest lower
//! order. With Preferences numbered above the others that clamp lands on
//! the tab bar, where a returning player starts anyway; numbered in tab
//! order it would land on the last volume stop, and the ring is often
//! already visible on entry (it stays visible from the title screen until
//! the pointer moves), so the player would see it there and the first Enter
//! would change a volume. A ring-driven tab switch leaves the memory on the
//! tab bar, so this is a re-entry matter. A pointer switch is the one other
//! way a panel can be shown over a stale memory - the ring is hidden then,
//! but still resolved every frame - and it splits the same way: switching
//! to a higher band clamps to the tab bar as re-entry does, switching to a
//! lower one clamps to the new panel's last control, where the next key
//! reveals the ring. That is a mixed-input case the pointer began, nothing
//! there is destructive, and putting the memory back on the tab bar on
//! every switch would need the core to expose its memory, which no screen
//! does today.
//!
//! The confirmation seeds on *Cancel*: it is order 0 and Confirm Reset is
//! order 1, although they are drawn the other way round. With two entries
//! the walk direction is invisible - either key toggles - and only the seed
//! shows, so it should not be the destructive one. That is the same
//! judgement [`FocusMemory`] makes when it forgets the modal slot on every
//! close.
//!
//! # What is in the ring and what is not
//!
//! Three controls on this screen are drawn disabled at times, and they are
//! not treated alike. The rule that decides them: a control leaves the ring
//! only when pressing it does nothing at all; one whose press gives
//! feedback, or whose leaving would move the ring away from where the
//! player just put it, stays.
//!
//! * A numeric input whose limit is switched off *leaves* the ring. Its
//!   press is a no-op and its click cue is muted, so it is dead to the
//!   pointer too; [`super::display`] carries that into the ring by
//!   inserting `bevy_ui`'s [`InteractionDisabled`], the rule the Replay
//!   Viewer's transport already follows. It rejoins the frame the limit is
//!   switched on.
//! * The selected tab *stays* in the ring, although pressing it does
//!   nothing. Taking it out would make every tab switch jump the ring
//!   backwards: focus is remembered by order, the order just activated
//!   would be gone, and resolution clamps to the nearest lower one. Staying
//!   put on the tab the player just chose is what a tab strip normally
//!   does.
//! * A controller row whose player has no pad assigned *stays* too. It is
//!   drawn disabled, but its press is not a no-op: it tells the player to
//!   connect a controller, which is the one piece of feedback a keyboard
//!   player would otherwise never see.
//!
//! # Yielding while a control owns the keys
//!
//! [`yield_focus_while_capturing`] raises the core's [`FocusYield`] for
//! every frame one of this screen's controls is consuming raw input: a
//! keyboard binding capture, a controller binding capture, or an open
//! numeric editor on the Game tab. While it is raised the ring neither moves
//! nor activates. Both halves are deliberate, and the screen behaves the
//! same way in all three states: the movement keys are the very keys the
//! control is reading - Tab and the arrows are bindable, the D-pad is
//! bindable, and the arrows move the numeric caret - so a ring that still
//! moved would wander while a capture waited for its key, and one that still
//! pressed would fire a second control on the Enter that commits a number.
//! The player leaves each state the way the pointer does: the captured key
//! or button fills the binding, Enter commits or Escape abandons the number,
//! and Escape (or a pad's Start) cancels a capture.
//!
//! The yield is not raised while the reset confirmation is open, because
//! that is exactly the reading every capture system on this screen makes -
//! each returns early under the dialog - and the dialog itself must stay
//! operable. That case is reachable, by a pointer: a click on Reset
//! Controls while a numeric editor holds text that will not commit leaves
//! the editor open *under* the dialog (the ring itself cannot get there, it
//! is yielded), and a yield raised for it would freeze the ring on Cancel
//! with Escape as the only way out. Closing the dialog gives the editor the
//! keys back: the ring returns to its field and holds there until Enter
//! commits or Escape abandons, and the Enter that pressed Cancel does not
//! reach the editor, because the capture re-arms on the frame it resumes.
//! `a_reset_dialog_over_a_failed_commit_keeps_its_ring_live` pins all of it.
//!
//! [`apply_focus_navigation`]: crate::ui::focus
//! [`FocusMemory`]: crate::ui::focus
//! [`InteractionDisabled`]: bevy::ui::InteractionDisabled

use bevy::prelude::*;

use super::{ControlMappingState, MappingTab};
use crate::ui::focus::{FocusGroup, FocusYield, Focusable};

/// The tab bar's first ring position; the four tabs follow
/// [`super::SETTINGS_TAB_ORDER`].
pub(super) const TAB_BAR_FOCUS_ORDER: u32 = 0;

/// The footer's first ring position: Reset Controls, then Back.
pub(super) const FOOTER_FOCUS_ORDER: u32 = 900;

/// A panel's first ring position. Only one panel is ever shown, so the
/// bands never meet in a ring; keeping them apart, with Preferences above
/// the rest, is what makes a position remembered inside a hidden panel
/// clamp to the tab bar on re-entry rather than to an unrelated control at
/// the same offset - see the module docs.
pub(super) const fn panel_focus_order(tab: MappingTab) -> u32 {
    match tab {
        MappingTab::Game => 200,
        MappingTab::Keyboard => 300,
        MappingTab::Controller => 400,
        MappingTab::Preferences => 500,
    }
}

/// Hands out consecutive [`FocusGroup::Screen`] positions within one band,
/// in the order the spawner asks for them - which is the order it draws
/// its controls, so the ring walks the screen the way it reads.
pub(super) struct RingOrder {
    next: u32,
}

impl RingOrder {
    pub(super) const fn starting_at(first: u32) -> Self {
        Self { next: first }
    }

    pub(super) fn next(&mut self) -> Focusable {
        let order = self.next;
        self.next += 1;
        Focusable {
            group: FocusGroup::Screen,
            order,
        }
    }
}

/// The reset confirmation's ring: Cancel first, so the dialog seeds on the
/// non-destructive button, then Confirm Reset. See the module docs.
pub(super) const fn reset_dialog_focusable(confirm: bool) -> Focusable {
    Focusable {
        group: FocusGroup::Modal,
        order: if confirm { 1 } else { 0 },
    }
}

/// Raises [`FocusYield`] for every frame one of this screen's controls owns
/// the raw devices - see the module docs for which states, why the ring
/// stops moving as well as pressing, and why an open reset confirmation
/// never yields. Runs last in the settings input chain, after every way a
/// capture or an editing session can begin or end this frame; the core
/// reads and lowers the flag in the next frame's `PreUpdate`, so writing it
/// every live frame is what holds it, and stopping is what releases it.
pub(super) fn yield_focus_while_capturing(
    mapping: Res<ControlMappingState>,
    mut focus_yield: ResMut<FocusYield>,
) {
    if mapping.owns_raw_input() {
        focus_yield.0 = true;
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input_focus::{InputFocus, InputFocusVisible};
    use bevy::ui::InteractionDisabled;
    use bevy::ui_widgets::Button;

    use super::super::layout::spawn_control_mapping_screen;
    use super::super::{
        ControllerMappingBinding, GameNumericInput, GameToggle, MappingBinding, PreferenceToggle,
        SETTINGS_TAB_ORDER, SettingsPlugin, SettingsTabButton, VolumeStop,
        settings_tab_action_and_label,
    };
    use super::*;
    use crate::app::{AppScreen, AppSystems, configure_app_systems};
    use crate::audio::{AudioChannel, AudioCueQueue};
    use crate::gameplay::InputGate;
    use crate::input::GameAction;
    use crate::input::controller::{
        ControllerAssignments, ControllerBindings, ControllerCaptureState, ControllerInput,
        DeviceFamily, ProfileAction,
    };
    use crate::input::keyboard::{KeyboardBindings, KeyboardKey};
    use crate::match_settings::{MatchSettingField, MatchSettings};
    use crate::platform::{SettingsStorage, SettingsStorageError};
    use crate::rules::Player;
    use crate::settings::{GamePreferences, SettingsStore, VolumeLevel};
    use crate::ui::button::{click, hover, mouse_button, queue_mouse_button};
    use crate::ui::focus::{UiFocusPlugin, real_layout_app};
    use crate::ui::hud::{HudAction, HudState, handle_ui_buttons};
    use crate::ui::text_input::TextEditor;

    /// Every kind of control this screen puts in the ring, so the marking
    /// test can name what it found.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Control {
        Action(HudAction),
        Preference(PreferenceToggle),
        Volume(VolumeStop),
        Toggle(GameToggle),
        Input(GameNumericInput),
        Key(GameAction),
        Pad(Player, ProfileAction),
    }

    fn control_of(world: &World, entity: Entity) -> Control {
        if let Some(action) = world.get::<HudAction>(entity) {
            Control::Action(*action)
        } else if let Some(preference) = world.get::<PreferenceToggle>(entity) {
            Control::Preference(*preference)
        } else if let Some(stop) = world.get::<VolumeStop>(entity) {
            Control::Volume(*stop)
        } else if let Some(toggle) = world.get::<GameToggle>(entity) {
            Control::Toggle(*toggle)
        } else if let Some(input) = world.get::<GameNumericInput>(entity) {
            Control::Input(*input)
        } else if let Some(binding) = world.get::<MappingBinding>(entity) {
            Control::Key(binding.0)
        } else if let Some(binding) = world.get::<ControllerMappingBinding>(entity) {
            Control::Pad(binding.0, binding.1)
        } else {
            panic!("a focusable control must carry an intent: {entity:?}")
        }
    }

    /// The whole ring as the spawners are meant to lay it out, assembled
    /// from the same constants they draw from.
    fn expected_ring() -> Vec<(FocusGroup, u32, Control)> {
        let mut expected = Vec::new();
        let mut screen = |order: &mut u32, control: Control| {
            expected.push((FocusGroup::Screen, *order, control));
            *order += 1;
        };

        let mut order = TAB_BAR_FOCUS_ORDER;
        for tab in SETTINGS_TAB_ORDER {
            screen(
                &mut order,
                Control::Action(settings_tab_action_and_label(tab).0),
            );
        }

        let mut order = panel_focus_order(MappingTab::Preferences);
        for preference in [
            PreferenceToggle::MatchTimer,
            PreferenceToggle::RoundNumber,
            PreferenceToggle::ReduceMotion,
        ] {
            screen(&mut order, Control::Preference(preference));
        }
        for channel in [AudioChannel::Game, AudioChannel::Ui] {
            for level in VolumeLevel::ALL {
                screen(&mut order, Control::Volume(VolumeStop { channel, level }));
            }
        }

        let mut order = panel_focus_order(MappingTab::Game);
        for control in [
            Control::Toggle(GameToggle::Limit(MatchSettingField::RoundLimit)),
            Control::Input(GameNumericInput::RoundLimit),
            Control::Toggle(GameToggle::Limit(MatchSettingField::RoundTime)),
            Control::Input(GameNumericInput::RoundTimeMinutes),
            Control::Input(GameNumericInput::RoundTimeSeconds),
            Control::Toggle(GameToggle::Limit(MatchSettingField::NoStoneRoundLimit)),
            Control::Input(GameNumericInput::NoStoneRoundLimit),
            Control::Toggle(GameToggle::Limit(MatchSettingField::MatchTime)),
            Control::Input(GameNumericInput::MatchTimeMinutes),
            Control::Input(GameNumericInput::MatchTimeSeconds),
            Control::Toggle(GameToggle::Limit(MatchSettingField::PointTarget)),
            Control::Input(GameNumericInput::PointTarget),
            Control::Toggle(GameToggle::CapturePoints),
        ] {
            screen(&mut order, control);
        }

        let mut order = panel_focus_order(MappingTab::Keyboard);
        for action in GameAction::PLAYER_TWO
            .iter()
            .chain(&GameAction::PLAYER_ONE)
            .chain(&GameAction::SHARED)
        {
            screen(&mut order, Control::Key(*action));
        }

        let mut order = panel_focus_order(MappingTab::Controller);
        for player in [Player::Two, Player::One] {
            for action in ProfileAction::ALL {
                screen(&mut order, Control::Pad(player, action));
            }
        }

        let mut order = FOOTER_FOCUS_ORDER;
        screen(&mut order, Control::Action(HudAction::RequestResetMappings));
        screen(&mut order, Control::Action(HudAction::BackToHome));

        expected.push((
            FocusGroup::Modal,
            0,
            Control::Action(HudAction::CancelResetMappings),
        ));
        expected.push((
            FocusGroup::Modal,
            1,
            Control::Action(HudAction::ConfirmResetMappings),
        ));
        expected
    }

    /// Marking is authored, not derived: exactly the sixty-three controls
    /// that carry an intent are in the ring, in the bands and the drawn
    /// order the module docs describe, with the confirmation's Cancel
    /// seeded first, and no button is outside the ring: the dialog's
    /// overlay root is a plain node.
    #[test]
    fn settings_controls_are_focusable_in_drawn_order_and_the_overlay_root_is_not() {
        let mut app = App::new();
        app.add_systems(Update, spawn_control_mapping_screen);
        app.update();

        let world = app.world_mut();
        let mut marked: Vec<_> = world
            .query::<(Entity, &Focusable)>()
            .iter(world)
            .map(|(entity, focusable)| (focusable.group, focusable.order, entity))
            .collect();
        marked.sort_by_key(|(group, order, _)| (*group, *order));
        let marked: Vec<_> = marked
            .into_iter()
            .map(|(group, order, entity)| (group, order, control_of(world, entity)))
            .collect();
        // Assembled in tab order for readability; the ring itself sorts by
        // (group, order), and the Preferences band sits above the other
        // panels' bands on purpose (see the module docs).
        let mut expected = expected_ring();
        expected.sort_by_key(|(group, order, _)| (*group, *order));
        assert_eq!(expected.len(), 63);
        assert_eq!(marked, expected);

        let unmarked = world
            .query_filtered::<(), (With<Button>, Without<Focusable>)>()
            .iter(world)
            .count();
        assert_eq!(
            unmarked, 0,
            "every button is in the ring: the dialog's overlay root is a plain node"
        );
    }

    /// The yield's condition, state by state: each of the three live
    /// controls yields on its own, and an open reset confirmation - which
    /// suspends all three - never does, even over a numeric editor a
    /// failed commit left open.
    #[test]
    fn the_ring_yields_to_each_live_control_and_never_under_the_reset_dialog() {
        let editor = || {
            Some(super::super::GameEditor {
                input: GameNumericInput::RoundLimit,
                editor: TextEditor::new(String::new()),
            })
        };
        let mut mapping = ControlMappingState::new(None);
        assert!(!mapping.owns_raw_input());

        mapping.capturing = Some(GameAction::PlayerOneSelect);
        assert!(mapping.owns_raw_input());
        mapping.reset_confirmation_open = true;
        assert!(!mapping.owns_raw_input());

        let mut mapping = ControlMappingState::new(None);
        mapping.capturing_controller = Some((Player::Two, ProfileAction::Select));
        assert!(mapping.owns_raw_input());
        mapping.reset_confirmation_open = true;
        assert!(!mapping.owns_raw_input());

        let mut mapping = ControlMappingState::new(None);
        mapping.game_editor = editor();
        assert!(mapping.owns_raw_input());
        mapping.reset_confirmation_open = true;
        assert!(!mapping.owns_raw_input());
    }

    struct DiscardingSettingsStorage;

    impl SettingsStorage for DiscardingSettingsStorage {
        fn load(&self) -> Result<Option<String>, SettingsStorageError> {
            Ok(None)
        }

        fn save(&self, _json: &str) -> Result<(), SettingsStorageError> {
            Ok(())
        }
    }

    /// The whole screen under real `bevy_ui` layout and the real production
    /// graph: the focus core, the ungated HUD button reader that carries
    /// `BackToHome`, and [`SettingsPlugin`] itself - its `OnEnter` chain,
    /// its input chain in its registered order and its display chain, in
    /// their production sets and gates - entered through a real state
    /// transition. One frame has run on the screen, so it is spawned, laid
    /// out and collected, with the ring parked on its first control and not
    /// yet drawn.
    fn settings_app() -> App {
        let mut app = real_layout_app(crate::app::visuals::WindowSize::OPENING.0);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(AppScreen::Home)
            .add_plugins(UiFocusPlugin)
            .add_plugins(SettingsPlugin::new(None))
            .insert_resource(HudState::default())
            .insert_resource(InputGate::default())
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(MatchSettings::default())
            .insert_resource(SettingsStore::from_storage(DiscardingSettingsStorage))
            .insert_resource(ControllerAssignments::default())
            .insert_resource(ControllerCaptureState::default())
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_ui_buttons.in_set(AppSystems::HudInput));
        configure_app_systems(&mut app);
        // Enter the screen the way the game does: through the transition
        // that runs the plugin's own `OnEnter` chain.
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ControlMapping);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::ControlMapping
        );
        app
    }

    fn mapping(app: &App) -> &ControlMappingState {
        app.world().resource::<ControlMappingState>()
    }

    fn focused(app: &App) -> Option<Entity> {
        app.world().resource::<InputFocus>().get()
    }

    /// No screen change is pending and none has happened: `NextState` is
    /// consumed by the transition one frame after it is set, so the state
    /// itself is checked too, which holds however many frames have passed.
    fn stays_on_settings(app: &App) -> bool {
        matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ) && *app.world().resource::<State<AppScreen>>().get() == AppScreen::ControlMapping
    }

    /// The one entity carrying a marker that satisfies `matches`.
    fn entity_with<M: Component>(app: &mut App, matches: impl Fn(&M) -> bool) -> Entity {
        let world = app.world_mut();
        let found: Vec<Entity> = world
            .query::<(Entity, &M)>()
            .iter(world)
            .filter(|(_, marker)| matches(marker))
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one matching control");
        found[0]
    }

    fn order_of(app: &App, entity: Entity) -> u32 {
        app.world().get::<Focusable>(entity).unwrap().order
    }

    fn press_key(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
        keys.press(key);
    }

    /// No `InputPlugin` runs headlessly, so `just_pressed` never expires on
    /// its own; every tap releases what it pressed after its frame.
    fn release_keys(app: &mut App) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        let held: Vec<KeyCode> = keys.get_pressed().copied().collect();
        for key in held {
            keys.release(key);
        }
        keys.clear();
    }

    /// One frame with `key` just pressed, then released.
    fn tap(app: &mut App, key: KeyCode) {
        press_key(app, key);
        app.update();
        release_keys(app);
    }

    fn tap_shift_tab(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        tap(app, KeyCode::Tab);
    }

    /// One frame with a real key press: the raw `KeyboardInput` message the
    /// numeric editor reads and the `ButtonInput` state the ring reads,
    /// together, as the keyboard delivers them.
    fn type_key(app: &mut App, key_code: KeyCode, logical_key: Key) {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        tap(app, key_code);
    }

    fn enter(app: &mut App) {
        type_key(app, KeyCode::Enter, Key::Enter);
    }

    fn reveal(app: &mut App) {
        if !app.world().resource::<InputFocusVisible>().0 {
            tap(app, KeyCode::Tab);
        }
    }

    /// Tabs until `target` is focused.
    fn tab_to(app: &mut App, target: Entity) {
        reveal(app);
        for _ in 0..100 {
            if focused(app) == Some(target) {
                return;
            }
            tap(app, KeyCode::Tab);
        }
        panic!("the ring never reached {target:?}");
    }

    /// Tabs once round the ring from wherever it is, returning the orders
    /// visited in the sequence they were visited, the start included.
    fn walk_ring(app: &mut App) -> Vec<u32> {
        reveal(app);
        let start = focused(app).expect("something is focused");
        let mut visited = vec![order_of(app, start)];
        for _ in 0..100 {
            tap(app, KeyCode::Tab);
            let now = focused(app).expect("something is focused");
            if now == start {
                return visited;
            }
            visited.push(order_of(app, now));
        }
        panic!("the ring never wrapped: {visited:?}");
    }

    /// A left press on `entity`, held: the release that would activate it
    /// is left for the test to put in a frame of its choosing, with
    /// [`queue_mouse_button`].
    fn press_on(app: &mut App, entity: Entity) {
        hover(app, entity);
        mouse_button(app, MouseButton::Left, ButtonState::Pressed);
    }

    fn queue_release(app: &mut App) {
        queue_mouse_button(app, MouseButton::Left, ButtonState::Released);
    }

    /// One frame with `button` just pressed on `pad`, then released - the
    /// per-frame clearing `bevy_input` does in production, by hand.
    fn tap_pad(app: &mut App, pad: Entity, button: GamepadButton) {
        {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            gamepad.digital_mut().clear();
            gamepad.digital_mut().press(button);
        }
        app.update();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
        gamepad.digital_mut().release(button);
        gamepad.digital_mut().clear();
    }

    fn pad_to(app: &mut App, pad: Entity, target: Entity) {
        for _ in 0..100 {
            if focused(app) == Some(target) && app.world().resource::<InputFocusVisible>().0 {
                return;
            }
            tap_pad(app, pad, GamepadButton::DPadDown);
        }
        panic!("the pad never reached {target:?}");
    }

    fn tab_button(app: &mut App, tab: MappingTab) -> Entity {
        entity_with::<SettingsTabButton>(app, |button| button.0 == tab)
    }

    fn action_button(app: &mut App, action: HudAction) -> Entity {
        entity_with::<HudAction>(app, |candidate| *candidate == action)
    }

    /// Activates a tab through the ring and returns its button.
    fn open_tab(app: &mut App, tab: MappingTab) -> Entity {
        let button = tab_button(app, tab);
        tab_to(app, button);
        enter(app);
        assert_eq!(mapping(app).tab, tab);
        button
    }

    fn expected_orders(
        panel: impl IntoIterator<Item = u32>,
        controls: impl IntoIterator<Item = u32>,
    ) -> Vec<u32> {
        let mut orders: Vec<u32> = (0..SETTINGS_TAB_ORDER.len() as u32).collect();
        orders.extend(
            panel
                .into_iter()
                .zip(controls)
                .map(|(base, offset)| base + offset),
        );
        orders.extend([FOOTER_FOCUS_ORDER, FOOTER_FOCUS_ORDER + 1]);
        orders
    }

    /// The premise the single ring rests on, on this screen's own tree under
    /// real layout: only the shown panel's controls are in the ring with the
    /// tab bar and the footer, switching tabs through the ring swaps them
    /// the same frame, three levels of `Display::None` nesting included -
    /// and on the Game tab a numeric field whose limit is off is not in the
    /// ring, and joins it the frame its checkbox is pressed.
    #[test]
    fn hidden_panels_leave_the_ring_and_the_shown_one_joins_it_under_real_layout() {
        let mut app = settings_app();
        let sorted = |mut orders: Vec<u32>| {
            orders.sort_unstable();
            orders
        };

        let preferences = panel_focus_order(MappingTab::Preferences);
        assert_eq!(
            sorted(walk_ring(&mut app)),
            expected_orders(std::iter::repeat(preferences), 0..11),
            "Preferences: four tabs, three toggles, eight volume stops, two footer buttons"
        );

        open_tab(&mut app, MappingTab::Game);
        let game = panel_focus_order(MappingTab::Game);
        // Every limit is off by default, so the seven fields are out and
        // only the six checkboxes remain - and the ring is still on the tab
        // the player just chose, not clamped backwards.
        assert_eq!(
            focused(&app),
            Some(tab_button(&mut app, MappingTab::Game)),
            "the selected tab stays in the ring"
        );
        assert_eq!(
            sorted(walk_ring(&mut app)),
            expected_orders(std::iter::repeat(game), [0, 2, 5, 7, 10, 12])
        );
        let disabled_inputs = {
            let world = app.world_mut();
            world
                .query_filtered::<&GameNumericInput, With<InteractionDisabled>>()
                .iter(world)
                .count()
        };
        assert_eq!(disabled_inputs, 7);

        let round_limit_checkbox = entity_with::<GameToggle>(&mut app, |toggle| {
            *toggle == GameToggle::Limit(MatchSettingField::RoundLimit)
        });
        tab_to(&mut app, round_limit_checkbox);
        enter(&mut app);
        assert!(
            app.world()
                .resource::<MatchSettings>()
                .enabled(MatchSettingField::RoundLimit)
        );
        assert_eq!(
            sorted(walk_ring(&mut app)),
            expected_orders(std::iter::repeat(game), [0, 1, 2, 5, 7, 10, 12]),
            "the enabled field joins the ring"
        );

        open_tab(&mut app, MappingTab::Keyboard);
        let keyboard = panel_focus_order(MappingTab::Keyboard);
        assert_eq!(
            sorted(walk_ring(&mut app)),
            expected_orders(std::iter::repeat(keyboard), 0..15)
        );

        open_tab(&mut app, MappingTab::Controller);
        let controller = panel_focus_order(MappingTab::Controller);
        // The panel joined the ring in the frame the tab switched: from the
        // Controller tab, the last of the tab bar, one Tab is already the
        // panel's first row.
        tap(&mut app, KeyCode::Tab);
        assert_eq!(order_of(&app, focused(&app).unwrap()), controller);
        // Unsorted this time: the ring walks the two columns top to bottom,
        // then the footer, then wraps through the tab bar.
        let mut sequence: Vec<u32> = (0..16).map(|offset| controller + offset).collect();
        sequence.extend([FOOTER_FOCUS_ORDER, FOOTER_FOCUS_ORDER + 1, 0, 1, 2, 3]);
        assert_eq!(
            walk_ring(&mut app),
            sequence,
            "rows with no pad assigned stay in the ring, in drawn order"
        );
        // And they stay for a reason: pressing one explains itself.
        let black_up = entity_with::<ControllerMappingBinding>(&mut app, |binding| {
            binding.0 == Player::One && binding.1 == ProfileAction::Up
        });
        tab_to(&mut app, black_up);
        enter(&mut app);
        assert!(mapping(&app).error.contains("Connect a controller"));
        assert_eq!(mapping(&app).capturing_controller, None);

        // And back to the first tab: nothing of the others lingers.
        open_tab(&mut app, MappingTab::Preferences);
        assert_eq!(
            sorted(walk_ring(&mut app)),
            expected_orders(std::iter::repeat(preferences), 0..11)
        );
    }

    /// The part that must not go wrong. `Back` carries `BackToHome`, read by
    /// the ungated HUD button reader that no Settings system can swallow, so
    /// the confirmation has to genuinely trap the ring: while it is open the
    /// ring is its two buttons and nothing else, in either direction,
    /// however far it is walked; it seeds on Cancel; Cancel, Confirm and
    /// Escape each close it without leaving Settings; and closing it hands
    /// the ring back to Reset Controls, where it was.
    #[test]
    fn the_reset_confirmation_traps_the_ring_and_back_stays_unreachable_while_it_is_open() {
        let mut app = settings_app();
        app.world_mut()
            .resource_mut::<KeyboardBindings>()
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        let reset = action_button(&mut app, HudAction::RequestResetMappings);
        let back = action_button(&mut app, HudAction::BackToHome);
        let cancel = action_button(&mut app, HudAction::CancelResetMappings);
        let confirm = action_button(&mut app, HudAction::ConfirmResetMappings);
        let bound_key = |app: &App| {
            app.world()
                .resource::<KeyboardBindings>()
                .key(GameAction::PlayerOneSelect)
        };

        tab_to(&mut app, reset);
        enter(&mut app);
        assert!(mapping(&app).reset_confirmation_open);
        assert_eq!(
            focused(&app),
            Some(cancel),
            "the dialog seeds on Cancel, never on the destructive button"
        );

        // Forty presses, both directions: the ring never leaves the dialog
        // and never lands on Back.
        for step in 0..20 {
            tap(&mut app, KeyCode::Tab);
            let now = focused(&app);
            assert!(
                now == Some(cancel) || now == Some(confirm),
                "step {step}: {now:?}"
            );
            assert_ne!(now, Some(back));
            assert!(stays_on_settings(&app), "step {step}");
        }
        for step in 0..20 {
            tap_shift_tab(&mut app);
            let now = focused(&app);
            assert!(
                now == Some(cancel) || now == Some(confirm),
                "step {step}: {now:?}"
            );
            assert_ne!(now, Some(back));
            assert!(stays_on_settings(&app), "step {step}");
        }
        assert!(mapping(&app).reset_confirmation_open);
        assert!(stays_on_settings(&app));

        // Cancel closes it, keeps the custom binding, and returns the ring
        // to Reset Controls rather than to the screen's first control.
        tab_to(&mut app, cancel);
        enter(&mut app);
        assert!(!mapping(&app).reset_confirmation_open);
        assert_eq!(bound_key(&app), KeyboardKey::F8);
        assert_eq!(focused(&app), Some(reset));
        assert!(stays_on_settings(&app));

        // Confirm resets every binding and stays on Settings.
        enter(&mut app);
        assert!(mapping(&app).reset_confirmation_open);
        tab_to(&mut app, confirm);
        enter(&mut app);
        assert!(!mapping(&app).reset_confirmation_open);
        assert_eq!(
            bound_key(&app),
            KeyboardBindings::default().key(GameAction::PlayerOneSelect)
        );
        assert_eq!(focused(&app), Some(reset));
        assert!(stays_on_settings(&app));

        // Escape closes it too, and the ring is where it was.
        enter(&mut app);
        assert!(mapping(&app).reset_confirmation_open);
        tap(&mut app, KeyCode::Escape);
        assert!(!mapping(&app).reset_confirmation_open);
        assert_eq!(focused(&app), Some(reset));
        assert!(stays_on_settings(&app));

        // Back is reachable again, exactly one step on.
        tap(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(back));

        // The riskiest case: the ring parked on Back when a pointer opens
        // the dialog. The dialog takes the ring in that same frame, before
        // any key can press, and hands it back to Back after.
        click(&mut app, reset);
        assert!(mapping(&app).reset_confirmation_open);
        assert_eq!(focused(&app), Some(cancel));
        assert!(stays_on_settings(&app));
        enter(&mut app);
        assert!(!mapping(&app).reset_confirmation_open);
        assert_eq!(focused(&app), Some(back));
        assert!(stays_on_settings(&app));

        // The positive control: the hazard is live in this graph. With no
        // dialog open, Enter on Back leaves Settings through the ungated HUD
        // reader - exactly what the trap above has to keep out of reach.
        enter(&mut app);
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Home)
        ));
    }

    /// The same trap by pad: South opens the confirmation, the D-pad walks
    /// only its two buttons in every direction, and South on Cancel closes
    /// it with the ring back on Reset Controls.
    #[test]
    fn the_reset_confirmation_traps_a_pad_too() {
        let mut app = settings_app();
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        let reset = action_button(&mut app, HudAction::RequestResetMappings);
        let back = action_button(&mut app, HudAction::BackToHome);
        let cancel = action_button(&mut app, HudAction::CancelResetMappings);
        let confirm = action_button(&mut app, HudAction::ConfirmResetMappings);

        pad_to(&mut app, pad, reset);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert!(mapping(&app).reset_confirmation_open);
        assert_eq!(focused(&app), Some(cancel));
        for button in [
            GamepadButton::DPadRight,
            GamepadButton::DPadDown,
            GamepadButton::DPadLeft,
            GamepadButton::DPadUp,
            GamepadButton::DPadRight,
            GamepadButton::DPadRight,
        ] {
            tap_pad(&mut app, pad, button);
            let now = focused(&app);
            assert!(
                now == Some(cancel) || now == Some(confirm),
                "{button:?}: {now:?}"
            );
            assert_ne!(now, Some(back));
            assert!(stays_on_settings(&app));
        }
        pad_to(&mut app, pad, cancel);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert!(!mapping(&app).reset_confirmation_open);
        assert_eq!(focused(&app), Some(reset));
        assert!(stays_on_settings(&app));
    }

    /// The case the yield's dialog gate exists for: a pointer requests a
    /// reset while a numeric editor holds text that will not commit, so the
    /// editor stays open *under* the dialog. The dialog's ring must still
    /// move and press; only once it closes does the editor own the keys
    /// again, until Escape abandons it.
    #[test]
    fn a_reset_dialog_over_a_failed_commit_keeps_its_ring_live() {
        let mut app = settings_app();
        app.world_mut()
            .resource_mut::<MatchSettings>()
            .set_enabled(MatchSettingField::RoundLimit, true);
        open_tab(&mut app, MappingTab::Game);
        let input = entity_with::<GameNumericInput>(&mut app, |input| {
            *input == GameNumericInput::RoundLimit
        });
        let reset = action_button(&mut app, HudAction::RequestResetMappings);
        let cancel = action_button(&mut app, HudAction::CancelResetMappings);
        let confirm = action_button(&mut app, HudAction::ConfirmResetMappings);

        tab_to(&mut app, input);
        enter(&mut app);
        assert!(mapping(&app).game_editor.is_some());
        for _ in 0..3 {
            type_key(&mut app, KeyCode::Backspace, Key::Backspace);
        }
        assert_eq!(
            mapping(&app).game_editor.as_ref().unwrap().editor.text,
            "",
            "an empty field is the text that will not commit"
        );

        // Only the pointer can reach Reset Controls now - the ring is
        // yielded - and its commit fails, leaving the editor open under the
        // dialog it opens.
        click(&mut app, reset);
        let state = mapping(&app);
        assert!(state.reset_confirmation_open);
        assert!(state.game_editor.is_some());
        assert!(!state.owns_raw_input());

        assert_eq!(focused(&app), Some(cancel));
        // The click moved the pointer, which hid the ring, so the first Tab
        // only draws it again where it is.
        assert!(!app.world().resource::<InputFocusVisible>().0);
        tap(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(cancel));
        tap(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(confirm), "the dialog's ring moves");
        tap(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(cancel));
        // A digit typed at the suspended editor goes unread under the
        // dialog and is still in flight on the next frame, when Enter
        // presses Cancel; the resuming capture must drain both, or the
        // digit would land and the Enter would commit it.
        type_key(&mut app, KeyCode::Digit5, Key::Character("5".into()));
        assert!(mapping(&app).reset_confirmation_open);
        enter(&mut app);
        assert!(
            !mapping(&app).reset_confirmation_open,
            "the dialog's ring presses"
        );
        assert!(stays_on_settings(&app));

        // The dialog gone, the still-open editor owns the keys again: the
        // ring is back on its field and frozen there, its text untouched.
        assert_eq!(focused(&app), Some(input));
        assert_eq!(
            mapping(&app)
                .game_editor
                .as_ref()
                .map(|editor| editor.editor.text.as_str()),
            Some(""),
            "neither the stray digit nor the Enter that pressed Cancel reached the editor"
        );
        tap(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(input));
        tap(&mut app, KeyCode::Escape);
        assert!(mapping(&app).game_editor.is_none());
        assert!(stays_on_settings(&app));
        tap(&mut app, KeyCode::Tab);
        assert_ne!(focused(&app), Some(input), "abandoned, the ring moves on");
    }

    /// While a keyboard binding row is listening, a navigation key is the
    /// key being bound: Tab neither moves the ring nor is lost, it becomes
    /// the binding, and the ring moves again on the next press. Escape
    /// cancels a capture without leaving Settings.
    #[test]
    fn a_keyboard_capture_yields_the_ring_and_the_next_key_binds_instead_of_moving() {
        let mut app = settings_app();
        open_tab(&mut app, MappingTab::Keyboard);
        let up =
            entity_with::<MappingBinding>(&mut app, |binding| binding.0 == GameAction::PlayerTwoUp);
        let down = entity_with::<MappingBinding>(&mut app, |binding| {
            binding.0 == GameAction::PlayerTwoDown
        });
        let key_of = |app: &App, action| app.world().resource::<KeyboardBindings>().key(action);

        tab_to(&mut app, up);
        enter(&mut app);
        assert_eq!(mapping(&app).capturing, Some(GameAction::PlayerTwoUp));
        assert!(
            app.world().resource::<FocusYield>().0,
            "the yield is raised for the coming frame"
        );
        assert_eq!(
            key_of(&app, GameAction::PlayerTwoUp),
            KeyboardKey::E,
            "the Enter that started the capture is not captured"
        );

        tap(&mut app, KeyCode::Tab);
        assert_eq!(focused(&app), Some(up), "the ring did not move");
        assert_eq!(
            key_of(&app, GameAction::PlayerTwoUp),
            KeyboardKey::Tab,
            "the key went to the capture"
        );
        assert_eq!(mapping(&app).capturing, None);

        tap(&mut app, KeyCode::Tab);
        assert_eq!(
            focused(&app),
            Some(down),
            "the capture over, the ring moves"
        );

        enter(&mut app);
        assert_eq!(mapping(&app).capturing, Some(GameAction::PlayerTwoDown));
        tap(&mut app, KeyCode::Escape);
        assert_eq!(mapping(&app).capturing, None);
        assert!(
            stays_on_settings(&app),
            "Escape cancels the capture, nothing more"
        );
        assert_eq!(key_of(&app, GameAction::PlayerTwoDown), KeyboardKey::D);
        tap(&mut app, KeyCode::Tab);
        assert_ne!(focused(&app), Some(down));
    }

    /// The same with a pad, driven by the pad alone: the D-pad walks the
    /// ring and South presses, and while a controller row is listening a
    /// D-pad press is the input being bound rather than a move. Start
    /// cancels the capture without leaving Settings.
    #[test]
    fn a_controller_capture_yields_the_ring_and_the_pad_binds_instead_of_moving() {
        let mut app = settings_app();
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.world_mut()
            .resource_mut::<ControllerAssignments>()
            .connect(pad, DeviceFamily::Xbox, "Pad".into());
        assert_eq!(
            app.world()
                .resource::<ControllerAssignments>()
                .assigned_entity(Player::Two),
            Some(pad),
            "the first pad fills White/Player 2"
        );
        let input_of = |app: &App, action| {
            app.world()
                .resource::<ControllerBindings>()
                .input(Player::Two, action)
        };

        let controller_tab = tab_button(&mut app, MappingTab::Controller);
        pad_to(&mut app, pad, controller_tab);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert_eq!(mapping(&app).tab, MappingTab::Controller);

        let up = entity_with::<ControllerMappingBinding>(&mut app, |binding| {
            binding.0 == Player::Two && binding.1 == ProfileAction::Up
        });
        let down = entity_with::<ControllerMappingBinding>(&mut app, |binding| {
            binding.0 == Player::Two && binding.1 == ProfileAction::Down
        });
        pad_to(&mut app, pad, up);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert_eq!(
            mapping(&app).capturing_controller,
            Some((Player::Two, ProfileAction::Up))
        );
        assert_eq!(
            input_of(&app, ProfileAction::Up),
            ControllerInput::DPadUp,
            "the South that started the capture is not captured"
        );
        // A thumb holds South for several frames after the press; a held
        // button is not a fresh press and is not captured either.
        {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            gamepad.digital_mut().press(GamepadButton::South);
            gamepad.digital_mut().clear();
        }
        app.update();
        assert_eq!(input_of(&app, ProfileAction::Up), ControllerInput::DPadUp);
        assert_eq!(
            mapping(&app).capturing_controller,
            Some((Player::Two, ProfileAction::Up))
        );
        {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            gamepad.digital_mut().release(GamepadButton::South);
            gamepad.digital_mut().clear();
        }

        tap_pad(&mut app, pad, GamepadButton::DPadRight);
        assert_eq!(focused(&app), Some(up), "the ring did not move");
        assert_eq!(
            input_of(&app, ProfileAction::Up),
            ControllerInput::DPadRight,
            "the press went to the capture"
        );
        assert_eq!(mapping(&app).capturing_controller, None);

        tap_pad(&mut app, pad, GamepadButton::DPadDown);
        assert_eq!(
            focused(&app),
            Some(down),
            "the capture over, the ring moves"
        );

        tap_pad(&mut app, pad, GamepadButton::South);
        assert_eq!(
            mapping(&app).capturing_controller,
            Some((Player::Two, ProfileAction::Down))
        );
        tap_pad(&mut app, pad, GamepadButton::Start);
        assert_eq!(mapping(&app).capturing_controller, None);
        assert!(
            stays_on_settings(&app),
            "Start cancels the capture, nothing more"
        );
        assert_eq!(
            input_of(&app, ProfileAction::Down),
            ControllerInput::DPadDown
        );
        tap_pad(&mut app, pad, GamepadButton::DPadDown);
        assert_ne!(focused(&app), Some(down));
    }

    /// While a numeric editor is open the arrows move its caret, not the
    /// ring; digits land in it; Enter commits and frees the ring; Escape
    /// abandons the text and frees it too.
    #[test]
    fn a_numeric_editor_yields_the_ring_until_it_commits_or_is_abandoned() {
        let mut app = settings_app();
        app.world_mut()
            .resource_mut::<MatchSettings>()
            .set_enabled(MatchSettingField::RoundLimit, true);
        open_tab(&mut app, MappingTab::Game);
        let input = entity_with::<GameNumericInput>(&mut app, |input| {
            *input == GameNumericInput::RoundLimit
        });
        let text = |app: &App| {
            mapping(app)
                .game_editor
                .as_ref()
                .map(|editor| editor.editor.text.clone())
        };
        let value = |app: &App| {
            app.world()
                .resource::<MatchSettings>()
                .value(MatchSettingField::RoundLimit)
        };

        tab_to(&mut app, input);
        enter(&mut app);
        assert_eq!(text(&app).as_deref(), Some("100"));

        type_key(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
        assert_eq!(
            focused(&app),
            Some(input),
            "an arrow moves the caret, not the ring"
        );
        type_key(&mut app, KeyCode::Digit5, Key::Character("5".into()));
        assert_eq!(text(&app).as_deref(), Some("1050"), "the caret had moved");
        type_key(&mut app, KeyCode::End, Key::End);
        for _ in 0..3 {
            type_key(&mut app, KeyCode::Backspace, Key::Backspace);
        }
        type_key(&mut app, KeyCode::Digit5, Key::Character("5".into()));
        assert_eq!(text(&app).as_deref(), Some("15"));
        assert_eq!(focused(&app), Some(input));

        enter(&mut app);
        assert_eq!(text(&app), None, "Enter commits");
        assert_eq!(value(&app), 15);
        tap(&mut app, KeyCode::Tab);
        assert_ne!(focused(&app), Some(input), "committed, the ring moves on");

        tab_to(&mut app, input);
        enter(&mut app);
        type_key(&mut app, KeyCode::Digit9, Key::Character("9".into()));
        assert_eq!(text(&app).as_deref(), Some("159"));
        tap(&mut app, KeyCode::Escape);
        assert_eq!(text(&app), None, "Escape abandons");
        assert_eq!(value(&app), 15);
        assert!(stays_on_settings(&app));
        tap(&mut app, KeyCode::Tab);
        assert_ne!(focused(&app), Some(input));
    }

    /// The pointer reaches every control on each tab at its centre, under
    /// real layout and real picking - nothing drawn over the panel, the tab
    /// bar or the footer stops it, which matters because picking stops at
    /// the first node under the pointer whatever that node is - and with
    /// the reset confirmation open, its two buttons and nothing beneath
    /// them. Each tab's sweep covers at least every control in its ring.
    #[test]
    fn the_pointer_reaches_every_control_on_every_tab_and_the_dialog_covers_the_rest() {
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::UiFocusState;

        let mut app = settings_app();
        for tab in [
            MappingTab::Preferences,
            MappingTab::Game,
            MappingTab::Keyboard,
            MappingTab::Controller,
        ] {
            open_tab(&mut app, tab);
            let shown = reachable_buttons(&mut app);
            let ring = app.world().resource::<UiFocusState>().candidate_entities();
            assert!(!ring.is_empty());
            for control in &ring {
                assert!(shown.contains(control), "{tab:?}: {control:?} is not shown");
            }
            assert_eq!(covered_at_their_centres(&mut app, &shown), [], "{tab:?}");
        }

        let tab_controls = reachable_buttons(&mut app);
        app.world_mut()
            .resource_mut::<ControlMappingState>()
            .reset_confirmation_open = true;
        app.update();
        app.update();
        let dialog = [
            action_button(&mut app, HudAction::CancelResetMappings),
            action_button(&mut app, HudAction::ConfirmResetMappings),
        ];
        assert_eq!(covered_at_their_centres(&mut app, &dialog), []);
        assert_eq!(
            covered_at_their_centres(&mut app, &tab_controls),
            tab_controls,
            "under the dialog, every control is covered"
        );
    }

    /// The same-frame race this screen inherited: a pointer click on Cancel
    /// and Escape in one frame pop one layer, not the dialog and then the
    /// screen. Back runs first, so the click finds the dialog already
    /// closed and closing it again is a no-op.
    #[test]
    fn a_same_frame_cancel_click_and_escape_pop_only_one_layer() {
        let mut app = settings_app();
        let reset = action_button(&mut app, HudAction::RequestResetMappings);
        let cancel = action_button(&mut app, HudAction::CancelResetMappings);
        tab_to(&mut app, reset);
        enter(&mut app);
        assert!(mapping(&app).reset_confirmation_open);

        // A click activates on its release, so that is the frame Escape
        // shares.
        press_on(&mut app, cancel);
        assert!(
            mapping(&app).reset_confirmation_open,
            "a press alone does not"
        );
        queue_release(&mut app);
        press_key(&mut app, KeyCode::Escape);
        app.update();
        release_keys(&mut app);

        assert!(!mapping(&app).reset_confirmation_open);
        assert!(
            stays_on_settings(&app),
            "one dismissal, not a dismissal plus an exit to Home"
        );
    }

    /// Escape and Enter landing in one frame with the ring on Confirm
    /// Reset: back wins the tie, so nothing is reset and the screen stays.
    /// Before back ran first, the pair reset every binding *and* left.
    #[test]
    fn a_same_frame_escape_and_confirm_reset_resets_nothing_and_stays() {
        let mut app = settings_app();
        app.world_mut()
            .resource_mut::<KeyboardBindings>()
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        let reset = action_button(&mut app, HudAction::RequestResetMappings);
        let confirm = action_button(&mut app, HudAction::ConfirmResetMappings);
        tab_to(&mut app, reset);
        enter(&mut app);
        tab_to(&mut app, confirm);

        press_key(&mut app, KeyCode::Escape);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        release_keys(&mut app);

        assert!(!mapping(&app).reset_confirmation_open);
        assert_eq!(
            app.world()
                .resource::<KeyboardBindings>()
                .key(GameAction::PlayerOneSelect),
            KeyboardKey::F8,
            "the destructive read of the pair loses the tie"
        );
        assert!(stays_on_settings(&app));
    }

    /// The one pure-keyboard pair the back-first ordering decides, pinned so
    /// it is a choice rather than a discovery: Escape and Enter in one frame
    /// with a number open abandon the typed value and stay on Settings -
    /// back wins the tie, as it does on the Match screen. Before back ran
    /// first, the pair committed the value and then left.
    #[test]
    fn escape_and_enter_in_one_frame_abandon_an_open_number() {
        let mut app = settings_app();
        app.world_mut()
            .resource_mut::<MatchSettings>()
            .set_enabled(MatchSettingField::RoundLimit, true);
        open_tab(&mut app, MappingTab::Game);
        let input = entity_with::<GameNumericInput>(&mut app, |input| {
            *input == GameNumericInput::RoundLimit
        });
        tab_to(&mut app, input);
        enter(&mut app);
        type_key(&mut app, KeyCode::Digit7, Key::Character("7".into()));
        assert_eq!(
            mapping(&app).game_editor.as_ref().unwrap().editor.text,
            "1007"
        );

        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        press_key(&mut app, KeyCode::Escape);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        release_keys(&mut app);

        assert!(mapping(&app).game_editor.is_none(), "the session is closed");
        assert_eq!(
            app.world()
                .resource::<MatchSettings>()
                .value(MatchSettingField::RoundLimit),
            100,
            "abandoned, not committed"
        );
        assert!(stays_on_settings(&app));
    }

    /// The louder direction the same ordering decides, pinned so it is a
    /// choice rather than a discovery: Escape and a click that would arm a
    /// binding capture, in one frame, leave Settings - back reads the
    /// screen with nothing open and steps out, and the capture is armed on
    /// a screen that is already leaving.
    #[test]
    fn escape_and_a_same_frame_press_that_arms_a_capture_leave_settings() {
        let mut app = settings_app();
        open_tab(&mut app, MappingTab::Keyboard);
        let row =
            entity_with::<MappingBinding>(&mut app, |binding| binding.0 == GameAction::PlayerTwoUp);

        press_on(&mut app, row);
        assert_eq!(mapping(&app).capturing, None, "a press alone does not arm");
        queue_release(&mut app);
        press_key(&mut app, KeyCode::Escape);
        app.update();
        release_keys(&mut app);

        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Home)
        ));
        assert_eq!(
            mapping(&app).capturing,
            Some(GameAction::PlayerTwoUp),
            "the press still arms, on a screen that is leaving"
        );
    }
}
