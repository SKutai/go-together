//! The Library's two dialogs: Change Replay Label, which renames a Replay,
//! and Delete Replay?, which asks before deleting one for good.
//!
//! Either is spawned over the page by [`super::layout`] while the screen's
//! state holds it, and rebuilt with the rest of the screen on every change
//! (see the respawn model in [`super`]); what opening, confirming and
//! cancelling one does is [`super`]'s, and where the ring seeds in each is
//! [`super::ring`]'s. They are a module of their own for their size rather
//! than their kind, as the Replay Viewer's overlays are: they were a fifth
//! of the screen's spawn code, and none of it has to do with the page of
//! cards under them.

use bevy::prelude::*;

use super::layout::{LibraryView, spawn_library_button};
use super::ring::RingOrder;
use super::{LibraryAction, LibraryDialog, replay_match_id_text};
use crate::presentation::{
    BLUE_BORDER, DANGER, ORANGE, OVERLAY_BACKGROUND, PANEL_BACKGROUND, TEXT_FIELD_BACKGROUND,
    TEXT_HEADING, TEXT_MUTED, TEXT_PRIMARY,
};
use crate::replay::REPLAY_LABEL_RULE;

const DELETE_CONFIRMATION_FONT_SIZE: f32 = 18.0;
const DELETE_MATCH_ID_FONT_SIZE: f32 = 14.0;
const DELETE_MATCH_ID_COLOR: Color = TEXT_MUTED;

pub(super) fn spawn_library_dialog(
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

#[cfg(test)]
mod tests {
    use super::super::super::test_support::fixed_metadata_replay;
    use super::*;
    use crate::architecture::sources_in;

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
}
