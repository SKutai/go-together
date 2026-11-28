//! How the Replay Library joins the keyboard and controller focus ring: the
//! premise the ring rests on, and where each control sits in it.
//!
//! # The premise: focus survives the rebuild
//!
//! This is the one screen whose entities are despawned and respawned with no
//! `AppScreen` transition. `refresh_library_screen` destroys and rebuilds the
//! whole tree whenever `dirty` is set - on every search keystroke, every sort
//! change, every rename and delete. The focus core keys its memory by
//! `(group, order)` rather than by `Entity` for exactly this screen, and
//! until this screen had a ring that key had never met the case it exists
//! for: none of the other consumers - Home, Settings, the Replay Viewer -
//! ever respawns.
//!
//! The argument that it works is a schedule trace. The despawn and the spawn
//! are queued into one `Commands` buffer and both apply at the end of
//! `Update`; `ui_layout_system` lays the new tree out in `PostUpdate`; and
//! the core's collector runs `.after(UiSystems::Layout)`, so it sees the new
//! entities at their real sizes in the same frame, resolves the remembered
//! key against them, and the ring never blinks. If any step of that were
//! wrong the ring would go out on every keystroke.
//! `focus_holds_its_place_on_every_frame_of_a_dirty_rebuild` runs that trace
//! rather than arguing it: the screen's own tree under real `bevy_ui`
//! layout, the production plugin entered through a real state transition,
//! and four rebuilds in a row asserted on every frame.
//!
//! # Bands
//!
//! One [`FocusGroup::Screen`] ring covers the whole screen, for the reason
//! Settings gives: the core has no cross-group movement, so a second
//! `Screen`-level group would be a ring a player could enter and not leave.
//! Orders are banded rather than consecutive, so a band can gain a control
//! without renumbering its neighbours, and within a band a [`RingOrder`]
//! hands out consecutive positions in the order the spawner draws its
//! controls: Home from [`HOME_FOCUS_ORDER`]; the toolbar - Search, Clear,
//! the sort field, the sort order - from [`TOOLBAR_FOCUS_ORDER`]; the two
//! paging buttons from [`PAGINATION_FOCUS_ORDER`]; and each card's three
//! buttons from [`card_focus_order`], a stride of [`CARD_FOCUS_STRIDE`] per
//! row of the page on show.
//!
//! **Pagination is numbered before the cards although it is drawn after
//! them, and that is deliberate.** The core clamps downward only: when a
//! remembered order is gone the ring lands on the *nearest lower* one - the
//! highest order still below it - so a band's number decides where the ring
//! goes when a control vanishes.
//!
//! The case that decides the band order is **Page Up pressed back onto the
//! first page**, which disables Page Up under the ring that just pressed it.
//! Numbered before the cards, the nearest lower order is the sort-order
//! button at the end of the toolbar band. Numbered after them, it would be
//! the final card's *Delete*. A later reader tidying these bands into draw
//! order would give that up, and
//! `disabled_paging_buttons_leave_the_ring_from_their_spawn_frame` is the test
//! that would stop them.
//!
//! Note that the mirror case decides nothing: Page Down vanishing at the end
//! of the list always lands on Page Up, whichever band comes first, because
//! the two are allocated as an adjacent pair and Page Up is necessarily
//! enabled when the player has just paged down.
//!
//! # The two dialogs
//!
//! Both dialogs are the screen's one [`FocusGroup::Modal`] group, so the
//! frame either is spawned its buttons own the ring and the frame it is
//! despawned the ring is back where it was on the screen underneath. Each
//! seeds where its first entry is, and the two seed differently on purpose.
//! The delete dialog seeds on *Cancel* - order 0, with Delete Permanently
//! order 1 although it is drawn first - because with two entries the walk
//! direction is invisible and only the seed shows, so it must not be the
//! destructive one; the core's own memory makes the same judgement when it
//! forgets the modal slot every time the ring leaves it. The rename dialog
//! seeds on *Save Label*, then Clear and Cancel in drawn order: while it is
//! open the ring is yielded to its text field, so the ordering is a legend
//! rather than a walk, and the one position ever shown should name what
//! Enter does.
//!
//! # Yielding while a text field owns the keys
//!
//! [`yield_focus_while_editing`] raises the core's [`FocusYield`] for every
//! frame the rename dialog is open or the search field is focused. While it
//! is raised the ring neither moves nor presses, for the reason Settings
//! gives: the arrows move the caret, Space is a character and Enter is the
//! field's own submit, so a ring that still moved would wander as the
//! player typed and one that still pressed would fire a second control on
//! the Enter that finished the text. It is not raised under the delete
//! dialog, which has no field: its two buttons are exactly what the ring
//! must keep moving between and pressing, and `capture_library_text` reads
//! no key under it either. The two share one predicate,
//! `ReplayLibraryUiState::owns_raw_input`, so they cannot disagree.
//!
//! The player leaves each field the way the pointer does, plus one way the
//! pointer never needed. Escape drops search focus and cancels the rename,
//! Enter saves the rename - and Enter also leaves the search field. That is
//! a small change a pointer user sees too, and it is wanted: Escape was the
//! field's only exit and Escape's next press leaves the screen, so a
//! keyboard player had one way out and pressing it twice ejected them.
//! Filtering is live as the player types, so Enter has nothing to commit and
//! is free to mean "done".
//!
//! A pad can open both fields and only back out of them, with East or
//! Start. The fields read keyboard messages and nothing else, as they did
//! before the ring, and giving the D-pad a role inside a text field would
//! change what the control does; Settings' numeric fields carry the same
//! limit, recorded the same way.
//!
//! # Disabled paging buttons
//!
//! Page Up and Page Down leave the ring while they are disabled - on the
//! first and the last page - by carrying `bevy_ui`'s `InteractionDisabled`,
//! which the collector filters on, as the Replay Viewer's transport and
//! Settings' numeric fields do. On this screen the marker is computed *at
//! spawn*, in `layout`'s button spawner, and `update_library_button_styles`
//! reconciles it only as defence: that system runs before
//! `refresh_library_screen` in the chain, so on a dirty frame it styles the
//! dying entities, and a marker written there alone would reach the rebuilt
//! buttons one frame late. The core rewrites its memory every frame, so
//! that would be a frame in which a dead paging button was in the ring with
//! its order recorded.
//!
//! # A change of the window's size
//!
//! A card's buttons are keyed by its place on the page, which is what keeps
//! the ring on the same place of the screen as the list scrolls under it.
//! A change of size changes how many cards a page holds, not which card is
//! first, so for every card still shown its place, and the ring's, stays the
//! same - with one exception. Where a larger page would run past the end of
//! the list, the first card moves back and every card on the page moves
//! down by as many places; there the screen moves the ring's remembered
//! place down with them ([`follow_cards_down_the_page`]), so the ring stays
//! on the Replay it was on rather than on the place it had. The focus core
//! offers that move for this screen alone; see its module docs. A card that
//! a smaller page has no room for leaves the ring with it, and the ring
//! falls to the last card still shown - its Delete, the nearest lower order,
//! as after a confirmed delete below.
//!
//! # Two residuals, accepted
//!
//! Delete is the last button in its row, and rows must walk in draw order,
//! so after a confirmed delete the ring lands on a Delete button: the same
//! row's if a later card moved up into it, the previous row's if the page
//! shrank. That is reachable only after a confirmed delete, the ring is
//! drawn when it happens, and pressing it opens a confirmation that names
//! the Replay and seeds on Cancel. It is stated here rather than designed
//! away because the alternatives - a row walking out of draw order, or a
//! band per row the core would have to be taught to leave - cost more than
//! it does. The paging band avoids the same shape by its numbering rather
//! than by accepting it; see the band order above.
//!
//! The other is the core's, not this screen's: the collector rewrites the
//! remembered key every frame, the ring drawn or not, so a pointer that
//! narrows the list while the ring is hidden can move where it next
//! appears: downward, onto something drawn and harmless. Settings accepted
//! the same thing, and the decision and its shape are written up in
//! `docs/ideas/focus-memory-hidden-clamp.md`.
//!
//! [`FocusGroup::Screen`]: crate::ui::focus::FocusGroup
//! [`FocusGroup::Modal`]: crate::ui::focus::FocusGroup

