//! Keyboard and controller operation of menu buttons: a focus ring over the
//! buttons a screen marks [`Focusable`], which activates the focused one
//! exactly as a click on it would.
//!
//! # Responsibility
//!
//! Every button intent in the crate is read by four sibling systems
//! (`handle_ui_buttons`, `handle_mapping_buttons`, `handle_library_buttons`,
//! `handle_viewer_buttons`) from one [`ButtonActivation`] message, which a
//! pointer click and this ring produce alike: each triggers Bevy's
//! [`Activate`] on the button, and `ui::button` forwards every `Activate`
//! into the message. So the ring teaches none of them a second input path.
//! [`apply_focus_navigation`] triggers `Activate` on the focused button
//! when Enter, Space or a pad's South goes down - on the press, where a
//! click activates on its release - in `PreUpdate`, after the frame's
//! pointer events, and every reader, the shared styling and the activation
//! cue answer it as they answer a click. An activation is an event rather
//! than a state, so nothing is left behind to undo at the end of the frame
//! and nothing can stick; and it draws no pressed fill, which for one frame
//! nobody could see - the ring's outline is the keyboard's feedback.
//!
//! The ring also publishes the focused button as Bevy's [`InputFocus`] (see
//! the last section), and Bevy's own button behaviour would activate that
//! button a second time on Enter or Space, and on a Space typed into a
//! field that has taken the keys. `crate::app` leaves that plugin out;
//! `ui::button`'s module docs say why, and its tests pin it.
//!
//! [`ButtonActivation`]: super::button::ButtonActivation
//!
//! # Marking and keying
//!
//! Marking is authored, not derived: each screen's own layout inserts
//! [`Focusable`] on the buttons it wants in the ring. The six full-screen
//! dialog overlays that block the pointer are plain nodes rather than
//! buttons, so there is nothing there to mark. Home marks its four menu buttons, the Replay Viewer its
//! thirteen controls, Settings its sixty-three - one `Screen` ring over its
//! tab bar, the tab panel on show and its footer, with the reset
//! confirmation in `Modal` - and the Replay Library every button it draws:
//! Home, its toolbar, its paging buttons and the page of cards on show in
//! one `Screen` ring, with its rename and delete dialogs in `Modal`. The
//! Help modal marks its two page arrows and its Close in `Modal`,
//! in drawn order, so the first Enter on page 1 is a no-op rather than a
//! dismissal (`ui::help` says why). A
//! disabled control is not in the ring either: `spawn_menu_button` applies
//! that at spawn by not marking one, and a screen whose control is disabled
//! while it is on screen (the Viewer's transport at either end of a replay,
//! a Settings numeric field whose limit is off, the Library's paging
//! buttons on the first and last page) carries the same rule by inserting
//! `bevy_ui`'s [`InteractionDisabled`], which the collector filters on.
//!
//! The Help modal's two page arrows are the one deliberate
//! exception. `ui::hud` paints them with the disabled background and mutes
//! their cue at either end of the page sequence, but they keep no
//! [`InteractionDisabled`] and stay in the ring, because marking them would
//! drop *Left* from the candidate list on page 1 - moving the seed to
//! *Right*, so the first Enter turned a page instead of doing nothing, and
//! shifting focus from *Left* to *Right* every time the player paged back.
//! An inert seed is the point; see `ui::help`.
//!
//! Focus is remembered per screen as an order within a [`FocusGroup`]
//! rather than as an `Entity`, because the Replay Library despawns and
//! respawns its whole screen on any list change with no `AppScreen`
//! transition - an entity-keyed focus would die there on every search
//! keystroke. That is demonstrated rather than anticipated:
//! `ui::replay::library::ring`'s
//! `focus_holds_its_place_on_every_frame_of_a_dirty_rebuild` runs four
//! rebuilds under real layout and the production graph with the ring
//! drawn on the same key throughout. Per screen, because the memory
//! outlives a screen transition on purpose - it is what puts Home's ring
//! back where it was after a Match or a visit to Settings - and a second
//! screen with a ring must neither inherit that position nor overwrite it.
//! [`refresh_focus_candidates`] re-reads the marked buttons
//! after [`UiSystems::Layout`] each frame (so it sees the layout the player
//! sees), drops anything whose [`ComputedNode`] size is zero - a
//! `Display::None` subtree zeroes its nodes recursively, which two tests
//! below check against real `bevy_ui` layout - and re-resolves the memory.
//!
//! # Groups and dialogs
//!
//! The highest [`FocusGroup`] present owns the ring: candidates are sorted
//! by `(group, order)`, so a dialog whose buttons become visible captures
//! focus with no bookkeeping and hands it back the frame they are hidden.
//! The memory keeps one slot per group, so closing a dialog - confirmed or
//! cancelled - returns the ring to where it was on the screen underneath
//! rather than to the screen's first control; on the Replay Library that
//! first control is Home, one Enter from leaving. The modal slot alone is
//! not kept between openings: [`refresh_focus_candidates`] forgets it on
//! every frame the `Modal` group does not own the ring - whether the
//! screen's own ring has taken it back or, on the Match screen, where
//! nothing else is focusable, nothing has - so every dialog seeds at its
//! first entry, because one `Modal` variant is shared by all of them. That
//! is why a dialog's first entry should be its safe button, as Settings'
//! reset confirmation orders Cancel before Confirm Reset. Resolution within
//! the owning group
//! is an exact order match first, then the nearest lower order, then the
//! group's first entry.
//!
//! # Keys belong to nobody
//!
//! Navigation reads raw [`ButtonInput<KeyCode>`] and [`Gamepad`] state
//! directly - deliberately not `GameAction`, `KeyboardBindings` or
//! `ControllerAssignments`. On Home, Player Two's default movement and
//! select keys are marimba bars, and menu focus must belong to neither
//! player; the keys used here (Tab, the arrows, Enter, Space, the D-pad and
//! South) are not notes, which the marimba's own non-note guard test pins.
//! Both players share one input stream, so same-frame collisions are
//! resolved by the pure [`focus_intent`]: an activation press wins outright
//! and movement is the signum of next-ward minus prev-ward presses, so
//! opposite presses cancel.
//!
//! # What is gated off, and the exits
//!
//! The ring and live gameplay are never active together - Player One's
//! default movement is the four arrow keys and Restart is Space, all of them
//! navigation keys here - and the gate that keeps them apart is
//! [`focus_ring_allowed`]: the ring may run on every screen but the Match,
//! and on the Match exactly while a HUD modal is open. For every frame but
//! the one that closes the modal, that is safe by a shared predicate rather
//! than by timing: [`modal_open`] is also what
//! [`crate::ui::hud::modal_pause_state`] blocks gameplay input on, so
//! gameplay reads no key for as long as the ring can move, and
//! `the_gate_never_admits_the_ring_while_gameplay_is_live` holds that for
//! every screen and every modal state.
//!
//! The closing frame is the exception and rests on ordering instead. The ring
//! moves in `PreUpdate` while the modal is still open, dismissal clears the
//! flag in the same chain, and `sync_gameplay_pause` then republishes an
//! unblocked [`crate::gameplay::GameplayPause`] in that same frame's
//! `Update` - so the key that moved the ring meets gameplay unblocked. What
//! keeps it off the board is [`crate::gameplay::InputGate::arm`], called by
//! `apply_hud_action` before the gate's own release pass runs: a
//! just-pressed key is also pressed, so it stays gated until it is
//! physically released. That is a timing guarantee, and the predicate test
//! cannot see it. During live play it is not the gate
//! that keeps the ring dead but the absence of [`Focusable`] on the Match
//! screen, which `ui::match_screen`'s
//! `the_match_screen_marks_nothing_focusable` pins: the Match HUD's own two
//! buttons stay pointer-only for exactly that reason. A keyboard player
//! opens the Help modal there with F1 instead - a reserved key
//! rather than a ring, read by [`handle_help_modal_keys`] - and then
//! operates it by keyboard or pad. A pad has no such button yet: Start is
//! already its back button, and none of the other buttons a binding cannot
//! claim (Select, Mode, the stick clicks, all absent from `ControllerInput`)
//! has been given the job, so a pad-only player still opens the modal there
//! with the pointer.
//!
//! Precisely: [`apply_focus_navigation`] runs under that gate;
//! [`hide_focus_ring_on_pointer_motion`] runs everywhere, ahead of it.
//! [`refresh_focus_candidates`] and [`update_focus_indicator`] run on every
//! screen, ungated: during live play nothing on the Match is `Focusable`
//! and the modal's buttons are `Display::None`, so both write once and then
//! find nothing to change, while gating them would leave a stale candidate
//! list, a dangling [`InputFocus`] and a drawn outline behind on the frame
//! a modal closes, when `help_open` goes false before `PostUpdate` runs.
//! [`handle_help_modal_keys`] runs everywhere and closes the Help
//! modal on Escape, East, Start or F1, and on Home and the Match opens it on
//! F1. [`handle_match_return`] runs on Match alone,
//! its back keys structurally unbindable and its confirm keys read only
//! while the confirmation has already blocked gameplay (see its doc
//! comment). The Help modal's own buttons - its
//! page arrows and its Close - are in the ring in `FocusGroup::Modal`, so
//! opening it hands them the ring by group ownership and closing it hands
//! the ring back. While the Return Home confirmation is open, navigation
//! and activation are instead suppressed outright: that dialog's buttons
//! are not in the ring, so the only buttons the ring could reach are the
//! ones under the overlay. A screen whose own control owns the raw devices
//! for a while - a binding capture, an open text field - raises
//! [`FocusYield`], which suppresses the same frame the same way. Settings
//! raises it for its two binding captures and its numeric editor, the
//! Replay Library for its search field and its rename dialog.
//!
//! Exits are deliberately wider than rings: every screen below the title
//! screen steps back with Escape and with a pad's Start button, and the
//! menu screens also take East - Start is structurally unbindable in the
//! way Escape is, and East is a gameplay binding during a Match and the
//! input being bound during a controller capture, so it is the one that
//! cannot be universal. The Help modal is the one place East closes
//! something during a Match: the dismissal reads it only while the modal is
//! already open, when gameplay is blocked for the modal's whole life, and
//! closing re-arms the input gate until the button is physically released,
//! so the default Teleport on East cannot fire on the way out either. F1 is
//! the one fixed key that opens something rather than closing it: the
//! Help modal, on the two screens that have a button for it, by the
//! same reservation that makes Escape safe during live play
//! (`crate::input::keyboard` says why neither can be bound). Home
//! itself is the root - there is nowhere back to go, and Escape there only
//! closes the Help modal. The Settings, Replay Library and Replay
//! Viewer exits live in those screens' own modules beside the input they
//! already read; the Match exit lives here because the Match reads no menu
//! input of its own.
//!
//! # The indicator and the screen reader
//!
//! The ring is an [`Outline`] - a shape difference rather than a colour
//! difference, since the focused control is the only control carrying an
//! outline at all - required onto every [`Focusable`] at spawn with
//! [`Color::NONE`] and recoloured in place, never inserted and removed.
//! [`InputFocusVisible`] hides it until a navigation key is used and hides
//! it again on any cursor movement - on every screen, by
//! [`hide_focus_ring_on_pointer_motion`], which is ungated: the flag is
//! global and would otherwise carry a menu screen's `true` through live
//! play into a modal the pointer opens during a Match, drawing a ring
//! nobody asked for. A modal F1 opens is the other case: a key was
//! pressed, so [`handle_help_modal_keys`] raises the flag and the ring
//! arrives drawn on the seed, as it does when Enter opens the modal from
//! Home. Every resolved focus change also lands
//! in [`InputFocus`], which is what makes `bevy_winit` publish the
//! screen-reader cursor alongside the ring; inbound AccessKit action
//! routing is deliberately absent here (part 4). When it comes, the
//! natural shape is an `Activate` on the button the screen reader clicked,
//! which every reader already answers.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::input::InputSystems;
use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::ui_widgets::Activate;
use bevy::window::CursorMoved;

use super::button::{Activated, PointerActivation, UiButtonPlugin};
use super::hud::{HudAction, HudState, apply_hud_action, hud_action_ui_cue, modal_open};
use crate::app::AppScreen;
use crate::audio::AudioCueQueue;
use crate::gameplay::InputGate;
use crate::presentation::ORANGE;

/// Which ring of controls a [`Focusable`] belongs to.
///
/// Declared in stacking order, and the `Ord` derive is load-bearing rather
/// than cosmetic: candidates are sorted by `(group, order)`, so the group of
/// the last candidate is the highest group present - the ring the player is
/// looking at - and that is the group [`resolve_focus`] hands the ring to.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum FocusGroup {
    /// A screen's own buttons, outside any modal.
    Screen,
    /// The buttons of the one dialog a screen can show at a time. A single
    /// variant covers every dialog rather than one per dialog: no screen
    /// can show two overlays at once (the Replay Viewer opens either panel
    /// by closing the other), and screens never coexist.
    Modal,
}

impl FocusGroup {
    /// Every group, in declaration order; [`FocusMemory`] keeps one slot
    /// per entry.
    const ALL: [Self; 2] = [Self::Screen, Self::Modal];
}

