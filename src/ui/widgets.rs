//! The two reusable button spawners, and the label a selected member of a
//! group wears.
//!
//! Both attach the [`UiSound`] cue implied by the button's [`HudAction`], so a
//! screen never has to remember which cue a given intent should make; see
//! [`super::hud::hud_action_ui_cue`].
//!
//! [`spawn_menu_button`] is the large home-menu button and can be spawned
//! disabled (no [`Button`], no action, muted colours). [`spawn_child_button`]
//! is the compact button used inside panels, modals and footers. Both take
//! the caller's optional [`Focusable`] marking rather than deriving one, so
//! each screen's layout stays the author of its own focus ring.
//!
//! [`selected_label`] is here because two screens draw a group of which
//! exactly one member is selected, and they should mark it the same way.

use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::focus::Focusable;
use super::hud::{HudAction, hud_action_ui_cue};
use crate::audio::UiSound;
use crate::presentation::{
    BLUE_BORDER, BUTTON_BACKGROUND, BUTTON_DISABLED, BUTTON_DISABLED_BORDER, TEXT_MUTED,
    TEXT_PRIMARY,
};

/// `focus` is the caller's [`Focusable`] marking, passed through rather than
/// derived here so the owning screen's layout stays the author of what is in
/// its focus ring. It is ignored on a disabled or action-less button, which
/// can do nothing when pressed.
pub(super) fn spawn_menu_button(
    parent: &mut ChildSpawnerCommands,
    action: Option<HudAction>,
    label: &'static str,
    enabled: bool,
    focus: Option<Focusable>,
) {
    let background = if enabled {
        BUTTON_BACKGROUND
    } else {
        BUTTON_DISABLED
    };
    let border = if enabled {
        BLUE_BORDER
    } else {
        BUTTON_DISABLED_BORDER
    };
    let text = if enabled { TEXT_PRIMARY } else { TEXT_MUTED };

    let mut button = parent.spawn((
        Node {
            width: px(390),
            height: px(54),
            border: UiRect::all(px(1)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(background),
        BorderColor::all(border),
    ));
    if let Some(action) = action {
        button.insert((Button, action, UiSound::new(hud_action_ui_cue(action))));
        if enabled && let Some(focus) = focus {
            button.insert(focus);
        }
    }
    button.with_child((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(19.0),
            ..default()
        },
        TextColor(text),
    ));
}

/// `focus` is the caller's [`Focusable`] marking, passed through for the
/// same reason [`spawn_menu_button`] takes it: the owning screen's layout
/// authors its own ring. `None` keeps the button pointer-only, which is
/// what the Return Home confirmation's two buttons are - not because the
/// Match screen can never carry a ring (it can now, while a modal is open),
/// but because that dialog is driven directly by `ui::focus`'s
/// `handle_match_return`, which reads Escape, Start, Enter and South itself.
/// `apply_focus_navigation` stands the ring down for that one dialog on
/// purpose; marking these two would put both mechanisms on the same
/// buttons.
pub(super) fn spawn_child_button(
    parent: &mut ChildSpawnerCommands,
    action: HudAction,
    label: &'static str,
    width: f32,
    focus: Option<Focusable>,
) {
    let mut button = parent.spawn((
        Button,
        UiSound::new(hud_action_ui_cue(action)),
        Node {
            width: px(width),
            height: px(36),
            border: UiRect::all(px(1)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BUTTON_BACKGROUND),
        BorderColor::all(BLUE_BORDER),
        action,
    ));
    if let Some(focus) = focus {
        button.insert(focus);
    }
    button.with_child((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(TEXT_PRIMARY),
    ));
}

/// The label of one member of a group of which exactly one is selected -
/// the Replay Viewer's speeds, a Settings volume row - bracketed when it is
/// the selected one: `[1x]` beside `0.5x` and `2x`.
///
/// The selected member's fill and border differ from the others' in hue
/// alone - 1.52:1 and 1.44:1, one colour to a player who cannot see hue -
/// so its label has to change with the state, as `On` and `Off` do on a
/// toggle. Brackets rather than a single leading mark, because both groups
/// are fixed-width buttons with centred labels in a monospace font: the
/// label's own characters stay exactly where they were and the brackets
/// appear either side of them, where a mark on one side would push the
/// label half a character off centre.
pub(super) fn selected_label(label: &str, selected: bool) -> String {
    if selected {
        format!("[{label}]")
    } else {
        label.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::selected_label;

    #[test]
    fn only_the_selected_member_of_a_group_is_bracketed() {
        assert_eq!(selected_label("1x", true), "[1x]");
        assert_eq!(selected_label("1x", false), "1x");
        assert_eq!(selected_label("Medium", true), "[Medium]");
        assert_eq!(selected_label("Medium", false), "Medium");
        // Two characters, one either side, so the label stays centred.
        for label in ["0.5x", "Mute"] {
            let selected = selected_label(label, true);
            assert_eq!(selected.len(), label.len() + 2);
            assert_eq!(&selected[1..=label.len()], label);
        }
    }
}
