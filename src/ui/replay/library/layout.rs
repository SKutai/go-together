//! Spawns the library screen: the toolbar, the current page of replay cards,
//! the pagination row and the rename and delete dialogs.
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
use super::ring::{
    HOME_FOCUS_ORDER, PAGINATION_FOCUS_ORDER, RingOrder, TOOLBAR_FOCUS_ORDER, card_focus_order,
};
use super::{
    LibraryAction, LibraryDialog, ReplayLibraryUiState, library_action_disabled,
    replay_match_id_text, replay_summary_text,
};
use crate::app::AppScreen;
use crate::app::visuals::stone_geometry;
use crate::audio::UiSound;
use crate::presentation::{
    APP_BACKGROUND, BLACK_STONE, BLUE_BORDER, BOARD_WOOD, BUTTON_BACKGROUND, BUTTON_DISABLED,
    CARD_BORDER, DANGER, DEEP_SURFACE_BACKGROUND, GRAY_STONE, Highlight, ORANGE,
    OVERLAY_BACKGROUND, PANEL_BACKGROUND, STONE_RING, SURFACE_BACKGROUND, TEXT_FIELD_BACKGROUND,
    TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY, TEXT_SUBTLE, THUMBNAIL_EMPTY, THUMBNAIL_GRID,
    WHITE_STONE, danger_button_background,
};
use crate::replay::REPLAY_LABEL_RULE;
use crate::replay_library::{ReplayCatalogEntry, ReplayLibrary, filter_and_sort_replays};
use crate::rules::{BOARD_SIZE, Cell, Point};
use crate::ui::focus::Focusable;
/// What the list says when the Library holds no Replays at all.
const NO_REPLAYS_MESSAGE: &str =
    "No completed match replays yet.\nFinish and score a match to save it automatically.";
