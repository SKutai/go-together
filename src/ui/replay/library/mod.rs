//! The replay library screen: a searchable, sortable, paginated list of saved
//! matches with rename and delete dialogs.
//!
//! # Responsibility
//!
//! [`ReplayLibraryUiState`] holds only view state - search text, sort, page and
//! which dialog is open. The replay data itself lives in
//! [`ReplayLibrary`], which owns persistence.
//!
//! This module owns that state and the systems that move it: the plugin, the
//! [`LibraryAction`] a widget carries, and the click, keystroke and scroll
//! handlers that read them. [`layout`] spawns the furniture those systems then
//! ask to be rebuilt, and [`modals`] the two dialogs over it. The two texts
//! that describe a replay to the player - [`replay_summary_text`] and
//! [`replay_match_id_text`] - stay here rather than in [`layout`] because the
//! Replay Viewer's tests format catalog entries with them too;
//! [`replay_summary_text`]'s own docs say why that fixes the placement.
//!
//! # Respawn model
//!
//! The list is not diffed. Any change that affects what is visible sets
//! `dirty`, and `refresh_library_screen` despawns and respawns the contents at
//! the end of the frame. That keeps every spawn path identical, which matters
//! because rows, dialogs and error text share layout helpers.
//!
//! # The window's size
//!
//! How many cards a page holds, in how many columns, and whether the header
//! stands on one row or two are [`geometry`]'s, for the window's size and the
//! notes over the list. A change of the window's size is just another change
//! that affects what is visible: [`place_library_for_window`], first in the
//! screen's chain, marks the list dirty when the window's size changes what
//! the screen is built from ([`geometry::LibraryBuild`]), and it is rebuilt
//! at the end of that frame. The screen takes its width from the window by
//! flex, so a change of width alone rebuilds nothing.
//!
//! The list has no fixed pages. `scroll_offset` is the index of the first
//! card shown, and Up, Down and the wheel each move it one card, or in two
//! columns one row of two - so a card keeps its column as the list moves
//! under it, as a grid does when scrolled, rather than every card stepping
//! one place along. A change of size changes how many cards follow the
//! first: the first card stays first wherever the list has room after it,
//! except in two cases. Where a larger page would run past the list's end,
//! the first card moves back as far as a page needs; and in two columns a
//! page starts at a row's first card, so where one column at an odd place
//! becomes two it moves back one. Either move keeps the ring on the Replay
//! it was on (see [`ring`]), and entering the screen makes the same moves
//! for a window resized while another screen was shown.
//!
//! # Keyboard and controller
//!
//! Every button here is in the focus ring; [`ring`] says where each sits,
//! why the paging band is numbered before the cards it is drawn after, how
//! the two dialogs seed, and when the ring yields to the search field and
//! the rename dialog. The rebuild above is the case the core's
//! `(group, order)` key exists for, and [`ring`]'s first test proves the
//! ring survives it. Escape, a pad's East and Start step back one layer at
//! a time ([`handle_library_back`]); Enter and Escape both leave the search
//! field. One residual is accepted and recorded in [`ring`]: after a
//! confirmed delete the ring lands on a Delete button.
//!
//! # Errors
//!
//! There is no "saved!" narration; only problems are shown. A load warning
//! (unreadable or corrupt files were skipped) is shown above the list, while
//! everything a rename can fail on - a keystroke the label cannot hold, a
//! label the rule rejects, a write that does not land - stays inside the
//! dialog that caused it so the player can correct the input in place. The
//! refused keystroke is reported as it is typed rather than on submit: a
//! character that vanishes silently reads as a dead key, not as a refusal.
//! The search field has no error line, so a keystroke it cannot take is
//! simply swallowed.

mod geometry;
mod layout;
mod modals;
mod ring;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use super::REPLAY_FIELD_SEPARATOR;
use super::viewer::ReplayViewerState;
use crate::app::visuals::{WindowSize, format_elapsed_time};
use crate::app::{AppScreen, despawn_screen_entities_now};
use crate::audio::{AudioCueQueue, UiAudioSet, UiCue, UiSound};
use crate::platform::ReplayStorageWarning;
use crate::presentation::{button_background, danger_button_background};
use crate::replay::{PlaybackController, validate_label};
use crate::replay_library::{
    ReplayCatalogEntry, ReplayLibrary, SortField, SortOrder, filter_and_sort_replays,
    format_local_timestamp,
};
use crate::ui::button::{Activated, PointerHighlight, UiButtonPlugin};
use crate::ui::focus::UiFocusState;
use crate::ui::text_input::{
    REJECTED_CHARACTER_MESSAGE, TextEditor, TextInputOutcome, apply_editor_input,
};
use geometry::{LibraryBuild, LibraryLayout, library_layout};
use layout::{note_lines, spawn_library_contents};

/// How the Library is laid out for the window's size, with the notes the
/// library has to show above the list.
fn page_layout(window: &WindowSize, library: &ReplayLibrary) -> LibraryLayout {
    library_layout(window.layout(), note_lines(library))
}

/// Registers the library screen's lifecycle and its ordered update pipeline.
///
/// The pipeline is window -> intent -> state -> style -> respawn -> yield, so
/// a change of size, a click, a keystroke and a scroll all settle before the
/// list is rebuilt once at the end of the frame, and the focus yield is
/// raised last, after the state it reads has stopped changing. The window
/// comes first so that every handler after it pages by the page the frame
/// will be rebuilt with.
pub(super) struct ReplayLibraryPlugin;

impl Plugin for ReplayLibraryPlugin {
    fn build(&self, app: &mut App) {
        UiButtonPlugin::require(app);
        app.init_resource::<ReplayLibraryUiState>()
            .add_systems(
                OnEnter(AppScreen::ReplayLibrary),
                (prepare_library_screen, spawn_library_screen).chain(),
            )
            .add_systems(
                Update,
                (
                    place_library_for_window.run_if(resource_changed::<WindowSize>),
                    handle_library_back,
                    handle_library_buttons,
                    capture_library_text,
                    scroll_library,
                    update_library_button_styles,
                    refresh_library_screen,
                    ring::yield_focus_while_editing,
                )
                    .chain()
                    .after(UiAudioSet)
                    .run_if(in_state(AppScreen::ReplayLibrary)),
            );
    }
}

#[derive(Resource, Default)]
/// View state of the library screen: search, sort, page and open dialog.
///
/// `dirty` requests a respawn of the list contents at the end of the frame,
/// so every change goes through the same single spawn path.
///
/// Unlike the Viewer's `ReplayViewerState`, this resource is never cleared:
/// `prepare_library_screen` closes any open dialog, drops search focus and
/// clamps the scroll offset on entry, but the search text, sort field and
/// sort order survive a trip to the Replay Viewer or to Home.
pub(super) struct ReplayLibraryUiState {
    search: TextEditor,
    search_focused: bool,
    sort_field: SortField,
    sort_order: SortOrder,
    scroll_offset: usize,
    dialog: Option<LibraryDialog>,
    dirty: bool,
    /// What the screen was last built from, so that a frame whose window's
    /// size changes nothing about it rebuilds nothing.
    built_with: Option<LibraryBuild>,
    /// Set by [`handle_library_buttons`] whenever it applies a press, and
    /// taken by [`capture_library_text`], whose rising-edge guard must
    /// re-arm on *any* applied action - not only on a change of capture
    /// target - or a press that leaves the rename dialog open would let its
    /// own activating Enter through as a submit.
    rearm_text_capture: bool,
}

impl ReplayLibraryUiState {
    /// Which text field is reading the keys this frame: the rename dialog's
    /// editor while that dialog is open, the search field while it is
    /// focused and no dialog covers it, and nothing under the delete dialog,
    /// which has no field. That last arm is what keeps this and
    /// [`Self::owns_raw_input`] one predicate rather than two that could
    /// disagree - the same early return Settings' numeric capture makes
    /// under its reset confirmation.
    fn text_target(&self) -> Option<LibraryTextTarget> {
        match &self.dialog {
            Some(LibraryDialog::Rename { .. }) => Some(LibraryTextTarget::Rename),
            Some(LibraryDialog::Delete { .. }) => None,
            None if self.search_focused => Some(LibraryTextTarget::Search),
            None => None,
        }
    }