use bevy::prelude::*;

use super::ReplayLibraryUiState;
use crate::ui::focus::{FocusGroup, FocusYield, Focusable, UiFocusState};

/// The Home button's ring position, and the screen's first control: a
/// player whose memory holds nothing seeds here, one Enter from leaving.
pub(super) const HOME_FOCUS_ORDER: u32 = 0;

/// The toolbar's first ring position: Search, Clear, the sort field, then
/// the sort order, in drawn order.
pub(super) const TOOLBAR_FOCUS_ORDER: u32 = 100;

/// The paging buttons' first ring position: Page Up, then Page Down. Below
/// the cards' band on purpose - see the module docs.
pub(super) const PAGINATION_FOCUS_ORDER: u32 = 200;

/// The first card's first ring position.
pub(super) const CARDS_FOCUS_ORDER: u32 = 300;

/// The distance between one card's band and the next: three buttons in use,
/// room for more.
pub(super) const CARD_FOCUS_STRIDE: u32 = 10;

/// A card's first ring position: View Replay, then Change Label, then
/// Delete. `place` is the card's place on the page on show - in two
/// columns, counted along each row of two in turn - not in the whole list,
/// so paging keeps the ring on the same place of the screen.
pub(super) const fn card_focus_order(place: usize) -> u32 {
    CARDS_FOCUS_ORDER + CARD_FOCUS_STRIDE * place as u32
}

/// Moves the ring's remembered place on this screen `places` cards further
/// down the page, when it is on a card: what the screen does when a change
/// of the window's size moves every card on the page down by as many
/// places, so that the ring stays on the Replay it was on. See the module
/// docs.
pub(super) fn follow_cards_down_the_page(focus: &mut UiFocusState, places: usize) {
    let screen = crate::app::AppScreen::ReplayLibrary;
    let Some(order) = focus.remembered(screen, FocusGroup::Screen) else {
        return;
    };
    if order >= CARDS_FOCUS_ORDER {
        let moved = order + CARD_FOCUS_STRIDE * places as u32;
        focus.move_remembered(screen, FocusGroup::Screen, moved);
    }
}

/// Hands out consecutive positions within one band, in the order the
/// spawner asks for them - which is the order it draws its controls, so the
/// ring walks the screen the way it reads. The same allocator Settings
/// threads through its spawners, copied rather than shared because the two
/// screens' `ring` modules are private to their screens.
pub(super) struct RingOrder {
    group: FocusGroup,
    next: u32,
}

impl RingOrder {
    /// A [`FocusGroup::Screen`] band starting at `first`.
    pub(super) const fn starting_at(first: u32) -> Self {
        Self {
            group: FocusGroup::Screen,
            next: first,
        }
    }

    /// A dialog's [`FocusGroup::Modal`] ring, from its seed. The order the
    /// positions are handed out in is the ring's order, whatever order the
    /// spawner then draws them in - see the module docs for which button
    /// each dialog seeds on and why.
    pub(super) const fn modal() -> Self {
        Self {
            group: FocusGroup::Modal,
            next: 0,
        }
    }

    pub(super) fn next(&mut self) -> Focusable {
        let order = self.next;
        self.next += 1;
        Focusable {
            group: self.group,
            order,
        }
    }
}

/// Raises [`FocusYield`] for every frame one of this screen's text fields
/// owns the raw keys - the rename dialog's editor, or the focused search
/// field - see the module docs for why the ring stops moving as well as
/// pressing, and why the delete dialog never yields. Runs last in the
/// Library's `Update` chain, after every way a field can gain or lose the
/// keys this frame; the core reads and lowers the flag in the next frame's
/// `PreUpdate`, so writing it every live frame is what holds it, and
/// stopping is what releases it.
pub(super) fn yield_focus_while_editing(
    ui: Res<ReplayLibraryUiState>,
    mut focus_yield: ResMut<FocusYield>,
) {
    if ui.owns_raw_input() {
        focus_yield.0 = true;
    }
}

#[cfg(test)]
mod tests {
    use bevy::input_focus::{InputFocus, InputFocusVisible};
    use bevy::ui_widgets::Button;

    use super::super::{
        LibraryAction, LibraryDialog, ReplayLibraryPlugin, ReplayLibraryUiState,
        refresh_library_screen, spawn_library_screen,
    };
    use super::*;
    use crate::app::AppScreen;
    use crate::audio::AudioCueQueue;
    use crate::gameplay::InputGate;
    use crate::presentation::ORANGE;
    use crate::replay_library::{ReplayLibrary, ReplayStore};
    use crate::ui::button::{click, record_activations, recorded_activations};
    use crate::ui::focus::{UiFocusPlugin, UiFocusState, real_layout_app};
    use crate::ui::hud::HudState;
    use crate::ui::replay::viewer::ReplayViewerState;
    use crate::ui::text_input::TextEditor;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::mouse::MouseWheel;
    use bevy::ui::InteractionDisabled;