/// What the list says when the search matches none of the Replays there are.
const NO_SEARCH_MATCH_MESSAGE: &str = "No replay labels match this search.";
const PAGE_INDICATOR_WIDTH: f32 = 130.0;
const PAGINATION_BUTTON_HEIGHT: f32 = 34.0;
const PAGE_INDICATOR_TOP_PADDING: f32 = 4.0;
const DELETE_CONFIRMATION_FONT_SIZE: f32 = 18.0;
const DELETE_MATCH_ID_FONT_SIZE: f32 = 14.0;
const DELETE_MATCH_ID_COLOR: Color = TEXT_MUTED;

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
struct LibraryView<'a> {
    ui: &'a ReplayLibraryUiState,
    library: &'a ReplayLibrary,
    layout: &'a LibraryLayout,
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
                padding: UiRect::axes(px(layout.padding.x), px(layout.padding.y)),
                flex_direction: FlexDirection::Column,
                row_gap: px(ROW_GAP),
                ..default()
            },
            BackgroundColor(APP_BACKGROUND),
            DespawnOnExit(AppScreen::ReplayLibrary),
        ))
        .with_children(|root| {
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
                            margin: UiRect::new(
                                px(HOME_TITLE_GAP),
                                px(TITLE_TOOLBAR_GAP),
                                px(0),
                                px(0),
                            ),
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

            if let Some(dialog) = &ui.dialog {
                spawn_library_dialog(root, view, dialog);
            }
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
        if ui.search_focused {
            "Search label: |".into()
        } else {
            "Search replay labels...".into()
        }
    } else {
        format!(
            "Search: {}",
            if ui.search_focused {
                ui.search.display_text()
            } else {
                ui.search.text.clone()
            }
        )
    };
    let mut ring = RingOrder::starting_at(TOOLBAR_FOCUS_ORDER);
    for section in LIBRARY_TOOLBAR_ORDER {
        match section {
            LibraryToolbarSection::Search => {
                controls
                    .spawn(library_toolbar_group_node())
                    .with_children(|search| {
                        spawn_library_button(
                            search,
                            view,
                            LibraryAction::FocusSearch,
                            &search_label,
                            view.layout.search_width,
                            40.0,
                            ring.next(),
                        );
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
/// rows of two cards side by side.
fn list_node(layout: &LibraryLayout) -> Node {
    let two_columns = layout.columns > 1;
    Node {
        width: percent(100),
        flex_grow: 1.0,
        flex_direction: if two_columns {
            FlexDirection::Row
        } else {
            FlexDirection::Column
        },
        flex_wrap: if two_columns {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
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
/// its own cells and draw a cropped board.
fn spawn_replay_row(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    replay: &ReplayCatalogEntry,
    place: usize,
) {
    parent
        .spawn((
            Node {
                width: if view.layout.columns > 1 {
                    px(view.layout.card_width())
                } else {
                    percent(100)
                },
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

fn spawn_library_dialog(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    dialog: &LibraryDialog,
) {
    parent
        .spawn((
            // A plain node, and that is what stops a click: picking halts at
            // the first node under the pointer that carries no `Pickable`
            // saying otherwise, so this overlay keeps every click off the
            // toolbar and cards underneath (`ui::button`). It has no action,
            // so it is never in the ring.
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                left: px(0),
                top: px(0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(OVERLAY_BACKGROUND),
            GlobalZIndex(200),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(610),
                        padding: UiRect::all(px(30)),
                        border: UiRect::all(px(2)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(18),
                        ..default()
                    },
                    BackgroundColor(PANEL_BACKGROUND),
                    BorderColor::all(ORANGE),
                ))
                .with_children(|modal| match dialog {
                    LibraryDialog::Rename { editor, error, .. } => {
                        modal.spawn((
                            Text::new("Change Replay Label"),
                            TextFont {
                                font_size: FontSize::Px(30.0),
                                ..default()
                            },
                            TextColor(TEXT_HEADING),
                        ));
                        modal.spawn((
                            Text::new(editor.display_text()),
                            TextFont {
                                font_size: FontSize::Px(19.0),
                                ..default()
                            },
                            TextColor(TEXT_PRIMARY),
                            TextLayout::justify(Justify::Center)
                                .with_linebreak(LineBreak::WordOrCharacter),
                            Node {
                                width: percent(100),
                                min_height: px(48),
                                padding: UiRect::all(px(10)),
                                border: UiRect::all(px(1)),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(TEXT_FIELD_BACKGROUND),
                            BorderColor::all(BLUE_BORDER),
                        ));
                        modal.spawn((
                            Text::new(if error.is_empty() {
                                REPLAY_LABEL_RULE.into()
                            } else {
                                error.clone()
                            }),
                            TextFont {
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(if error.is_empty() { TEXT_MUTED } else { DANGER }),
                            TextLayout::justify(Justify::Center),
                            Node {
                                width: percent(100),
                                min_height: px(42),
                                ..default()
                            },
                        ));
                        modal
                            .spawn(Node {
                                column_gap: px(12),
                                ..default()
                            })
                            .with_children(|actions| {
                                // Save Label seeds, then the rest in drawn
                                // order - see the ring module's docs.
                                let mut ring = RingOrder::modal();
                                let save_label = ring.next();
                                let clear = ring.next();
                                let cancel = ring.next();
                                spawn_library_button(
                                    actions,
                                    view,
                                    LibraryAction::ClearRename,
                                    "Clear",
                                    90.0,
                                    38.0,
                                    clear,
                                );
                                spawn_library_button(
                                    actions,
                                    view,
                                    LibraryAction::ConfirmRename,
                                    "Save Label",
                                    130.0,
                                    38.0,
                                    save_label,
                                );
                                spawn_library_button(
                                    actions,
                                    view,
                                    LibraryAction::CancelDialog,
                                    "Cancel",
                                    100.0,
                                    38.0,
                                    cancel,
                                );
                            });
                    }
                    LibraryDialog::Delete { id, error } => {
                        let replay = view.library.get(id);
                        modal.spawn((
                            Text::new("Delete Replay?"),
                            TextFont {
                                font_size: FontSize::Px(30.0),
                                ..default()
                            },
                            TextColor(DANGER),
                        ));
                        modal.spawn((
                            Text::new(delete_confirmation_question(
                                replay.map_or("Unknown replay", |replay| replay.label.as_str()),
                            )),
                            TextFont {
                                font_size: FontSize::Px(DELETE_CONFIRMATION_FONT_SIZE),
                                ..default()
                            },
                            TextColor(TEXT_PRIMARY),
                            TextLayout::justify(Justify::Center)
                                .with_linebreak(LineBreak::WordOrCharacter),
                            Node {
                                width: percent(100),
                                ..default()
                            },
                        ));
                        modal.spawn((
                            Text::new(replay_match_id_text(id)),
                            TextFont {
                                font_size: FontSize::Px(DELETE_MATCH_ID_FONT_SIZE),
                                ..default()
                            },
                            TextColor(DELETE_MATCH_ID_COLOR),
                            TextLayout::justify(Justify::Center)
                                .with_linebreak(LineBreak::WordOrCharacter),
                            Node {
                                width: percent(100),
                                margin: UiRect::bottom(px(2)),
                                ..default()
                            },
                        ));
                        if !error.is_empty() {
                            modal.spawn((
                                Text::new(error.clone()),
                                TextFont {
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(DANGER),
                                TextLayout::justify(Justify::Center),
                            ));
                        }
                        modal
                            .spawn(Node {
                                column_gap: px(12),
                                margin: UiRect::top(px(4)),
                                ..default()
                            })
                            .with_children(|actions| {
                                // Cancel seeds although it is drawn second:
                                // the ring must not open on the destructive
                                // button - see the ring module's docs.
                                let mut ring = RingOrder::modal();
                                let cancel = ring.next();
                                let delete_permanently = ring.next();
                                spawn_library_button(
                                    actions,
                                    view,
                                    LibraryAction::ConfirmDelete,
                                    "Delete Permanently",
                                    180.0,
                                    38.0,
                                    delete_permanently,
                                );
                                spawn_library_button(
                                    actions,
                                    view,
                                    LibraryAction::CancelDialog,
                                    "Cancel",
                                    100.0,
                                    38.0,
                                    cancel,
                                );
                            });
                    }
                });
        });
}

fn delete_confirmation_question(label: &str) -> String {
    format!("Permanently delete \"{label}\"?")
}

fn spawn_library_button(
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

fn library_button_node(width: f32, height: f32) -> Node {
    Node {
        width: px(width),
        height: px(height),
        padding: UiRect::horizontal(px(8)),
        border: UiRect::all(px(1)),
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
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(TEXT_PRIMARY),
        text_layout,
    ));
}

#[cfg(test)]
mod tests {
    use super::super::super::test_support::fixed_metadata_replay;
    use super::super::confirm_delete;
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

    #[test]
    fn delete_confirmation_keeps_full_match_id_separate_and_deemphasized() {
        assert_eq!(
            delete_confirmation_question("Sample Replay"),
            "Permanently delete \"Sample Replay\"?"
        );
        assert_eq!(
            replay_match_id_text(&fixed_metadata_replay().id),
            "Match ID: 00000000-0000-4000-8000-000000000000"
        );
        const { assert!(DELETE_MATCH_ID_FONT_SIZE < DELETE_CONFIRMATION_FONT_SIZE) };
        assert_eq!(DELETE_MATCH_ID_COLOR, TEXT_MUTED);
        assert_eq!(DELETE_MATCH_ID_COLOR, Color::srgb(0.68, 0.72, 0.76));
    }

    #[test]
    fn delete_dialog_omits_the_removed_irreversible_warning() {
        // Every `.rs` file in this directory, read from disk at test time rather
        // than from a hand-written `include_str!` list - see
        // `crate::architecture` for why a list cannot guard a negative
        // assertion, and why the count is a floor the walk must reach.
        let source = sources_in("ui/replay/library", 2);
        let removed_warning = ["This removes only this replay", " and cannot be undone."].concat();

        assert!(!source.contains(&removed_warning));
        assert_eq!(
            delete_confirmation_question("Sample Replay"),
            "Permanently delete \"Sample Replay\"?"
        );
        assert!(source.contains("\"Delete Permanently\""));
        assert!(source.contains("\"Cancel\""));
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

    /// Every card on show, first to last: its box, its thumbnail's and the
    /// list's.
    fn cards(app: &mut App) -> Vec<(Rect, Rect, Rect)> {
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
                    node_rect(app, list),
                )
            })
            .collect()
    }

    /// At every named size, and in three windows where a line of notes
    /// costs a card, under real layout and real picking, the Library in
    /// every state a page can be in - no Replays, one, a full page, more
    /// than a page, a full page of the longest labels a Replay can have, a
    /// browser's store with its retention note, a Replay that could not be
    /// read - and with each dialog open over a full page:
    /// every text stands inside the window and, with no dialog open, no two
    /// meet; the page shows as many cards as the layout says, each inside
    /// the list, none meeting another, each its thumbnail whole at 108 px
    /// and as wide as the layout makes it, and the ring reaching them in
    /// the order they are drawn, row by row; the list stands as wide as the
    /// layout holds it, centred; the pointer reaches every control shown;
    /// and a dialog's overlay covers the window, its buttons reached and
    /// everything beneath covered.
    #[test]
    fn every_page_state_fits_at_every_size() {
        use super::super::geometry::library_layout;
        use crate::app::visuals::NAMED_WINDOW_SIZES;
        use crate::ui::button::{covered_at_their_centres, reachable_buttons};
        use crate::ui::focus::{drawn_texts, laid_out_texts};

        // A label of the 60 characters a label can have, in words that wrap
        // onto two lines in the narrowest card.
        const LONGEST_LABEL: &str = "Club night game with an early capture and a long ending race";
        assert_eq!(LONGEST_LABEL.len(), 60);
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
                ("no Replays", 0, None, false, false),
                ("one", 1, None, false, false),
                ("a full page", page, None, false, false),
                ("more than a page", page + 3, None, false, false),
                ("the longest labels", page, None, false, true),
                ("a browser's store", 3, Some(3), false, false),
                ("a Replay not read", page + 3, None, true, false),
            ];
            for (state, replays, retention, warning, longest) in states {
                let context = format!("{window}, {state}");
                let mut app = library_entered_at(window, replays, retention, warning);
                if longest {
                    for entry in &mut app.world_mut().resource_mut::<ReplayLibrary>().entries {
                        entry.label = LONGEST_LABEL.into();
                    }
                    app.world_mut().resource_mut::<ReplayLibraryUiState>().dirty = true;
                    app.update();
                    app.update();
                    let world = app.world_mut();
                    let shown = world
                        .query::<&Text>()
                        .iter(world)
                        .filter(|text| text.0 == LONGEST_LABEL)
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
                for (index, (card, thumbnail, list)) in shown.iter().enumerate() {
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
                    for (other, _, _) in &shown[index + 1..] {
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
