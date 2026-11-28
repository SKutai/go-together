//! Spawns the library screen: the toolbar, the current page of replay cards
//! and the pagination row, and over them a rename or delete dialog, which
//! [`super::modals`] spawns.
//!
//! Nothing here keeps state. Every helper takes the current
//! [`ReplayLibraryUiState`] and [`ReplayLibrary`] and builds from scratch,
//! because the screen is rebuilt wholesale on every change - see the respawn
//! model in [`super`]. That is also why the toolbar's section order and the
//! sizes only spawning reads live here: they describe furniture.
//!
//! How the screen stands in the window is [`super::geometry`]'s - how many
//! cards a page holds and in how many columns, whether the header takes one
//! row or two, and the width everything is held to - and every spawner reads
//! it from the [`LibraryLayout`] it is handed, because the update systems
//! page and scroll by the same layout.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Button;

use super::geometry::{
    CARD_GAP, CARD_HEIGHT, CLEAR_CONTROL_WIDTH, HOME_BUTTON_WIDTH, HOME_TITLE_GAP, LIBRARY_TITLE,
    LIBRARY_TITLE_SIZE, LibraryLayout, NOTE_SIZE, PAGINATION_ROW_HEIGHT, ROW_GAP,
    SORT_FIELD_CONTROL_MIN_WIDTH, SORT_ORDER_CONTROL_WIDTH, TITLE_TOOLBAR_GAP, TOOLBAR_COLUMN_GAP,
    TOOLBAR_HEIGHT, TOOLBAR_MIN_SPACER_WIDTH,
};
use super::modals::spawn_library_dialog;
use super::ring::{
    HOME_FOCUS_ORDER, PAGINATION_FOCUS_ORDER, RingOrder, TOOLBAR_FOCUS_ORDER, card_focus_order,
};
use super::{
    LibraryAction, ReplayLibraryUiState, library_action_disabled, replay_match_id_text,
    replay_summary_text,
};
use crate::app::AppScreen;
use crate::app::visuals::stone_geometry;
use crate::audio::UiSound;
use crate::presentation::{
    APP_BACKGROUND, BLACK_STONE, BLUE_BORDER, BOARD_WOOD, BUTTON_BACKGROUND, BUTTON_DISABLED,
    CARD_BORDER, DANGER, DEEP_SURFACE_BACKGROUND, GRAY_STONE, Highlight, STONE_RING,
    SURFACE_BACKGROUND, TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY, TEXT_SUBTLE, THUMBNAIL_EMPTY,
    THUMBNAIL_GRID, WHITE_STONE, danger_button_background,
};
use crate::replay_library::{ReplayCatalogEntry, ReplayLibrary, filter_and_sort_replays};
use crate::rules::{BOARD_SIZE, Cell, Point};
use crate::ui::focus::Focusable;
use crate::ui::line_breaks;
use crate::ui::text_metrics::CUT_MARK;
/// What the list says when the Library holds no Replays at all.
const NO_REPLAYS_MESSAGE: &str =
    "No completed match replays yet.\nFinish and score a match to save it automatically.";
/// What the list says when the search matches none of the Replays there are.
const NO_SEARCH_MATCH_MESSAGE: &str = "No replay labels match this search.";
const PAGE_INDICATOR_WIDTH: f32 = 130.0;
const PAGINATION_BUTTON_HEIGHT: f32 = 34.0;
const PAGE_INDICATOR_TOP_PADDING: f32 = 4.0;

const LIBRARY_TOOLBAR_ORDER: [LibraryToolbarSection; 3] = [
    LibraryToolbarSection::Search,
    LibraryToolbarSection::FlexibleSpacer,
    LibraryToolbarSection::Sort,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LibraryToolbarSection {
    Search,
    FlexibleSpacer,
    Sort,
}

/// The state the screen is drawn from, carried together because every
/// spawner reads them: `library` for what to show, `ui` for how, and
/// `layout` for where - and the button spawner reads all three again to
/// decide, at spawn, whether a control is disabled.
#[derive(Clone, Copy)]
pub(super) struct LibraryView<'a> {
    pub(super) ui: &'a ReplayLibraryUiState,
    pub(super) library: &'a ReplayLibrary,
    pub(super) layout: &'a LibraryLayout,
}

/// How many lines of notes stand above the list: the retention note and
/// the line counting Replays that could not be read, each when shown.
pub(super) fn note_lines(library: &ReplayLibrary) -> usize {
    usize::from(replay_retention_text(library).is_some())
        + usize::from(replay_library_warning_text(library).is_some())
}

pub(super) fn spawn_library_contents(
    commands: &mut Commands,
    ui: &ReplayLibraryUiState,
    library: &ReplayLibrary,
    layout: &LibraryLayout,
) {
    let view = LibraryView {
        ui,
        library,
        layout,
    };
    let filtered = filter_and_sort_replays(
        &library.entries,
        &ui.search.text,
        ui.sort_field,
        ui.sort_order,
    );
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(layout.padding.y)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(APP_BACKGROUND),
            DespawnOnExit(AppScreen::ReplayLibrary),
        ))
        .with_children(|screen| {
            // Everything but a dialog, held to the list's widest and
            // centred: the window's width reaches it by flex, so a change of
            // width alone rebuilds nothing (see `LibraryBuild`).
            screen
                .spawn(Node {
                    width: percent(100),
                    max_width: px(layout.max_width()),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(ROW_GAP),
                    ..default()
                })
                .with_children(|root| spawn_page(root, view, &filtered));
            if let Some(dialog) = &ui.dialog {
                spawn_library_dialog(screen, view, dialog);
            }
        });
}