    /// A library of `count` saved Replays under a temporary root, labelled
    /// `Replay 1` to `Replay {count}` and completed in that order, so the
    /// default sort (newest first) shows the highest number on top.
    fn saved_library(count: usize) -> (ReplayLibrary, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("go-together-library-ring-{}", uuid::Uuid::new_v4()));
        let store = ReplayStore::new(root.clone());
        for index in 1..=count {
            let replay = crate::replay::test_support::sample_replay(
                1_704_067_200_000 + index as i64 * 60_000,
                1_704_067_201_000 + index as i64 * 60_000,
                &format!("Replay {index}"),
            );
            store.save(&replay).unwrap();
        }
        let library = ReplayLibrary::from_store(store);
        assert_eq!(library.entries.len(), count);
        (library, root)
    }

    /// The whole screen under real `bevy_ui` layout and the production
    /// graph: the focus core and [`ReplayLibraryPlugin`] itself - its
    /// `OnEnter` chain and its `Update` chain in their registered order -
    /// entered through a real state transition. One frame has run on the
    /// screen, so it is spawned, laid out and collected, with the ring
    /// parked on Home and not yet drawn. The temporary root is returned so
    /// a test can remove it.
    fn library_app(replays: usize) -> (App, std::path::PathBuf) {
        let (library, root) = saved_library(replays);
        let mut app = real_layout_app(crate::app::visuals::WindowSize::OPENING.0);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(AppScreen::Home)
            .add_plugins(crate::app::visuals::WindowLayoutPlugin)
            .add_plugins(UiFocusPlugin)
            .add_plugins(ReplayLibraryPlugin)
            .insert_resource(library)
            .init_resource::<ReplayViewerState>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .insert_resource(HudState::default())
            .insert_resource(InputGate::default())
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_message::<MouseWheel>();
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ReplayLibrary);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::ReplayLibrary
        );
        (app, root)
    }

    fn focused(app: &App) -> Option<Entity> {
        app.world().resource::<InputFocus>().get()
    }

    fn key_of(app: &App, entity: Entity) -> (FocusGroup, u32) {
        let focusable = app
            .world()
            .get::<Focusable>(entity)
            .expect("a focused entity is a live Focusable");
        (focusable.group, focusable.order)
    }

    /// The one control marked with `key`.
    fn control_at(app: &mut App, key: (FocusGroup, u32)) -> Entity {
        let world = app.world_mut();
        let found: Vec<Entity> = world
            .query::<(Entity, &Focusable)>()
            .iter(world)
            .filter(|(_, focusable)| (focusable.group, focusable.order) == key)
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one control at {key:?}");
        found[0]
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

    /// The one entity carrying a `LibraryAction` that satisfies `matches`.
    fn control(app: &mut App, matches: impl Fn(&LibraryAction) -> bool) -> Entity {
        let world = app.world_mut();
        let found: Vec<Entity> = world
            .query::<(Entity, &LibraryAction)>()
            .iter(world)
            .filter(|(_, action)| matches(action))
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one matching control");
        found[0]
    }

    fn tap_shift_tab(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        tap(app, KeyCode::Tab);
    }

    /// One frame with a real key press: the raw `KeyboardInput` message the
    /// text fields read and the `ButtonInput` state the ring reads,
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

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            if character == ' ' {
                type_key(app, KeyCode::Space, Key::Space);
            } else {
                type_key(
                    app,
                    KeyCode::KeyA,
                    Key::Character(character.to_string().into()),
                );
            }
        }
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

    /// Tabs once round the ring from wherever it is, returning the orders
    /// visited in the sequence they were visited, the start included.
    fn walk_ring(app: &mut App) -> Vec<u32> {
        reveal(app);
        let start = focused(app).expect("something is focused");
        let mut visited = vec![key_of(app, start).1];
        for _ in 0..100 {
            tap(app, KeyCode::Tab);
            let now = focused(app).expect("something is focused");
            if now == start {
                return visited;
            }
            visited.push(key_of(app, now).1);
        }
        panic!("the ring never wrapped: {visited:?}");
    }

    fn ui(app: &App) -> &ReplayLibraryUiState {
        app.world().resource::<ReplayLibraryUiState>()
    }

    fn yielded(app: &App) -> bool {
        app.world().resource::<FocusYield>().0
    }

    fn label_of(app: &App, id: &str) -> Option<String> {
        app.world()
            .resource::<ReplayLibrary>()
            .get(id)
            .map(|entry| entry.label.clone())
    }

    /// No screen change is pending and none has happened: `NextState` is
    /// consumed by the transition one frame after it is set, so the state
    /// itself is checked too, which holds however many frames have passed.
    fn stays_on_library(app: &App) -> bool {
        matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ) && *app.world().resource::<State<AppScreen>>().get() == AppScreen::ReplayLibrary
    }

    /// The Replay id a card button acts on.
    fn card_id(app: &App, entity: Entity) -> String {
        match app.world().get::<LibraryAction>(entity) {
            Some(
                LibraryAction::Play(id)
                | LibraryAction::OpenRename(id)
                | LibraryAction::OpenDelete(id),
            ) => id.clone(),
            _ => panic!("not a card button"),
        }
    }

    /// The premise the whole ring rests on, run rather than argued: with the
    /// ring on a known card, four consecutive dirty rebuilds - the despawn
    /// and respawn every search keystroke, sort change, rename and delete
    /// causes - keep the ring drawn on the same `(group, order)` on every
    /// frame, rebuild frames included. The candidate list is never empty,
    /// `InputFocus` always names a live entity, and the resolved key never
    /// changes; the entity does change on every rebuild, which is the
    /// positive control that the respawn really happened.
    #[test]
    fn focus_holds_its_place_on_every_frame_of_a_dirty_rebuild() {
        let (mut app, root) = library_app(3);
        // The second card's Change Label: the only way onto it is through
        // the ring, so the memory is exactly what a keyboard player left.
        let key = (FocusGroup::Screen, card_focus_order(1) + 1);
        let target = control_at(&mut app, key);
        assert!(matches!(
            app.world().get::<LibraryAction>(target),
            Some(LibraryAction::OpenRename(_))
        ));
        tab_to(&mut app, target);

        let check = |app: &App, when: &str| -> Entity {
            let candidates = app.world().resource::<UiFocusState>().candidate_entities();
            assert!(!candidates.is_empty(), "{when}: the ring must not be empty");
            let entity = focused(app).unwrap_or_else(|| panic!("{when}: nothing is focused"));
            assert!(
                app.world().get_entity(entity).is_ok(),
                "{when}: focus names a despawned entity"
            );
            assert_eq!(key_of(app, entity), key, "{when}: the key moved");
            assert!(
                candidates.contains(&entity),
                "{when}: the focused entity is not a candidate"
            );
            assert_eq!(
                app.world().get::<Outline>(entity).unwrap().color,
                ORANGE,
                "{when}: the ring is not drawn on the focused control"
            );
            entity
        };

        let mut previous = check(&app, "before any rebuild");
        for rebuild in 1..=4 {
            app.world_mut().resource_mut::<ReplayLibraryUiState>().dirty = true;
            app.update();
            assert!(
                app.world().get_entity(previous).is_err(),
                "rebuild {rebuild}: the old entity must be gone, or nothing was rebuilt"
            );
            let now = check(&app, &format!("rebuild {rebuild}"));
            assert_ne!(now, previous, "rebuild {rebuild}: the entity must be new");
            previous = now;
        }
        app.update();
        assert_eq!(
            check(&app, "the frame after"),
            previous,
            "a quiet frame changes nothing"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Every kind of control this screen puts in the ring, so the marking
    /// test can name what it found; card actions carry the Replay's label
    /// rather than its generated id.
    #[derive(Clone, Debug, PartialEq)]
    enum Control {
        Home,
        Search,
        Clear,
        SortField,
        SortOrder,
        PageUp,
        PageDown,
        Play(String),
        Rename(String),
        Delete(String),
        ClearRename,
        SaveLabel,
        ConfirmDelete,
        Cancel,
    }

    fn control_of(library: &ReplayLibrary, action: &LibraryAction) -> Control {
        let label = |id: &str| {
            library
                .get(id)
                .expect("a card names a saved Replay")
                .label
                .clone()
        };
        match action {
            LibraryAction::Home => Control::Home,
            LibraryAction::FocusSearch => Control::Search,
            LibraryAction::ClearSearch => Control::Clear,
            LibraryAction::CycleSortField => Control::SortField,
            LibraryAction::CycleSortOrder => Control::SortOrder,
            LibraryAction::ScrollUp => Control::PageUp,
            LibraryAction::ScrollDown => Control::PageDown,
            LibraryAction::Play(id) => Control::Play(label(id)),
            LibraryAction::OpenRename(id) => Control::Rename(label(id)),
            LibraryAction::OpenDelete(id) => Control::Delete(label(id)),
            LibraryAction::ClearRename => Control::ClearRename,
            LibraryAction::ConfirmRename => Control::SaveLabel,
            LibraryAction::ConfirmDelete => Control::ConfirmDelete,
            LibraryAction::CancelDialog => Control::Cancel,
        }
    }

    /// The ring as marked, sorted by `(group, order)`.
    fn marked_ring(app: &mut App) -> Vec<(FocusGroup, u32, Control)> {
        let world = app.world_mut();
        let mut query = world.query::<(&Focusable, &LibraryAction)>();
        let world: &World = world;
        let library = world.resource::<ReplayLibrary>();
        let mut marked: Vec<_> = query
            .iter(world)
            .map(|(focusable, action)| {
                (
                    focusable.group,
                    focusable.order,
                    control_of(library, action),
                )
            })
            .collect();
        marked.sort_by_key(|(group, order, _)| (*group, *order));
        marked
    }

    /// Marking is authored, not derived: every control carrying a
    /// `LibraryAction` is in the ring, in the bands and the drawn order the
    /// module docs describe - Home, the toolbar, the paging buttons before
    /// the cards, then the page's cards row by row - and the dialogs'
    /// buttons are the screen's `Modal` group. No button is outside the
    /// ring: a dialog's overlay root is a plain node, and it still blocks
    /// the pointer, as the crate's other overlays do.
    #[test]
    fn library_controls_are_focusable_in_drawn_order_and_the_overlay_root_is_not() {
        let (library, root) = saved_library(5);
        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .add_systems(Startup, spawn_library_screen)
            .add_systems(Update, refresh_library_screen);
        app.update();

        let screen = |order: u32, control: Control| (FocusGroup::Screen, order, control);
        let mut expected = vec![
            screen(HOME_FOCUS_ORDER, Control::Home),
            screen(TOOLBAR_FOCUS_ORDER, Control::Search),
            screen(TOOLBAR_FOCUS_ORDER + 1, Control::Clear),
            screen(TOOLBAR_FOCUS_ORDER + 2, Control::SortField),
            screen(TOOLBAR_FOCUS_ORDER + 3, Control::SortOrder),
            screen(PAGINATION_FOCUS_ORDER, Control::PageUp),
            screen(PAGINATION_FOCUS_ORDER + 1, Control::PageDown),
        ];
        // Newest first: the page shows Replays 5 to 2, and Replay 1 waits
        // on the next page.
        for (row, number) in [5, 4, 3, 2].into_iter().enumerate() {
            let label = format!("Replay {number}");
            expected.push(screen(card_focus_order(row), Control::Play(label.clone())));
            expected.push(screen(
                card_focus_order(row) + 1,
                Control::Rename(label.clone()),
            ));
            expected.push(screen(card_focus_order(row) + 2, Control::Delete(label)));
        }
        assert_eq!(marked_ring(&mut app), expected);
        let unmarked_buttons = |app: &mut App| -> usize {
            let world = app.world_mut();
            world
                .query_filtered::<(), (With<Button>, Without<Focusable>)>()
                .iter(world)
                .count()
        };
        assert_eq!(
            unmarked_buttons(&mut app),
            0,
            "with no dialog open every button is in the ring"
        );

        let id = app.world().resource::<ReplayLibrary>().entries[0]
            .id
            .clone();
        let open = |app: &mut App, dialog: LibraryDialog| {
            let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
            ui.dialog = Some(dialog);
            ui.dirty = true;
            app.update();
        };
        open(
            &mut app,
            LibraryDialog::Rename {
                id: id.clone(),
                editor: TextEditor::new("Replay 1".into()),
                error: String::new(),
            },
        );
        let modal: Vec<_> = marked_ring(&mut app)
            .into_iter()
            .filter(|(group, _, _)| *group == FocusGroup::Modal)
            .collect();
        assert_eq!(
            modal,
            [
                (FocusGroup::Modal, 0, Control::SaveLabel),
                (FocusGroup::Modal, 1, Control::ClearRename),
                (FocusGroup::Modal, 2, Control::Cancel),
            ],
            "the rename dialog seeds on Save Label, which is what Enter does"
        );
        assert_eq!(
            unmarked_buttons(&mut app),
            0,
            "a dialog's overlay root is no button, so it is not in the ring"
        );

        open(
            &mut app,
            LibraryDialog::Delete {
                id,
                error: String::new(),
            },
        );
        let modal: Vec<_> = marked_ring(&mut app)
            .into_iter()
            .filter(|(group, _, _)| *group == FocusGroup::Modal)
            .collect();
        assert_eq!(
            modal,
            [
                (FocusGroup::Modal, 0, Control::Cancel),
                (FocusGroup::Modal, 1, Control::ConfirmDelete),
            ],
            "the delete dialog seeds on Cancel, never on the destructive button"
        );
        assert_eq!(unmarked_buttons(&mut app), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The pointer reaches every control the screen shows at its centre,
    /// a full page of cards included, under real layout and real picking -
    /// nothing drawn over the toolbar, the cards or the paging buttons stops
    /// it - and with either dialog open, the dialog's buttons and nothing
    /// beneath them.
    #[test]
    fn the_pointer_reaches_every_library_control_and_a_dialog_covers_the_rest() {
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};

        let (mut app, root) = library_app(5);
        let screen = reachable_buttons(&mut app);
        assert_eq!(
            screen.len(),
            app.world()
                .resource::<UiFocusState>()
                .candidate_entities()
                .len()
                + 1,
            "the ring's controls and the disabled Page Up, which is shown but \
             not in the ring"
        );
        assert_eq!(covered_at_their_centres(&mut app, &screen), []);

        let id = app.world().resource::<ReplayLibrary>().entries[0]
            .id
            .clone();
        let dialogs = [
            LibraryDialog::Rename {
                id: id.clone(),
                editor: TextEditor::new("Replay 1".into()),
                error: String::new(),
            },
            LibraryDialog::Delete {
                id,
                error: String::new(),
            },
        ];
        for dialog in dialogs {
            {
                let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
                ui.dialog = Some(dialog);
                ui.dirty = true;
            }
            app.update();
            let shown = reachable_buttons(&mut app);
            let (in_dialog, beneath): (Vec<_>, Vec<_>) = shown.into_iter().partition(|entity| {
                app.world()
                    .get::<Focusable>(*entity)
                    .is_some_and(|focusable| focusable.group == FocusGroup::Modal)
            });
            assert!(!in_dialog.is_empty());
            assert_eq!(covered_at_their_centres(&mut app, &in_dialog), []);
            assert_eq!(covered_at_their_centres(&mut app, &beneath), beneath);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Either dialog's overlay is a plain node, and it blocks every click
    /// to the screen beneath it: a click on the toolbar's sort order, under
    /// the overlay, reaches no button at all. The same click with the
    /// dialog closed sorts the list, so it was the overlay that held it
    /// back. Under real layout and real picking, through [`library_app`].
    #[test]
    fn a_dialog_overlay_blocks_every_click_to_the_screen_beneath() {
        let (mut app, root) = library_app(5);
        record_activations(&mut app);
        let id = app.world().resource::<ReplayLibrary>().entries[0]
            .id
            .clone();
        let sort_order = |app: &App| app.world().resource::<ReplayLibraryUiState>().sort_order;
        let sort_button = |app: &mut App| {
            control(app, |action| {
                matches!(action, LibraryAction::CycleSortOrder)
            })
        };
        let dialogs = [
            LibraryDialog::Rename {
                id: id.clone(),
                editor: TextEditor::new("Replay 1".into()),
                error: String::new(),
            },
            LibraryDialog::Delete {
                id,
                error: String::new(),
            },
        ];
        for dialog in dialogs {
            {
                let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
                ui.dialog = Some(dialog);
                ui.dirty = true;
            }
            app.update();
            let before = sort_order(&app);
            let under_the_overlay = sort_button(&mut app);
            click(&mut app, under_the_overlay);
            assert!(recorded_activations(&mut app).is_empty());
            assert_eq!(sort_order(&app), before);
            assert!(ui(&app).dialog.is_some());

            {
                let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
                ui.dialog = None;
                ui.dirty = true;
            }
            app.update();
            let uncovered = sort_button(&mut app);
            click(&mut app, uncovered);
            assert_eq!(recorded_activations(&mut app), [uncovered]);
            assert_ne!(sort_order(&app), before);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The yield's predicate, state by state: the rename dialog and the
    /// focused search field each yield on their own, the delete dialog
    /// never does, and a search focus left set under a dialog - which no
    /// path leaves today - does not yield under the delete dialog either.
    #[test]
    fn the_ring_yields_to_each_text_field_and_never_under_the_delete_dialog() {
        let rename = || {
            Some(LibraryDialog::Rename {
                id: "id".into(),
                editor: TextEditor::new("Label".into()),
                error: String::new(),
            })
        };
        let delete = || {
            Some(LibraryDialog::Delete {
                id: "id".into(),
                error: String::new(),
            })
        };
        assert!(!ReplayLibraryUiState::default().owns_raw_input());

        let mut state = ReplayLibraryUiState {
            search_focused: true,
            ..default()
        };
        assert!(state.owns_raw_input());
        state.dialog = delete();
        assert!(!state.owns_raw_input());
        state.dialog = rename();
        assert!(state.owns_raw_input());

        let mut state = ReplayLibraryUiState {
            dialog: rename(),
            ..default()
        };
        assert!(state.owns_raw_input());
        state.dialog = delete();
        assert!(!state.owns_raw_input());
    }

    /// The part that must not go wrong: the delete dialog genuinely traps
    /// the ring. While it is open the ring is its two buttons and nothing
    /// else, in either direction, however far it is walked; it seeds on
    /// Cancel; Cancel and Escape each close it with the ring back on the
    /// card's Delete, where it was, across the rebuild that closing causes;
    /// a pointer opening it while the ring rests on Home hands the ring back
    /// to Home after; and confirming lands the ring where the module docs
    /// say - on a Delete, the same row's when a later card moves up, the
    /// previous row's when the page shrinks.
    #[test]
    fn the_delete_dialog_traps_the_ring_and_closing_it_returns_the_ring_to_the_card() {
        let (mut app, root) = library_app(5);
        let delete_of = |app: &mut App, row: usize| {
            control_at(app, (FocusGroup::Screen, card_focus_order(row) + 2))
        };
        let target = delete_of(&mut app, 1);
        let id = card_id(&app, target);
        assert_eq!(label_of(&app, &id).as_deref(), Some("Replay 4"));

        tab_to(&mut app, target);
        enter(&mut app);
        assert!(matches!(
            ui(&app).dialog,
            Some(LibraryDialog::Delete { .. })
        ));
        assert!(!yielded(&app), "the delete dialog never yields");
        let cancel = control(&mut app, |action| {
            matches!(action, LibraryAction::CancelDialog)
        });
        let confirm = control(&mut app, |action| {
            matches!(action, LibraryAction::ConfirmDelete)
        });
        assert_eq!(
            focused(&app),
            Some(cancel),
            "the dialog seeds on Cancel, never on the destructive button"
        );
        assert_eq!(
            app.world().get::<Outline>(cancel).unwrap().color,
            ORANGE,
            "the ring is drawn on the seed the frame the dialog appears"
        );

        // Forty presses, both directions: the ring never leaves the dialog.
        for step in 0..20 {
            tap(&mut app, KeyCode::Tab);
            let now = focused(&app);
            assert!(
                now == Some(cancel) || now == Some(confirm),
                "step {step}: {now:?}"
            );
            assert!(stays_on_library(&app), "step {step}");
        }
        for step in 0..20 {
            tap_shift_tab(&mut app);
            let now = focused(&app);
            assert!(
                now == Some(cancel) || now == Some(confirm),
                "step {step}: {now:?}"
            );
            assert!(stays_on_library(&app), "step {step}");
        }
        assert!(matches!(
            ui(&app).dialog,
            Some(LibraryDialog::Delete { .. })
        ));
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 5);

        // Cancel closes it, deletes nothing, and returns the ring to the
        // card's Delete - a new entity, the same key.
        tab_to(&mut app, cancel);
        enter(&mut app);
        assert!(ui(&app).dialog.is_none());
        assert_eq!(label_of(&app, &id).as_deref(), Some("Replay 4"));
        let back = focused(&app).expect("the ring is back on the screen");
        assert_eq!(
            key_of(&app, back),
            (FocusGroup::Screen, card_focus_order(1) + 2)
        );
        assert_eq!(card_id(&app, back), id);
        assert!(stays_on_library(&app));

        // Escape closes it too, and the ring is where it was.
        enter(&mut app);
        assert!(matches!(
            ui(&app).dialog,
            Some(LibraryDialog::Delete { .. })
        ));
        tap(&mut app, KeyCode::Escape);
        assert!(ui(&app).dialog.is_none());
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Screen, card_focus_order(1) + 2)
        );
        assert!(stays_on_library(&app));

        // The ring parked on Home when a pointer opens the dialog: the
        // dialog takes the ring in that same frame, before any key can
        // press, and hands it back to Home after.
        let home = control_at(&mut app, (FocusGroup::Screen, HOME_FOCUS_ORDER));
        tab_to(&mut app, home);
        let target = delete_of(&mut app, 1);
        click(&mut app, target);
        assert!(matches!(
            ui(&app).dialog,
            Some(LibraryDialog::Delete { .. })
        ));
        let cancel = control(&mut app, |action| {
            matches!(action, LibraryAction::CancelDialog)
        });
        assert_eq!(focused(&app), Some(cancel));
        enter(&mut app);
        assert!(ui(&app).dialog.is_none());
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Screen, HOME_FOCUS_ORDER)
        );
        assert!(stays_on_library(&app));

        // Confirm deletes. The list still fills the page, so the next card
        // moved up into the row and the ring is on *its* Delete: the
        // residual the module docs state.
        let target = delete_of(&mut app, 1);
        tab_to(&mut app, target);
        enter(&mut app);
        let confirm = control(&mut app, |action| {
            matches!(action, LibraryAction::ConfirmDelete)
        });
        tab_to(&mut app, confirm);
        enter(&mut app);
        assert!(ui(&app).dialog.is_none());
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 4);
        assert_eq!(label_of(&app, &id), None);
        let landed = focused(&app).unwrap();
        assert_eq!(
            key_of(&app, landed),
            (FocusGroup::Screen, card_focus_order(1) + 2)
        );
        assert_eq!(
            label_of(&app, &card_id(&app, landed)).as_deref(),
            Some("Replay 3")
        );
        assert!(stays_on_library(&app));

        // Deleting the page's last card shrinks the page, and the ring
        // clamps to the previous row's Delete.
        let last = delete_of(&mut app, 3);
        assert_eq!(
            label_of(&app, &card_id(&app, last)).as_deref(),
            Some("Replay 1")
        );
        tab_to(&mut app, last);
        enter(&mut app);
        let confirm = control(&mut app, |action| {
            matches!(action, LibraryAction::ConfirmDelete)
        });
        tab_to(&mut app, confirm);
        enter(&mut app);
        assert_eq!(app.world().resource::<ReplayLibrary>().entries.len(), 3);
        let landed = focused(&app).unwrap();
        assert_eq!(
            key_of(&app, landed),
            (FocusGroup::Screen, card_focus_order(2) + 2)
        );
        assert_eq!(
            label_of(&app, &card_id(&app, landed)).as_deref(),
            Some("Replay 2")
        );

        // The positive control: the hazard is live in this graph. With no
        // dialog open, Enter on Home leaves the screen - exactly what the
        // trap above has to keep out of reach.
        let home = control_at(&mut app, (FocusGroup::Screen, HOME_FOCUS_ORDER));
        tab_to(&mut app, home);
        enter(&mut app);
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Home)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// While the rename dialog is open its editor owns the keys: the ring is
    /// drawn on Save Label as a legend and does not move on Tab; Space and
    /// characters land in the field rather than pressing; every keystroke's
    /// rebuild keeps the ring on the legend; Enter saves and frees the ring
    /// back onto the card's Change Label; Escape abandons and frees it too.
    /// The Enter that opened the dialog is never the Enter that saves it.
    #[test]
    fn the_rename_dialog_yields_the_ring_until_enter_saves_or_escape_abandons() {
        let (mut app, root) = library_app(3);
        let key = (FocusGroup::Screen, card_focus_order(0) + 1);
        let target = control_at(&mut app, key);
        let id = card_id(&app, target);
        assert_eq!(label_of(&app, &id).as_deref(), Some("Replay 3"));
        let editor_text = |app: &App| match &ui(app).dialog {
            Some(LibraryDialog::Rename { editor, .. }) => Some(editor.text.clone()),
            _ => None,
        };

        tab_to(&mut app, target);
        enter(&mut app);
        assert_eq!(
            editor_text(&app).as_deref(),
            Some("Replay 3"),
            "the Enter that opened the dialog did not submit it"
        );
        assert!(yielded(&app), "the yield is raised for the coming frame");
        let save = control(&mut app, |action| {
            matches!(action, LibraryAction::ConfirmRename)
        });
        assert_eq!(focused(&app), Some(save), "the dialog seeds on Save Label");
        assert_eq!(
            app.world().get::<Outline>(save).unwrap().color,
            ORANGE,
            "the legend is drawn"
        );

        type_key(&mut app, KeyCode::Tab, Key::Tab);
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Modal, 0),
            "the ring did not move"
        );
        type_text(&mut app, " again");
        assert_eq!(editor_text(&app).as_deref(), Some("Replay 3 again"));
        assert!(
            matches!(ui(&app).dialog, Some(LibraryDialog::Rename { .. })),
            "Space typed a space rather than pressing Save Label"
        );
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Modal, 0),
            "every keystroke rebuilt the dialog under a ring that stayed put"
        );

        enter(&mut app);
        assert!(ui(&app).dialog.is_none(), "Enter saves");
        assert_eq!(label_of(&app, &id).as_deref(), Some("Replay 3 again"));
        assert!(!yielded(&app));
        let back = focused(&app).unwrap();
        assert_eq!(key_of(&app, back), key);
        assert_eq!(card_id(&app, back), id);
        tap(&mut app, KeyCode::Tab);
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Screen, card_focus_order(0) + 2),
            "saved, the ring moves on"
        );

        let target = control_at(&mut app, key);
        tab_to(&mut app, target);
        enter(&mut app);
        type_text(&mut app, "?");
        assert_eq!(editor_text(&app).as_deref(), Some("Replay 3 again?"));
        tap(&mut app, KeyCode::Escape);
        assert!(ui(&app).dialog.is_none(), "Escape abandons");
        assert_eq!(label_of(&app, &id).as_deref(), Some("Replay 3 again"));
        assert!(stays_on_library(&app));
        assert_eq!(key_of(&app, focused(&app).unwrap()), key);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// While the search field is focused it owns the keys: the ring stays
    /// drawn on Search and does not move; every keystroke narrows the list
    /// and rebuilds the screen under a ring that stays put, an empty result
    /// included; Enter leaves the field with the text kept, and so does
    /// Escape - whose next press leaves the screen, which is why Enter
    /// matters. Clear puts the player into the field the way it always has.
    #[test]
    fn the_search_field_yields_the_ring_and_enter_or_escape_leaves_it() {
        let (mut app, root) = library_app(5);
        let search_key = (FocusGroup::Screen, TOOLBAR_FOCUS_ORDER);
        let cards = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<&LibraryAction>()
                .iter(world)
                .filter(|action| matches!(action, LibraryAction::Play(_)))
                .count()
        };
        let on_search = |app: &App, when: &str| {
            let entity = focused(app).unwrap_or_else(|| panic!("{when}: nothing focused"));
            assert_eq!(key_of(app, entity), search_key, "{when}");
            assert_eq!(
                app.world().get::<Outline>(entity).unwrap().color,
                ORANGE,
                "{when}: the ring is drawn on Search"
            );
        };

        let search = control_at(&mut app, search_key);
        tab_to(&mut app, search);
        enter(&mut app);
        assert!(ui(&app).search_focused);
        assert!(yielded(&app));
        on_search(&app, "focused");
        assert_eq!(cards(&mut app), 4);

        type_key(&mut app, KeyCode::Tab, Key::Tab);
        on_search(&app, "Tab does not move the ring");
        type_text(&mut app, "5");
        assert_eq!(ui(&app).search.text, "5");
        assert_eq!(cards(&mut app), 1);
        on_search(&app, "one match");
        type_text(&mut app, "z");
        assert_eq!(cards(&mut app), 0);
        on_search(&app, "no match, no cards, no paging");
        type_key(&mut app, KeyCode::Backspace, Key::Backspace);
        assert_eq!(cards(&mut app), 1);
        on_search(&app, "back to one match");

        // Enter is "done": the text is kept, the field is left, and the
        // ring - no longer yielded - moves on the next key.
        enter(&mut app);
        assert!(!ui(&app).search_focused);
        assert_eq!(ui(&app).search.text, "5");
        assert_eq!(cards(&mut app), 1);
        assert!(!yielded(&app));
        on_search(&app, "left by Enter");
        assert!(stays_on_library(&app));
        tap(&mut app, KeyCode::Tab);
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Screen, TOOLBAR_FOCUS_ORDER + 1)
        );

        // Escape leaves the field too, and its next press leaves the screen.
        let search = control_at(&mut app, search_key);
        tab_to(&mut app, search);
        enter(&mut app);
        assert!(ui(&app).search_focused);
        tap(&mut app, KeyCode::Escape);
        assert!(!ui(&app).search_focused);
        assert!(stays_on_library(&app));
        tap(&mut app, KeyCode::Escape);
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Home)
        ));
        *app.world_mut().resource_mut::<NextState<AppScreen>>() = NextState::Unchanged;

        // Clear empties the text and puts the player in the field, as a
        // click always has; Enter leaves it.
        let clear = control_at(&mut app, (FocusGroup::Screen, TOOLBAR_FOCUS_ORDER + 1));
        tab_to(&mut app, clear);
        enter(&mut app);
        assert!(ui(&app).search_focused);
        assert_eq!(ui(&app).search.text, "");
        assert_eq!(cards(&mut app), 4);
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Screen, TOOLBAR_FOCUS_ORDER + 1)
        );
        enter(&mut app);
        assert!(!ui(&app).search_focused);
        assert!(stays_on_library(&app));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A disabled paging button is not in the ring, and the marker that
    /// takes it out is on the button from its spawn frame - on entry and
    /// after a rebuild alike. Pressing Page Down onto the last page disables
    /// it under the ring, and on that very frame the ring is on Page Up:
    /// the pagination band sits below the cards' band for exactly this.
    /// Were the marker a frame late, the rebuilt Page Down would be
    /// collected on the rebuild frame, its order recorded, and the ring
    /// would sit on a dead button.
    #[test]
    fn disabled_paging_buttons_leave_the_ring_from_their_spawn_frame() {
        let (mut app, root) = library_app(5);
        let disabled =
            |app: &App, entity: Entity| app.world().get::<InteractionDisabled>(entity).is_some();
        let up = control(&mut app, |action| matches!(action, LibraryAction::ScrollUp));
        let down = control(&mut app, |action| {
            matches!(action, LibraryAction::ScrollDown)
        });
        assert!(disabled(&app, up), "the first page has no page above it");
        assert!(!disabled(&app, down));

        let mut expected = vec![
            HOME_FOCUS_ORDER,
            TOOLBAR_FOCUS_ORDER,
            TOOLBAR_FOCUS_ORDER + 1,
            TOOLBAR_FOCUS_ORDER + 2,
            TOOLBAR_FOCUS_ORDER + 3,
            PAGINATION_FOCUS_ORDER + 1,
        ];
        for row in 0..4 {
            expected.extend((0..3).map(|offset| card_focus_order(row) + offset));
        }
        assert_eq!(walk_ring(&mut app), expected);

        tab_to(&mut app, down);
        enter(&mut app);
        assert_eq!(ui(&app).scroll_offset, 1);
        let up = control(&mut app, |action| matches!(action, LibraryAction::ScrollUp));
        let down = control(&mut app, |action| {
            matches!(action, LibraryAction::ScrollDown)
        });
        assert!(disabled(&app, down), "the last page has no page below it");
        assert!(!disabled(&app, up));
        assert_eq!(
            focused(&app),
            Some(up),
            "on the rebuild frame the ring is on Page Up, not on the dead Page Down"
        );
        assert!(
            !app.world()
                .resource::<UiFocusState>()
                .candidate_entities()
                .contains(&down)
        );

        // And back: Page Up onto the first page disables itself, and the
        // ring drops to the end of the toolbar band.
        enter(&mut app);
        assert_eq!(ui(&app).scroll_offset, 0);
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Screen, TOOLBAR_FOCUS_ORDER + 3)
        );
        std::fs::remove_dir_all(root).unwrap();

        // A list that fits one page has no paging at all.
        let (mut app, root) = library_app(3);
        let up = control(&mut app, |action| matches!(action, LibraryAction::ScrollUp));
        let down = control(&mut app, |action| {
            matches!(action, LibraryAction::ScrollDown)
        });
        assert!(disabled(&app, up));
        assert!(disabled(&app, down));
        assert!(
            walk_ring(&mut app)
                .iter()
                .all(|order| !(PAGINATION_FOCUS_ORDER..CARDS_FOCUS_ORDER).contains(order))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// When a taller window holds more cards than the rest of the list from
    /// the first card shown, the first card moves back and every card on
    /// the page moves down as many places, and the ring moves with them: it
    /// stays drawn on the same button of the same Replay, not on the same
    /// place on the page. Through the screen's own plugin, the focus core
    /// and the game's own window tracking, the window itself resized, and
    /// all of it in the first frame after the resize: a pass that left the
    /// ring's memory where it was would draw it on another Replay's card,
    /// and one that read the size a frame late would not have moved the
    /// page yet.
    #[test]
    fn the_ring_stays_on_its_replay_when_a_taller_window_moves_the_page_back() {
        use super::super::geometry::library_layout;
        use crate::app::visuals::WindowSize;

        let (mut app, root) = library_app(10);
        let taller = Vec2::new(1040.0, 1080.0);
        let before = library_layout(WindowSize::OPENING.0, 0).cards_per_page();
        let after = library_layout(taller, 0).cards_per_page();
        assert!(after > before, "the taller window holds more cards");
        // Down to the last page: the list's last cards.
        for _ in 0..10 - before {
            app.world_mut().write_message(MouseWheel {
                unit: bevy::input::mouse::MouseScrollUnit::Line,
                x: 0.0,
                y: -1.0,
                window: Entity::PLACEHOLDER,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
            app.update();
        }
        app.update();
        assert_eq!(ui(&app).scroll_offset, 10 - before);
        // The ring on the second card's Change Label.
        let target = control_at(&mut app, (FocusGroup::Screen, card_focus_order(1) + 1));
        let replay = card_id(&app, target);
        tab_to(&mut app, target);
        assert_eq!(focused(&app), Some(target));

        {
            let world = app.world_mut();
            let mut windows =
                world.query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>();
            let mut window = windows.single_mut(world).unwrap();
            window.resolution.set(taller.x, taller.y);
        }
        // One frame: the size, the page and the ring all follow in it.
        app.update();
        assert_eq!(app.world().resource::<WindowSize>().0, taller);
        assert_eq!(ui(&app).scroll_offset, 10 - after);
        let now = focused(&app).expect("the ring is drawn");
        assert_ne!(now, target, "the page was rebuilt");
        assert_eq!(card_id(&app, now), replay);
        assert!(matches!(
            app.world().get::<LibraryAction>(now),
            Some(LibraryAction::OpenRename(_))
        ));
        assert_eq!(
            key_of(&app, now),
            (FocusGroup::Screen, card_focus_order(1 + after - before) + 1)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The same screen driven by a pad alone: the D-pad walks the ring and
    /// South presses; the delete dialog traps the D-pad and South on Cancel,
    /// Start or East each close it with the ring back on the card; and the
    /// two fields, which a pad can open but not type into, are left with
    /// Start or East and the ring moves again.
    #[test]
    fn a_pad_walks_the_library_and_backs_out_of_its_dialogs_and_fields() {
        let (mut app, root) = library_app(3);
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        let delete_key = (FocusGroup::Screen, card_focus_order(0) + 2);
        let target = control_at(&mut app, delete_key);
        let id = card_id(&app, target);

        pad_to(&mut app, pad, target);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert!(matches!(
            ui(&app).dialog,
            Some(LibraryDialog::Delete { .. })
        ));
        let cancel = control(&mut app, |action| {
            matches!(action, LibraryAction::CancelDialog)
        });
        let confirm = control(&mut app, |action| {
            matches!(action, LibraryAction::ConfirmDelete)
        });
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
            assert!(stays_on_library(&app));
        }
        pad_to(&mut app, pad, cancel);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert!(ui(&app).dialog.is_none());
        assert_eq!(key_of(&app, focused(&app).unwrap()), delete_key);
        assert_eq!(label_of(&app, &id).as_deref(), Some("Replay 3"));

        for back in [GamepadButton::Start, GamepadButton::East] {
            tap_pad(&mut app, pad, GamepadButton::South);
            assert!(matches!(
                ui(&app).dialog,
                Some(LibraryDialog::Delete { .. })
            ));
            tap_pad(&mut app, pad, back);
            assert!(ui(&app).dialog.is_none(), "{back:?} closes the dialog");
            assert_eq!(key_of(&app, focused(&app).unwrap()), delete_key);
            assert!(stays_on_library(&app));
        }

        // The rename dialog: opened by South, typed into by nobody, left by
        // Start with the label untouched.
        let rename_key = (FocusGroup::Screen, card_focus_order(0) + 1);
        let target = control_at(&mut app, rename_key);
        pad_to(&mut app, pad, target);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert!(matches!(
            ui(&app).dialog,
            Some(LibraryDialog::Rename { .. })
        ));
        assert_eq!(key_of(&app, focused(&app).unwrap()), (FocusGroup::Modal, 0));
        tap_pad(&mut app, pad, GamepadButton::DPadDown);
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Modal, 0),
            "yielded, the ring does not move"
        );
        tap_pad(&mut app, pad, GamepadButton::Start);
        assert!(ui(&app).dialog.is_none());
        assert_eq!(label_of(&app, &id).as_deref(), Some("Replay 3"));
        assert_eq!(key_of(&app, focused(&app).unwrap()), rename_key);
        assert!(stays_on_library(&app));

        // The search field: focused by South, left by East, and the ring
        // moves again.
        let search_key = (FocusGroup::Screen, TOOLBAR_FOCUS_ORDER);
        let search = control_at(&mut app, search_key);
        pad_to(&mut app, pad, search);
        tap_pad(&mut app, pad, GamepadButton::South);
        assert!(ui(&app).search_focused);
        tap_pad(&mut app, pad, GamepadButton::DPadDown);
        assert_eq!(key_of(&app, focused(&app).unwrap()), search_key);
        tap_pad(&mut app, pad, GamepadButton::East);
        assert!(!ui(&app).search_focused);
        assert!(stays_on_library(&app));
        tap_pad(&mut app, pad, GamepadButton::DPadDown);
        assert_eq!(
            key_of(&app, focused(&app).unwrap()),
            (FocusGroup::Screen, TOOLBAR_FOCUS_ORDER + 1)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
