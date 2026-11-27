//! Spawns the library screen: the toolbar, the current page of replay cards,
//! the pagination row and the rename and delete dialogs.
//!
//! Nothing here keeps state. Every helper takes the current
//! [`ReplayLibraryUiState`] and [`ReplayLibrary`] and builds from scratch,
//! because the screen is rebuilt wholesale on every change - see the respawn
//! model in [`super`]. That is also why the sizing constants and the toolbar's
//! section order live here: they describe furniture, and nothing outside
//! spawning reads them.
//!
//! `ROWS_PER_PAGE` is the one measurement that stays in [`super`]. It is not
//! furniture but paging state, clamped and scrolled by the update systems.

use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Button;

use super::ring::{
    HOME_FOCUS_ORDER, PAGINATION_FOCUS_ORDER, RingOrder, TOOLBAR_FOCUS_ORDER, card_focus_order,
};
use super::{
    LibraryAction, LibraryDialog, ROWS_PER_PAGE, ReplayLibraryUiState, library_action_disabled,
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
const LIBRARY_SCREEN_PADDING: f32 = 24.0;
/// A replay card's height, and so the height of the slot an empty list's
/// message stands in - see [`spawn_empty_list_message`].
const REPLAY_CARD_HEIGHT: f32 = 142.0;
/// What the list says when the Library holds no Replays at all.
const NO_REPLAYS_MESSAGE: &str =
    "No completed match replays yet.\nFinish and score a match to save it automatically.";
/// What the list says when the search matches none of the Replays there are.
const NO_SEARCH_MATCH_MESSAGE: &str = "No replay labels match this search.";
const TOOLBAR_COLUMN_GAP: f32 = 10.0;
const TOOLBAR_MIN_SPACER_WIDTH: f32 = 14.0;
const SEARCH_CONTROL_WIDTH: f32 = 500.0;
const CLEAR_CONTROL_WIDTH: f32 = 74.0;
const SORT_FIELD_CONTROL_MIN_WIDTH: f32 = 210.0;
const SORT_ORDER_CONTROL_WIDTH: f32 = 126.0;
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
/// spawner reads both: `library` for what to show and `ui` for how - and
/// the button spawner reads both again to decide, at spawn, whether a
/// control is disabled.
#[derive(Clone, Copy)]
struct LibraryView<'a> {
    ui: &'a ReplayLibraryUiState,
    library: &'a ReplayLibrary,
}

pub(super) fn spawn_library_contents(
    commands: &mut Commands,
    ui: &ReplayLibraryUiState,
    library: &ReplayLibrary,
) {
    let view = LibraryView { ui, library };
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
                padding: UiRect::all(px(LIBRARY_SCREEN_PADDING)),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(APP_BACKGROUND),
            DespawnOnExit(AppScreen::ReplayLibrary),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: percent(100),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|header| {
                spawn_library_button(
                    header,
                    view,
                    LibraryAction::Home,
                    "Home",
                    90.0,
                    38.0,
                    RingOrder::starting_at(HOME_FOCUS_ORDER).next(),
                );
                header.spawn((
                    Text::new("Match Replays"),
                    TextFont {
                        font_size: FontSize::Px(38.0),
                        ..default()
                    },
                    TextColor(TEXT_HEADING),
                ));
                header.spawn(Node {
                    width: px(90),
                    ..default()
                });
            });

            root.spawn(library_toolbar_node())
                .with_children(|controls| {
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
                                controls.spawn(library_toolbar_group_node()).with_children(
                                    |search| {
                                        spawn_library_button(
                                            search,
                                            view,
                                            LibraryAction::FocusSearch,
                                            &search_label,
                                            SEARCH_CONTROL_WIDTH,
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
                                    },
                                );
                            }
                            LibraryToolbarSection::FlexibleSpacer => {
                                controls.spawn(library_toolbar_spacer_node());
                            }
                            LibraryToolbarSection::Sort => {
                                controls.spawn(library_toolbar_group_node()).with_children(
                                    |sort| {
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
                                    },
                                );
                            }
                        }
                    }
                });

            if let Some(note) = replay_retention_text(library) {
                root.spawn((
                    Text::new(note),
                    TextFont {
                        font_size: FontSize::Px(14.0),
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
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(DANGER),
                    Node {
                        width: percent(100),
                        ..default()
                    },
                ));
            }

            root.spawn((
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(DEEP_SURFACE_BACKGROUND),
            ))
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
                    for (row, replay) in filtered
                        .iter()
                        .skip(ui.scroll_offset)
                        .take(ROWS_PER_PAGE)
                        .enumerate()
                    {
                        spawn_replay_row(list, view, replay, row);
                    }
                }
            });

            root.spawn(Node {
                width: percent(100),
                height: px(40),
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
                    Text::new(page_indicator_text(ui.scroll_offset, filtered.len())),
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
        height: px(REPLAY_CARD_HEIGHT),
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

fn page_indicator_text(scroll_offset: usize, filtered_count: usize) -> String {
    if filtered_count == 0 {
        "0 of 0".into()
    } else {
        format!(
            "{}-{} of {}",
            scroll_offset + 1,
            (scroll_offset + ROWS_PER_PAGE).min(filtered_count),
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

/// Spawns one card. `row` is its position on the page, which decides its
/// three buttons' places in the ring - see [`card_focus_order`].
fn spawn_replay_row(
    parent: &mut ChildSpawnerCommands,
    view: LibraryView,
    replay: &ReplayCatalogEntry,
    row: usize,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(REPLAY_CARD_HEIGHT),
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
                ..default()
            })
            .with_children(|actions| {
                let mut ring = RingOrder::starting_at(card_focus_order(row));
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
    let disabled = library_action_disabled(&action, view.ui, view.library);
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

    #[test]
    fn page_indicator_reports_visible_replay_range() {
        assert_eq!(page_indicator_text(0, 0), "0 of 0");
        assert_eq!(page_indicator_text(0, 10), "1-4 of 10");
        assert_eq!(page_indicator_text(6, 10), "7-10 of 10");
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

    #[test]
    fn replay_toolbar_minimum_width_fits_normal_window() {
        let control_widths = SEARCH_CONTROL_WIDTH
            + CLEAR_CONTROL_WIDTH
            + SORT_FIELD_CONTROL_MIN_WIDTH
            + SORT_ORDER_CONTROL_WIDTH;
        let gaps = TOOLBAR_COLUMN_GAP * 2.0 + TOOLBAR_MIN_SPACER_WIDTH;
        let available_width =
            crate::app::visuals::WINDOW_WIDTH as f32 - LIBRARY_SCREEN_PADDING * 2.0;

        assert!(control_widths + gaps <= available_width);
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
}