/// The page under the screen's padding: the header, the toolbar, the notes,
/// the list and the paging row.
fn spawn_page(
    root: &mut ChildSpawnerCommands,
    view: LibraryView,
    filtered: &[&ReplayCatalogEntry],
) {
    let LibraryView {
        ui,
        library,
        layout,
    } = view;
    if layout.one_row_header {
        // Home, the title and the toolbar on one row.
        root.spawn(Node {
            width: percent(100),
            height: px(TOOLBAR_HEIGHT),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|header| {
            spawn_home_button(header, view);
            header.spawn((
                library_title(),
                Node {
                    margin: UiRect::new(px(HOME_TITLE_GAP), px(TITLE_TOOLBAR_GAP), px(0), px(0)),
                    ..default()
                },
            ));
            // The rest of the row, at least as wide as its controls:
            // the one header row is used only where they fit.
            header
                .spawn(Node {
                    width: Val::Auto,
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    ..library_toolbar_node()
                })
                .with_children(|controls| spawn_toolbar_sections(controls, view));
        });
    } else {
        root.spawn(Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|header| {
            spawn_home_button(header, view);
            header.spawn(library_title());
            header.spawn(Node {
                width: px(HOME_BUTTON_WIDTH),
                ..default()
            });
        });

        root.spawn(library_toolbar_node())
            .with_children(|controls| spawn_toolbar_sections(controls, view));
    }

    if let Some(note) = replay_retention_text(library) {
        root.spawn((
            Text::new(note),
            TextFont {
                font_size: FontSize::Px(NOTE_SIZE),
                ..default()
            },
            TextColor(TEXT_MUTED),
            Node {
                width: percent(100),
                ..default()
            },
        ));
    }

    if let Some(warning) = replay_library_warning_text(library) {
        root.spawn((
            Text::new(warning),
            TextFont {
                font_size: FontSize::Px(NOTE_SIZE),
                ..default()
            },
            TextColor(DANGER),
            Node {
                width: percent(100),
                ..default()
            },
        ));
    }

    root.spawn((list_node(layout), BackgroundColor(DEEP_SURFACE_BACKGROUND)))
        .with_children(|list| {
            if filtered.is_empty() {
                spawn_empty_list_message(
                    list,
                    if library.entries.is_empty() {
                        NO_REPLAYS_MESSAGE
                    } else {
                        NO_SEARCH_MATCH_MESSAGE
                    },
                );
            } else {
                for (place, replay) in filtered
                    .iter()
                    .skip(ui.scroll_offset)
                    .take(layout.cards_per_page())
                    .enumerate()
                {
                    spawn_replay_row(list, view, replay, place);
                }
            }
        });

    root.spawn(Node {
        width: percent(100),
        height: px(PAGINATION_ROW_HEIGHT),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        column_gap: px(12),
        ..default()
    })
    .with_children(|scroll| {
        let mut ring = RingOrder::starting_at(PAGINATION_FOCUS_ORDER);
        scroll
            .spawn(pagination_side_node(JustifyContent::FlexEnd))
            .with_children(|left| {
                spawn_library_button(
                    left,
                    view,
                    LibraryAction::ScrollUp,
                    "Up",
                    70.0,
                    PAGINATION_BUTTON_HEIGHT,
                    ring.next(),
                );
            });
        scroll.spawn(pagination_indicator_node()).with_child((
            Text::new(page_indicator_text(
                ui.scroll_offset,
                filtered.len(),
                layout.cards_per_page(),
            )),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
            TextLayout::justify(Justify::Center),
        ));
        scroll
            .spawn(pagination_side_node(JustifyContent::FlexStart))
            .with_children(|right| {
                spawn_library_button(
                    right,
                    view,
                    LibraryAction::ScrollDown,
                    "Down",
                    70.0,
                    PAGINATION_BUTTON_HEIGHT,
                    ring.next(),
                );
            });
    });
}

/// The Home button, the screen's first control.
fn spawn_home_button(parent: &mut ChildSpawnerCommands, view: LibraryView) {
    spawn_library_button(
        parent,
        view,
        LibraryAction::Home,
        "Home",
        HOME_BUTTON_WIDTH,
        38.0,
        RingOrder::starting_at(HOME_FOCUS_ORDER).next(),
    );
}

/// The screen's title.
fn library_title() -> impl Bundle {
    (
        Text::new(LIBRARY_TITLE),
        TextFont {
            font_size: FontSize::Px(LIBRARY_TITLE_SIZE),
            ..default()
        },
        TextColor(TEXT_HEADING),
    )
}

/// The toolbar's controls in their sections: the search field and its
/// Clear, the flexible spacer, and the two sort controls.
fn spawn_toolbar_sections(controls: &mut ChildSpawnerCommands, view: LibraryView) {
    let ui = view.ui;
    let search_label = if ui.search.text.is_empty() {
        SearchLabel {
            text: if ui.search_focused {
                "Search label: |".into()
            } else {
                "Search replay labels...".into()
            },
            cut: false,
        }
    } else {
        const PREFIX: &str = "Search: ";
        let label = format!(
            "{PREFIX}{}",
            if ui.search_focused {
                ui.search.display_text()
            } else {
                ui.search.text.clone()
            }
        );
        let caret = if ui.search_focused {
            PREFIX.len() + ui.search.caret
        } else {
            label.chars().count()
        };
        shown_search_label(&label, caret, search_field_chars(view.layout.search_width))
    };
    let search_cut = search_label.cut;
    let search_label = search_label.text;
    let mut ring = RingOrder::starting_at(TOOLBAR_FOCUS_ORDER);
    for section in LIBRARY_TOOLBAR_ORDER {
        match section {
            LibraryToolbarSection::Search => {
                controls
                    .spawn(library_toolbar_group_node())
                    .with_children(|search| {
                        if search_cut {
                            // One line, never wrapped: the text is cut to
                            // fit it, and the field clips it besides.
                            spawn_library_button_with_layout(
                                search,
                                view,
                                LibraryAction::FocusSearch,
                                &search_label,
                                Node {
                                    overflow: Overflow::clip(),
                                    ..library_button_node(view.layout.search_width, 40.0)
                                },
                                TextLayout::new(Justify::Center, LineBreak::NoWrap),
                                ring.next(),
                            );
                        } else {
                            spawn_library_button(
                                search,
                                view,
                                LibraryAction::FocusSearch,
                                &search_label,
                                view.layout.search_width,
                                40.0,
                                ring.next(),
                            );
                        }
                        spawn_library_button(
                            search,
                            view,
                            LibraryAction::ClearSearch,
                            "Clear",
                            CLEAR_CONTROL_WIDTH,
                            40.0,
                            ring.next(),
                        );
                    });
            }
            LibraryToolbarSection::FlexibleSpacer => {
                controls.spawn(library_toolbar_spacer_node());
            }
            LibraryToolbarSection::Sort => {
                controls
                    .spawn(library_toolbar_group_node())
                    .with_children(|sort| {
                        spawn_sort_field_button(
                            sort,
                            view,
                            LibraryAction::CycleSortField,
                            &format!("Sort: {}", ui.sort_field.label()),
                            ring.next(),
                        );
                        spawn_sort_order_button(
                            sort,
                            view,
                            LibraryAction::CycleSortOrder,
                            ui.sort_order.label(),
                            ring.next(),
                        );
                    });
            }
        }
    }
}

/// The list the cards stand in: one column of cards the list's width, or
/// rows of two side by side, a grid of two equal tracks whose width the
/// list takes from the window by flex - so a card's width is never an exact
/// number the page would have to be rebuilt for, and two cards and the gap
/// between them can never come out wider than the list and wrap.
fn list_node(layout: &LibraryLayout) -> Node {
    let two_columns = layout.columns > 1;
    Node {
        width: percent(100),
        flex_grow: 1.0,
        display: if two_columns {
            Display::Grid
        } else {
            Display::Flex
        },
        flex_direction: FlexDirection::Column,
        grid_template_columns: if two_columns {
            RepeatedGridTrack::flex(2, 1.0)
        } else {
            Vec::new()
        },
        align_content: AlignContent::FlexStart,
        row_gap: px(CARD_GAP),
        column_gap: px(CARD_GAP),
        overflow: Overflow::clip(),
        ..default()
    }
}

/// Spawns the message an empty list shows instead of its cards, standing
/// where the first card would: centred across the list, and centred down a
/// slot one card tall at its top.
///
/// It stands there rather than in the middle of the list because that is
/// where the player is already looking. Every change to the search returns
/// the list to its first page, so whatever a search last showed began in the
/// first card's place, and the message that nothing matches appears in that
/// place, just under the field that caused it. With nothing listed, both
/// paging buttons are disabled and leave the ring, so every control the ring
/// can reach is in the header and toolbar above the list; a message in the
/// list's middle would sit some 340 px below all of them, which a magnifier
/// following focus would keep out of view at all but low zoom.
///
/// The slot is a plain node and the message its `Text` child because a
/// `Text` node cannot move its own glyphs down its box: `justify_content` and
/// `align_items` place a node's children, and glyphs are not children.
/// [`crate::ui::countdown_card`] explains it; until 2026-10-06 this message
/// was such a node, asking to be centred in the whole list and drawn at its
/// top edge. `TextLayout::justify` is what centres the shorter line of the
/// two-line message under the longer.
fn spawn_empty_list_message(list: &mut ChildSpawnerCommands, message: &str) {
    list.spawn(Node {
        width: percent(100),
        height: px(CARD_HEIGHT),
        // Across every column, where the list is a grid of two.
        grid_column: GridPlacement::start_end(1, -1),
        flex_shrink: 0.0,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    })
    .with_child((
        Text::new(message),
        TextFont {
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
        TextLayout::justify(Justify::Center),
    ));
}

/// Which cards the page shows, counted by the page as it is: `1-3 of 13`.
fn page_indicator_text(scroll_offset: usize, filtered_count: usize, page: usize) -> String {
    if filtered_count == 0 {
        "0 of 0".into()
    } else {
        format!(
            "{}-{} of {}",
            scroll_offset + 1,
            (scroll_offset + page).min(filtered_count),
            filtered_count
        )
    }
}

fn pagination_side_node(justify_content: JustifyContent) -> Node {
    Node {
        flex_basis: px(0),
        flex_grow: 1.0,
        justify_content,
        align_items: AlignItems::Center,
        column_gap: px(12),
        ..default()
    }
}

fn pagination_indicator_node() -> Node {
    Node {
        width: px(PAGE_INDICATOR_WIDTH),
        height: px(PAGINATION_BUTTON_HEIGHT),
        padding: UiRect::top(px(PAGE_INDICATOR_TOP_PADDING)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// Spawns one card. `place` is its place on the page, which decides its
/// three buttons' places in the ring - see [`card_focus_order`].
///
/// The thumbnail and the buttons keep their widths and the card's words
/// give way, wrapping, when the card is narrow - as it is in two columns,
/// where a summary takes two lines. A thumbnail that gave way would wrap
/// its own cells and draw a cropped board. The label wraps between words,
/// and within a word too long for its line, so a label of one long word
/// gives way as well rather than pushing the buttons out of the card.
fn spawn_replay_row(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    replay: &ReplayCatalogEntry,
    place: usize,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(CARD_HEIGHT),
                padding: UiRect::all(px(12)),
                border: UiRect::all(px(1)),
                align_items: AlignItems::Center,
                column_gap: px(16),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(SURFACE_BACKGROUND),
            BorderColor::all(CARD_BORDER),
        ))
        .with_children(|card| {
            spawn_thumbnail(card, replay);
            card.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                ..default()
            })
            .with_children(|details| {
                details.spawn((
                    Text::new(replay.label.clone()),
                    TextFont {
                        font_size: FontSize::Px(20.0),
                        ..default()
                    },
                    TextColor(TEXT_PRIMARY),
                    TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                ));
                details.spawn((
                    Text::new(replay_summary_text(replay)),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                ));
                details.spawn((
                    Text::new(replay_match_id_text(&replay.id)),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(TEXT_SUBTLE),
                ));
            });
            card.spawn(Node {
                width: px(220),
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|actions| {
                let mut ring = RingOrder::starting_at(card_focus_order(place));
                spawn_library_button(
                    actions,
                    view,
                    LibraryAction::Play(replay.id.clone()),
                    "View Replay",
                    220.0,
                    34.0,
                    ring.next(),
                );
                spawn_library_button(
                    actions,
                    view,
                    LibraryAction::OpenRename(replay.id.clone()),
                    "Change Label",
                    220.0,
                    34.0,
                    ring.next(),
                );
                spawn_library_button(
                    actions,
                    view,
                    LibraryAction::OpenDelete(replay.id.clone()),
                    "Delete",
                    220.0,
                    34.0,
                    ring.next(),
                );
            });
        });
}

/// The one line the Library screen shows about warnings: a count of the
/// Replays that are missing from the list, or - when the backend reported a
/// problem that is not about any particular Replay - a sentence that does
/// not pretend to a count.
///
/// This is broadcast text by the [`crate::platform::ReplayStorageWarning`]
/// policy: the player
/// did not ask for it and it is rendered whenever the screen is, so it
/// never carries a warning's own message. The type is what keeps that true - the
/// technical strings are behind variants this function only counts, rather
/// than a `Vec<String>` one line of code away from being displayed.
fn replay_library_warning_text(library: &ReplayLibrary) -> Option<String> {
    let skipped = library
        .warnings
        .iter()
        .filter(|warning| warning.is_skipped_replay())
        .count();
    match (skipped, library.warnings.len()) {
        (0, 0) => None,
        (0, _) => Some("Replay storage reported a problem. See logs for details.".into()),
        (1, _) => Some("1 saved Replay could not be loaded. See logs for details.".into()),
        (count, _) => Some(format!(
            "{count} saved Replays could not be loaded. See logs for details."
        )),
    }
}

fn replay_retention_text(library: &ReplayLibrary) -> Option<String> {
    library
        .retention_limit()
        .map(|limit| format!("Browser stores the newest {limit} Replays."))
}

fn spawn_thumbnail(parent: &mut ChildSpawnerCommands, replay: &ReplayCatalogEntry) {
    parent
        .spawn((
            Node {
                width: px(108),
                height: px(108),
                padding: UiRect::all(px(4)),
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(BOARD_WOOD),
        ))
        .with_children(|board| {
            for y in (0..BOARD_SIZE).rev() {
                for x in 0..BOARD_SIZE {
                    let (fill, border) =
                        thumbnail_cell_colors(replay.final_board.get(Point::new(x, y)));
                    board.spawn((
                        Node {
                            width: px(11),
                            height: px(11),
                            border: UiRect::all(px(1)),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(fill),
                        BorderColor::all(border),
                    ));
                }
            }
        });
}

/// Fill and border of one thumbnail cell.
///
/// A cell is an 11px square, so the board's ring is its one-pixel border:
/// the stones that carry a ring on the two boards - [`stone_geometry`] is
/// the single answer to which - get it in [`STONE_RING`] here, and every
/// other cell keeps the faint grid line.
fn thumbnail_cell_colors(cell: Cell) -> (Color, Color) {
    let fill = match cell {
        Cell::Empty => THUMBNAIL_EMPTY,
        Cell::Black => BLACK_STONE,
        Cell::White => WHITE_STONE,
        Cell::Gray => GRAY_STONE,
    };
    let border = if stone_geometry(cell).ring_scale.is_some() {
        STONE_RING
    } else {
        THUMBNAIL_GRID
    };
    (fill, border)
}

/// How many characters of a button's text a line of the search field
/// holds when the field is `width` px wide. The font is monospaced, so this
/// is known before layout, as the layout engine counts it
/// ([`line_breaks::chars_per_line`]).
fn search_field_chars(width: f32) -> usize {
    let inside = width - 2.0 * (BUTTON_PADDING + BUTTON_BORDER);
    line_breaks::chars_per_line(inside, BUTTON_TEXT_SIZE)
}

/// How many lines of a button's text the search field holds: two of its
/// 15 px text in its 40 px, as it always has.
const SEARCH_FIELD_LINES: usize = 2;

/// The search field's text, and whether it was cut to one line.
struct SearchLabel {
    text: String,
    cut: bool,
}

/// The search field's `label` as it is shown, for a field whose lines hold
/// `chars` characters. Where `bevy_ui` sets the label on the field's
/// [`SEARCH_FIELD_LINES`] lines, breaking them where it breaks them -
/// after a space, a hyphen, a slash or the caret's `|`, and never before
/// `)` `]` `}` `!` `?` `,` `.` `:` `;` or `/`, even after a space - and no
/// line runs past the field's sides, it is shown whole and
/// `bevy_ui` sets it so, as it always has. Otherwise - a longer label, one
/// that would take a third line, or a word longer than a line - it is cut to
/// one line, as a field scrolls its text, with [`CUT_MARK`] at each end cut
/// away. [`line_breaks::fits`] says which, following `bevy_ui`'s breaks
/// exactly. `caret` is the index, in characters, of the caret's
/// mark in the label - or the label's length, with the field unfocused -
/// and the caret is always shown: with the end of the label where the caret
/// is among its last characters, as it is while a query is typed; otherwise
/// with the start, where the caret is among the first; and otherwise with
/// the run of the label that ends at the caret.
fn shown_search_label(label: &str, caret: usize, chars: usize) -> SearchLabel {
    let fits = line_breaks::fits(label, chars, SEARCH_FIELD_LINES);
    if fits {
        return SearchLabel {
            text: label.to_owned(),
            cut: false,
        };
    }
    let all: Vec<char> = label.chars().collect();
    let run = |from: usize, to: usize| all[from..to].iter().collect::<String>();
    let room = chars - CUT_MARK.len();
    let text = if caret >= all.len().saturating_sub(room) {
        format!("{CUT_MARK}{}", run(all.len() - room, all.len()))
    } else if caret < room {
        format!("{}{CUT_MARK}", run(0, room))
    } else {
        let room = room - CUT_MARK.len();
        format!("{CUT_MARK}{}{CUT_MARK}", run(caret + 1 - room, caret + 1))
    };
    SearchLabel { text, cut: true }
}

pub(super) fn spawn_library_button(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    action: LibraryAction,
    label: &str,
    width: f32,
    height: f32,
    focus: Focusable,
) {
    spawn_library_button_with_layout(
        parent,
        view,
        action,
        label,
        library_button_node(width, height),
        TextLayout::justify(Justify::Center),
        focus,
    );
}

fn library_toolbar_node() -> Node {
    Node {
        width: percent(100),
        height: px(48),
        align_items: AlignItems::Center,
        ..default()
    }
}

fn library_toolbar_group_node() -> Node {
    Node {
        align_items: AlignItems::Center,
        column_gap: px(TOOLBAR_COLUMN_GAP),
        flex_shrink: 0.0,
        ..default()
    }
}

fn library_toolbar_spacer_node() -> Node {
    Node {
        min_width: px(TOOLBAR_MIN_SPACER_WIDTH),
        flex_basis: px(0),
        flex_grow: 1.0,
        ..default()
    }
}

fn spawn_sort_field_button(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    action: LibraryAction,
    label: &str,
    focus: Focusable,
) {
    spawn_library_button_with_layout(
        parent,
        view,
        action,
        label,
        sort_field_button_node(),
        TextLayout::justify(Justify::Center).with_no_wrap(),
        focus,
    );
}

fn spawn_sort_order_button(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    action: LibraryAction,
    label: &str,
    focus: Focusable,
) {
    spawn_library_button_with_layout(
        parent,
        view,
        action,
        label,
        sort_order_button_node(),
        TextLayout::justify(Justify::Center).with_no_wrap(),
        focus,
    );
}

/// A button's text size, and the padding and border between its text and
/// its edges.
const BUTTON_TEXT_SIZE: f32 = 15.0;
const BUTTON_PADDING: f32 = 8.0;
const BUTTON_BORDER: f32 = 1.0;

fn library_button_node(width: f32, height: f32) -> Node {
    Node {
        width: px(width),
        height: px(height),
        padding: UiRect::horizontal(px(BUTTON_PADDING)),
        border: UiRect::all(px(BUTTON_BORDER)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn sort_field_button_node() -> Node {
    Node {
        min_width: px(SORT_FIELD_CONTROL_MIN_WIDTH),
        flex_shrink: 0.0,
        ..library_button_node(SORT_FIELD_CONTROL_MIN_WIDTH, 40.0)
    }
}

fn sort_order_button_node() -> Node {
    Node {
        min_width: px(SORT_ORDER_CONTROL_WIDTH),
        flex_shrink: 0.0,
        ..library_button_node(SORT_ORDER_CONTROL_WIDTH, 40.0)
    }
}

/// Spawns one button. Every button this spawner makes carries a
/// `LibraryAction` and joins the focus ring at the position `focus` gives it -
/// marking is authored here, by the code that knows the drawn order. The
/// screen's dialog overlay is not one of them: it is a plain node, spawned
/// separately, that blocks the pointer by being under it.
fn spawn_library_button_with_layout(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    action: LibraryAction,
    label: &str,
    node: Node,
    text_layout: TextLayout,
    focus: Focusable,
) {
    let danger = action.is_danger();
    // Computed here, at spawn, and not only by `update_library_button_styles`:
    // that system runs *before* `refresh_library_screen` in the chain, so a
    // marker written there alone would reach a rebuilt button one frame
    // late - and the focus core rewrites its memory every frame, so that
    // would be a frame in which a dead paging button was in the ring with
    // its order recorded. The background is set to match for the same
    // frame, so the button never draws enabled while it is not.
    let disabled = library_action_disabled(&action, view.ui, view.library, view.layout);
    let mut button = parent.spawn((
        Button,
        UiSound::new(action.sound()),
        action,
        focus,
        node,
        BackgroundColor(if disabled {
            BUTTON_DISABLED
        } else if danger {
            danger_button_background(Highlight::None)
        } else {
            BUTTON_BACKGROUND
        }),
        BorderColor::all(if danger { DANGER } else { BLUE_BORDER }),
    ));
    if disabled {
        button.insert(InteractionDisabled);
    }
    button.with_child((
        Text::new(label.to_string()),
        TextFont {
            font_size: FontSize::Px(BUTTON_TEXT_SIZE),
            ..default()
        },
        TextColor(TEXT_PRIMARY),
        text_layout,
    ));
}

#[cfg(test)]
mod tests {
    use super::super::super::test_support::fixed_metadata_replay;
    use super::super::{LibraryDialog, confirm_delete};
    use super::*;
    use crate::architecture::sources_in;
    use crate::platform::ReplayStorageWarning;
    use crate::replay_library::test_support::LEAKED_REPLAY_STORAGE_ERROR;
    use crate::replay_library::{ReplayStore, SortField, SortOrder};

    #[test]
    fn thumbnail_stones_carry_the_ring_the_boards_give_them() {
        let (_, empty_border) = thumbnail_cell_colors(Cell::Empty);
        assert_ne!(empty_border, STONE_RING);
        for cell in [Cell::Empty, Cell::Black, Cell::White, Cell::Gray] {
            let (_, border) = thumbnail_cell_colors(cell);
            let ringed = stone_geometry(cell).ring_scale.is_some();
            assert_eq!(
                border == STONE_RING,
                ringed,
                "{cell:?} must be ringed in a thumbnail exactly when it is on the board"
            );
            if !ringed {
                assert_eq!(border, empty_border);
            }
        }
        assert_eq!(thumbnail_cell_colors(Cell::White).0, WHITE_STONE);
        assert_eq!(thumbnail_cell_colors(Cell::Gray).0, GRAY_STONE);
        assert_eq!(thumbnail_cell_colors(Cell::Black).0, BLACK_STONE);
    }

    /// The Library's warning line is broadcast text: rendered whenever the
    /// screen is, whether or not the player did anything. It therefore
    /// reports how many Replays are missing and nothing else - never a
    /// warning's own message, which is what
    /// `ReplayStorageWarning::SkippedReplay`'s payload holds and what the
    /// log gets instead.
    #[test]
    fn the_library_warning_line_counts_missing_replays_and_never_quotes_one() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-warning-text-{}",
            uuid::Uuid::new_v4()
        ));
        let mut library = ReplayLibrary::from_store(ReplayStore::new(root));
        assert_eq!(replay_library_warning_text(&library), None);

        // A storage-level warning is not a countable missing Replay, so it
        // gets a sentence that does not claim a count.
        library
            .warnings
            .push(ReplayStorageWarning::Storage("cannot reconcile".into()));
        assert_eq!(
            replay_library_warning_text(&library),
            Some("Replay storage reported a problem. See logs for details.".into())
        );

        library.warnings.push(ReplayStorageWarning::SkippedReplay(
            LEAKED_REPLAY_STORAGE_ERROR.into(),
        ));
        let one = replay_library_warning_text(&library).expect("one Replay is missing");
        assert_eq!(
            one,
            "1 saved Replay could not be loaded. See logs for details."
        );
        assert!(
            crate::diagnostics::carries_internal_detail(LEAKED_REPLAY_STORAGE_ERROR),
            "the warning really does carry something unfit to show a player, so the \
             assertion below cannot pass because the setup was harmless"
        );
        crate::diagnostics::assert_carries_no_internal_detail(&one);

        library
            .warnings
            .push(ReplayStorageWarning::SkippedReplay("corrupt replay".into()));
        assert_eq!(
            replay_library_warning_text(&library),
            Some("2 saved Replays could not be loaded. See logs for details.".into()),
            "the count is of skipped Replays, not of every warning"
        );
    }

    #[test]
    fn replay_library_renders_load_and_delete_errors_contextually() {
        let root = std::env::temp_dir().join(format!(
            "go-together-replay-ui-errors-{}",
            uuid::Uuid::new_v4()
        ));
        let mut library = ReplayLibrary::from_store(ReplayStore::new(root));
        assert_eq!(replay_library_warning_text(&library), None);
        library
            .warnings
            .push(ReplayStorageWarning::SkippedReplay("corrupt replay".into()));
        assert_eq!(
            replay_library_warning_text(&library),
            Some("1 saved Replay could not be loaded. See logs for details.".into())
        );
        library
            .entries
            .push(ReplayCatalogEntry::from_replay(&fixed_metadata_replay()));
        let mut ui = ReplayLibraryUiState {
            dialog: Some(LibraryDialog::Delete {
                id: library.entries[0].id.clone(),
                error: String::new(),
            }),
            ..default()
        };

        confirm_delete(
            &mut ui,
            &mut library,
            &mut crate::diagnostics::Diagnostics::default(),
        );

        let Some(LibraryDialog::Delete { error, .. }) = ui.dialog else {
            panic!("delete failure should keep its dialog open");
        };
        assert!(error.starts_with("Could not delete replay:"));
    }

    /// The search field's text, on texts worked out by hand: whole where
    /// `bevy_ui` sets it on the field's two lines, breaking them after a space,
    /// a hyphen, a slash or the caret; and otherwise - a longer text, a word
    /// longer than a line, or a space before "!" that no line may break at -
    /// cut to one line, with its end, where the caret is among its last
    /// characters or the field is unfocused; with its start, where the caret is
    /// among its first; and otherwise with the run that ends at the caret. A
    /// cut text is exactly a line long, and always shows the caret.
    #[test]
    fn the_search_fields_text_is_whole_in_two_lines_or_cut_around_the_caret() {
        assert_eq!(search_field_chars(250.0), 25);
        assert_eq!(search_field_chars(500.0), 53);
        let shown = |label: &str, caret: usize, chars: usize| {
            let shown = shown_search_label(label, caret, chars);
            (shown.text, shown.cut)
        };
        assert_eq!(
            shown("Search: abc|", 11, 12),
            ("Search: abc|".to_string(), false)
        );
        // "Search: abcd" and "efghijkl|": two lines of 12.
        let two_lines = "Search: abcd efghijkl|";
        assert_eq!(shown(two_lines, 20, 12), (two_lines.to_string(), false));
        // "Search: abc-" and "defghijkl|", and the same at a slash.
        let hyphen = "Search: abc-defghijkl|";
        assert_eq!(shown(hyphen, 21, 12), (hyphen.to_string(), false));
        let slash = "Search: abc/defghijkl|";
        assert_eq!(shown(slash, 21, 12), (slash.to_string(), false));
        // "Search: abc|" and "defghijkl": a break after the caret's mark.
        let caret = "Search: abc|defghijkl";
        assert_eq!(shown(caret, 11, 12), (caret.to_string(), false));
        // No break before the "!": "Search: ", "abc ! " and "defghij|".
        let bang = "Search: abc ! defghij|";
        assert_eq!(shown(bang, 21, 12), ("... defghij|".to_string(), true));
        let typed = "Search: abcdefghijklmnopqrstuvwxyz|";
        let caret = typed.len() - 1;
        assert_eq!(shown(typed, caret, 12), ("...stuvwxyz|".to_string(), true));
        let three_lines = "Search: abcdefgh ijklmnop qrstuvwx|";
        assert_eq!(
            shown(three_lines, three_lines.len() - 1, 12),
            ("...qrstuvwx|".to_string(), true)
        );
        let unfocused = "Search: abcdefghijklmnopqrstuvwxyz";
        assert_eq!(
            shown(unfocused, unfocused.len(), 12),
            ("...rstuvwxyz".to_string(), true)
        );
        let at_start = "Search: |abcdefghijklmnopqrstuvwxyz";
        assert_eq!(shown(at_start, 8, 12), ("Search: |...".to_string(), true));
        let inside = "Search: abcdefghijk|lmnopqrstuvwxyz";
        assert_eq!(shown(inside, 19, 12), ("...ghijk|...".to_string(), true));
        for (label, caret) in [(typed, caret), (at_start, 8), (inside, 19)] {
            let (text, cut) = shown(label, caret, 12);
            assert!(cut);
            assert_eq!(text.len(), 12, "{text:?}");
            assert!(text.contains('|'), "{text:?}");
        }
    }

    /// A query of the 60 characters a label can have, and a shorter one, in
    /// words, as one word, and in words joined by hyphens and by slashes; and
    /// queries with a space before "!" as the last character a line of the
    /// field holds, of 39 characters and of 97 - longer than a label can be, as
    /// a query may be, since in the wider field no 60 can put the "!" there and
    /// still take a third line. Each is typed in and not, with the header on
    /// two rows at 1040x900, where the field is 500 px, and on one at 1150x768,
    /// where it is 250. Where `bevy_ui` sets the query on the field's two
    /// lines, it is drawn whole on them, as it always was: words joined by
    /// hyphens or slashes break after them. Where it does not, it is drawn on
    /// one line, cut around the caret: a longer query, a word longer than a
    /// line, and the space before "!", where no line may break, so that the
    /// word before it moves down and takes a third line, one more than spaces
    /// alone would. Laid out whole, as a query that fits is, each of those
    /// takes that third line or runs past the field's side. Either way the
    /// field's text stays inside it, nothing runs under Clear or over the
    /// title, and no two texts meet. Under real layout.
    #[test]
    fn a_long_query_stays_inside_the_search_field() {
        use super::super::LibraryAction;
        use super::super::geometry::library_layout;
        use crate::app::visuals::WindowSize;
        use crate::ui::focus::drawn_texts;
        use bevy::text::TextLayoutInfo;

        const WORDS: &str = "Club night game with an early capture and a long ending race";
        const ONE_WORD: &str = "Clubnightgamewithanearlycaptureandalongendingracetothefinish";
        const SHORT: &str = "Club night game with an early";
        const HYPHENS: &str = "Club-night-game-with-an-early-capture-and-a-long-ending-race";
        const SLASHES: &str = "Club/night/game/with/an/early/capture/and/a/long/ending/race";
        const SHORT_HYPHENS: &str = "Club-night-game-with-an-early";
        const SHORT_SLASHES: &str = "Club/night/game/with/an/early";
        // "Search: " and the query to the space before "!" fill a line of
        // 53 characters, and of 25.
        const SPACED_BANG: &str = concat!(
            "Club night game, an early capture and a race ! ",
            "Then a close count and a white win by three points"
        );
        const SHORT_SPACED_BANG: &str = "Club night games ! An early capture won";
        let one_row = Vec2::new(1150.0, 768.0);
        assert!(library_layout(one_row, 0).one_row_header);
        assert!(!library_layout(WindowSize::OPENING.0, 0).one_row_header);
        // The query, the window, and whether it is drawn whole there.
        let cases = [
            (WORDS, WindowSize::OPENING.0, true),
            (ONE_WORD, WindowSize::OPENING.0, false),
            (HYPHENS, WindowSize::OPENING.0, true),
            (SLASHES, WindowSize::OPENING.0, true),
            (SPACED_BANG, WindowSize::OPENING.0, false),
            (SHORT, one_row, true),
            (WORDS, one_row, false),
            (ONE_WORD, one_row, false),
            (SHORT_HYPHENS, one_row, true),
            (SHORT_SLASHES, one_row, true),
            (SHORT_SPACED_BANG, one_row, false),
        ];
        // How many lines the layout engine set `text` on.
        let lines_of = |app: &App, text: Entity| {
            app.world()
                .get::<TextLayoutInfo>(text)
                .unwrap()
                .glyphs
                .iter()
                .map(|glyph| glyph.line_index as usize)
                .max()
                .unwrap()
                + 1
        };
        for (query, window, whole) in cases {
            for focused in [true, false] {
                let context = format!("{window}, {query:?}, focused {focused}");
                let mut app = library_entered_at(window, 4, None, false);
                {
                    let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
                    ui.search = crate::ui::text_input::TextEditor::new(query.into());
                    ui.search_focused = focused;
                    ui.dirty = true;
                }
                app.update();
                app.update();
                let world = app.world_mut();
                let field = world
                    .query::<(Entity, &LibraryAction)>()
                    .iter(world)
                    .find(|(_, action)| matches!(action, LibraryAction::FocusSearch))
                    .map(|(entity, _)| entity)
                    .unwrap();
                let text = app.world().get::<Children>(field).unwrap()[0];
                let drawn_text = app.world().get::<Text>(text).unwrap().0.clone();
                let label = format!("Search: {query}{}", if focused { "|" } else { "" });
                let lines = lines_of(&app, text);
                if whole {
                    assert_eq!(drawn_text, label, "{context}: drawn whole");
                    assert!((1..=2).contains(&lines), "{context}: {lines} lines");
                } else {
                    assert_ne!(drawn_text, label, "{context}: cut");
                    assert_eq!(lines, 1, "{context}");
                }
                let field = node_rect(&app, field);
                let drawn = drawn_texts(&mut app);
                let (_, shown, ink) = drawn
                    .iter()
                    .find(|(entity, _, _)| *entity == text)
                    .expect("the field's text is drawn");
                assert!(
                    field.contains(ink.min) && field.contains(ink.max),
                    "{context}: {shown:?} inked {ink:?} leaves the field {field:?}"
                );
                for (index, (_, a_text, a)) in drawn.iter().enumerate() {
                    for (_, b_text, b) in &drawn[index + 1..] {
                        assert!(
                            a.intersect(*b).is_empty(),
                            "{context}: {a_text:?} meets {b_text:?}"
                        );
                    }
                }
                if whole {
                    continue;
                }
                // The label laid out whole in the field, as one that fits
                // is: the cut is one `bevy_ui` needs.
                let world = app.world_mut();
                world.get_mut::<Text>(text).unwrap().0 = label.clone();
                *world.get_mut::<TextLayout>(text).unwrap() = TextLayout::justify(Justify::Center);
                app.update();
                let lines = lines_of(&app, text);
                let (_, _, ink) = drawn_texts(&mut app)
                    .into_iter()
                    .find(|(entity, _, _)| *entity == text)
                    .expect("the field's text is drawn");
                assert!(
                    lines > SEARCH_FIELD_LINES
                        || ink.min.x < field.min.x
                        || ink.max.x > field.max.x,
                    "{context}: whole, it takes {lines} lines inked {ink:?} in the field {field:?}"
                );
            }
        }
    }

    /// The indicator counts by the page as it is: which cards are shown,
    /// however many a page holds, and the last page full.
    #[test]
    fn page_indicator_reports_visible_replay_range() {
        assert_eq!(page_indicator_text(0, 0, 4), "0 of 0");
        assert_eq!(page_indicator_text(0, 10, 4), "1-4 of 10");
        assert_eq!(page_indicator_text(6, 10, 4), "7-10 of 10");
        assert_eq!(page_indicator_text(0, 13, 3), "1-3 of 13");
        assert_eq!(page_indicator_text(0, 2, 12), "1-2 of 2");
        assert_eq!(page_indicator_text(2, 13, 12), "3-13 of 13");
    }

    #[test]
    fn pagination_sides_share_space_equally_around_indicator() {
        let left = pagination_side_node(JustifyContent::FlexEnd);
        let right = pagination_side_node(JustifyContent::FlexStart);

        assert_eq!(left.flex_basis, px(0));
        assert_eq!(right.flex_basis, left.flex_basis);
        assert_eq!(right.flex_grow, left.flex_grow);
        assert_eq!(left.justify_content, JustifyContent::FlexEnd);
        assert_eq!(right.justify_content, JustifyContent::FlexStart);
    }

    #[test]
    fn page_indicator_aligns_with_pagination_buttons() {
        let indicator = pagination_indicator_node();

        assert_eq!(indicator.width, px(PAGE_INDICATOR_WIDTH));
        assert_eq!(indicator.height, px(PAGINATION_BUTTON_HEIGHT));
        assert_eq!(
            indicator.padding.top,
            px(PAGE_INDICATOR_TOP_PADDING),
            "the font needs a small top inset for visual baseline alignment"
        );
        assert_eq!(indicator.justify_content, JustifyContent::Center);
        assert_eq!(indicator.align_items, AlignItems::Center);
    }

    #[test]
    fn sort_field_control_keeps_longest_label_on_one_line() {
        let node = sort_field_button_node();
        let layout = TextLayout::justify(Justify::Center).with_no_wrap();

        assert_eq!(node.width, px(SORT_FIELD_CONTROL_MIN_WIDTH));
        assert_eq!(node.min_width, px(SORT_FIELD_CONTROL_MIN_WIDTH));
        assert_eq!(node.flex_shrink, 0.0);
        assert_eq!(layout.linebreak, LineBreak::NoWrap);
        assert_eq!(
            format!("Sort: {}", SortField::CreatedDate.label()),
            "Sort: Created Date"
        );
    }

    #[test]
    fn sort_order_control_keeps_longest_label_on_one_line() {
        let node = sort_order_button_node();
        let layout = TextLayout::justify(Justify::Center).with_no_wrap();

        assert_eq!(node.width, px(SORT_ORDER_CONTROL_WIDTH));
        assert_eq!(node.min_width, px(SORT_ORDER_CONTROL_WIDTH));
        assert_eq!(node.flex_shrink, 0.0);
        assert_eq!(layout.linebreak, LineBreak::NoWrap);
        assert_eq!(SortOrder::Descending.label(), "Descending");
    }

    #[test]
    fn replay_toolbar_uses_a_flexible_spacer_before_the_right_sort_group() {
        let toolbar = library_toolbar_node();
        let group = library_toolbar_group_node();
        let spacer = library_toolbar_spacer_node();

        assert_eq!(
            LIBRARY_TOOLBAR_ORDER,
            [
                LibraryToolbarSection::Search,
                LibraryToolbarSection::FlexibleSpacer,
                LibraryToolbarSection::Sort,
            ]
        );
        assert_eq!(toolbar.width, percent(100));
        assert_eq!(toolbar.align_items, AlignItems::Center);
        assert_eq!(group.column_gap, px(TOOLBAR_COLUMN_GAP));
        assert_eq!(group.flex_shrink, 0.0);
        assert_eq!(spacer.flex_basis, px(0));
        assert_eq!(spacer.flex_grow, 1.0);
        assert_eq!(spacer.min_width, px(TOOLBAR_MIN_SPACER_WIDTH));
    }

    /// Both empty-list messages, laid out with their text by `bevy_ui`
    /// itself and read back by their glyphs, stand where the first card
    /// stands in a list that has one: centred across that card and down it,
    /// each line centred on its own, and inside it. The card is measured
    /// from a real list rather than taken from [`REPLAY_CARD_HEIGHT`], so the
    /// slot is held to where a card is actually drawn. Until 2026-10-06 the
    /// message was a `Text` node the size of the whole list asking to be
    /// centred in it, and its glyphs were drawn against the list's top edge -
    /// 317 px above where they asked to be for the two-line message and
    /// 330 px for the one-line one. Its node was exactly where it was meant
    /// to be, which is why the glyphs are what is measured.
    #[test]
    fn the_empty_list_message_is_drawn_where_the_first_card_stands() {
        use crate::presentation::DEEP_SURFACE_BACKGROUND;
        use crate::replay::Replay;
        use crate::replay_library::test_support::MockReplayStorage;
        use crate::ui::focus::{measure_drawn_text, real_layout_app};

        /// The screen over real layout and text, searching for `search` in
        /// a Library that holds `replays` in memory.
        fn laid_out(replays: Vec<Replay>, search: &str) -> App {
            let storage = MockReplayStorage::new(None);
            storage.replays.lock().unwrap().extend(replays);
            let mut ui = ReplayLibraryUiState::default();
            ui.search.text = search.into();
            let mut app = real_layout_app(crate::app::visuals::WindowSize::OPENING.0);
            app.insert_resource(ReplayLibrary::from_store(storage))
                .insert_resource(ui)
                .add_systems(Startup, super::super::spawn_library_screen);
            app.update();
            app
        }

        // A glyph's ink is not quite centred in its advance and line, so a
        // few pixels are allowed; the defect was 44 px for the two-line
        // message and 57 px for the one-line one.
        const INK_TOLERANCE: f32 = 3.0;

        let mut listed = laid_out(vec![fixed_metadata_replay()], "");
        let label = measure_drawn_text(&mut listed, "Sample Replay");
        assert_eq!(
            label.surface_fill, SURFACE_BACKGROUND,
            "the label is read off its card"
        );
        let card = label.surface;

        for (replays, search, message) in [
            (vec![], "", NO_REPLAYS_MESSAGE),
            (
                vec![fixed_metadata_replay()],
                "zzz",
                NO_SEARCH_MATCH_MESSAGE,
            ),
        ] {
            let mut app = laid_out(replays, search);
            let drawn = measure_drawn_text(&mut app, message);
            assert_eq!(
                drawn.surface_fill, DEEP_SURFACE_BACKGROUND,
                "{message:?} is drawn on the list itself"
            );
            let list = drawn.surface;
            assert!(
                (card.min.y - list.min.y).abs() < 0.5 && (card.width() - list.width()).abs() < 0.5,
                "the first card {card:?} spans the top of the list {list:?}"
            );

            // Each line on its own, as well as the whole: the shorter line
            // of the two-line message is centred under the longer one only
            // by its `TextLayout::justify`, and a box around both lines
            // cannot see whether it is.
            assert_eq!(
                drawn.lines.len(),
                message.lines().count(),
                "{message:?} is drawn on as many lines as it has"
            );
            for line in &drawn.lines {
                assert!(
                    (line.center().x - card.center().x).abs() <= INK_TOLERANCE,
                    "a line of {message:?} at {line:?} is not centred across the first card's place {card:?}"
                );
            }

            let ink = drawn.ink;
            let offset = ink.center() - card.center();
            assert!(
                offset.x.abs() <= INK_TOLERANCE && offset.y.abs() <= INK_TOLERANCE,
                "{message:?} is drawn {offset:?} from the centre of the first card's place {card:?}"
            );
            assert!(
                card.contains(ink.min) && card.contains(ink.max),
                "{message:?} at {ink:?} spills out of the first card's place {card:?}"
            );
        }
    }

    /// The toolbar's controls at their least, with every gap at its least,
    /// fit the width the screen is held to in every window: across the
    /// list on a row of their own, and on the one header row beside Home
    /// and the title, with the search control narrowed, from 1150 px wide.
    #[test]
    fn replay_toolbar_minimum_width_fits_every_window() {
        use super::super::geometry::{
            ONE_ROW_HEADER_FROM, ONE_ROW_SEARCH_CONTROL_WIDTH, SEARCH_CONTROL_WIDTH, library_layout,
        };
        use crate::app::visuals::layout_sweep;
        use crate::ui::text_metrics::line_width;

        let toolbar = |search: f32| {
            search
                + CLEAR_CONTROL_WIDTH
                + SORT_FIELD_CONTROL_MIN_WIDTH
                + SORT_ORDER_CONTROL_WIDTH
                + TOOLBAR_COLUMN_GAP * 2.0
                + TOOLBAR_MIN_SPACER_WIDTH
        };
        let one_row = HOME_BUTTON_WIDTH
            + HOME_TITLE_GAP
            + line_width(LIBRARY_TITLE.len(), LIBRARY_TITLE_SIZE)
            + TITLE_TOOLBAR_GAP
            + toolbar(ONE_ROW_SEARCH_CONTROL_WIDTH);
        let mut one_rows = 0;
        for size in layout_sweep() {
            for notes in [0, 1, 2] {
                let layout = library_layout(size, notes);
                let available = layout.content_width();
                if layout.one_row_header {
                    one_rows += 1;
                    assert_eq!(layout.search_width, ONE_ROW_SEARCH_CONTROL_WIDTH);
                    assert!(one_row <= available, "{size}: {one_row} in {available}");
                } else {
                    assert_eq!(layout.search_width, SEARCH_CONTROL_WIDTH);
                    let needed = toolbar(SEARCH_CONTROL_WIDTH);
                    assert!(needed <= available, "{size}: {needed} in {available}");
                }
            }
        }
        assert!(one_rows > 0);
        let at = library_layout(Vec2::new(ONE_ROW_HEADER_FROM, 768.0), 0);
        assert!(at.one_row_header);
        assert!(one_row <= at.content_width(), "{one_row} at 1150");
    }

    #[test]
    fn replay_library_omits_removed_helper_copy() {
        // Every `.rs` file in this directory, read from disk at test time rather
        // than from a hand-written `include_str!` list - see
        // `crate::architecture` for why a list cannot guard a negative
        // assertion, and why the count is a floor the walk must reach.
        let source = sources_in("ui/replay/library", 2);
        let removed_sort_order = ["Nei", "ther"].concat();
        let old_default_helper = ["newest completed", " first"].concat();
        let scroll_helper = ["Mouse wheel", " also scrolls"].concat();

        assert!(!source.contains(&removed_sort_order));
        assert!(!source.contains(&old_default_helper));
        assert!(!source.contains(&scroll_helper));
    }

    /// The Library as the game enters it - its own plugin and the focus
    /// core, entered through a state transition - under real layout in a
    /// window of `window` logical pixels, holding `replays` Replays, in a
    /// browser-like store keeping `retention` of them if given, and with a
    /// Replay that could not be read if `warning`. Two frames have run on
    /// the screen.
    pub(in crate::ui::replay::library) fn library_entered_at(
        window: Vec2,
        replays: usize,
        retention: Option<usize>,
        warning: bool,
    ) -> App {
        use super::super::super::viewer::ReplayViewerState;
        use super::super::ReplayLibraryPlugin;
        use crate::audio::AudioCueQueue;
        use crate::gameplay::InputGate;
        use crate::replay_library::test_support::MockReplayStorage;
        use crate::ui::focus::{UiFocusPlugin, real_layout_app};
        use crate::ui::hud::HudState;
        use bevy::input::keyboard::KeyboardInput;
        use bevy::input::mouse::MouseWheel;

        let mut library = ReplayLibrary::from_store(MockReplayStorage::new(retention));
        for index in 0..replays {
            let mut entry = ReplayCatalogEntry::from_replay(&fixed_metadata_replay());
            entry.id = format!("00000000-0000-4000-8000-{index:012}");
            entry.label = format!("Replay {}", index + 1);
            entry.completed_at_ms += index as i64 * 60_000;
            library.entries.push(entry);
        }
        if warning {
            library
                .warnings
                .push(ReplayStorageWarning::SkippedReplay("unreadable".into()));
        }
        let mut app = real_layout_app(window);
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
        app.update();
        app
    }

    fn node_rect(app: &App, entity: Entity) -> Rect {
        use bevy::ui::{ComputedNode, UiGlobalTransform};

        let node = app.world().get::<ComputedNode>(entity).unwrap();
        let transform = app.world().get::<UiGlobalTransform>(entity).unwrap();
        let scale = node.inverse_scale_factor;
        Rect::from_center_size(transform.translation * scale, node.size * scale)
    }

    /// Every card on show, first to last: its box, its thumbnail's, its
    /// button column's and the list's.
    fn cards(app: &mut App) -> Vec<(Rect, Rect, Rect, Rect)> {
        let world = app.world_mut();
        let mut plays: Vec<(Entity, u32)> = world
            .query::<(Entity, &LibraryAction, &Focusable)>()
            .iter(world)
            .filter(|(_, action, _)| matches!(action, LibraryAction::Play(_)))
            .map(|(entity, _, focus)| (entity, focus.order))
            .collect();
        plays.sort_by_key(|(_, order)| *order);
        plays
            .into_iter()
            .map(|(play, _)| {
                let actions = app.world().get::<ChildOf>(play).unwrap().parent();
                let card = app.world().get::<ChildOf>(actions).unwrap().parent();
                let list = app.world().get::<ChildOf>(card).unwrap().parent();
                let thumbnail = app.world().get::<Children>(card).unwrap()[0];
                (
                    node_rect(app, card),
                    node_rect(app, thumbnail),
                    node_rect(app, actions),
                    node_rect(app, list),
                )
            })
            .collect()
    }

    /// At every named size, and in three windows where a line of notes
    /// costs a card, under real layout and real picking, the Library in
    /// every state a page can be in - no Replays, one, a full page, more
    /// than a page, a full page of the longest labels a Replay can have, in
    /// words and as one word of 60 characters, each with a summary as long
    /// as a long Match makes it, a browser's store with its retention note,
    /// a Replay that could not be read - and with each dialog open over a
    /// full page: every text stands inside the window and, with no dialog
    /// open, no two meet; the page shows as many cards as the layout says,
    /// each inside the list, none meeting another, each its thumbnail whole
    /// at 108 px, its buttons inside it and as wide as the layout makes it,
    /// and the ring reaching them in the order they are drawn, row by row;
    /// the list stands as wide as the layout holds it, centred; the pointer
    /// reaches every control shown; and a dialog's overlay covers the
    /// window, its buttons reached and everything beneath covered.
    #[test]
    fn every_page_state_fits_at_every_size() {
        use super::super::geometry::library_layout;
        use crate::app::visuals::NAMED_WINDOW_SIZES;
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::{drawn_texts, laid_out_texts};

        // A label of the 60 characters a label can have, in words that wrap
        // onto two lines in the narrowest card, and as one word, which can
        // break only between characters.
        const LONGEST_LABEL: &str = "Club night game with an early capture and a long ending race";
        const ONE_WORD_LABEL: &str = "Clubnightgamewithanearlycaptureandalongendingracetothefinish";
        assert_eq!(LONGEST_LABEL.len(), 60);
        assert_eq!(ONE_WORD_LABEL.len(), 60);
        let notes_cost_a_card = [
            Vec2::new(1040.0, 640.0),
            Vec2::new(1280.0, 740.0),
            Vec2::new(1920.0, 1030.0),
        ];
        for window in notes_cost_a_card {
            assert!(
                library_layout(window, 1).cards_per_page()
                    < library_layout(window, 0).cards_per_page(),
                "{window}"
            );
        }
        for window in NAMED_WINDOW_SIZES.into_iter().chain(notes_cost_a_card) {
            let page = library_layout(window, 0).cards_per_page();
            let states = [
                ("no Replays", 0, None, false, None),
                ("one", 1, None, false, None),
                ("a full page", page, None, false, None),
                ("more than a page", page + 3, None, false, None),
                ("the longest labels", page, None, false, Some(LONGEST_LABEL)),
                ("one-word labels", page, None, false, Some(ONE_WORD_LABEL)),
                ("a browser's store", 3, Some(3), false, None),
                ("a Replay not read", page + 3, None, true, None),
            ];
            for (state, replays, retention, warning, label) in states {
                let context = format!("{window}, {state}");
                let mut app = library_entered_at(window, replays, retention, warning);
                if let Some(label) = label {
                    // And the longest summary a Match plausibly makes: a
                    // close score, hours of play and over a hundred rounds.
                    for entry in &mut app.world_mut().resource_mut::<ReplayLibrary>().entries {
                        entry.label = label.into();
                        entry.score = crate::rules::Score {
                            black: 41,
                            white: 40,
                        };
                        entry.duration_seconds = 3.0 * 3600.0 + 59.0 * 60.0 + 59.0;
                        entry.round_count = 120;
                        entry.opening_completed = true;
                    }
                    app.world_mut().resource_mut::<ReplayLibraryUiState>().dirty = true;
                    app.update();
                    app.update();
                    let world = app.world_mut();
                    let shown = world
                        .query::<&Text>()
                        .iter(world)
                        .filter(|text| text.0 == label)
                        .count();
                    assert_eq!(shown, replays, "{context}: the labels are drawn");
                }
                let notes = usize::from(retention.is_some()) + usize::from(warning);
                let layout = library_layout(window, notes);
                let inside = Rect::from_corners(Vec2::ZERO, window);
                for (text, rect) in laid_out_texts(&mut app) {
                    assert!(
                        inside.contains(rect.min) && inside.contains(rect.max),
                        "{context}: {text:?} at {rect:?}"
                    );
                }
                let drawn = drawn_texts(&mut app);
                for (index, (_, a_text, a)) in drawn.iter().enumerate() {
                    for (_, b_text, b) in &drawn[index + 1..] {
                        assert!(
                            a.intersect(*b).is_empty(),
                            "{context}: {a_text:?} meets {b_text:?}"
                        );
                    }
                }
                let shown = cards(&mut app);
                assert_eq!(
                    shown.len(),
                    replays.min(layout.cards_per_page()),
                    "{context}"
                );
                for (index, (card, thumbnail, actions, list)) in shown.iter().enumerate() {
                    assert!(
                        list.contains(card.min) && list.contains(card.max),
                        "{context}: card {index} {card:?} leaves the list {list:?}"
                    );
                    assert_eq!(
                        thumbnail.size(),
                        Vec2::splat(108.0),
                        "{context}: card {index}"
                    );
                    assert!(
                        card.contains(actions.min) && card.contains(actions.max),
                        "{context}: card {index}'s buttons {actions:?} leave it {card:?}"
                    );
                    assert!(
                        (card.width() - layout.card_width()).abs() <= 1.0,
                        "{context}: card {index} is {} wide of {}",
                        card.width(),
                        layout.card_width()
                    );
                    assert!(
                        (list.width() - layout.content_width()).abs() <= 1.0
                            && (list.center().x - window.x / 2.0).abs() <= 1.0,
                        "{context}: the list {list:?}"
                    );
                    for (other, _, _, _) in &shown[index + 1..] {
                        assert!(card.intersect(*other).is_empty(), "{context}");
                    }
                }
                // The ring reads the page as it is drawn: down one column,
                // and in two along each row of two, then down to the next.
                for (index, pair) in shown.windows(2).enumerate() {
                    let (card, next) = (pair[0].0, pair[1].0);
                    if layout.columns == 2 && index % 2 == 0 {
                        assert!(
                            next.min.x > card.max.x && (next.min.y - card.min.y).abs() < 0.5,
                            "{context}: card {} is not beside card {index}",
                            index + 1
                        );
                    } else {
                        assert!(
                            next.min.y > card.max.y && (next.min.x - shown[0].0.min.x).abs() < 0.5,
                            "{context}: card {} does not start the next row",
                            index + 1
                        );
                    }
                }
                let buttons = reachable_buttons(&mut app);
                assert!(buttons.len() >= 6, "{context}");
                assert_eq!(
                    covered_at_their_centres(&mut app, &buttons),
                    [],
                    "{context}"
                );
            }

            for dialog in ["rename", "delete"] {
                let context = format!("{window}, the {dialog} dialog");
                let mut app = library_entered_at(window, page, None, false);
                let beneath = reachable_buttons(&mut app);
                let id = "00000000-0000-4000-8000-000000000000".to_string();
                {
                    let mut ui = app.world_mut().resource_mut::<ReplayLibraryUiState>();
                    ui.dialog = Some(if dialog == "rename" {
                        LibraryDialog::Rename {
                            id,
                            editor: crate::ui::text_input::TextEditor::new("Replay 1".into()),
                            error: String::new(),
                        }
                    } else {
                        LibraryDialog::Delete {
                            id,
                            error: String::new(),
                        }
                    });
                    ui.dirty = true;
                }
                app.update();
                app.update();
                let overlay = {
                    let world = app.world_mut();
                    let found: Vec<Entity> = world
                        .query::<(Entity, &GlobalZIndex)>()
                        .iter(world)
                        .filter(|(_, z)| z.0 == 200)
                        .map(|(entity, _)| entity)
                        .collect();
                    assert_eq!(found.len(), 1, "{context}: one overlay");
                    found[0]
                };
                let covers = node_rect(&app, overlay);
                assert_eq!(covers, Rect::from_corners(Vec2::ZERO, window), "{context}");
                let all = reachable_buttons(&mut app);
                let in_dialog: Vec<Entity> = all
                    .iter()
                    .copied()
                    .filter(|entity| {
                        matches!(
                            app.world()
                                .get::<Focusable>(*entity)
                                .map(|focus| focus.group),
                            Some(crate::ui::focus::FocusGroup::Modal)
                        )
                    })
                    .collect();
                assert!(in_dialog.len() >= 2, "{context}");
                assert_eq!(
                    covered_at_their_centres(&mut app, &in_dialog),
                    [],
                    "{context}"
                );
                let under: Vec<Entity> = all
                    .into_iter()
                    .filter(|entity| !in_dialog.contains(entity))
                    .collect();
                assert_eq!(under.len(), beneath.len(), "{context}");
                assert_eq!(
                    covered_at_their_centres(&mut app, &under),
                    under,
                    "{context}"
                );
                let inside = Rect::from_corners(Vec2::ZERO, window);
                for (text, rect) in laid_out_texts(&mut app) {
                    assert!(
                        inside.contains(rect.min) && inside.contains(rect.max),
                        "{context}: {text:?} at {rect:?}"
                    );
                }
            }
        }
    }

    /// A window smaller than the minimum - a browser's page can be - is
    /// built as the minimum is: at 800x400 the screen's own reading of the
    /// window raises it to 1040x600, so the page is built from the
    /// minimum's layout, with the minimum's two cards, under real layout.
    /// Laid out for its own size, that window would hold one; at 800x480 it
    /// would hold the same two, which is why the test is not run there.
    #[test]
    fn a_window_below_the_minimum_is_built_as_the_minimum() {
        use super::super::geometry::library_layout;
        use crate::app::visuals::WindowSize;

        let below = Vec2::new(800.0, 400.0);
        let minimum = library_layout(WindowSize::MINIMUM.0, 0);
        assert_ne!(library_layout(below, 0).build(), minimum.build());
        let mut app = library_entered_at(below, 20, None, false);
        let ui = app.world().resource::<ReplayLibraryUiState>();
        assert_eq!(ui.built_with, Some(minimum.build()));
        assert_eq!(cards(&mut app).len(), minimum.cards_per_page());
    }

    /// The game draws that window as the minimum's, scaled down and
    /// centred, with the pointer landing where things are drawn: the page
    /// of two cards, laid out as at the minimum, drawn inside the window.
    /// Through the screen's own plugin and the game's window layout.
    #[test]
    fn a_window_below_the_minimum_draws_the_minimums_page_scaled_down() {
        use crate::app::visuals::WindowSize;
        use crate::ui::focus::{
            BELOW_THE_MINIMUM, assert_drawn_as_the_minimum_scaled_down, draw_as_the_game_does,
        };

        let mut below = library_entered_at(BELOW_THE_MINIMUM, 20, None, false);
        draw_as_the_game_does(&mut below);
        let mut minimum = library_entered_at(WindowSize::MINIMUM.0, 20, None, false);
        draw_as_the_game_does(&mut minimum);
        assert_eq!(cards(&mut below).len(), 2);
        assert_drawn_as_the_minimum_scaled_down(&mut below, &mut minimum, "the Library");
    }

    /// A change of the window's width that changes nothing the screen is
    /// built from rebuilds nothing - every card is the entity it was - and
    /// the page follows the window all the same, by flex: in one column from
    /// 1040x900 to 1100x900, and in two from 1800x1080 to 2000x1080, the list
    /// and every card stand as wide as the layout makes them at the new
    /// width, the list centred. The window itself resized, through the
    /// screen's own plugin and the game's window tracking.
    #[test]
    fn a_change_of_width_alone_rebuilds_nothing_and_the_page_follows_it() {
        use super::super::geometry::library_layout;

        for (from, to) in [
            (Vec2::new(1040.0, 900.0), Vec2::new(1100.0, 900.0)),
            (Vec2::new(1800.0, 1080.0), Vec2::new(2000.0, 1080.0)),
        ] {
            let context = format!("{from} to {to}");
            let (before, after) = (library_layout(from, 0), library_layout(to, 0));
            assert_eq!(before.build(), after.build(), "{context}");
            assert_ne!(before.card_width(), after.card_width(), "{context}");
            let mut app = library_entered_at(from, 20, None, false);
            let plays = |app: &mut App| {
                let world = app.world_mut();
                let mut plays: Vec<(u32, Entity)> = world
                    .query::<(Entity, &LibraryAction, &Focusable)>()
                    .iter(world)
                    .filter(|(_, action, _)| matches!(action, LibraryAction::Play(_)))
                    .map(|(entity, _, focus)| (focus.order, entity))
                    .collect();
                plays.sort();
                plays
            };
            let built = plays(&mut app);
            {
                let world = app.world_mut();
                let mut windows =
                    world.query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>();
                let mut window = windows.single_mut(world).unwrap();
                window.resolution.set(to.x, to.y);
            }
            app.update();
            app.update();
            assert_eq!(plays(&mut app), built, "{context}: the page was rebuilt");
            let shown = cards(&mut app);
            assert_eq!(shown.len(), after.cards_per_page(), "{context}");
            for (index, (card, _, _, list)) in shown.iter().enumerate() {
                assert!(
                    (card.width() - after.card_width()).abs() <= 1.0,
                    "{context}: card {index} is {} wide of {}",
                    card.width(),
                    after.card_width()
                );
                assert!(
                    (list.width() - after.content_width()).abs() <= 1.0
                        && (list.center().x - to.x / 2.0).abs() <= 1.0,
                    "{context}: the list {list:?}"
                );
            }
        }
    }

    /// In two columns a scroll moves the list a row of two, so every card
    /// keeps its column: Down twice from the top shows the fifth card
    /// first, the wheel the same, Up back; and the paging counts by the
    /// page as it is.
    #[test]
    fn two_columns_scroll_a_row_of_two_at_a_time() {
        use super::super::LibraryAction;
        use crate::ui::button::trigger_activation;

        let window = Vec2::new(1920.0, 1080.0);
        let mut app = library_entered_at(window, 20, None, false);
        let offset = |app: &App| app.world().resource::<ReplayLibraryUiState>().scroll_offset;
        let indicator = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<&Text>()
                .iter(world)
                .find(|text| text.0.contains(" of "))
                .map(|text| text.0.clone())
                .unwrap()
        };
        let press = |app: &mut App, wanted: fn(&LibraryAction) -> bool| {
            let world = app.world_mut();
            let button = world
                .query::<(Entity, &LibraryAction)>()
                .iter(world)
                .find(|(_, action)| wanted(action))
                .map(|(entity, _)| entity)
                .unwrap();
            trigger_activation(app, button);
            app.update();
            app.update();
        };
        assert_eq!(indicator(&mut app), "1-12 of 20");
        press(&mut app, |action| {
            matches!(action, LibraryAction::ScrollDown)
        });
        assert_eq!(offset(&app), 2);
        assert_eq!(indicator(&mut app), "3-14 of 20");
        app.world_mut()
            .write_message(bevy::input::mouse::MouseWheel {
                unit: bevy::input::mouse::MouseScrollUnit::Line,
                x: 0.0,
                y: -1.0,
                window: Entity::PLACEHOLDER,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
        app.update();
        app.update();
        assert_eq!(offset(&app), 4);
        assert_eq!(indicator(&mut app), "5-16 of 20");
        press(&mut app, |action| matches!(action, LibraryAction::ScrollUp));
        assert_eq!(offset(&app), 2);
        // The first card of a row of two keeps its column: the left one.
        let first = cards(&mut app)[0].0;
        let second = cards(&mut app)[1].0;
        assert!(first.max.x < second.min.x && (first.min.y - second.min.y).abs() < 0.5);
    }
}