/// One remembered order per [`FocusGroup`].
///
/// The slot for the group that owns the ring is normalized every frame to
/// the candidate actually resolved, so later movement steps from where the
/// ring is drawn. Every other group's slot is untouched while a higher
/// group holds the ring, which is what brings the ring back where it was
/// when a dialog closes - with one exception: the `Modal` slot is kept
/// only while a dialog owns the ring, and [`refresh_focus_candidates`]
/// calls [`Self::forget`] on it the frame one does not. Because one `Modal`
/// variant is shared by every dialog, a slot carried from one dialog to the
/// next would seed the Replay Library's Delete dialog on *Delete
/// Permanently* for a player who had last stood on Settings' *Confirm
/// Reset*; and it is ownership rather than a lower group's write that
/// forgets it, because on the Match screen nothing outside the Help
/// modal is ever focusable, so no lower group would ever write.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct FocusMemory([Option<u32>; FocusGroup::ALL.len()]);

impl FocusMemory {
    fn get(self, group: FocusGroup) -> Option<u32> {
        self.0[group as usize]
    }

    /// Records where the ring is within `group`.
    fn remember(&mut self, group: FocusGroup, order: u32) {
        self.0[group as usize] = Some(order);
    }

    /// Empties one group's slot, so its next resolution seeds at the
    /// group's first entry.
    fn forget(&mut self, group: FocusGroup) {
        self.0[group as usize] = None;
    }
}

/// Marks a button as reachable by keyboard and controller focus.
///
/// Inserted by the owning screen's layout beside the button's intent
/// component. `order` is the button's position in its group's ring, and with
/// `group` it is the key focus is remembered by - see the module docs for
/// why the key is not an `Entity`.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[require(Outline = focus_outline())]
pub(crate) struct Focusable {
    pub(crate) group: FocusGroup,
    pub(crate) order: u32,
}

/// The focus ring, invisible until [`update_focus_indicator`] colours it.
///
/// `Outline` draws outside the node and takes no part in layout, so nothing
/// reflows at the fixed window size. Keeping the component in place and
/// writing only its `color` avoids the archetype moves that repeated
/// insertion and removal would cause - `Outline`'s own docs recommend
/// exactly this.
fn focus_outline() -> Outline {
    Outline::new(px(3), px(2), Color::NONE)
}

/// A focusable button as collected after layout: its key and its entity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FocusCandidate {
    group: FocusGroup,
    order: u32,
    entity: Entity,
}

/// Focus memory and this frame's candidate ring.
#[derive(Resource, Default)]
pub(crate) struct UiFocusState {
    /// Where focus is on each screen: one [`FocusMemory`] per `AppScreen`,
    /// keyed by order so it survives a despawn-and-respawn and so a
    /// screen's slot survives a dialog opening over it and closing again.
    /// A screen never visited has no entry and seeds at its first control.
    memory: HashMap<AppScreen, FocusMemory>,
    /// Visible focusable buttons, sorted by `(group, order)`, collected by
    /// [`refresh_focus_candidates`] after layout. [`apply_focus_navigation`]
    /// reads this in the next frame's `PreUpdate`, which is exactly the
    /// layout the player was looking at when the key went down.
    candidates: Vec<FocusCandidate>,
}

impl UiFocusState {
    /// The entities in this frame's ring, in ring order, for a screen's own
    /// tests to assert on what the collector saw - the Replay Library's
    /// rebuild test needs the list itself, not only the focus it resolved to.
    #[cfg(test)]
    pub(crate) fn candidate_entities(&self) -> Vec<Entity> {
        self.candidates
            .iter()
            .map(|candidate| candidate.entity)
            .collect()
    }

    fn memory(&self, screen: AppScreen) -> FocusMemory {
        self.memory.get(&screen).copied().unwrap_or_default()
    }

    fn remember(&mut self, screen: AppScreen, group: FocusGroup, order: u32) {
        self.memory
            .entry(screen)
            .or_default()
            .remember(group, order);
    }

    /// Forgets one group's slot on `screen`; a screen never visited has
    /// nothing to forget and gains no entry.
    fn forget(&mut self, screen: AppScreen, group: FocusGroup) {
        if let Some(memory) = self.memory.get_mut(&screen) {
            memory.forget(group);
        }
    }
}

/// Raised by a screen for each frame one of its own controls owns the raw
/// devices - a binding capture listening for the next key or pad button, or
/// a text field taking characters - so the ring neither moves nor presses on
/// the very keys that control is reading.
///
/// [`apply_focus_navigation`] lowers it after reading it, so a screen holds
/// it by writing `true` every frame the control is live and can never leave
/// it stuck when the control ends or the screen despawns. On the Match
/// screen navigation runs only while a HUD modal is open, so a yield still
/// raised on the frame the Match is entered is spent on the modal's first
/// open frame and lowered there - one suppressed frame, the same price a
/// yield costs anywhere. No screen the Match is entered from raises one
/// (Home has no capturing control), so this is the contract rather than a
/// case that occurs. It sits beside the Return Home confirmation's own
/// suppression in [`apply_focus_navigation`] rather than replacing it: that
/// dialog's buttons are not in the ring, so the ring is stood down outright
/// while it is open.
///
/// Settings raises it, from `ui::settings::ring`, for its keyboard and
/// controller binding captures and its numeric editor - and, having decided
/// that the ring should stop moving as well as pressing while a control
/// reads the keys, records why there. The Replay Library raises it, from
/// `ui::replay::library::ring`, for its search field and its rename
/// dialog. Landing the resource ahead of both was a deliberate exception to
/// not inventing mechanisms ahead of their screens: they would otherwise
/// each have added the same resource and edited the same three lines here.
#[derive(Resource, Default)]
pub(crate) struct FocusYield(pub(crate) bool);

/// What one frame's combined key and pad presses ask focus to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FocusIntent {
    /// Press the focused button; any movement the same frame is discarded.
    Activate,
    /// Step the ring by exactly one place in either direction.
    Move(i32),
    Idle,
}

/// Resolves one frame of navigation input from both players' hands.
///
/// Pure so the collision rules can be tested with no `App`: an activation
/// key just-pressed wins and discards movement that frame; otherwise the
/// ring moves by the signum of (next-ward presses - prev-ward presses), so
/// opposite presses cancel and the ring does not move.
fn focus_intent(activate: bool, next_presses: u32, prev_presses: u32) -> FocusIntent {
    if activate {
        return FocusIntent::Activate;
    }
    match (i64::from(next_presses) - i64::from(prev_presses)).signum() {
        1 => FocusIntent::Move(1),
        -1 => FocusIntent::Move(-1),
        _ => FocusIntent::Idle,
    }
}

/// Resolves the memory against this frame's candidates, returning an index
/// into the sorted candidate list.
///
/// The highest group present owns the ring. The candidates are sorted by
/// `(group, order)`, so that is the last candidate's group, and only that
/// group's slot is consulted - a remembered position in a lower group waits
/// untouched. Within the owning group an exact order match wins; the exact
/// order gone clamps to the nearest lower order; nothing lower left, or an
/// empty slot, gives the group's first entry.
fn resolve_focus(candidates: &[FocusCandidate], memory: FocusMemory) -> Option<usize> {
    let owner = candidates.last()?.group;
    let first = candidates
        .iter()
        .position(|candidate| candidate.group == owner)
        .expect("the last candidate's group is present");
    let Some(order) = memory.get(owner) else {
        return Some(first);
    };
    let mut nearest_lower = None;
    // The owning group is contiguous at the end of the sorted list.
    for (index, candidate) in candidates.iter().enumerate().skip(first) {
        if candidate.order == order {
            return Some(index);
        }
        if candidate.order < order {
            nearest_lower = Some(index);
        }
    }
    Some(nearest_lower.unwrap_or(first))
}

/// Registers the focus core. Added once by [`super::UiPlugin`]; screens
/// participate by inserting [`Focusable`], never by talking to this plugin.
pub(super) struct UiFocusPlugin;

impl Plugin for UiFocusPlugin {
    fn build(&self, app: &mut App) {
        UiButtonPlugin::require(app);
        // The two `bevy_input_focus` resources normally come from
        // `DefaultPlugins`; initializing them here too keeps this plugin
        // usable on a minimal headless `App`, and `init_resource` never
        // overwrites the production copies. Same for the `CursorMoved`
        // message that `WindowPlugin` registers.
        app.init_resource::<UiFocusState>()
            .init_resource::<FocusYield>()
            .init_resource::<InputFocus>()
            .init_resource::<InputFocusVisible>()
            .add_message::<CursorMoved>()
            .add_systems(
                PreUpdate,
                (
                    // The pointer drain first and ungated, so a mouse
                    // movement lowers the ring's visibility on every screen
                    // before navigation reads it.
                    hide_focus_ring_on_pointer_motion,
                    // Its activation is applied at the sync point this chain
                    // inserts after it, so the two systems below already see
                    // it as a `ButtonActivation`.
                    apply_focus_navigation.run_if(focus_ring_allowed_now),
                    // The Match exit reads the back keys before the help
                    // modal's keys do and stands aside while the
                    // Help modal is open, so the Escape that closes
                    // the modal cannot also open the Return Home
                    // confirmation in the same frame; the next press does.
                    // Running first also settles the one tie F1 can have
                    // with a back key: F1 and Escape with the modal closed
                    // open the confirmation, and the help key stands aside
                    // from it. The help modal's keys run after navigation
                    // because they stand aside when Help or Close has already
                    // been activated this frame, by the ring or the pointer,
                    // and can only see that afterwards: a same-frame Escape and
                    // Enter then close the modal once, with one cue, and a
                    // same-frame F1 and Enter open or close it once. The
                    // frame that closes the modal cannot press a button
                    // underneath it in either order - that is group
                    // ownership, not ordering: navigation reads the
                    // candidate list the previous `PostUpdate` collected, in
                    // which the modal's own buttons are the highest
                    // `FocusGroup` present, so `resolve_focus` confines the
                    // ring to them. (The pointer's opening needs no edge -
                    // `help_open` is set in `Update`, a schedule after all
                    // four; F1's opening lives in the same system as the
                    // closing, which is what lets one frame decide once.)
                    handle_match_return.run_if(in_state(AppScreen::Match)),
                    handle_help_modal_keys,
                )
                    .chain()
                    // After the pointer's activations, so the frame's come
                    // first and the Help keys see them.
                    .after(InputSystems)
                    .after(PointerActivation),
            )
            // Ungated on purpose, the Match included - see the module docs
            // for what gating this pair off the Match would leave behind on
            // the frame a modal closes.
            .add_systems(
                PostUpdate,
                (refresh_focus_candidates, update_focus_indicator)
                    .chain()
                    .after(UiSystems::Layout),
            );
    }
}

/// Whether the ring may move or press on `screen` while the HUD is in the
/// state `hud` describes - the one rule [`apply_focus_navigation`]'s run
/// condition enforces, written as a pure predicate so a test can hold it
/// against every screen and every modal state at once.
///
/// The rule protects one outcome: the ring and live gameplay are never
/// active together, because Player One's default movement is the four arrow
/// keys and Restart is Space, all of them navigation keys here. Every menu
/// screen has the ring. The Match has it exactly while a HUD modal is open,
/// and that is safe by construction rather than by timing: [`modal_open`]
/// is the same predicate [`crate::ui::hud::modal_pause_state`] blocks
/// gameplay input on, so for as long as the ring can move,
/// `apply_player_input` reads no key. It is deliberately not
/// `GameplayPause` - that is gameplay's copy, republished across a boundary
/// so gameplay need not know modals exist, and a frame stale in
/// `PreUpdate` - and deliberately `modal_open` rather than `help_open`, so
/// that the Return Home confirmation, whose buttons are not in the ring, is
/// stood down inside [`apply_focus_navigation`] rather than by this gate.
/// `the_gate_never_admits_the_ring_while_gameplay_is_live` pins the
/// property.
fn focus_ring_allowed(screen: AppScreen, hud: &HudState) -> bool {
    screen != AppScreen::Match || modal_open(hud)
}

/// [`focus_ring_allowed`] as a run condition over the live resources.
fn focus_ring_allowed_now(screen: Res<State<AppScreen>>, hud: Res<HudState>) -> bool {
    focus_ring_allowed(*screen.get(), &hud)
}

/// Counts this frame's navigation presses across the keyboard and every
/// connected pad: `(activate, next-ward presses, prev-ward presses)`.
fn navigation_presses<'a>(
    keys: &ButtonInput<KeyCode>,
    gamepads: impl Iterator<Item = &'a Gamepad>,
) -> (bool, u32, u32) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let mut activate = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
    let mut next = 0;
    let mut prev = 0;
    for key in [KeyCode::ArrowDown, KeyCode::ArrowRight] {
        next += u32::from(keys.just_pressed(key));
    }
    for key in [KeyCode::ArrowUp, KeyCode::ArrowLeft] {
        prev += u32::from(keys.just_pressed(key));
    }
    if keys.just_pressed(KeyCode::Tab) {
        if shift {
            prev += 1;
        } else {
            next += 1;
        }
    }
    for gamepad in gamepads {
        activate |= gamepad.just_pressed(GamepadButton::South);
        for button in [GamepadButton::DPadDown, GamepadButton::DPadRight] {
            next += u32::from(gamepad.just_pressed(button));
        }
        for button in [GamepadButton::DPadUp, GamepadButton::DPadLeft] {
            prev += u32::from(gamepad.just_pressed(button));
        }
    }
    (activate, next, prev)
}