    /// Whether one of this screen's text fields is reading the raw keys this
    /// frame, which is when the focus ring yields (see [`ring`]). Mirrors
    /// `ControlMappingState::owns_raw_input`: true while the rename dialog
    /// is open or the search field is focused, false under the delete
    /// dialog, whose two buttons the ring must keep moving between and
    /// pressing.
    fn owns_raw_input(&self) -> bool {
        self.text_target().is_some()
    }
}

/// The modal currently over the list, if any. Each carries its own error so a
/// failed rename or delete is reported where it happened.
enum LibraryDialog {
    Rename {
        id: String,
        editor: TextEditor,
        error: String,
    },
    Delete {
        id: String,
        error: String,
    },
}

#[derive(Component, Clone)]
enum LibraryAction {
    Home,
    FocusSearch,
    ClearSearch,
    CycleSortField,
    CycleSortOrder,
    ScrollUp,
    ScrollDown,
    Play(String),
    OpenRename(String),
    OpenDelete(String),
    ClearRename,
    ConfirmRename,
    ConfirmDelete,
    CancelDialog,
}

impl LibraryAction {
    const fn is_danger(&self) -> bool {
        matches!(self, Self::OpenDelete(_) | Self::ConfirmDelete)
    }

    const fn sound(&self) -> UiCue {
        match self {
            Self::Home
            | Self::ClearSearch
            | Self::ScrollUp
            | Self::ClearRename
            | Self::CancelDialog => UiCue::Cancel,
            Self::ConfirmDelete => UiCue::Destructive,
            Self::FocusSearch
            | Self::CycleSortField
            | Self::CycleSortOrder
            | Self::OpenRename(_)
            | Self::OpenDelete(_) => UiCue::Neutral,
            Self::ScrollDown | Self::Play(_) | Self::ConfirmRename => UiCue::Confirm,
        }
    }
}

/// Formats one catalog entry's result line.
///
/// In production only `layout.rs` spawns text from this, so it looks like it
/// belongs there. It stays in this module because the Replay Viewer's tests
/// format catalog entries with it too: reaching it inside a private sibling
/// module would have to be spelled `pub(in crate::ui::replay)`, while
/// `pub(super)` on an item declared here says the same thing in the
/// vocabulary the rest of the crate uses. Move it only if you are prepared to
/// change how the viewer's tests reach it.
pub(super) fn replay_summary_text(replay: &ReplayCatalogEntry) -> String {
    let round_count = replay.round_count;
    let round_label = if round_count == 1 { "round" } else { "rounds" };
    let round_summary = if replay.opening_completed {
        format!("Round 0 + {round_count} ordinary {round_label}")
    } else {
        format!("{round_count} {round_label}")
    };
    format!(
        "{}{REPLAY_FIELD_SEPARATOR}{}-{}{REPLAY_FIELD_SEPARATOR}{}{REPLAY_FIELD_SEPARATOR}{}{REPLAY_FIELD_SEPARATOR}{round_summary}",
        replay.winner().label(),
        replay.score.black,
        replay.score.white,
        format_local_timestamp(replay.started_at_ms),
        format_elapsed_time(replay.duration_seconds),
    )
}

/// Formats a Replay's id for the card's detail line. Lives here rather than
/// in `layout.rs` for the same reason as [`replay_summary_text`].
pub(super) fn replay_match_id_text(id: &str) -> String {
    format!("Match ID: {id}")
}

/// Readies the screen on entry: the search field unfocused, no dialog, and
/// the page's first card where the layout lets it start, the ring's
/// remembered place moved with the cards ([`clamp_and_follow`]) - since the
/// window may have changed size while another screen was shown.
fn prepare_library_screen(
    mut ui: ResMut<ReplayLibraryUiState>,
    mut focus: ResMut<UiFocusState>,
    library: Res<ReplayLibrary>,
    window: Res<WindowSize>,
) {
    ui.search_focused = false;
    ui.dialog = None;
    clamp_and_follow(
        &mut ui,
        &mut focus,
        &library,
        &page_layout(&window, &library),
    );
}

fn spawn_library_screen(
    mut commands: Commands,
    mut ui: ResMut<ReplayLibraryUiState>,
    library: Res<ReplayLibrary>,
    window: Res<WindowSize>,
) {
    let layout = page_layout(&window, &library);
    spawn_library_contents(&mut commands, &ui, &library, &layout);
    ui.built_with = Some(layout.build());
    ui.dirty = false;
}

/// Rebuilds the card list in place after a search, sort, rename, or delete,
/// or a change of the window's size.
///
/// This does not change `AppScreen`, so the built-in on-exit despawn never
/// fires for it - [`despawn_screen_entities_now`] is the one place in the
/// crate that still needs a manual, immediate despawn.
fn refresh_library_screen(
    mut commands: Commands,
    mut ui: ResMut<ReplayLibraryUiState>,
    library: Res<ReplayLibrary>,
    window: Res<WindowSize>,
    entities: Query<(Entity, &DespawnOnExit<AppScreen>)>,
) {
    if !ui.dirty {
        return;
    }
    let layout = page_layout(&window, &library);
    despawn_screen_entities_now(&mut commands, &entities, AppScreen::ReplayLibrary);
    clamp_scroll(&mut ui, &library, &layout);
    spawn_library_contents(&mut commands, &ui, &library, &layout);
    ui.built_with = Some(layout.build());
    ui.dirty = false;
}

/// Marks the list dirty on a frame the window's size changes what the
/// screen is built from - how many cards a page holds, in how many columns,
/// the header or the padding ([`LibraryBuild`]) - so that it is rebuilt for
/// the new size at the end of the frame. A change of width alone rebuilds
/// nothing: the screen takes its width from the window by flex.
///
/// The page's first card moves to where the new layout lets it start, and
/// the ring's remembered place with the cards ([`clamp_and_follow`]), so the
/// ring stays on the Replay it was on rather than on the place on the page.
/// Where a smaller page would leave the ring's card off it, the first card
/// moves on just far enough to keep that card shown, and the ring stays on
/// its button (see [`ring`]).
fn place_library_for_window(
    window: Res<WindowSize>,
    library: Res<ReplayLibrary>,
    mut ui: ResMut<ReplayLibraryUiState>,
    mut focus: ResMut<UiFocusState>,
) {
    let layout = page_layout(&window, &library);
    if ui.built_with == Some(layout.build()) {
        return;
    }
    clamp_and_follow(&mut ui, &mut focus, &library, &layout);
    ui.dirty = true;
}

/// Moves the page's first card to where `layout` lets a page start
/// ([`LibraryLayout::first_card`]), and the ring's remembered place on this
/// screen with the cards, so that the ring stays on the Replay it was on.
/// The first card moves back in two cases: where a page from it would run
/// past the list's end, and in two columns to the first card of its row, as
/// when one column at an odd place becomes two; every card on the page then
/// moves down by as many places. It moves on in one: where the ring is on a
/// card that a smaller page would leave off its end, just far enough for
/// that card to be the page's last - its row the page's last, in two
/// columns - and every card moves up. With the ring on the toolbar or the
/// paging row, or not yet anywhere on this screen, only the first two
/// apply. On a change of the window's size and on entry alike.
fn clamp_and_follow(
    ui: &mut ReplayLibraryUiState,
    focus: &mut UiFocusState,
    library: &ReplayLibrary,
    layout: &LibraryLayout,
) {
    let count = filtered_count(library, ui);
    let cards = layout.cards_per_page();
    let mut first = layout.first_card(ui.scroll_offset, count);
    if let Some(place) = ring::remembered_card(focus) {
        let card = ui.scroll_offset + place;
        if card >= first + cards {
            // A page that ends with the ring's card; `first_card` starts it
            // at a row's first card, which in two columns makes the card's
            // row the page's last.
            first = layout.first_card(card + layout.columns - cards, count);
        }
    }
    if first != ui.scroll_offset {
        ring::follow_the_cards(focus, ui.scroll_offset, first);
        ui.scroll_offset = first;
    }
}