/// The raw devices menu navigation reads: the keyboard and every connected
/// pad. Deliberately not the bindings - see the module docs. One cohesive
/// group, because these two are exactly "what the player's hands pressed
/// this frame"; the pointer's motion, which only hides the ring, is read by
/// [`hide_focus_ring_on_pointer_motion`] on every screen instead.
#[derive(SystemParam)]
struct FocusDevices<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    gamepads: Query<'w, 's, &'static Gamepad>,
}

/// The pointer taking over hides the ring until a navigation key is used
/// again - on every screen, in every state, the Match during live play
/// included. Its own system rather than the first lines of
/// [`apply_focus_navigation`] because navigation is gated and this must not
/// be: [`InputFocusVisible`] is one global flag, and a `true` left by the
/// last menu screen would otherwise survive the whole Match, so that a
/// modal the pointer opened there arrived with a ring already drawn on its
/// first button. `count()` drains the reader: a burst of several moves in
/// one frame must not leave a leftover message to hide the ring a frame
/// later.
fn hide_focus_ring_on_pointer_motion(
    mut cursor_moves: MessageReader<CursorMoved>,
    mut visible: ResMut<InputFocusVisible>,
) {
    if cursor_moves.read().count() > 0 && visible.0 {
        visible.0 = false;
    }
}

/// The two things that can stand the ring down for a frame: the Return
/// Home confirmation covering the Match screen, and a screen's own control
/// owning the devices.
#[derive(SystemParam)]
struct FocusSuppressors<'w> {
    hud: Res<'w, HudState>,
    focus_yield: ResMut<'w, FocusYield>,
}

/// Moves the ring or activates the focused button, from raw device state -
/// see the module docs for why the bindings are deliberately not consulted.
/// An activation is Bevy's [`Activate`] on the focused button, exactly what
/// a pointer click on it triggers, so every reader handles both alike.
fn apply_focus_navigation(
    devices: FocusDevices,
    screen: Res<State<AppScreen>>,
    mut suppressors: FocusSuppressors,
    mut state: ResMut<UiFocusState>,
    mut input_focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    mut commands: Commands,
) {
    let (activate, next, prev) = navigation_presses(&devices.keys, devices.gamepads.iter());
    // A raised yield covers exactly one frame; lowering it here is what
    // lets a screen hold it by writing every frame and never get it stuck.
    let yielded = suppressors.focus_yield.0;
    if yielded {
        suppressors.focus_yield.0 = false;
    }
    // While the Return Home confirmation covers the Match screen, its
    // buttons are not in the ring, so the only buttons the ring could reach
    // are the ones under the overlay, and pressing *Help* through it would
    // be worse than doing nothing. Today nothing on the
    // Match is `Focusable` at all, so this stands down a ring with no
    // candidates; it is kept because the protection becomes load-bearing
    // the moment anyone marks the Match HUD's buttons, and would otherwise
    // fail silently. The Help modal needs no such line: its own
    // buttons are in the ring, and group ownership confines the ring to
    // them while it is open. While a screen's own control owns the devices,
    // these keys are its input, not the ring's.
    if suppressors.hud.return_home_open || yielded {
        return;
    }
    let intent = focus_intent(activate, next, prev);
    if intent == FocusIntent::Idle || state.candidates.is_empty() {
        return;
    }
    let Some(index) = resolve_focus(&state.candidates, state.memory(*screen.get())) else {
        return;
    };
    let target = match intent {
        FocusIntent::Move(delta) if visible.0 => {
            // The ring is one group's candidates; they are contiguous
            // because the list is sorted by (group, order).
            let group = state.candidates[index].group;
            let start = state
                .candidates
                .iter()
                .position(|candidate| candidate.group == group)
                .expect("the resolved candidate's group is present");
            let length = state.candidates[start..]
                .iter()
                .take_while(|candidate| candidate.group == group)
                .count();
            let position = (index - start) as i32;
            let moved = (position + delta).rem_euclid(length as i32) as usize;
            state.candidates[start + moved]
        }
        // The first navigation key only reveals the ring: the player cannot
        // see where focus is until it is drawn, so moving on the same press
        // would step away from a position they never saw.
        FocusIntent::Move(_) | FocusIntent::Activate => state.candidates[index],
        FocusIntent::Idle => unreachable!("idle returns above"),
    };
    visible.0 = true;
    state.remember(*screen.get(), target.group, target.order);
    if input_focus.get() != Some(target.entity) {
        input_focus.set(target.entity, FocusCause::Navigated);
    }
    if intent == FocusIntent::Activate {
        commands.trigger(Activate {
            entity: target.entity,
        });
    }
}

/// Whether F1 opens the Help modal on `screen`: the two screens that have
/// a button for it, Home and the Match, and no other. The modal is
/// application-persistent and could be shown over Settings and the two
/// Replay screens as well, but F1 is kept at parity with the buttons it
/// mirrors rather than made global - that would be three new entry points,
/// and is a decision to take on its own. A pure predicate, so a test can
/// hold it against every screen.
fn help_key_screen(screen: AppScreen) -> bool {
    matches!(screen, AppScreen::Home | AppScreen::Match)
}

/// The Help modal's fixed keys. Escape, or a pad's East or Start button,
/// closes it on whichever screen it is open, exactly as its Close button
/// does, and so does F1; on the two screens that have a Help button
/// ([`help_key_screen`]) F1 also opens it when it is closed, exactly as
/// that button does. Every path is the same [`apply_hud_action`] on
/// [`HudAction::CloseHelp`] or [`HudAction::OpenHelp`] (set or clear the
/// flag, arm the input gate) and the same cue, [`hud_action_ui_cue`]'s for
/// that action, so the keys and the buttons cannot drift apart.
///
/// One system rather than a dismissal and a separate toggle, because F1 is
/// read as `just_pressed` and a second system in the same chain would see
/// the same press: opening in one and reading a back key in the other would
/// close a modal the player had just opened, in one frame, with two cues.
/// Here the frame is decided once. While the modal is open any of the four
/// keys closes it. While it is closed only F1 opens it, and it yields to
/// the Match exit's two keys, Escape and a pad's Start, pressed in the same
/// frame: [`handle_match_return`] runs earlier in the chain and has already
/// done what they do (opened the Return Home confirmation, or cancelled
/// it), and on Home they do nothing, so either way the back key's own
/// effect stands alone and F1 adds nothing to it. A pad's East is not
/// yielded to while the modal is closed, because it is not a back button
/// then: F1 and East together during a Match open the modal and gate East
/// until it is released, exactly as F1 and an arrow do.
///
/// Closing is screen-agnostic on purpose. East is refused as a back button
/// on the idle Match screen, being a bindable gameplay input and the
/// default Teleport to Marker; here it is read only while the modal is
/// already open, when gameplay is blocked for the modal's whole life, and
/// closing re-arms the input gate until the button is physically released,
/// so the teleport cannot fire on the release either. Start is the pad's
/// back on every screen and Escape is the keyboard's; both are structurally
/// unbindable, as F1 is (`crate::input::keyboard` says how), see
/// [`handle_match_return`].
///
/// F1 does nothing while the Return Home confirmation is open, alone or
/// with the Escape or Start that cancels it: the pointer cannot reach the
/// Match HUD's Help button through that overlay, and the key keeps parity.
/// When F1 opens the modal it also reveals the ring, on
/// the inert Left seed, as Enter on Home's Help button would - a
/// key was pressed, so the outline is asked for - where a pointer-opened
/// modal arrives with it hidden (see [`hide_focus_ring_on_pointer_motion`]).
/// Closing leaves the flag alone, as Escape does.
///
/// Stands aside when Help or Close has already been activated this frame -
/// by [`apply_focus_navigation`], earlier in this chain, or by a pointer
/// click, which picking delivers before the chain runs. That activation
/// reaches `handle_ui_buttons` in `Update` and opens or closes the modal,
/// and its cue was queued when it happened; a second open or close here
/// would be idempotent in state but audible - two cues in one frame, which
/// would be the only same-frame pair in the crate with an audible artifact.
/// An activation of a page arrow does not stand it down: that is two inputs
/// asking for two different things, and both are honoured.
fn handle_help_modal_keys(
    devices: FocusDevices,
    mut activated: Activated<HudAction>,
    screen: Res<State<AppScreen>>,
    mut hud: ResMut<HudState>,
    mut input_gate: ResMut<InputGate>,
    mut visible: ResMut<InputFocusVisible>,
    mut audio: ResMut<AudioCueQueue>,
) {
    // Drained in full whatever this frame decides, so that no activation is
    // read again next frame.
    let activated: Vec<HudAction> = activated.read().copied().collect();
    let help_key = devices.keys.just_pressed(KeyCode::F1);
    // The Match exit's two keys, which `handle_match_return` has already
    // acted on this frame wherever there was something for them to do.
    let exit_key = devices.keys.just_pressed(KeyCode::Escape)
        || devices
            .gamepads
            .iter()
            .any(|gamepad| gamepad.just_pressed(GamepadButton::Start));
    let action = if hud.help_open {
        let back = exit_key
            || devices
                .gamepads
                .iter()
                .any(|gamepad| gamepad.just_pressed(GamepadButton::East));
        if !(back || help_key) || activated.contains(&HudAction::CloseHelp) {
            return;
        }
        HudAction::CloseHelp
    } else {
        if !(help_key && help_key_screen(*screen.get()))
            || hud.return_home_open
            || exit_key
            || activated.contains(&HudAction::OpenHelp)
        {
            return;
        }
        HudAction::OpenHelp
    };
    audio.ui(hud_action_ui_cue(action));
    let screen_change = apply_hud_action(&mut hud, &mut input_gate, action);
    debug_assert!(
        screen_change.is_none(),
        "opening or closing the Help modal never changes screen"
    );
    if action == HudAction::OpenHelp {
        visible.0 = true;
    }
}

/// The Match screen's non-pointer exit: Escape or a pad's Start button steps
/// back one layer (cancel the Return Home confirmation, or open it), and
/// Enter or South confirms the return while the confirmation is open.
/// Everything routes through [`apply_hud_action`], so the modal state, the
/// input-gate arming and the same-frame pause behave exactly as the pointer
/// buttons do. The Help modal is the top of the Match's back stack
/// and belongs to [`handle_help_modal_keys`], which runs after this system
/// in the same chain: while it is open this system stands aside, so the
/// press that closes it cannot also open the confirmation, and the next
/// press does.
///
/// Start is the pad's back button on every screen; this is the one screen
/// where East is not also back, deliberately: `ControllerInput::East` is a
/// bindable gameplay input and the *default Teleport to Marker*, so a back
/// button on East would fire mid-match on every teleport. (East does close
/// the Help modal here, in [`handle_help_modal_keys`], which reads
/// it only while that modal already blocks gameplay.) Start is absent from
/// `ControllerInput`'s twenty variants, which makes it structurally
/// unbindable in exactly the way `KeyCode::Escape` and `KeyCode::F1` are
/// (the two reserved keys) - the same property that makes Escape safe here,
/// F1 safe as the Help key, and the reason
/// Start can be back everywhere else too. Enter and South *are* bindable,
/// but they are read only while the confirmation is open, when the input
/// gate is armed and gameplay is paused, so neither can reach the board;
/// and when back and confirm land in the same frame, back wins, because the
/// destructive read of the pair should lose ties.
///
/// Escape stays live during a Match on purpose - it is how a keyboard
/// player leaves at all - and nothing else about the gameplay keys
/// changes: arrows, E/S/D/F, M/B/N and Space reach gameplay bit-for-bit,
/// and no focus ring exists during live play.
fn handle_match_return(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut hud: ResMut<HudState>,
    mut input_gate: ResMut<InputGate>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut audio: ResMut<AudioCueQueue>,
) {
    if hud.help_open {
        return;
    }
    let back = keys.just_pressed(KeyCode::Escape)
        || gamepads
            .iter()
            .any(|gamepad| gamepad.just_pressed(GamepadButton::Start));
    let confirm = hud.return_home_open
        && (keys.just_pressed(KeyCode::Enter)
            || gamepads
                .iter()
                .any(|gamepad| gamepad.just_pressed(GamepadButton::South)));
    let action = if back {
        if hud.return_home_open {
            HudAction::CancelReturnHome
        } else {
            HudAction::OpenReturnHome
        }
    } else if confirm {
        HudAction::ConfirmReturnHome
    } else {
        return;
    };
    audio.ui(hud_action_ui_cue(action));
    if let Some(screen) = apply_hud_action(&mut hud, &mut input_gate, action) {
        next_screen.set(screen);
    }
}

/// Re-collects the visible, enabled focusable buttons and re-resolves the
/// memory against them.
///
/// Runs on every screen, the Match included. During live play nothing there
/// is `Focusable`, so this writes an empty list once and then finds nothing
/// to change; on the frame a modal closes it is what drops the modal's
/// now-hidden buttons and clears the published focus, and `help_open` goes
/// false in `PreUpdate` or `Update`, so a gate on the modal being open would
/// already be false here and leave both behind for the rest of the Match.
///
/// Runs in `PostUpdate` after [`UiSystems::Layout`] - one stage earlier and
/// it would read last frame's `ComputedNode`, admitting buttons that were
/// hidden this frame. The same ordering is load-bearing for the Replay
/// Library's respawn: its rebuilt entities apply at the end of `Update` and
/// are laid out in this frame's layout pass, so this collector sees them at
/// their real sizes on the very frame the old ones died and the ring never
/// blinks. Dropping zero-size nodes is what excludes hidden
/// subtrees: `Display::None` zeroes a node and recurses into its children
/// (pinned against real layout by the tests below). Dropping
/// [`InteractionDisabled`] nodes is the rule that a disabled control is not
/// in the ring, for a control disabled while it is on screen.
fn refresh_focus_candidates(
    nodes: Query<(Entity, &Focusable, &ComputedNode), Without<InteractionDisabled>>,
    screen: Res<State<AppScreen>>,
    mut state: ResMut<UiFocusState>,
    mut input_focus: ResMut<InputFocus>,
) {
    state.candidates.clear();
    for (entity, focusable, computed) in &nodes {
        if computed.size() == Vec2::ZERO {
            continue;
        }
        state.candidates.push(FocusCandidate {
            group: focusable.group,
            order: focusable.order,
            entity,
        });
    }
    state
        .candidates
        .sort_unstable_by_key(|candidate| (candidate.group, candidate.order));
    match resolve_focus(&state.candidates, state.memory(*screen.get())) {
        Some(index) => {
            let candidate = state.candidates[index];
            // Normalizing the memory to the candidate actually resolved
            // keeps later movement stepping from where the ring is drawn,
            // not from an order that no longer exists.
            state.remember(*screen.get(), candidate.group, candidate.order);
            if input_focus.get() != Some(candidate.entity) {
                input_focus.set(candidate.entity, FocusCause::Navigated);
            }
        }
        // The memory is kept - it is what brings the ring back where it was
        // - but the published screen-reader cursor must not dangle on a
        // despawned or hidden entity.
        None => {
            if input_focus.get().is_some() {
                input_focus.clear();
            }
        }
    }
    // The modal slot lives only while a dialog owns the ring, so every
    // dialog seeds at its first entry - on a screen whose own ring takes the
    // ring back, and on the Match, where nothing does. See `FocusMemory`.
    let owner = state.candidates.last().map(|candidate| candidate.group);
    if owner != Some(FocusGroup::Modal) {
        state.forget(*screen.get(), FocusGroup::Modal);
    }
}

/// Colours the focused button's [`Outline`] and blanks every other, writing
/// only on change. Hue is a redundant second channel here: the focused
/// control is the only control on screen carrying an outline at all, a
/// shape difference that survives greyscale and any colour vision
/// deficiency.
fn update_focus_indicator(
    visible: Res<InputFocusVisible>,
    input_focus: Res<InputFocus>,
    mut outlines: Query<(Entity, &mut Outline), With<Focusable>>,
) {
    let focused = if visible.0 { input_focus.get() } else { None };
    for (entity, mut outline) in &mut outlines {
        let target = if focused == Some(entity) {
            ORANGE
        } else {
            Color::NONE
        };
        if outline.color != target {
            outline.color = target;
        }
    }
}

/// The production UI on a headless `App`: `bevy_ui`'s own plugin (layout,
/// the text systems and the font the game draws with, stacking and
/// visibility), `bevy_picking` with its UI backend, and `ui::button`'s
/// plugin, so a screen's tests run over the layout and the hit-testing the
/// player gets. Shared with every screen's tests. A test presses a button the way a
/// player does with `ui::button`'s `click` and the helpers beside it, which
/// write the window events winit would; nothing writes button state by
/// hand.
///
/// No renderer runs, so the camera's render target is filled in by hand, as
/// `bevy_ui`'s own layout tests do it, and the stores the renderer's
/// plugins would make are added empty. The window is the game's, 1040 by
/// 900 at a scale factor of 1, so a logical pixel is a physical one. Keys
/// are not part of it: a test presses them by writing `ButtonInput<KeyCode>`
/// directly, which `InputPlugin` would clear before the ring could read it.
#[cfg(test)]
pub(crate) fn real_layout_app() -> App {
    let mut app = engine_ui_app();
    app.add_plugins(super::button::UiButtonPlugin);
    app
}

/// [`real_layout_app`] before `ui::button`'s plugin is added: Bevy's UI and
/// picking alone, for the one test that needs Bevy's own button behaviour
/// in its place.
#[cfg(test)]
pub(crate) fn engine_ui_app() -> App {
    use bevy::app::TaskPoolPlugin;
    use bevy::asset::AssetPlugin;
    use bevy::camera::visibility::VisibilityPlugin;
    use bevy::camera::{Camera, Camera2d, ComputedCameraValues, RenderTargetInfo, Viewport};
    use bevy::input::keyboard::KeyboardInput;
    use bevy::input::mouse::{MouseButtonInput, mouse_button_input_system};
    use bevy::input::touch::{TouchInput, touch_screen_input_system};
    use bevy::picking::input::PointerInputPlugin;
    use bevy::picking::{InteractionPlugin, PickingPlugin};
    use bevy::text::TextPlugin;
    use bevy::window::{ExitCondition, WindowEvent, WindowPlugin, WindowResolution};

    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        bevy::transform::TransformPlugin,
        WindowPlugin {
            primary_window: Some(Window {
                resolution: WindowResolution::new(1040, 900).with_scale_factor_override(1.0),
                ..default()
            }),
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
            ..default()
        },
        AssetPlugin::default(),
        TextPlugin,
        VisibilityPlugin,
        bevy::ui::UiPlugin,
        PickingPlugin,
        InteractionPlugin,
        PointerInputPlugin,
    ))
    // What the plugins above read and no plugin here provides headlessly:
    // the image, atlas and mesh stores the renderer's plugins would make.
    .init_asset::<Image>()
    .init_asset::<TextureAtlasLayout>()
    .init_asset::<Mesh>()
    .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
    // Time without `TimePlugin`, which would overwrite it from the wall
    // clock every frame: a test advances it by hand.
    .init_resource::<Time>()
    .init_resource::<Time<bevy::time::Real>>()
    .add_message::<WindowEvent>()
    .add_message::<KeyboardInput>()
    .add_message::<MouseButtonInput>()
    .add_message::<TouchInput>()
    .init_resource::<ButtonInput<MouseButton>>()
    .init_resource::<bevy::input::touch::Touches>()
    .add_systems(
        PreUpdate,
        (mouse_button_input_system, touch_screen_input_system).in_set(InputSystems),
    );
    let physical_size = UVec2::new(1040, 900);
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size,
                    scale_factor: 1.0,
                }),
                ..Default::default()
            },
            viewport: Some(Viewport {
                physical_size,
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    app
}

/// Where one `Text` was drawn, read from a laid-out app by
/// [`measure_drawn_text`], in logical pixels from the window's top-left.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct DrawnText {
    /// The inked extent of the text's glyphs.
    pub(crate) ink: Rect,
    /// The inked extent of each of its lines, top to bottom, so a test can
    /// see how lines of different lengths sit against each other.
    pub(crate) lines: Vec<Rect>,
    /// The border box of the nearest node that draws a fill, starting from
    /// the text's own: the surface the player sees the text on.
    pub(crate) surface: Rect,
    /// That node's fill, so a test can say which surface it expected.
    pub(crate) surface_fill: Color,
}

/// Reads the one `Text` on screen whose content is `content`, on a
/// [`real_layout_app`].
///
/// The text is measured by its glyphs rather than by its node, because the
/// defect this exists to catch is a node in the right place drawing its
/// glyphs in the wrong one - see "A Text node does not centre its own text"
/// in `docs/TESTING.md`. The surface is the nearest node that draws a fill,
/// starting from the text's own, so a `Text` that is itself a filled card is
/// measured against itself, as the player sees it. Text drawn straight onto
/// the window has no such node; [`measure_drawn_lines`] reads that.
#[cfg(test)]
pub(crate) fn measure_drawn_text(app: &mut App, content: &str) -> DrawnText {
    use bevy::ui::{ComputedNode, UiGlobalTransform};

    let (entity, lines) = drawn_glyph_lines(app, content);
    let ink = lines
        .iter()
        .copied()
        .reduce(|all, line| all.union(line))
        .expect("drawn_glyph_lines finds at least one line");

    let world = app.world_mut();
    let mut nodes = world.query::<(
        &ComputedNode,
        &UiGlobalTransform,
        &BackgroundColor,
        Option<&ChildOf>,
    )>();
    let mut current = entity;
    loop {
        let (node, transform, fill, child_of) = nodes
            .get(world, current)
            .expect("the text and its ancestors are nodes");
        if fill.0.alpha() > 0.0 {
            let scale = node.inverse_scale_factor;
            return DrawnText {
                ink,
                lines,
                surface: Rect::from_center_size(transform.translation * scale, node.size * scale),
                surface_fill: fill.0,
            };
        }
        current = child_of
            .unwrap_or_else(|| panic!("no node under {content:?} draws a fill"))
            .parent();
    }
}

/// The inked extent of each line of the one `Text` reading `content`, top
/// to bottom, in logical pixels from the window's top-left - the glyph half
/// of [`measure_drawn_text`], for text with no filled node beneath it, such
/// as a heading drawn straight onto the window.
#[cfg(test)]
pub(crate) fn measure_drawn_lines(app: &mut App, content: &str) -> Vec<Rect> {
    drawn_glyph_lines(app, content).1
}