/// Every button on the screen, with whether it currently carries
/// [`InteractionDisabled`] so the marker is written only on change - it is
/// an archetype move.
type LibraryButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        PointerHighlight,
        &'static LibraryAction,
        &'static mut BackgroundColor,
        &'static mut UiSound,
        Has<InteractionDisabled>,
    ),
>;

fn update_library_button_styles(
    mut commands: Commands,
    ui: Res<ReplayLibraryUiState>,
    library: Res<ReplayLibrary>,
    window: Res<WindowSize>,
    mut buttons: LibraryButtons,
) {
    let layout = page_layout(&window, &library);
    for (entity, highlight, action, mut background, mut sound, was_disabled) in &mut buttons {
        let disabled = library_action_disabled(action, &ui, &library, &layout);
        // A disabled control is not in the focus ring: the core filters
        // `bevy_ui`'s `InteractionDisabled`, the rule the Replay Viewer's
        // transport and Settings' numeric fields follow. On this screen the
        // marker is computed at spawn, in `layout`'s button spawner, and
        // that is the copy that matters: this system runs *before*
        // `refresh_library_screen` in the chain, so on a dirty frame it
        // styles the dying entities, and every change that can disable a
        // paging button also marks the screen dirty. Reconciling here is
        // defence for a change that one day does not.
        if disabled != was_disabled {
            if disabled {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
        background.0 = if disabled {
            crate::presentation::BUTTON_DISABLED
        } else if action.is_danger() {
            danger_button_background(highlight.get())
        } else {
            button_background(highlight.get(), false, false)
        };
        sound.set_enabled(!disabled);
    }
}

fn handle_library_buttons(
    mut activated: Activated<LibraryAction>,
    mut ui: ResMut<ReplayLibraryUiState>,
    mut viewer: ResMut<ReplayViewerState>,
    mut library: ResMut<ReplayLibrary>,
    window: Res<WindowSize>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut diagnostics: ResMut<crate::diagnostics::Diagnostics>,
) {
    for action in activated.read() {
        let layout = page_layout(&window, &library);
        if library_action_disabled(action, &ui, &library, &layout) {
            continue;
        }
        // Any applied press re-arms the text capture's rising-edge guard:
        // the key press that activated this action must never also land in
        // whatever editor the action leaves (or puts) on screen.
        ui.rearm_text_capture = true;
        match action {
            LibraryAction::Home => next_screen.set(AppScreen::Home),
            LibraryAction::FocusSearch => {
                ui.search_focused = true;
                ui.search.move_end();
                ui.dialog = None;
                ui.dirty = true;
            }
            LibraryAction::ClearSearch => {
                ui.search.clear();
                ui.scroll_offset = 0;
                ui.search_focused = true;
                ui.dirty = true;
            }
            LibraryAction::CycleSortField => {
                cycle_sort_field(&mut ui);
            }
            LibraryAction::CycleSortOrder => {
                cycle_sort_order(&mut ui);
            }
            LibraryAction::ScrollUp => scroll_up(&mut ui, &layout),
            LibraryAction::ScrollDown => {
                let count = filtered_count(&library, &ui);
                scroll_down(&mut ui, &layout, count);
            }

            LibraryAction::Play(id) => match library.load_full(id) {
                Ok(replay) => {
                    viewer.replay_id = Some(id.clone());
                    viewer.replay = Some(replay);
                    viewer.controller = PlaybackController::default();
                    viewer.key_open = false;
                    ui.search_focused = false;
                    next_screen.set(AppScreen::ReplayViewer);
                }
                Err(error) => {
                    let message = format!("Could not load replay: {error}");
                    crate::diagnostics::report_recoverable(
                        &mut diagnostics,
                        &message,
                        "Could not open this Replay.",
                    );
                    // One Replay the player asked for and did not get -
                    // the same shape a catalog load reports, so the
                    // screen's count includes it.
                    library
                        .warnings
                        .push(ReplayStorageWarning::SkippedReplay(message));
                    ui.dirty = true;
                }
            },
            LibraryAction::OpenRename(id) => {
                if let Some(replay) = library.get(id) {
                    ui.dialog = Some(LibraryDialog::Rename {
                        id: id.clone(),
                        editor: TextEditor::new(replay.label.clone()),
                        error: String::new(),
                    });
                    ui.search_focused = false;
                    ui.dirty = true;
                }
            }
            LibraryAction::OpenDelete(id) => {
                ui.dialog = Some(LibraryDialog::Delete {
                    id: id.clone(),
                    error: String::new(),
                });
                ui.search_focused = false;
                ui.dirty = true;
            }
            LibraryAction::ClearRename => {
                if let Some(LibraryDialog::Rename { editor, error, .. }) = &mut ui.dialog {
                    editor.clear();
                    error.clear();
                    ui.dirty = true;
                }
            }
            LibraryAction::ConfirmRename => {
                confirm_rename(&mut ui, &mut library, &mut diagnostics);
            }
            LibraryAction::ConfirmDelete => confirm_delete(&mut ui, &mut library, &mut diagnostics),
            LibraryAction::CancelDialog => {
                ui.dialog = None;
                ui.dirty = true;
            }
        }
    }
}

fn library_action_disabled(
    action: &LibraryAction,
    ui: &ReplayLibraryUiState,
    library: &ReplayLibrary,
    layout: &LibraryLayout,
) -> bool {
    match action {
        LibraryAction::ScrollUp => ui.scroll_offset == 0,
        LibraryAction::ScrollDown => {
            ui.scroll_offset + layout.cards_per_page() >= filtered_count(library, ui)
        }
        _ => false,
    }
}

/// Moves the list one step towards its start: one card, or in two columns
/// one row of two.
fn scroll_up(ui: &mut ReplayLibraryUiState, layout: &LibraryLayout) {
    ui.scroll_offset = ui.scroll_offset.saturating_sub(layout.columns);
    ui.dirty = true;
}

/// Moves the list one step towards its end, while a card is still left
/// after the page.
fn scroll_down(ui: &mut ReplayLibraryUiState, layout: &LibraryLayout, count: usize) {
    if ui.scroll_offset + layout.cards_per_page() < count {
        ui.scroll_offset += layout.columns;
        ui.dirty = true;
    }
}

fn cycle_sort_field(ui: &mut ReplayLibraryUiState) {
    ui.sort_field = ui.sort_field.next();
    ui.scroll_offset = 0;
    ui.dirty = true;
}

fn cycle_sort_order(ui: &mut ReplayLibraryUiState) {
    ui.sort_order = ui.sort_order.next();
    ui.scroll_offset = 0;
    ui.dirty = true;
}

fn confirm_rename(
    ui: &mut ReplayLibraryUiState,
    library: &mut ReplayLibrary,
    diagnostics: &mut crate::diagnostics::Diagnostics,
) {
    let Some(LibraryDialog::Rename { id, editor, .. }) = &ui.dialog else {
        return;
    };
    let id = id.clone();
    let buffer = editor.text.clone();
    if let Err(error) = validate_label(&buffer) {
        if let Some(LibraryDialog::Rename { error: shown, .. }) = &mut ui.dialog {
            *shown = error.to_string();
        }
        ui.dirty = true;
        return;
    }
    match library.rename(&id, buffer) {
        Ok(()) => ui.dialog = None,
        Err(error) => {
            // Scope, not secrecy, is what separates these two messages.
            // This dialog is open only because the player just attempted
            // this rename, so naming the cause - an unwritable path, an OS
            // error - is actionable for them on a local desktop game, and
            // the dialog deliberately keeps the detailed message.
            // `Diagnostics` gets the generic one because it is not scoped
            // to an action the player just took: it is a shared log any
            // screen may surface later, out of context, where a path or OS
            // string would be internal detail rather than something to act
            // on. `docs/SECURITY.md` covers why a networked host would have
            // to re-review even the dialog's text.
            let detailed = format!("Could not rename replay: {error}");
            crate::diagnostics::report_recoverable(
                diagnostics,
                &detailed,
                "Could not rename this Replay.",
            );
            if let Some(LibraryDialog::Rename { error: shown, .. }) = &mut ui.dialog {
                *shown = detailed;
            }
        }
    }
    ui.dirty = true;
}

fn confirm_delete(
    ui: &mut ReplayLibraryUiState,
    library: &mut ReplayLibrary,
    diagnostics: &mut crate::diagnostics::Diagnostics,
) {
    let Some(LibraryDialog::Delete { id, .. }) = &ui.dialog else {
        return;
    };
    let id = id.clone();
    match library.delete(&id) {
        Ok(()) => ui.dialog = None,
        Err(error) => {
            // Same split as [`confirm_rename`]: the dialog the player just
            // opened may name the cause; the unscoped `Diagnostics` log
            // stays generic.
            let detailed = format!("Could not delete replay: {error}");
            crate::diagnostics::report_recoverable(
                diagnostics,
                &detailed,
                "Could not delete this Replay.",
            );
            if let Some(LibraryDialog::Delete { error: shown, .. }) = &mut ui.dialog {
                *shown = detailed;
            }
        }
    }
    ui.dirty = true;
}

/// Escape, or a pad's East or Start button, steps back one layer: it
/// closes the open dialog, then drops search focus, then returns to Home -
/// the same layering `handle_mapping_escape` gives the Settings screen,
/// with the same cue every step. Start is the pad's back on every screen;
/// East is safe as back here too for the same reason the arrow keys are
/// safe as focus movement on Home: this is a menu screen, and no gameplay
/// or capture system reads a pad on it.
///
/// Runs *first* in the chain, the shape the Viewer's keyboard handler has,
/// so a click landing in the same frame as a back press cannot make the
/// pair pop two layers or strand state: back reads the layer stack as the
/// frame began, and the click's own handler then runs against whatever back
/// left (closing an already-closed dialog is a no-op; a same-frame `Play`
/// still wins the `NextState` write, keeping the loaded Replay and the
/// screen together). Running before [`capture_library_text`] also means the
/// capture sees an editor this press closed and skips it, so an Escape that
/// cancels the rename dialog is not also fed to its editor, and the message
/// expires unread.
fn handle_library_back(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut ui: ResMut<ReplayLibraryUiState>,
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
    if ui.dialog.is_some() {
        ui.dialog = None;
        ui.dirty = true;
    } else if ui.search_focused {
        ui.search_focused = false;
        ui.dirty = true;
    } else {
        next_screen.set(AppScreen::Home);
    }
    audio.ui(UiCue::Cancel);
}

/// Which of the two text editors [`capture_library_text`] is feeding, for
/// its rising-edge guard.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryTextTarget {
    Search,
    Rename,
}

fn capture_library_text(
    mut keyboard_events: MessageReader<KeyboardInput>,
    (keys, chord): (
        Res<ButtonInput<KeyCode>>,
        Option<Res<crate::app::window::FullscreenChord>>,
    ),
    mut ui: ResMut<ReplayLibraryUiState>,
    mut library: ResMut<ReplayLibrary>,
    mut audio: ResMut<AudioCueQueue>,
    mut diagnostics: ResMut<crate::diagnostics::Diagnostics>,
    mut active: Local<Option<LibraryTextTarget>>,
) {
    let editing = ui.text_target();
    // The rising-edge guard `capture_controller_mapping` keeps: the frame an
    // editor gains the keyboard, the messages already in flight - including
    // the Enter press a keyboard-focus activation used to open it, which
    // would otherwise submit the rename dialog the instant it appeared - are
    // drained unread. Any applied button press re-arms too (the flag from
    // `handle_library_buttons`), because a press that leaves the same editor
    // open - reopening the rename dialog for the row already being renamed -
    // changes no target and would otherwise slip its own activating key
    // through. A pointer click sends no keyboard message; the one
    // pointer-visible shift is that a keystroke from the click's own frame,
    // or buffered unread just before it, no longer lands in a field it
    // predates.
    let rearm = std::mem::take(&mut ui.rearm_text_capture);
    let just_armed = editing.is_some() && (rearm || *active != editing);
    *active = editing;
    if editing.is_none() {
        return;
    }
    if just_armed {
        keyboard_events.clear();
        return;
    }
    let command_down = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    for event in keyboard_events.read() {
        // The F of Control-Command-F, on a Mac, is the fullscreen chord's,
        // and so are its repeats once Control and Command are let go
        // (`app::window`).
        if event.state != ButtonState::Pressed
            || chord
                .as_ref()
                .is_some_and(|chord| chord.takes(event.key_code, &keys))
        {
            continue;
        }
        let rename_outcome =
            if let Some(LibraryDialog::Rename { editor, error, .. }) = &mut ui.dialog {
                let outcome = apply_editor_input(editor, &event.logical_key, command_down);
                match outcome {
                    TextInputOutcome::Changed => error.clear(),
                    // The dialog's own error line is the only place a
                    // refused character can be reported, and it has to be
                    // set here rather than on submit: the keystroke is where
                    // the player learns the field did not take it.
                    TextInputOutcome::Rejected => *error = REJECTED_CHARACTER_MESSAGE.into(),
                    _ => {}
                }
                Some(outcome)
            } else {
                None
            };
        if let Some(outcome) = rename_outcome {
            if !outcome.consumed() {
                continue;
            }
            match outcome {
                TextInputOutcome::Ignored => unreachable!("ignored input is not consumed"),
                TextInputOutcome::Consumed
                | TextInputOutcome::Rejected
                | TextInputOutcome::Changed => {
                    ui.dirty = true;
                }
                TextInputOutcome::Submit => {
                    confirm_rename(&mut ui, &mut library, &mut diagnostics);
                    audio.ui(UiCue::Confirm);
                }
                TextInputOutcome::Cancel => {
                    ui.dialog = None;
                    ui.dirty = true;
                    audio.ui(UiCue::Cancel);
                }
            }
            continue;
        }
        if !ui.search_focused {
            break;
        }
        let outcome = apply_editor_input(&mut ui.search, &event.logical_key, command_down);
        match outcome {
            TextInputOutcome::Ignored => {}
            TextInputOutcome::Cancel => {
                ui.search_focused = false;
                ui.dirty = true;
                audio.ui(UiCue::Cancel);
            }
            TextInputOutcome::Changed => {
                ui.scroll_offset = 0;
                ui.dirty = true;
            }
            // Enter leaves the field. Filtering is live as the player
            // types, so there is nothing to commit and Enter is free to mean
            // "done" - and it gives a keyboard player an exit that is not
            // Escape, whose next press leaves the screen.
            TextInputOutcome::Submit => {
                ui.search_focused = false;
                ui.dirty = true;
                audio.ui(UiCue::Confirm);
            }
            // The search field has no error line of its own, so a refused
            // character is swallowed here exactly like a caret key.
            TextInputOutcome::Consumed | TextInputOutcome::Rejected => {
                ui.dirty = true;
            }
        }
    }
}

fn scroll_library(
    mut wheel_events: MessageReader<MouseWheel>,
    mut ui: ResMut<ReplayLibraryUiState>,
    library: Res<ReplayLibrary>,
    window: Res<WindowSize>,
) {
    if ui.dialog.is_some() {
        return;
    }
    let delta = wheel_events.read().map(|event| event.y).sum::<f32>();
    if delta == 0.0 {
        return;
    }
    let layout = page_layout(&window, &library);
    let count = filtered_count(&library, &ui);
    if delta > 0.0 {
        scroll_up(&mut ui, &layout);
    } else {
        scroll_down(&mut ui, &layout, count);
    }
    ui.dirty = true;
}

fn filtered_count(library: &ReplayLibrary, ui: &ReplayLibraryUiState) -> usize {
    filter_and_sort_replays(
        &library.entries,
        &ui.search.text,
        ui.sort_field,
        ui.sort_order,
    )
    .len()
}

fn clamp_scroll(ui: &mut ReplayLibraryUiState, library: &ReplayLibrary, layout: &LibraryLayout) {
    let count = filtered_count(library, ui);
    ui.scroll_offset = layout.first_card(ui.scroll_offset, count);
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fixed_metadata_replay;
    use super::super::viewer::ViewerAction;
    use super::*;
    use crate::architecture::sources_in;
    use crate::presentation::DANGER;
    use crate::replay::REPLAY_LABEL_RULE;
    use crate::replay_library::ReplayStore;
    use crate::rules::Score;
    use crate::ui::button::{UiButtonPlugin, trigger_activation};
    use bevy::input::keyboard::Key;

    /// Writes one logical key press and runs a frame.
    ///
    /// `crate::test_support::send_keyboard_event` cannot serve here: it always
    /// sends `Key::Character("a")`, and which character arrives is the whole
    /// point of a rejection test.
    fn send_logical_key(app: &mut App, logical_key: Key) {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    /// The rename dialog's error, as the player would read it, or `None` while
    /// the dialog is showing its guidance instead.
    ///
    /// Guidance and error are one text node - the label rule in muted text
    /// until something goes wrong, the message in `DANGER` once it has - so
    /// the colour is what separates them. A library with load warnings would
    /// also render a danger-coloured line above the list; the libraries these
    /// tests build have none.
    fn shown_error_line(app: &mut App) -> Option<String> {
        let mut texts = app.world_mut().query::<(&Text, &TextColor)>();
        texts
            .iter(app.world())
            .find(|(_, color)| color.0 == DANGER)
            .map(|(text, _)| text.as_str().to_owned())
    }

    fn rename_editor_text(app: &mut App) -> String {
        let ui = app.world().resource::<ReplayLibraryUiState>();
        let Some(LibraryDialog::Rename { editor, .. }) = &ui.dialog else {
            panic!("the rename dialog should still be open");
        };
        editor.text.clone()
    }

    /// The one mechanism in the crate that still despawns and respawns
    /// entities without an `AppScreen` transition: repeatedly marking the
    /// list dirty (as a search, sort, rename, or delete does) must rebuild
    /// exactly one card per Replay every time, never accumulating a copy of
    /// the previous rebuild's cards.
    #[test]
    fn repeated_dirty_rebuilds_never_accumulate_library_cards() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-dirty-rebuild-{}",
            uuid::Uuid::new_v4()
        ));
        let mut library = ReplayLibrary::from_store(ReplayStore::new(root));
        library
            .entries
            .push(ReplayCatalogEntry::from_replay(&fixed_metadata_replay()));

        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .add_systems(Startup, spawn_library_screen)
            .add_systems(Update, refresh_library_screen);
        app.update();

        let play_button_count = |app: &mut App| {
            app.world_mut()
                .query::<&LibraryAction>()
                .iter(app.world())
                .filter(|action| matches!(action, LibraryAction::Play(_)))
                .count()
        };
        assert_eq!(play_button_count(&mut app), 1);

        for _ in 0..4 {
            app.world_mut().resource_mut::<ReplayLibraryUiState>().dirty = true;
            app.update();
            assert_eq!(
                play_button_count(&mut app),
                1,
                "a dirty rebuild must not leave the previous rebuild's cards behind"
            );
        }
    }

    /// The frame-sharing defect the rising-edge guard exists for, on this
    /// capture: `handle_library_buttons` opens the rename dialog and
    /// `capture_library_text` reads the same frame's `KeyboardInput`
    /// messages one system later, so the Enter press that activated the
    /// Rename button (reachable once the focus core can press buttons)
    /// would arrive as `Submit` and confirm the rename the instant its
    /// dialog appeared. The arming frame drains those messages instead.
    #[test]
    fn the_key_press_that_opens_the_rename_dialog_never_submits_it() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-rename-arming-{}",
            uuid::Uuid::new_v4()
        ));
        let replay = crate::replay::test_support::sample_replay(1, 2, "Original Label");
        let store = ReplayStore::new(root.clone());
        store.save(&replay).unwrap();
        let library = ReplayLibrary::from_store(store);
        let id = library.entries[0].id.clone();

        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .init_resource::<ReplayViewerState>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (handle_library_buttons, capture_library_text).chain(),
            );
        let pressed = app
            .world_mut()
            .spawn(LibraryAction::OpenRename(id.clone()))
            .id();
        trigger_activation(&mut app, pressed);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();

        assert!(
            matches!(
                app.world().resource::<ReplayLibraryUiState>().dialog,
                Some(LibraryDialog::Rename { .. })
            ),
            "the dialog must survive the frame that opened it"
        );
        assert_eq!(rename_editor_text(&mut app), "Original Label");

        // From the next frame on the dialog behaves normally: keystrokes
        // land and Enter confirms.
        send_logical_key(&mut app, Key::Character("!".into()));
        assert_eq!(rename_editor_text(&mut app), "Original Label!");
        send_logical_key(&mut app, Key::Enter);
        assert!(
            app.world()
                .resource::<ReplayLibraryUiState>()
                .dialog
                .is_none(),
            "a fresh Enter still confirms the rename"
        );
        assert_eq!(
            app.world()
                .resource::<ReplayLibrary>()
                .get(&id)
                .unwrap()
                .label,
            "Original Label!"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Escape (or a pad's East or Start button) steps back one layer at a
    /// time: dialog, then search focus, then the screen itself - the same
    /// layering Settings' Escape has - and the capture system never also
    /// reads the press that a layer consumed.
    #[test]
    fn escape_east_or_start_steps_back_through_dialog_search_then_home() {
        let mut library = ReplayLibrary::from_store(ReplayStore::new(
            std::env::temp_dir().join(format!("go-together-library-back-{}", uuid::Uuid::new_v4())),
        ));
        library
            .entries
            .push(ReplayCatalogEntry::from_replay(&fixed_metadata_replay()));
        let id = library.entries[0].id.clone();

        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, (handle_library_back, capture_library_text).chain());
        {
            let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
            ui.dialog = Some(LibraryDialog::Rename {
                id,
                editor: TextEditor::new("Label".into()),
                error: String::new(),
            });
        }
        let press_escape = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Escape);
            keys.clear();
            keys.press(KeyCode::Escape);
        };

        press_escape(&mut app);
        app.update();
        let ui = app.world().resource::<ReplayLibraryUiState>();
        assert!(ui.dialog.is_none(), "the first back closes the dialog");
        assert!(ui.dirty);

        app.world_mut()
            .resource_mut::<ReplayLibraryUiState>()
            .search_focused = true;
        press_escape(&mut app);
        app.update();
        assert!(
            !app.world()
                .resource::<ReplayLibraryUiState>()
                .search_focused,
            "the next back drops search focus"
        );

        press_escape(&mut app);
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Home)
        ));

        // A pad's East button is the same back, one layer at a time.
        *app.world_mut().resource_mut::<NextState<AppScreen>>() = NextState::Unchanged;
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Escape);
            keys.clear();
        }
        app.world_mut()
            .resource_mut::<ReplayLibraryUiState>()
            .search_focused = true;
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::East);
        let pad = app.world_mut().spawn(gamepad).id();
        app.update();
        assert!(
            !app.world()
                .resource::<ReplayLibraryUiState>()
                .search_focused
        );
        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::East);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::East);
        }
        app.update();
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Pending(AppScreen::Home)
        ));

        // Start is the same back as East, layer by layer.
        *app.world_mut().resource_mut::<NextState<AppScreen>>() = NextState::Unchanged;
        app.world_mut()
            .resource_mut::<ReplayLibraryUiState>()
            .search_focused = true;
        {
            let mut pad_state = app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            pad_state.digital_mut().clear();
            pad_state.digital_mut().release(GamepadButton::East);
            pad_state.digital_mut().clear();
            pad_state.digital_mut().press(GamepadButton::Start);
        }
        app.update();
        assert!(
            !app.world()
                .resource::<ReplayLibraryUiState>()
                .search_focused
        );
        assert!(matches!(
            *app.world().resource::<NextState<AppScreen>>(),
            NextState::Unchanged
        ));
    }

    /// A click and a back press in the same frame must resolve to one
    /// dismissal, not two: back runs first in the chain, so the click's own
    /// handler then finds the layer already closed and closing it again is
    /// a no-op - the player is not ejected from the screen as well.
    #[test]
    fn a_same_frame_cancel_click_and_escape_pop_only_one_layer() {
        let mut library = ReplayLibrary::from_store(ReplayStore::new(std::env::temp_dir().join(
            format!("go-together-library-one-pop-{}", uuid::Uuid::new_v4()),
        )));
        library
            .entries
            .push(ReplayCatalogEntry::from_replay(&fixed_metadata_replay()));
        let id = library.entries[0].id.clone();

        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(library)
            .insert_resource(ReplayLibraryUiState {
                dialog: Some(LibraryDialog::Delete {
                    id,
                    error: String::new(),
                }),
                ..default()
            })
            .init_resource::<ReplayViewerState>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (
                    handle_library_back,
                    handle_library_buttons,
                    capture_library_text,
                )
                    .chain(),
            );
        let pressed = app.world_mut().spawn(LibraryAction::CancelDialog).id();
        trigger_activation(&mut app, pressed);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);

        app.update();

        assert!(
            app.world()
                .resource::<ReplayLibraryUiState>()
                .dialog
                .is_none()
        );
        assert!(
            matches!(
                *app.world().resource::<NextState<AppScreen>>(),
                NextState::Unchanged
            ),
            "one dismissal, not a dismissal plus an exit to Home"
        );
    }

    /// The rising-edge guard must re-arm on *any* applied press, not only
    /// on a change of capture target: reopening the rename dialog for the
    /// row already being renamed leaves the target unchanged, and without
    /// the re-arm the Enter that pressed the button would submit the
    /// reopened dialog the instant it appeared.
    #[test]
    fn a_press_that_keeps_the_same_editor_open_still_rearms_the_capture() {
        let root = std::env::temp_dir().join(format!(
            "go-together-library-rearm-{}",
            uuid::Uuid::new_v4()
        ));
        let replay = crate::replay::test_support::sample_replay(1, 2, "Original Label");
        let store = ReplayStore::new(root.clone());
        store.save(&replay).unwrap();
        let library = ReplayLibrary::from_store(store);
        let id = library.entries[0].id.clone();

        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .init_resource::<ReplayViewerState>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (handle_library_buttons, capture_library_text).chain(),
            );

        // Open the dialog and let the guard arm.
        let pressed = app
            .world_mut()
            .spawn(LibraryAction::OpenRename(id.clone()))
            .id();
        trigger_activation(&mut app, pressed);
        app.update();
        assert!(matches!(
            app.world().resource::<ReplayLibraryUiState>().dialog,
            Some(LibraryDialog::Rename { .. })
        ));

        // Reopen the same row's dialog: the target does not change, but the
        // Enter that activated the button arrives the same frame.
        let pressed = app
            .world_mut()
            .spawn(LibraryAction::OpenRename(id.clone()))
            .id();
        trigger_activation(&mut app, pressed);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(
            matches!(
                app.world().resource::<ReplayLibraryUiState>().dialog,
                Some(LibraryDialog::Rename { .. })
            ),
            "the reopening press must re-arm the guard, not submit through it"
        );

        // The next frame's Enter still submits normally.
        send_logical_key(&mut app, Key::Enter);
        assert!(
            app.world()
                .resource::<ReplayLibraryUiState>()
                .dialog
                .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A catalog entry can outlive the file it was built from - deleted or
    /// corrupted outside the application between launches. Opening it must
    /// report the failure and stay on the Library, never enter a broken
    /// Viewer with no Replay loaded.
    #[test]
    fn failed_lazy_load_reports_a_warning_and_never_enters_the_viewer() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-failed-open-{}",
            uuid::Uuid::new_v4()
        ));
        let replay = crate::replay::test_support::sample_replay(1, 2, "Failed Load");
        let store = ReplayStore::new(root.clone());
        store.save(&replay).unwrap();
        let library = ReplayLibrary::from_store(store);
        assert_eq!(library.entries.len(), 1);
        // Remove the underlying file after cataloging, so the catalog entry
        // is stale - exactly what a file deleted or corrupted outside the
        // application between launches would look like.
        std::fs::remove_file(root.join(format!("{}.json", replay.id))).unwrap();

        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .init_resource::<ReplayViewerState>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_library_buttons);
        let pressed = app
            .world_mut()
            .spawn(LibraryAction::Play(replay.id.clone()))
            .id();
        trigger_activation(&mut app, pressed);
        app.update();

        assert_eq!(
            *app.world().resource::<State<AppScreen>>().get(),
            AppScreen::Home
        );
        assert!(app.world().resource::<ReplayViewerState>().replay.is_none());
        assert!(
            app.world()
                .resource::<ReplayViewerState>()
                .replay_id
                .is_none()
        );
        assert_eq!(app.world().resource::<ReplayLibrary>().warnings.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Nothing in production reads `Diagnostics` back, so without this the
    /// three player-safe strings below are never exercised - which is
    /// exactly how a call site can quietly end up pushing its *technical*
    /// message instead. `PathLeakingReplayStorage` fails every operation
    /// with the shape a real storage failure has, so a leak here would show
    /// up rather than being masked by a tidy mock message.
    #[test]
    fn every_replay_library_diagnostic_is_a_player_safe_sentence() {
        use crate::replay_library::test_support::{
            LEAKED_REPLAY_STORAGE_ERROR, PathLeakingReplayStorage,
        };

        let mut library = ReplayLibrary::from_store(PathLeakingReplayStorage);
        library
            .entries
            .push(ReplayCatalogEntry::from_replay(&fixed_metadata_replay()));
        let id = library.entries[0].id.clone();

        // Open, through the real button-handling system.
        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .init_resource::<ReplayViewerState>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_systems(Update, handle_library_buttons);
        let pressed = app.world_mut().spawn(LibraryAction::Play(id.clone())).id();
        trigger_activation(&mut app, pressed);
        app.update();

        // Rename and delete, whose confirmations are plain functions.
        let mut diagnostics = crate::diagnostics::Diagnostics::default();
        let mut library = app.world_mut().remove_resource::<ReplayLibrary>().unwrap();
        let mut ui = ReplayLibraryUiState {
            dialog: Some(LibraryDialog::Rename {
                id: id.clone(),
                editor: TextEditor::new("Valid Label".into()),
                error: String::new(),
            }),
            ..default()
        };
        confirm_rename(&mut ui, &mut library, &mut diagnostics);
        ui.dialog = Some(LibraryDialog::Delete {
            id,
            error: String::new(),
        });
        confirm_delete(&mut ui, &mut library, &mut diagnostics);

        let opened = app
            .world()
            .resource::<crate::diagnostics::Diagnostics>()
            .active()
            .to_vec();
        let messages: Vec<String> = opened
            .iter()
            .chain(diagnostics.active())
            .map(|diagnostic| {
                assert_eq!(diagnostic.severity, crate::diagnostics::Severity::Warning);
                diagnostic.message.clone()
            })
            .collect();
        assert_eq!(
            messages,
            [
                "Could not open this Replay.",
                "Could not rename this Replay.",
                "Could not delete this Replay.",
            ]
        );
        for message in &messages {
            crate::diagnostics::assert_carries_no_internal_detail(message);
        }

        // The dialog deliberately keeps the detailed cause - and that cause
        // is exactly what the messages above must not be, which is also what
        // makes those assertions non-vacuous.
        let Some(LibraryDialog::Delete { error, .. }) = &ui.dialog else {
            panic!("a delete failure keeps its dialog open");
        };
        assert!(error.contains(LEAKED_REPLAY_STORAGE_ERROR));
        assert!(crate::diagnostics::carries_internal_detail(error));
        assert!(crate::diagnostics::carries_internal_detail(
            &library.warnings[0].to_string()
        ));
    }

    #[test]
    fn replay_card_metadata_uses_requested_order_and_match_start() {
        let mut replay = fixed_metadata_replay();
        replay.completed_at_ms = replay.started_at_ms + 86_400_000;
        replay.duration_seconds = 312.0;
        replay.score = Score {
            black: 42,
            white: 38,
        };
        replay.round_count = 27;
        let started = format_local_timestamp(replay.started_at_ms);
        let completed = format_local_timestamp(replay.completed_at_ms);

        assert_eq!(
            replay_summary_text(&ReplayCatalogEntry::from_replay(&replay)),
            format!("Black | 42-38 | {started} | 05:12 | 27 rounds")
        );
        assert_ne!(started, completed);
        assert!(
            !replay_summary_text(&ReplayCatalogEntry::from_replay(&replay)).contains(&completed)
        );
        assert_eq!(REPLAY_FIELD_SEPARATOR, " | ");
        for prefix in ["Winner:", "Started:", "Length:", "Score:", "Rounds:"] {
            assert!(
                !replay_summary_text(&ReplayCatalogEntry::from_replay(&replay)).contains(prefix)
            );
        }
    }

    #[test]
    fn replay_card_round_count_uses_singular_and_legacy_inference() {
        let mut replay = fixed_metadata_replay();
        replay.round_count = 1;
        assert!(
            replay_summary_text(&ReplayCatalogEntry::from_replay(&replay)).ends_with(" | 1 round")
        );

        replay.opening_completed = true;
        assert!(
            replay_summary_text(&ReplayCatalogEntry::from_replay(&replay))
                .ends_with(" | Round 0 + 1 ordinary round")
        );

        replay.version = 1;
        replay.opening_completed = false;
        replay.round_count = 0;
        replay.events.push(crate::replay::ReplayEvent {
            at_seconds: 0.5,
            kind: crate::replay::ReplayEventKind::RoundResolved {
                resolution: crate::rules::ResolutionKind::Sequential,
                captures: [0, 0],
            },
            frame: Default::default(),
        });
        assert!(
            replay_summary_text(&ReplayCatalogEntry::from_replay(&replay)).ends_with(" | 1 round")
        );
    }

    #[test]
    fn replay_card_second_line_contains_only_the_full_match_id() {
        let replay = fixed_metadata_replay();
        let id_line = replay_match_id_text(&replay.id);

        assert_eq!(id_line, "Match ID: 00000000-0000-4000-8000-000000000000");
        for excluded in ["Winner:", "Started:", "Length:", "Score:", "Rounds:"] {
            assert!(!id_line.contains(excluded));
        }
    }

    #[test]
    fn rename_guidance_is_short_and_consistent_with_validation_errors() {
        assert_eq!(
            REPLAY_LABEL_RULE,
            "Start with a letter. Use up to 60 characters."
        );
        assert_eq!(
            validate_label("1 invalid").unwrap_err().to_string(),
            REPLAY_LABEL_RULE
        );
        assert!(validate_label("A punctuation: !@#$%^&*()[]{}|~").is_ok());
    }

    #[test]
    fn replay_library_has_no_routine_success_narration() {
        // Both trees whole, walked from disk at test time rather than
        // listed: `replay` (the headless schema, compatibility,
        // validation, recording, and playback layer) and `replay_library`
        // (the client's catalog and storage-backed collection). The list
        // that used to be here is where this failure actually happened -
        // `catalog.rs` fell out of it when the catalog was extracted, and a
        // negative assertion cannot notice a file nobody listed. See
        // `crate::architecture`.
        let source = [sources_in("replay", 12), sources_in("replay_library", 3)].concat();

        for removed in [
            ["Replay ", "saved."].concat(),
            ["Replay ", "renamed."].concat(),
            ["Replay ", "deleted."].concat(),
        ] {
            assert!(!source.contains(&removed));
        }
    }

    #[test]
    fn replay_library_keeps_rename_validation_and_persistence_errors_in_modal() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-rename-errors-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&root, "not a directory").unwrap();
        let mut library = ReplayLibrary::from_store(ReplayStore::new(root.clone()));
        library
            .entries
            .push(ReplayCatalogEntry::from_replay(&fixed_metadata_replay()));
        let replay_id = library.entries[0].id.clone();
        let mut ui = ReplayLibraryUiState {
            dialog: Some(LibraryDialog::Rename {
                id: replay_id.clone(),
                editor: TextEditor::new("1 invalid".into()),
                error: String::new(),
            }),
            ..default()
        };

        confirm_rename(
            &mut ui,
            &mut library,
            &mut crate::diagnostics::Diagnostics::default(),
        );
        let Some(LibraryDialog::Rename { error, .. }) = &ui.dialog else {
            panic!("rename validation failure should keep its dialog open");
        };
        assert_eq!(error, REPLAY_LABEL_RULE);

        ui.dialog = Some(LibraryDialog::Rename {
            id: replay_id,
            editor: TextEditor::new("Valid Label".into()),
            error: String::new(),
        });
        confirm_rename(
            &mut ui,
            &mut library,
            &mut crate::diagnostics::Diagnostics::default(),
        );
        let Some(LibraryDialog::Rename { error, .. }) = ui.dialog else {
            panic!("rename persistence failure should keep its dialog open");
        };
        assert!(error.starts_with("Could not rename replay:"));

        std::fs::remove_file(root).unwrap();
    }

    #[test]
    fn rename_inserts_shifted_punctuation_and_colon_at_the_caret() {
        let mut editor = TextEditor::new("Replay".into());
        editor.move_home();
        editor.move_right();
        let punctuation = ":;!@#$%^&*()/\\\"'[]{}|~<>?+=`";

        let outcome = apply_editor_input(&mut editor, &Key::Character(punctuation.into()), false);

        assert_eq!(outcome, TextInputOutcome::Changed);
        assert!(outcome.consumed());
        assert_eq!(editor.text, format!("R{punctuation}eplay"));
        assert_eq!(editor.caret, 1 + punctuation.chars().count());
    }

    #[test]
    fn rename_rejects_non_printable_or_non_ascii_character_input() {
        for text in ["\n", "\t", "\u{7f}", "é"] {
            let mut editor = TextEditor::new("Replay".into());
            let outcome = apply_editor_input(&mut editor, &Key::Character(text.into()), false);

            assert_eq!(outcome, TextInputOutcome::Rejected);
            assert!(
                outcome.consumed(),
                "a refused character must not reach the screen underneath"
            );
            assert_eq!(editor.text, "Replay");
        }
    }

    /// The point of the rejection, asserted where the player would see it: a
    /// character the label cannot hold replaces the dialog's guidance line
    /// with an error, in the dialog's error colour, and the next accepted
    /// keystroke puts the guidance back. Driven through the real keystroke
    /// system and the real respawn, because the error only becomes visible if
    /// the rejected keystroke also marks the screen dirty.
    #[test]
    fn a_rejected_rename_keystroke_becomes_a_visible_error_a_valid_one_clears() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-rejected-rename-{}",
            uuid::Uuid::new_v4()
        ));
        let mut library = ReplayLibrary::from_store(ReplayStore::new(root));
        library
            .entries
            .push(ReplayCatalogEntry::from_replay(&fixed_metadata_replay()));
        let replay_id = library.entries[0].id.clone();

        let mut app = App::new();
        app.init_resource::<crate::app::visuals::WindowSize>();
        app.insert_resource(library)
            .init_resource::<ReplayLibraryUiState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(AudioCueQueue::default())
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (capture_library_text, refresh_library_screen).chain(),
            );
        {
            let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
            ui.dialog = Some(LibraryDialog::Rename {
                id: replay_id,
                editor: TextEditor::new("Cafe".into()),
                error: String::new(),
            });
            ui.dirty = true;
        }
        app.update();
        assert!(
            shown_error_line(&mut app).is_none(),
            "an untouched dialog shows its guidance, not an error"
        );

        // The accented letter in `Cafe rematch` typed as `Café`.
        send_logical_key(&mut app, Key::Character("\u{e9}".into()));

        assert_eq!(
            shown_error_line(&mut app),
            Some(REJECTED_CHARACTER_MESSAGE.into())
        );
        assert_eq!(rename_editor_text(&mut app), "Cafe");

        send_logical_key(&mut app, Key::Character("!".into()));

        assert!(shown_error_line(&mut app).is_none());
        assert_eq!(rename_editor_text(&mut app), "Cafe!");
    }

    #[test]
    fn rename_space_and_arrows_are_consumed_by_the_editor() {
        let mut editor = TextEditor::new("Replay".into());

        let space = apply_editor_input(&mut editor, &Key::Space, false);
        let left = apply_editor_input(&mut editor, &Key::ArrowLeft, false);
        let right = apply_editor_input(&mut editor, &Key::ArrowRight, false);

        assert_eq!(space, TextInputOutcome::Changed);
        assert!(space.consumed());
        assert_eq!(left, TextInputOutcome::Consumed);
        assert!(left.consumed());
        assert_eq!(right, TextInputOutcome::Consumed);
        assert!(right.consumed());
        assert_eq!(editor.text, "Replay ");
        assert_eq!(editor.caret, 7);
    }

    #[test]
    fn search_editor_supports_caret_editing_clear_and_live_filtering() {
        let mut editor = TextEditor::new("Apha".into());
        editor.move_home();
        assert_eq!(
            apply_editor_input(&mut editor, &Key::ArrowRight, false),
            TextInputOutcome::Consumed
        );
        assert_eq!(
            apply_editor_input(&mut editor, &Key::Character("l".into()), false),
            TextInputOutcome::Changed
        );
        assert_eq!(editor.text, "Alpha");
        assert_eq!(editor.display_text(), "Al|pha");

        assert_eq!(
            apply_editor_input(&mut editor, &Key::Space, false),
            TextInputOutcome::Changed
        );
        assert_eq!(
            apply_editor_input(&mut editor, &Key::Delete, false),
            TextInputOutcome::Changed
        );
        assert_eq!(editor.text, "Al ha");
        assert_eq!(
            apply_editor_input(&mut editor, &Key::Backspace, false),
            TextInputOutcome::Changed
        );
        assert_eq!(editor.text, "Alha");
        apply_editor_input(&mut editor, &Key::Character("p".into()), false);
        assert_eq!(editor.text, "Alpha");

        assert_eq!(
            apply_editor_input(&mut editor, &Key::Home, false),
            TextInputOutcome::Consumed
        );
        for _ in 0..3 {
            apply_editor_input(&mut editor, &Key::ArrowLeft, false);
        }
        assert_eq!(editor.caret, 0);
        assert_eq!(
            apply_editor_input(&mut editor, &Key::End, false),
            TextInputOutcome::Consumed
        );
        for _ in 0..3 {
            apply_editor_input(&mut editor, &Key::ArrowRight, false);
        }
        assert_eq!(editor.caret, 5);

        let mut alpha = fixed_metadata_replay();
        alpha.label = "Alpha Match".into();
        let mut beta = fixed_metadata_replay();
        beta.id = "00000000-0000-4000-8000-000000000001".into();
        beta.label = "Beta Match".into();
        let alpha = ReplayCatalogEntry::from_replay(&alpha);
        let beta = ReplayCatalogEntry::from_replay(&beta);
        assert_eq!(
            filter_and_sort_replays(
                &[alpha.clone(), beta],
                &editor.text,
                SortField::Name,
                SortOrder::Ascending
            ),
            vec![&alpha]
        );

        editor.clear();
        assert!(editor.text.is_empty());
        assert_eq!(editor.caret, 0);
        assert_eq!(editor.display_text(), "|");
    }

    #[test]
    fn replay_library_defaults_to_newest_completed_replay_first() {
        let ui = ReplayLibraryUiState::default();

        assert_eq!(ui.sort_field, SortField::CreatedDate);
        assert_eq!(ui.sort_order, SortOrder::Descending);
        assert_eq!(ui.scroll_offset, 0);
    }

    #[test]
    fn replay_actions_have_directional_single_note_classification() {
        assert_eq!(LibraryAction::Play("id".into()).sound(), UiCue::Confirm);
        assert_eq!(LibraryAction::CancelDialog.sound(), UiCue::Cancel);
        assert_eq!(LibraryAction::ConfirmDelete.sound(), UiCue::Destructive);
        assert_eq!(LibraryAction::CycleSortField.sound(), UiCue::Neutral);
        assert_eq!(ViewerAction::Rewind.sound(), UiCue::Cancel);
        assert_eq!(ViewerAction::JumpForward.sound(), UiCue::Confirm);
        assert_eq!(ViewerAction::MatchSettings.sound(), UiCue::Neutral);
        assert_eq!(ViewerAction::CloseMatchSettings.sound(), UiCue::Cancel);
        assert_eq!(ViewerAction::CloseKey.sound(), UiCue::Cancel);
    }

    #[test]
    fn sort_controls_cycle_two_options_and_reset_pagination() {
        let mut ui = ReplayLibraryUiState {
            scroll_offset: 7,
            ..default()
        };

        cycle_sort_order(&mut ui);
        assert_eq!(ui.sort_order, SortOrder::Ascending);
        assert_eq!(ui.scroll_offset, 0);
        assert!(ui.dirty);

        ui.scroll_offset = 3;
        cycle_sort_order(&mut ui);
        assert_eq!(ui.sort_order, SortOrder::Descending);
        assert_eq!(ui.scroll_offset, 0);

        ui.scroll_offset = 2;
        cycle_sort_field(&mut ui);
        assert_eq!(ui.sort_field, SortField::Name);
        assert_eq!(ui.scroll_offset, 0);
    }

    #[test]
    fn search_filtering_clamps_pagination_without_changing_sort_defaults() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-pagination-{}",
            uuid::Uuid::new_v4()
        ));
        let mut library = ReplayLibrary::from_store(ReplayStore::new(root));
        for index in 0..6 {
            let mut replay = fixed_metadata_replay();
            replay.id = format!("00000000-0000-4000-8000-{index:012}");
            replay.label = format!("Match {index}");
            replay.completed_at_ms += index;
            library
                .entries
                .push(ReplayCatalogEntry::from_replay(&replay));
        }
        let mut ui = ReplayLibraryUiState {
            search: TextEditor::new("Match 5".into()),
            scroll_offset: 5,
            ..default()
        };

        clamp_scroll(&mut ui, &library, &library_layout(WindowSize::OPENING.0, 0));

        assert_eq!(filtered_count(&library, &ui), 1);
        assert_eq!(ui.scroll_offset, 0);
        assert_eq!(ui.sort_field, SortField::CreatedDate);
        assert_eq!(ui.sort_order, SortOrder::Descending);
    }
}