#[cfg(test)]
fn drawn_glyph_lines(app: &mut App, content: &str) -> (Entity, Vec<Rect>) {
    use bevy::text::TextLayoutInfo;
    use bevy::ui::{ComputedNode, UiGlobalTransform};

    let world = app.world_mut();
    let mut texts = world.query::<(
        Entity,
        &Text,
        &ComputedNode,
        &UiGlobalTransform,
        &TextLayoutInfo,
    )>();
    let mut matching = texts
        .iter(world)
        .filter(|(_, text, ..)| text.0 == content)
        .map(|(entity, _, node, transform, layout)| (entity, *node, *transform, layout.clone()));
    let (entity, node, transform, layout) = matching
        .next()
        .unwrap_or_else(|| panic!("no text reading {content:?} on screen"));
    assert!(
        matching.next().is_none(),
        "more than one text reading {content:?} on screen"
    );

    // Logical pixels from the window's top-left, the way the renderer
    // places glyphs: each centred on its position, measured from the
    // top-left corner of the node's content box.
    let scale = node.inverse_scale_factor;
    let content_origin = transform.translation + node.content_box().min;
    let mut lines = std::collections::BTreeMap::<u32, Rect>::new();
    for glyph in &layout.glyphs {
        let centre = (content_origin + glyph.position) * scale;
        let ink = Rect::from_center_size(centre, glyph.atlas_info.rect.size() * scale);
        lines
            .entry(glyph.line_index)
            .and_modify(|line| *line = line.union(ink))
            .or_insert(ink);
    }
    let lines: Vec<Rect> = lines.into_values().collect();
    assert!(
        !lines.is_empty(),
        "the text reading {content:?} has no glyphs"
    );
    (entity, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppScreen, AppSystems, configure_app_systems};
    use crate::audio::{UiCue, UiSound, queue_activation_cue};
    use crate::gameplay::GameplayPause;
    use crate::ui::button::{UiButtonPlugin, record_activations, recorded_activations};
    use crate::ui::hud::{HudAction, handle_ui_buttons, sync_gameplay_pause};
    use bevy::ui::Pressed;
    use bevy::ui_widgets::Button;

    fn candidate(group: FocusGroup, order: u32, index: u32) -> FocusCandidate {
        FocusCandidate {
            group,
            order,
            entity: Entity::from_raw_u32(index).unwrap(),
        }
    }

    #[test]
    fn an_activation_press_wins_and_discards_the_same_frames_movement() {
        assert_eq!(focus_intent(true, 3, 0), FocusIntent::Activate);
        assert_eq!(focus_intent(true, 0, 0), FocusIntent::Activate);
        assert_eq!(focus_intent(false, 0, 0), FocusIntent::Idle);
    }

    /// The outcome that must be impossible - a focus ring live while
    /// gameplay is live - held as a property of the gate's own predicate
    /// rather than of the schedule around it: on the Match screen, whenever
    /// the ring is allowed to run, the same `HudState` already blocks
    /// gameplay input through `modal_pause_state`. Every screen and every
    /// combination of the two modal flags is checked, so the property
    /// survives a refinement of the gate and a refactor of the schedule
    /// alike.
    #[test]
    fn the_gate_never_admits_the_ring_while_gameplay_is_live() {
        for screen in AppScreen::ALL {
            for (help_open, return_home_open) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let hud = HudState {
                    help_open,
                    help_page: 0,
                    return_home_open,
                };
                if screen == AppScreen::Match && focus_ring_allowed(screen, &hud) {
                    assert!(
                        crate::ui::hud::modal_pause_state(&hud).input_blocked,
                        "the ring may run on the Match with {hud:?} while gameplay reads keys"
                    );
                }
            }
        }
        // Positive controls, so the loop above cannot pass by never admitting
        // the ring anywhere: every menu screen has one with no modal open,
        // and the Match has one exactly while either modal is open - which
        // is the refinement itself, and would pass the loop above vacuously
        // if the gate still read "not during a Match".
        for screen in AppScreen::ALL {
            if screen != AppScreen::Match {
                assert!(
                    focus_ring_allowed(screen, &HudState::default()),
                    "{screen:?}"
                );
            }
        }
        assert!(!focus_ring_allowed(AppScreen::Match, &HudState::default()));
        for (help_open, return_home_open) in [(true, false), (false, true), (true, true)] {
            let hud = HudState {
                help_open,
                help_page: 0,
                return_home_open,
            };
            assert!(focus_ring_allowed(AppScreen::Match, &hud), "{hud:?}");
        }
    }

    /// The Return Home confirmation stands the ring down outright even when
    /// something on the Match screen is focusable: the gate admits the ring
    /// while that modal is open, so `apply_focus_navigation`'s early return
    /// is what keeps a marked HUD button from being pressed through the
    /// overlay. Nothing on the Match is `Focusable` today, so the candidate
    /// here is a fabricated stand-in for a button someone marks later -
    /// exactly the case the line is kept for. Space rather than Enter, so the
    /// confirmation's own confirm key is not what the assertion is about.
    #[test]
    fn the_return_home_confirmation_stands_the_ring_down_even_with_candidates() {
        let (mut app, _entities) = focus_app(0);
        enter_match(&mut app);
        let hud_button = app
            .world_mut()
            .spawn((
                Button,
                HudAction::OpenHelp,
                Focusable {
                    group: FocusGroup::Screen,
                    order: 0,
                },
                ComputedNode {
                    size: Vec2::new(172.0, 34.0),
                    ..Default::default()
                },
            ))
            .id();
        app.world_mut().resource_mut::<HudState>().return_home_open = true;
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(hud_button),
            "the gate admits the ring, and the button is collected"
        );

        press_key(&mut app, KeyCode::Space);
        app.update();
        let hud = app.world().resource::<HudState>();
        assert!(
            !hud.help_open,
            "a button under the confirmation must not be pressed through it"
        );
        assert!(hud.return_home_open);
        assert!(recorded_activations(&mut app).is_empty());
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));

        // The positive control: the same Space with the Help modal open in
        // the confirmation's place - which admits the ring just as the
        // confirmation does, and stands nothing down - presses the button,
        // so it was the confirmation that held it back.
        release_keys(&mut app);
        {
            let mut hud = app.world_mut().resource_mut::<HudState>();
            hud.return_home_open = false;
            hud.help_open = true;
        }
        app.update();
        press_key(&mut app, KeyCode::Space);
        app.update();
        assert_eq!(recorded_activations(&mut app), [hud_button]);
    }

    #[test]
    fn opposite_presses_cancel_and_a_majority_moves_one_place_at_most() {
        assert_eq!(focus_intent(false, 1, 1), FocusIntent::Idle);
        assert_eq!(focus_intent(false, 2, 2), FocusIntent::Idle);
        assert_eq!(focus_intent(false, 1, 0), FocusIntent::Move(1));
        assert_eq!(focus_intent(false, 3, 1), FocusIntent::Move(1));
        assert_eq!(focus_intent(false, 0, 1), FocusIntent::Move(-1));
        assert_eq!(focus_intent(false, 1, 4), FocusIntent::Move(-1));
    }

    fn memory(group: FocusGroup, order: u32) -> FocusMemory {
        let mut memory = FocusMemory::default();
        memory.remember(group, order);
        memory
    }

    #[test]
    fn focus_resolution_prefers_exact_then_clamps_then_falls_back() {
        let candidates = [
            candidate(FocusGroup::Screen, 0, 1),
            candidate(FocusGroup::Screen, 2, 2),
            candidate(FocusGroup::Screen, 5, 3),
        ];

        // An exact order match wins.
        assert_eq!(
            resolve_focus(&candidates, memory(FocusGroup::Screen, 2)),
            Some(1)
        );
        // The exact order gone: the nearest lower order in the group.
        assert_eq!(
            resolve_focus(&candidates, memory(FocusGroup::Screen, 4)),
            Some(1)
        );
        assert_eq!(
            resolve_focus(&candidates, memory(FocusGroup::Screen, 9)),
            Some(2)
        );
        // Nothing lower left: the group's first entry.
        let upper = [
            candidate(FocusGroup::Screen, 3, 1),
            candidate(FocusGroup::Screen, 4, 2),
        ];
        assert_eq!(
            resolve_focus(&upper, memory(FocusGroup::Screen, 1)),
            Some(0)
        );
        // No memory at all: the first entry.
        assert_eq!(resolve_focus(&candidates, FocusMemory::default()), Some(0));
        // No candidates: nothing to focus.
        assert_eq!(resolve_focus(&[], memory(FocusGroup::Screen, 0)), None);
    }

    /// The highest group present owns the ring, whatever the memory says
    /// about lower groups; the owning group's own slot decides the entry,
    /// and an empty slot seeds at its first entry rather than at index 0
    /// of the whole list.
    #[test]
    fn the_highest_group_present_owns_the_ring() {
        let with_modal = [
            candidate(FocusGroup::Screen, 0, 1),
            candidate(FocusGroup::Screen, 1, 2),
            candidate(FocusGroup::Modal, 0, 3),
            candidate(FocusGroup::Modal, 1, 4),
        ];
        // A remembered Screen position does not hold the ring under an
        // open dialog: the dialog's first entry takes it.
        assert_eq!(
            resolve_focus(&with_modal, memory(FocusGroup::Screen, 1)),
            Some(2)
        );
        // The dialog's own slot, once it has one, is what is resolved.
        let mut both = memory(FocusGroup::Screen, 1);
        both.remember(FocusGroup::Modal, 1);
        assert_eq!(resolve_focus(&with_modal, both), Some(3));
        // The dialog gone, the Screen slot is untouched underneath.
        assert_eq!(resolve_focus(&with_modal[..2], both), Some(1));
    }

    /// A minimal app running the real plugin registration plus the real
    /// production `Update` graph, on the Home screen, with `count` focusable
    /// buttons whose layout sizes are fabricated by choice - the production
    /// graph is what this fixture exercises; [`real_layout_app`] is the one
    /// for tests that need the layout pass itself.
    fn focus_app(count: u32) -> (App, Vec<Entity>) {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_plugins(UiButtonPlugin)
            .add_plugins(UiFocusPlugin)
            .insert_resource(HudState::default())
            .insert_resource(crate::gameplay::InputGate::default())
            .insert_resource(GameplayPause::default())
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(
                Update,
                (handle_ui_buttons, sync_gameplay_pause)
                    .chain()
                    .in_set(AppSystems::HudInput),
            );
        configure_app_systems(&mut app);
        record_activations(&mut app);
        let actions = [
            HudAction::PlayMatch,
            HudAction::OpenHelp,
            HudAction::OpenControlMapping,
            HudAction::OpenReplays,
        ];
        let entities = (0..count)
            .map(|order| {
                app.world_mut()
                    .spawn((
                        Button,
                        actions[order as usize % actions.len()],
                        Focusable {
                            group: FocusGroup::Screen,
                            order,
                        },
                        ComputedNode {
                            size: Vec2::new(390.0, 54.0),
                            ..Default::default()
                        },
                    ))
                    .id()
            })
            .collect();
        (app, entities)
    }

    fn press_key(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
        keys.press(key);
    }

    /// No `InputPlugin` runs headlessly, so `just_pressed` never expires on
    /// its own; tests that update without pressing first must clear it or
    /// the previous key repeats.
    fn release_keys(app: &mut App) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        let held: Vec<KeyCode> = keys.get_pressed().copied().collect();
        for key in held {
            keys.release(key);
        }
        keys.clear();
    }

    fn focused_entity(app: &App) -> Option<Entity> {
        app.world().resource::<InputFocus>().get()
    }

    /// The ring's activation through the real production schedule: the
    /// plugin's own `PreUpdate` activation and `handle_ui_buttons` under the
    /// real [`configure_app_systems`] graph. Enter on the focused Play
    /// button is exactly one activation of it, which asks for the Match
    /// screen; a held Enter is no second one; and the next Enter is, so an
    /// activated button never goes dead. Nothing marks it `Pressed`: that
    /// is the pointer's alone.
    #[test]
    fn enter_activates_the_focused_button_once_per_press() {
        let (mut app, entities) = focus_app(4);
        app.update();

        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(recorded_activations(&mut app), [entities[0]]);
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Match)
        ));
        assert!(app.world().get::<Pressed>(entities[0]).is_none());

        // Held, not pressed again: `just_pressed` is cleared, `pressed` is
        // not.
        *app.world_mut().resource_mut::<NextState<AppScreen>>() = NextState::Unchanged;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert!(recorded_activations(&mut app).is_empty());

        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(recorded_activations(&mut app), [entities[0]]);
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Match)
        ));
    }

    /// The first movement key only reveals the ring; movement starts from
    /// the second, steps one place per frame, and wraps in both directions.
    /// Opposite same-frame presses cancel instead of moving.
    #[test]
    fn movement_reveals_first_then_steps_and_wraps_the_ring() {
        let (mut app, entities) = focus_app(4);
        app.update();
        assert!(!app.world().resource::<InputFocusVisible>().0);

        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert!(app.world().resource::<InputFocusVisible>().0);
        assert_eq!(focused_entity(&app), Some(entities[0]));

        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));

        // Shift+Tab moves back; from the first entry it wraps to the last.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[0]));
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[3]));

        // Two players pressing opposite directions in the same frame: the
        // ring must not move.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::ShiftLeft);
            keys.release(KeyCode::Tab);
            keys.clear();
            keys.press(KeyCode::ArrowDown);
            keys.press(KeyCode::ArrowUp);
        }
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[3]));
    }

    /// A pad drives the same ring: the D-pad moves and South activates.
    #[test]
    fn a_gamepad_moves_the_ring_and_activates_with_south() {
        let (mut app, entities) = focus_app(4);
        app.update();

        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::DPadDown);
        let pad = app.world_mut().spawn(gamepad).id();
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(entities[0]),
            "first press reveals"
        );

        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .unwrap()
            .digital_mut()
            .clear();
        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .unwrap()
            .digital_mut()
            .press(GamepadButton::DPadRight);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));

        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::DPadRight);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::South);
        }
        app.update();

        // The focused button is Help, so South's activation shows as the
        // help modal opening through the same reader a click reaches.
        assert!(app.world().resource::<HudState>().help_open);
        assert_eq!(
            recorded_activations(&mut app),
            [entities[1]],
            "one press of South, one activation"
        );
    }

    /// The ring is drawn as the focused button's `Outline` colour - set in
    /// place, never inserted or removed - visible only after a navigation
    /// key and hidden again the moment the pointer moves.
    #[test]
    fn the_indicator_recolours_one_outline_and_yields_to_the_pointer() {
        let (mut app, entities) = focus_app(4);
        app.update();
        assert_eq!(
            app.world().get::<Outline>(entities[0]).unwrap().color,
            Color::NONE,
            "hidden until a key is used"
        );

        press_key(&mut app, KeyCode::ArrowDown);
        app.update();
        assert_eq!(
            app.world().get::<Outline>(entities[0]).unwrap().color,
            ORANGE
        );
        assert!(
            entities[1..]
                .iter()
                .all(|entity| app.world().get::<Outline>(*entity).unwrap().color == Color::NONE),
            "exactly one control may carry a visible outline"
        );
        let outline = app.world().get::<Outline>(entities[0]).unwrap();
        assert_eq!((outline.width, outline.offset), (px(3), px(2)));

        // A burst of several cursor moves in the same frame as a navigation
        // key must be drained whole: a leftover message would hide the ring
        // one motionless frame later.
        for _ in 0..3 {
            app.world_mut().write_message(CursorMoved {
                window: Entity::PLACEHOLDER,
                position: Vec2::new(1.0, 1.0),
                delta: None,
            });
        }
        press_key(&mut app, KeyCode::ArrowDown);
        app.update();
        assert_eq!(
            app.world().get::<Outline>(entities[0]).unwrap().color,
            ORANGE
        );
        release_keys(&mut app);
        app.update();
        assert_eq!(
            app.world().get::<Outline>(entities[0]).unwrap().color,
            ORANGE,
            "a motionless frame after the burst must not hide the ring"
        );

        app.world_mut().write_message(CursorMoved {
            window: Entity::PLACEHOLDER,
            position: Vec2::new(1.0, 1.0),
            delta: None,
        });
        app.update();
        assert_eq!(
            app.world().get::<Outline>(entities[0]).unwrap().color,
            Color::NONE
        );
    }

    /// Focus is keyed on `(group, order)`, not `Entity`: despawning the
    /// focused button and respawning the screen (as the Replay Library's
    /// dirty rebuild does) resolves the same key again, and a key whose
    /// exact order is gone clamps to the nearest lower order.
    #[test]
    fn focus_memory_survives_a_despawn_and_clamps_when_its_order_is_gone() {
        let (mut app, entities) = focus_app(4);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[2]));
        release_keys(&mut app);

        // The focused entity dies; its (group, order) key does not.
        app.world_mut().entity_mut(entities[2]).despawn();
        let respawned = app
            .world_mut()
            .spawn((
                Button,
                HudAction::OpenControlMapping,
                Focusable {
                    group: FocusGroup::Screen,
                    order: 2,
                },
                ComputedNode {
                    size: Vec2::new(390.0, 54.0),
                    ..Default::default()
                },
            ))
            .id();
        app.update();
        assert_eq!(focused_entity(&app), Some(respawned));

        // Now the order itself disappears: focus clamps to order 1.
        app.world_mut().entity_mut(respawned).despawn();
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));
    }

    /// The premise every hidden modal and hidden tab panel leaves the ring
    /// by, checked against the engine rather than asserted: under real
    /// `bevy_ui` layout, a `Display::None` parent zeroes the `ComputedNode`
    /// of its `Button` child, and a shown sibling's child is non-zero - the
    /// positive control, so a layout that never ran cannot pass this
    /// vacuously. The fixture is `bevy_ui`'s own layout-test fixture: the
    /// real `ui_layout_system` fed by a camera with a hand-filled render
    /// target, since no window exists headlessly.
    #[test]
    fn display_none_zeroes_a_hidden_buttons_computed_node_under_real_layout() {
        let mut app = real_layout_app();
        let (hidden, shown) = spawn_hidden_and_shown_buttons(app.world_mut());
        app.update();

        assert_eq!(
            app.world().get::<ComputedNode>(shown).unwrap().size(),
            Vec2::new(390.0, 54.0),
            "the shown sibling's child must be laid out at its declared size, \
             or the layout never ran and the zero below proves nothing"
        );
        assert_eq!(
            app.world().get::<ComputedNode>(hidden).unwrap().size(),
            Vec2::ZERO,
            "a Display::None parent must zero its descendants' ComputedNodes; \
             if it no longer does, the candidate collector needs an explicit \
             active-group resource instead of the zero-size test"
        );
    }

    /// The same premise through the production collector: under real layout
    /// the hidden button is not a candidate and the shown one is, and a
    /// parent flipped from `Display::None` to `Display::Flex` - the toggle
    /// every modal here opens with - admits its button on that very frame,
    /// with the reverse flip dropping it again.
    #[test]
    fn hidden_buttons_leave_and_rejoin_the_ring_under_real_layout() {
        let mut app = real_layout_app();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_plugins(UiFocusPlugin)
            .insert_resource(HudState::default())
            .insert_resource(crate::gameplay::InputGate::default())
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>();
        let (hidden, shown) = spawn_hidden_and_shown_buttons(app.world_mut());
        app.update();

        let candidates = |app: &App| -> Vec<Entity> {
            app.world()
                .resource::<UiFocusState>()
                .candidates
                .iter()
                .map(|candidate| candidate.entity)
                .collect()
        };
        assert_eq!(candidates(&app), [shown]);

        let flip = |app: &mut App, entity: Entity, display: Display| {
            let parent = app.world().get::<ChildOf>(entity).unwrap().parent();
            app.world_mut().get_mut::<Node>(parent).unwrap().display = display;
        };
        flip(&mut app, hidden, Display::Flex);
        app.update();
        assert_eq!(candidates(&app), [hidden, shown]);

        flip(&mut app, shown, Display::None);
        app.update();
        assert_eq!(candidates(&app), [hidden]);
    }

    /// One root holding two panels, one `Display::None` and one shown, each
    /// with a focusable `Button` child at Home's menu-button size. Returns
    /// `(hidden button, shown button)`.
    fn spawn_hidden_and_shown_buttons(world: &mut World) -> (Entity, Entity) {
        let button = |order: u32| {
            (
                Button,
                Focusable {
                    group: FocusGroup::Screen,
                    order,
                },
                Node {
                    width: px(390),
                    height: px(54),
                    ..default()
                },
            )
        };
        let mut ids = [Entity::PLACEHOLDER; 2];
        world
            .spawn(Node {
                width: percent(100),
                height: percent(100),
                ..default()
            })
            .with_children(|root| {
                root.spawn(Node {
                    display: Display::None,
                    ..default()
                })
                .with_children(|hidden| {
                    ids[0] = hidden.spawn(button(0)).id();
                });
                root.spawn(Node::default()).with_children(|shown| {
                    ids[1] = shown.spawn(button(1)).id();
                });
            });
        (ids[0], ids[1])
    }

    /// A zero-size candidate must not be focusable - it is not on screen.
    /// The zero is written by hand here, on the production `Update` graph;
    /// that `Display::None` really produces it is pinned separately, under
    /// real layout, by `display_none_zeroes_a_hidden_buttons_computed_node_under_real_layout`.
    #[test]
    fn hidden_zero_size_buttons_leave_the_ring() {
        let (mut app, entities) = focus_app(2);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));
        release_keys(&mut app);

        app.world_mut()
            .get_mut::<ComputedNode>(entities[1])
            .unwrap()
            .size = Vec2::ZERO;
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[0]));
    }

    /// While the Help modal covers Home, the ring belongs to the
    /// modal's own buttons and cannot press the screen's buttons through
    /// the overlay: Enter on the seed (Left, a no-op on page 1) changes no
    /// screen, the ring walks to Close and Enter there closes the modal
    /// through the unmodified button reader, arming the gate. Escape, or a
    /// pad's East or Start button, is the other way out, doing exactly what
    /// Close does - one Cancel cue included.
    #[test]
    fn the_modals_buttons_take_the_ring_and_escape_east_or_start_close_it() {
        let (mut app, entities) = focus_app(4);
        let modal = spawn_help_modal_buttons(&mut app);
        app.update();
        // The pointer opened the Help modal: Home's ring is on Play, hidden.
        open_help(&mut app, &modal);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[0]));

        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));
        assert!(app.world().resource::<HudState>().help_open);
        assert_eq!(app.world().resource::<HudState>().help_page, 0);

        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[2]));
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        close_help_layout(&mut app, &modal);
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(entities[0]),
            "closing hands the ring back to the screen"
        );
        release_keys(&mut app);

        open_help(&mut app, &modal);
        app.update();
        app.world_mut().resource_mut::<AudioCueQueue>().drain();
        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        assert_eq!(
            app.world_mut().resource_mut::<AudioCueQueue>().drain(),
            vec![crate::audio::AudioCue::Ui(UiCue::Cancel)]
        );
        close_help_layout(&mut app, &modal);

        open_help(&mut app, &modal);
        release_keys(&mut app);
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::East);
        let pad = app.world_mut().spawn(gamepad).id();
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        close_help_layout(&mut app, &modal);

        // Start is the pad's back on every screen, so it closes the modal
        // exactly as East does.
        open_help(&mut app, &modal);
        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::East);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::Start);
        }
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
    }

    /// A same-frame Escape and Enter on Close - two inputs that both close
    /// the modal - close it once and queue exactly one Cancel cue: the
    /// dismissal stands aside for the activation it can see, and the
    /// button's own `UiSound` is the one that sounds. A same-frame Escape
    /// and Enter on a page arrow are two different requests, and both are
    /// honoured: the page turns and the modal closes.
    #[test]
    fn a_same_frame_escape_and_enter_on_close_close_the_modal_once_with_one_cue() {
        let (mut app, _entities) = focus_app(4);
        app.add_observer(queue_activation_cue);
        let modal = spawn_help_modal_buttons(&mut app);
        for entity in modal {
            let action = *app.world().get::<HudAction>(entity).unwrap();
            app.world_mut()
                .entity_mut(entity)
                .insert(UiSound::new(hud_action_ui_cue(action)));
        }
        app.update();
        let cues = |app: &mut App| {
            let mut cues = app.world_mut().resource_mut::<AudioCueQueue>().drain();
            cues.sort_by_key(|cue| format!("{cue:?}"));
            cues
        };
        let cancel = crate::audio::AudioCue::Ui(UiCue::Cancel);
        let confirm = crate::audio::AudioCue::Ui(UiCue::Confirm);

        // Enter alone on Close: the button's own cue, once. The first Tab
        // only reveals the ring (on Left); the next two reach Close.
        open_help(&mut app, &modal);
        app.update();
        for _ in 0..3 {
            press_key(&mut app, KeyCode::Tab);
            app.update();
        }
        assert_eq!(focused_entity(&app), Some(modal[2]));
        cues(&mut app);
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert_eq!(cues(&mut app), vec![cancel]);
        release_keys(&mut app);
        close_help_layout(&mut app, &modal);
        app.update();

        // Escape and Enter together, ring on Close: one close, one cue. The
        // ring is visible from here on, so every key moves it.
        open_help(&mut app, &modal);
        app.update();
        for _ in 0..2 {
            press_key(&mut app, KeyCode::Tab);
            app.update();
        }
        assert_eq!(focused_entity(&app), Some(modal[2]));
        release_keys(&mut app);
        app.world_mut().resource_mut::<HudState>().help_page = 3;
        cues(&mut app);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::Escape);
            keys.press(KeyCode::Enter);
        }
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert_eq!(
            app.world().resource::<HudState>().help_page,
            3,
            "Close was pressed, not an arrow"
        );
        assert_eq!(
            cues(&mut app),
            vec![cancel],
            "the dismissal must stand aside for the press it can see"
        );
        release_keys(&mut app);
        close_help_layout(&mut app, &modal);
        app.update();

        // Escape and Enter together, ring on Right: the page turns and the
        // modal closes - two requests, both honoured, two different cues.
        open_help(&mut app, &modal);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[1]));
        release_keys(&mut app);
        app.world_mut().resource_mut::<HudState>().help_page = 3;
        cues(&mut app);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::Escape);
            keys.press(KeyCode::Enter);
        }
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert_eq!(app.world().resource::<HudState>().help_page, 4);
        assert_eq!(cues(&mut app), vec![cancel, confirm]);
    }

    /// During a Match a pad's East closes the Help modal as it does
    /// on Home, and touches nothing else: the Return Home confirmation
    /// stays closed, the input gate is re-armed so the default Teleport on
    /// East cannot fire on the release, and the same East on the idle Match
    /// screen still does nothing at all.
    #[test]
    fn east_closes_the_help_modal_during_a_match_and_re_arms_the_gate() {
        let (mut app, _entities) = focus_app(0);
        let modal = spawn_help_modal_buttons(&mut app);
        enter_match(&mut app);
        open_help(&mut app, &modal);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[0]));
        app.world_mut()
            .resource_mut::<crate::gameplay::InputGate>()
            .open();

        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::East);
        let pad = app.world_mut().spawn(gamepad).id();
        app.update();
        let hud = app.world().resource::<HudState>();
        assert!(!hud.help_open);
        assert!(!hud.return_home_open);
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        close_help_layout(&mut app, &modal);
        app.update();
        assert_eq!(
            focused_entity(&app),
            None,
            "nothing on the Match is left to focus"
        );

        // East again, modal closed: still not a back button here.
        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::East);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::East);
        }
        app.update();
        assert!(!app.world().resource::<HudState>().return_home_open);
    }

    /// F1 opens the Help modal on the two screens whose buttons it
    /// mirrors, doing exactly what those buttons do - the flag, page 1, an
    /// armed gate, a Confirm cue, and the ring seeded on Left and drawn,
    /// since a key asked for it - and closes it again doing exactly what
    /// Close does: the flag, an armed gate, a Cancel cue, and the ring
    /// handed back to the screen on Home and to nothing on the Match.
    #[test]
    fn f1_opens_and_closes_the_help_modal_on_home_and_the_match() {
        let (mut app, entities) = focus_app(4);
        let modal = spawn_help_modal_buttons(&mut app);
        app.update();
        let cues = |app: &mut App| app.world_mut().resource_mut::<AudioCueQueue>().drain();
        let cancel = crate::audio::AudioCue::Ui(UiCue::Cancel);
        let confirm = crate::audio::AudioCue::Ui(UiCue::Confirm);
        cues(&mut app);

        // Home, with no key touched yet: the ring is undrawn until F1.
        assert!(!app.world().resource::<InputFocusVisible>().0);
        press_key(&mut app, KeyCode::F1);
        app.update();
        {
            let hud = app.world().resource::<HudState>();
            assert!(hud.help_open);
            assert_eq!(hud.help_page, 0);
            assert!(!hud.return_home_open);
        }
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        assert_eq!(cues(&mut app), vec![confirm]);
        assert!(
            app.world().resource::<InputFocusVisible>().0,
            "a key opened the modal, so the ring is drawn"
        );
        // Headless, `just_pressed` never expires: release before the layout
        // frame, or that frame reads the same F1 again.
        release_keys(&mut app);
        show_help_layout(&mut app, &modal);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[0]));
        assert_eq!(app.world().get::<Outline>(modal[0]).unwrap().color, ORANGE);

        // Nothing in this fixture releases the gate the open armed, so open
        // it by hand: the assertion after the close is then Close's own.
        app.world_mut()
            .resource_mut::<crate::gameplay::InputGate>()
            .open();
        press_key(&mut app, KeyCode::F1);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        assert_eq!(cues(&mut app), vec![cancel]);
        release_keys(&mut app);
        close_help_layout(&mut app, &modal);
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(entities[0]),
            "closing hands the ring back to the screen"
        );

        // The Match: the same, over live play. Leaving Home despawns its
        // buttons in production; the fixture has to do it by hand.
        enter_match(&mut app);
        for entity in &entities {
            app.world_mut().entity_mut(*entity).despawn();
        }
        app.world_mut()
            .resource_mut::<crate::gameplay::InputGate>()
            .open();
        cues(&mut app);
        press_key(&mut app, KeyCode::F1);
        app.update();
        {
            let hud = app.world().resource::<HudState>();
            assert!(hud.help_open);
            assert!(!hud.return_home_open);
        }
        assert!(
            app.world().resource::<GameplayPause>().match_paused,
            "the modal must pause the match the same frame, like the pointer path"
        );
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        assert_eq!(cues(&mut app), vec![confirm]);
        release_keys(&mut app);
        show_help_layout(&mut app, &modal);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[0]));

        app.world_mut()
            .resource_mut::<crate::gameplay::InputGate>()
            .open();
        press_key(&mut app, KeyCode::F1);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert!(!app.world().resource::<GameplayPause>().match_paused);
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        assert_eq!(cues(&mut app), vec![cancel]);
        release_keys(&mut app);
        close_help_layout(&mut app, &modal);
        app.update();
        assert_eq!(
            focused_entity(&app),
            None,
            "nothing on the Match is left to focus"
        );
    }

    /// F1 opens the Help modal on the two screens that have a button for
    /// it and nowhere else: the predicate the system reads, pinned against
    /// every screen, and then the production graph on each screen without
    /// a button, where the modal exists all the same. Closing is not
    /// gated the same way - a help key is expected to close help wherever
    /// it is open - so on those same screens a modal that is somehow open
    /// still closes on F1.
    #[test]
    fn the_help_key_opens_the_modal_on_home_and_the_match_only() {
        assert_eq!(
            AppScreen::ALL
                .into_iter()
                .filter(|screen| help_key_screen(*screen))
                .collect::<Vec<_>>(),
            vec![AppScreen::Home, AppScreen::Match]
        );

        let (mut app, _entities) = focus_app(0);
        let _modal = spawn_help_modal_buttons(&mut app);
        for screen in AppScreen::ALL
            .into_iter()
            .filter(|screen| !help_key_screen(*screen))
        {
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(screen);
            app.update();
            press_key(&mut app, KeyCode::F1);
            app.update();
            assert!(!app.world().resource::<HudState>().help_open, "{screen:?}");
            assert!(
                app.world_mut()
                    .resource_mut::<AudioCueQueue>()
                    .drain()
                    .is_empty(),
                "{screen:?}"
            );
            release_keys(&mut app);

            app.world_mut().resource_mut::<HudState>().help_open = true;
            press_key(&mut app, KeyCode::F1);
            app.update();
            assert!(
                !app.world().resource::<HudState>().help_open,
                "F1 closes the modal on {screen:?} even though it cannot open it there"
            );
            assert_eq!(
                app.world_mut().resource_mut::<AudioCueQueue>().drain(),
                vec![crate::audio::AudioCue::Ui(UiCue::Cancel)],
                "{screen:?}"
            );
            release_keys(&mut app);
        }
    }

    /// One frame decides the modal once. F1 with the ring's Enter on Home's
    /// Help button, or on the modal's Close, opens or closes it once with
    /// the button's own cue; F1 with Escape while it is open closes it once
    /// with one cue; F1 with Escape while it is closed does nothing on Home
    /// and opens the Return Home confirmation on the Match - the back key's
    /// own effect, with F1 standing aside from it, as it does for as long
    /// as that confirmation is open and for the Escape or Start that
    /// cancels it. F1 with a
    /// pad's East while it is closed on the Match opens it and gates East,
    /// exactly as F1 with an arrow would: East is a gameplay input there,
    /// not a back button, until the modal is open.
    #[test]
    fn the_help_key_stands_aside_for_the_rings_press_and_for_the_match_exit() {
        let (mut app, entities) = focus_app(4);
        app.add_observer(queue_activation_cue);
        let modal = spawn_help_modal_buttons(&mut app);
        for entity in modal.iter().chain(entities.iter()).copied() {
            let action = *app.world().get::<HudAction>(entity).unwrap();
            app.world_mut()
                .entity_mut(entity)
                .insert(UiSound::new(hud_action_ui_cue(action)));
        }
        app.update();
        let cues = |app: &mut App| {
            let mut cues = app.world_mut().resource_mut::<AudioCueQueue>().drain();
            cues.sort_by_key(|cue| format!("{cue:?}"));
            cues
        };
        let cancel = crate::audio::AudioCue::Ui(UiCue::Cancel);
        let confirm = crate::audio::AudioCue::Ui(UiCue::Confirm);
        let press_together = |app: &mut App, keys: &[KeyCode]| {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            for key in keys {
                input.press(*key);
            }
        };

        // The ring onto Help: the first Tab reveals it on Play.
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));
        release_keys(&mut app);
        cues(&mut app);

        // F1 and Enter on Help: opened once, the button's cue.
        press_together(&mut app, &[KeyCode::F1, KeyCode::Enter]);
        app.update();
        assert!(app.world().resource::<HudState>().help_open);
        assert_eq!(cues(&mut app), vec![confirm]);
        release_keys(&mut app);
        show_help_layout(&mut app, &modal);
        app.update();

        // F1 and Enter on Close: closed once, the button's cue.
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[2]));
        release_keys(&mut app);
        cues(&mut app);
        press_together(&mut app, &[KeyCode::F1, KeyCode::Enter]);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert_eq!(cues(&mut app), vec![cancel]);
        release_keys(&mut app);
        close_help_layout(&mut app, &modal);
        app.update();

        // F1 and Escape with the modal open: closed once, one cue.
        press_key(&mut app, KeyCode::F1);
        app.update();
        release_keys(&mut app);
        show_help_layout(&mut app, &modal);
        app.update();
        cues(&mut app);
        press_together(&mut app, &[KeyCode::F1, KeyCode::Escape]);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert_eq!(cues(&mut app), vec![cancel]);
        release_keys(&mut app);
        close_help_layout(&mut app, &modal);
        app.update();

        // F1 and Escape with the modal closed on Home: the back key's own
        // effect stands alone, and here it has none.
        cues(&mut app);
        press_together(&mut app, &[KeyCode::F1, KeyCode::Escape]);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert!(cues(&mut app).is_empty());
        release_keys(&mut app);

        // The Match: F1 and Escape with the modal closed open the Return
        // Home confirmation, and F1 stands aside from it - now and for as
        // long as it is open.
        enter_match(&mut app);
        cues(&mut app);
        press_together(&mut app, &[KeyCode::F1, KeyCode::Escape]);
        app.update();
        {
            let hud = app.world().resource::<HudState>();
            assert!(hud.return_home_open);
            assert!(!hud.help_open);
        }
        assert_eq!(cues(&mut app), vec![confirm]);
        release_keys(&mut app);
        press_key(&mut app, KeyCode::F1);
        app.update();
        {
            let hud = app.world().resource::<HudState>();
            assert!(hud.return_home_open);
            assert!(!hud.help_open);
        }
        assert!(cues(&mut app).is_empty());
        release_keys(&mut app);

        // F1 and the Escape that cancels the confirmation, in one frame:
        // the cancel stands alone, and F1 does not open the modal from a
        // frame that began with the confirmation open.
        press_together(&mut app, &[KeyCode::F1, KeyCode::Escape]);
        app.update();
        {
            let hud = app.world().resource::<HudState>();
            assert!(!hud.return_home_open);
            assert!(!hud.help_open);
        }
        assert_eq!(cues(&mut app), vec![cancel]);
        release_keys(&mut app);

        // The same with a pad's Start doing the cancelling.
        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<HudState>().return_home_open);
        release_keys(&mut app);
        cues(&mut app);
        let mut pad = Gamepad::default();
        pad.digital_mut().press(GamepadButton::Start);
        let pad = app.world_mut().spawn(pad).id();
        press_key(&mut app, KeyCode::F1);
        app.update();
        {
            let hud = app.world().resource::<HudState>();
            assert!(!hud.return_home_open);
            assert!(!hud.help_open);
        }
        assert_eq!(cues(&mut app), vec![cancel]);
        release_keys(&mut app);
        app.world_mut()
            .resource_mut::<crate::gameplay::InputGate>()
            .open();
        cues(&mut app);

        // F1 and a pad's East with the modal closed on the Match: the modal
        // opens and the gate holds East, as it would an arrow.
        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::Start);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::East);
        }
        press_key(&mut app, KeyCode::F1);
        app.update();
        {
            let hud = app.world().resource::<HudState>();
            assert!(hud.help_open);
            assert!(!hud.return_home_open);
        }
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        assert_eq!(cues(&mut app), vec![confirm]);
        show_help_layout(&mut app, &modal);
        release_keys(&mut app);
        // Headless, the pad's `just_pressed` never expires either: clear it
        // before the layout frame, or that frame reads the same East as a
        // back press and closes what F1 just opened.
        app.world_mut()
            .get_mut::<Gamepad>(pad)
            .unwrap()
            .digital_mut()
            .clear();
        app.update();
        assert!(
            app.world().resource::<HudState>().help_open,
            "the East that arrived with F1 must not also close the modal"
        );

        // A fresh East, with the modal open, is back again.
        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::East);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::East);
        }
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert_eq!(cues(&mut app), vec![cancel]);
    }

    /// The other half of the ring-visibility rule
    /// (`a_pointer_opened_modal_on_the_match_arrives_with_the_ring_hidden`):
    /// a modal F1 opens during a Match arrives with its ring drawn on Left
    /// even after the mouse has moved, because a key asked for it - what
    /// Enter on Home's Help button gives, rather than what a click
    /// on the Match's Help button gives.
    #[test]
    fn an_f1_opened_modal_on_the_match_arrives_with_the_ring_drawn() {
        let (mut app, _entities) = focus_app(4);
        let modal = spawn_help_modal_buttons(&mut app);
        app.update();
        enter_match(&mut app);
        app.world_mut().write_message(CursorMoved {
            window: Entity::PLACEHOLDER,
            position: Vec2::new(500.0, 400.0),
            delta: None,
        });
        app.update();
        assert!(!app.world().resource::<InputFocusVisible>().0);

        press_key(&mut app, KeyCode::F1);
        app.update();
        assert!(app.world().resource::<HudState>().help_open);
        assert!(app.world().resource::<InputFocusVisible>().0);
        release_keys(&mut app);
        show_help_layout(&mut app, &modal);
        app.update();
        assert!(app.world().resource::<HudState>().help_open);
        assert_eq!(focused_entity(&app), Some(modal[0]));
        assert_eq!(
            app.world().get::<Outline>(modal[0]).unwrap().color,
            ORANGE,
            "an F1-opened modal arrives with its ring drawn"
        );
    }

    /// The Match screen has no ring of its own to take the ring back when
    /// the Help modal closes, and the modal must still reopen on
    /// Left rather than on whichever button was last focused - the same
    /// seed as on Home, by ownership rather than by another group's write.
    #[test]
    fn a_dialog_over_an_empty_screen_is_seeded_at_its_first_entry_each_time_too() {
        let (mut app, _entities) = focus_app(0);
        let modal = spawn_help_modal_buttons(&mut app);
        enter_match(&mut app);
        open_help(&mut app, &modal);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[2]));
        release_keys(&mut app);

        app.world_mut().resource_mut::<HudState>().help_open = false;
        close_help_layout(&mut app, &modal);
        app.update();
        assert_eq!(focused_entity(&app), None);

        open_help(&mut app, &modal);
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(modal[0]),
            "a reopened modal seeds on Left on the Match as it does on Home"
        );
    }

    /// The Help modal's three buttons as `spawn_help_modal` marks
    /// them - Left 0, Right 1, Close 2 in `Modal` - spawned into a
    /// `focus_app` with fabricated zero sizes, hidden as the modal is until
    /// it opens. Returns them in ring order.
    fn spawn_help_modal_buttons(app: &mut App) -> [Entity; 3] {
        [
            (HudAction::PreviousHelpPage, 0),
            (HudAction::NextHelpPage, 1),
            (HudAction::CloseHelp, 2),
        ]
        .map(|(action, order)| spawn_hidden_modal_button(app, action, order))
    }

    /// Opens the Help modal the way the pointer button does -
    /// `help_open` set in `Update` - and gives its buttons the size the
    /// layout pass would.
    fn open_help(app: &mut App, modal: &[Entity; 3]) {
        app.world_mut().resource_mut::<HudState>().help_open = true;
        show_help_layout(app, modal);
    }

    /// The layout half of opening: the modal's buttons take the size the
    /// layout pass would give them once `help_open` is set, by whichever
    /// path set it.
    fn show_help_layout(app: &mut App, modal: &[Entity; 3]) {
        for entity in modal {
            set_size(app, *entity, Vec2::new(90.0, 36.0));
        }
    }

    /// The layout half of closing: the modal's buttons go `Display::None`
    /// and so zero-size. `help_open` itself is cleared by whatever closed
    /// the modal.
    fn close_help_layout(app: &mut App, modal: &[Entity; 3]) {
        for entity in modal {
            set_size(app, *entity, Vec2::ZERO);
        }
    }

    /// Spawns a dialog button into a `focus_app`, in the modal group, with
    /// a fabricated zero size - hidden, as an overlay's Close is until the
    /// overlay opens.
    fn spawn_hidden_modal_button(app: &mut App, action: HudAction, order: u32) -> Entity {
        app.world_mut()
            .spawn((
                Button,
                action,
                Focusable {
                    group: FocusGroup::Modal,
                    order,
                },
                ComputedNode {
                    size: Vec2::ZERO,
                    ..Default::default()
                },
            ))
            .id()
    }

    fn set_size(app: &mut App, entity: Entity, size: Vec2) {
        app.world_mut()
            .get_mut::<ComputedNode>(entity)
            .unwrap()
            .size = size;
    }

    /// A dialog's buttons becoming visible take the ring on that frame with
    /// no other bookkeeping, seeded on the dialog's first entry; movement is
    /// confined to the dialog; and hiding them again returns the ring to
    /// the screen position it left - not to the screen's first control.
    #[test]
    fn an_appearing_dialog_takes_the_ring_and_closing_returns_it_where_it_was() {
        let (mut app, entities) = focus_app(4);
        let cancel = spawn_hidden_modal_button(&mut app, HudAction::CancelReturnHome, 0);
        let confirm = spawn_hidden_modal_button(&mut app, HudAction::ConfirmReturnHome, 1);
        app.update();
        for _ in 0..3 {
            press_key(&mut app, KeyCode::Tab);
            app.update();
        }
        assert_eq!(focused_entity(&app), Some(entities[2]));
        release_keys(&mut app);

        // The dialog opens: its buttons gain a size, and the ring moves to
        // its first entry without any key being pressed.
        set_size(&mut app, cancel, Vec2::new(120.0, 36.0));
        set_size(&mut app, confirm, Vec2::new(120.0, 36.0));
        app.update();
        assert_eq!(focused_entity(&app), Some(cancel));

        // Movement wraps within the dialog, never reaching the screen.
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(confirm));
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(cancel));
        release_keys(&mut app);

        // The dialog closes: the ring is back on the third screen button,
        // not the first.
        set_size(&mut app, cancel, Vec2::ZERO);
        set_size(&mut app, confirm, Vec2::ZERO);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[2]));
    }

    /// The modal slot is not kept between openings: a dialog left from its
    /// second entry reopens on its first.
    #[test]
    fn a_dialog_is_seeded_at_its_first_entry_each_time_it_opens() {
        let (mut app, _entities) = focus_app(2);
        let cancel = spawn_hidden_modal_button(&mut app, HudAction::CancelReturnHome, 0);
        let confirm = spawn_hidden_modal_button(&mut app, HudAction::ConfirmReturnHome, 1);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        release_keys(&mut app);

        set_size(&mut app, cancel, Vec2::new(120.0, 36.0));
        set_size(&mut app, confirm, Vec2::new(120.0, 36.0));
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(confirm));
        release_keys(&mut app);

        set_size(&mut app, cancel, Vec2::ZERO);
        set_size(&mut app, confirm, Vec2::ZERO);
        app.update();
        set_size(&mut app, cancel, Vec2::new(120.0, 36.0));
        set_size(&mut app, confirm, Vec2::new(120.0, 36.0));
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(cancel),
            "a reopened dialog must not remember its destructive entry"
        );
    }

    /// A raised `FocusYield` suppresses navigation and activation for one
    /// frame and is lowered by that read, so a screen that stops raising it
    /// gets its ring back the next frame; the cursor-move drain is its own
    /// system and runs on a yielded frame too, so a burst of moves cannot
    /// hide the ring a frame late.
    #[test]
    fn a_raised_yield_suppresses_one_frame_and_still_drains_cursor_moves() {
        let (mut app, entities) = focus_app(4);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[0]));
        assert!(app.world().resource::<InputFocusVisible>().0);

        // Yielded: Enter neither presses nor moves...
        app.world_mut().resource_mut::<FocusYield>().0 = true;
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));
        assert!(
            !app.world().resource::<FocusYield>().0,
            "the read lowers the yield"
        );
        // ...and the same press, no longer yielded, activates next frame.
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Match)
        ));
        *app.world_mut().resource_mut::<NextState<AppScreen>>() = NextState::Unchanged;
        release_keys(&mut app);

        // A yielded frame still drains a burst of cursor moves whole: the
        // ring hides on that frame and a motionless frame after it must
        // not hide it a second time (it would be visible again by then).
        app.world_mut().resource_mut::<FocusYield>().0 = true;
        for _ in 0..3 {
            app.world_mut().write_message(CursorMoved {
                window: Entity::PLACEHOLDER,
                position: Vec2::new(1.0, 1.0),
                delta: None,
            });
        }
        app.update();
        assert!(!app.world().resource::<InputFocusVisible>().0);
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert!(app.world().resource::<InputFocusVisible>().0);
        release_keys(&mut app);
        app.update();
        assert!(
            app.world().resource::<InputFocusVisible>().0,
            "a leftover cursor message from the yielded frame must not hide the ring later"
        );
    }

    /// A yield raised on the Match screen is nobody's to lower during live
    /// play - navigation does not run there - and is spent on the first
    /// frame a modal is open: that frame's navigation reads it, lowers it
    /// and does nothing else, whether or not a key was pressed, the
    /// one-frame price a yield costs anywhere; the next key is handled
    /// normally. No real screen raises one there; this pins the contract
    /// `FocusYield`'s docs state. (A yield raised *before* the transition
    /// frame is spent by the departing screen's navigation in that frame's
    /// `PreUpdate`, which runs before `StateTransition`, so it is raised
    /// here after the Match is entered.)
    #[test]
    fn a_yield_raised_into_the_match_is_spent_on_the_first_frame_a_modal_is_open() {
        let (mut app, _entities) = focus_app(0);
        let modal = spawn_help_modal_buttons(&mut app);
        enter_match(&mut app);
        app.world_mut().resource_mut::<FocusYield>().0 = true;
        app.update();
        app.update();
        assert!(
            app.world().resource::<FocusYield>().0,
            "nothing runs during live play to lower it"
        );

        open_help(&mut app, &modal);
        app.update();
        assert!(
            !app.world().resource::<FocusYield>().0,
            "the modal's first frame spends the yield"
        );
        assert!(!app.world().resource::<InputFocusVisible>().0);
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert!(
            app.world().resource::<InputFocusVisible>().0,
            "the next key is handled normally"
        );
    }

    /// Memory is kept per screen: a second screen with a ring seeds at its
    /// own first control rather than inheriting the first screen's
    /// position, moving on it leaves the first screen's slot untouched, and
    /// the Match round trip leaves it untouched too. The fixture's buttons
    /// carry no `DespawnOnExit`, so the same four stand in for each screen's
    /// own.
    #[test]
    fn focus_memory_is_kept_per_screen() {
        let (mut app, entities) = focus_app(4);
        app.update();
        for _ in 0..4 {
            press_key(&mut app, KeyCode::Tab);
            app.update();
        }
        assert_eq!(focused_entity(&app), Some(entities[3]));
        release_keys(&mut app);

        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ReplayLibrary);
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(entities[0]),
            "a screen never visited seeds at its first control"
        );
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));
        release_keys(&mut app);

        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Home);
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(entities[3]),
            "Home's slot must not be overwritten by another screen's ring"
        );

        enter_match(&mut app);
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Home);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[3]));
    }

    /// A control carrying `bevy_ui`'s `InteractionDisabled` is not in the
    /// ring - the rule `spawn_menu_button` applies at spawn, here for a
    /// control disabled while on screen - and rejoins when it is enabled.
    #[test]
    fn a_disabled_control_leaves_the_ring_and_rejoins_when_enabled() {
        let (mut app, entities) = focus_app(3);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));
        release_keys(&mut app);

        app.world_mut()
            .entity_mut(entities[1])
            .insert(InteractionDisabled);
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(entities[0]),
            "focus clamps to the nearest lower enabled control"
        );
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(
            focused_entity(&app),
            Some(entities[2]),
            "movement skips the disabled control"
        );
        release_keys(&mut app);

        // Re-enabled, it is collected on the next frame's refresh (the ring
        // is always one layout behind the keys, by design) and movement
        // reaches it again.
        app.world_mut()
            .entity_mut(entities[1])
            .remove::<InteractionDisabled>();
        app.update();
        press_key(&mut app, KeyCode::ArrowUp);
        app.update();
        assert_eq!(focused_entity(&app), Some(entities[1]));
    }

    /// `InputFocusVisible` is one global flag: a ring revealed on Home is
    /// still "visible" on entering the Match, where nothing draws it. The
    /// pointer's motion during live play lowers it all the same, so a modal
    /// the pointer then opens arrives with its ring hidden, and the first
    /// navigation key reveals it - exactly as on any other screen. Without
    /// the ungated drain the modal would open with an outline already drawn
    /// on Left for a player who had not touched a key since the title screen.
    #[test]
    fn a_pointer_opened_modal_on_the_match_arrives_with_the_ring_hidden() {
        let (mut app, _entities) = focus_app(4);
        let modal = spawn_help_modal_buttons(&mut app);
        app.update();
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert!(app.world().resource::<InputFocusVisible>().0);
        release_keys(&mut app);
        enter_match(&mut app);
        assert!(
            app.world().resource::<InputFocusVisible>().0,
            "the flag carries over into the Match"
        );

        // Live play: the mouse moves, and nothing else happens.
        app.world_mut().write_message(CursorMoved {
            window: Entity::PLACEHOLDER,
            position: Vec2::new(500.0, 400.0),
            delta: None,
        });
        app.update();
        assert!(!app.world().resource::<InputFocusVisible>().0);

        // The pointer opens the Help modal: focus seeds on Left, undrawn.
        open_help(&mut app, &modal);
        app.update();
        assert_eq!(focused_entity(&app), Some(modal[0]));
        assert_eq!(
            app.world().get::<Outline>(modal[0]).unwrap().color,
            Color::NONE,
            "a pointer-opened modal must not arrive with a ring drawn"
        );
        press_key(&mut app, KeyCode::Tab);
        app.update();
        assert_eq!(
            app.world().get::<Outline>(modal[0]).unwrap().color,
            ORANGE,
            "the first navigation key reveals it"
        );
    }

    /// Puts a `focus_app` onto the Match screen, where nothing of the
    /// screen's own is focusable and only the exit keys are alive until a
    /// modal opens.
    fn enter_match(app: &mut App) {
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::Match);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::Match
        );
    }

    /// The Match screen's keyboard exit, through the real production graph:
    /// Escape opens the Return Home confirmation (arming the gate and
    /// pausing gameplay the same frame, exactly as the pointer button
    /// does), Escape again cancels it, Enter confirms it, and a same-frame
    /// Escape+Enter resolves to the non-destructive side.
    #[test]
    fn escape_walks_the_match_back_stack_and_enter_confirms() {
        let (mut app, _entities) = focus_app(0);
        enter_match(&mut app);

        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<HudState>().return_home_open);
        assert!(
            app.world()
                .resource::<crate::gameplay::InputGate>()
                .any_gated()
        );
        assert!(
            app.world().resource::<GameplayPause>().match_paused,
            "the confirmation must pause the match the same frame, like the pointer path"
        );

        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<HudState>().return_home_open);

        // Back and confirm in one frame: back wins, because the
        // destructive read of the pair should lose ties.
        press_key(&mut app, KeyCode::Escape);
        app.update();
        release_keys(&mut app);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::Escape);
            keys.press(KeyCode::Enter);
        }
        app.update();
        assert!(!app.world().resource::<HudState>().return_home_open);
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));
        release_keys(&mut app);

        // Enter alone does nothing while no confirmation is open...
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));

        // ...and confirms once it is. The write happens in `PreUpdate`, so
        // the transition applies inside the same frame.
        press_key(&mut app, KeyCode::Escape);
        app.update();
        press_key(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::Home
        );
    }

    /// The pad's Match exit is Start (structurally unbindable, like
    /// Escape), never East: East is a bindable input and the default
    /// Teleport to Marker, so a back on East would fire mid-match on every
    /// teleport. South confirms only while the confirmation is open.
    #[test]
    fn start_walks_the_match_back_stack_and_east_never_does() {
        let (mut app, _entities) = focus_app(0);
        enter_match(&mut app);

        // East - a teleport, under default bindings - must not touch the
        // confirmation.
        let mut east_pad = Gamepad::default();
        east_pad.digital_mut().press(GamepadButton::East);
        let east = app.world_mut().spawn(east_pad).id();
        app.update();
        assert!(!app.world().resource::<HudState>().return_home_open);
        app.world_mut().entity_mut(east).despawn();

        let mut pad = Gamepad::default();
        pad.digital_mut().press(GamepadButton::Start);
        let pad = app.world_mut().spawn(pad).id();
        app.update();
        assert!(app.world().resource::<HudState>().return_home_open);

        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::Start);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::South);
        }
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::Home
        );
    }

    /// The help modal is the top layer of the Match back stack: Escape (or
    /// Start) closes it first and only a later press asks about returning.
    #[test]
    fn the_match_help_modal_closes_before_the_return_confirmation_opens() {
        let (mut app, _entities) = focus_app(0);
        enter_match(&mut app);
        app.world_mut().resource_mut::<HudState>().help_open = true;

        press_key(&mut app, KeyCode::Escape);
        app.update();
        let hud = app.world().resource::<HudState>();
        assert!(!hud.help_open);
        assert!(!hud.return_home_open);

        press_key(&mut app, KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<HudState>().return_home_open);
    }
}
