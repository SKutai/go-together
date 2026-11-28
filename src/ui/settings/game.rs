//! The Game tab: the Match-rules panel, and the numeric editing behind it.
//!
//! This is the only tab whose widgets are not just pressed. Each limit has a
//! text field the player types into, so the tab carries an editing session
//! ([`GameEditor`]) and a commit step that can *fail* - the two things the
//! other tabs have no equivalent of. Spawning the rows and running that
//! editing session live together here because the row shape and the parse
//! rules are one design: which fields share a [`MatchSettingField`], which
//! ones are a minutes/seconds pair, and where the per-row error line goes.
//! The per-frame refresh stays in [`super::display`] with the rest of the
//! screen's display pass.
//!
//! # Editing is a session, not a keystroke
//!
//! Activating a field - a click, or Enter, Space or a pad's South on the
//! focused one - opens an editor seeded from the saved value; typing only
//! changes the editor. The value reaches [`MatchSettings`] on *commit* -
//! Enter, a click elsewhere, a tab switch, or a reset request - and a commit
//! that will not parse keeps the editor open with a per-row message rather
//! than discarding what was typed. Escape abandons the session and is handled
//! with the screen's other navigation in [`super`].
//!
//! Only digits are accepted, so the editor never has to reject a committed
//! string for containing letters, and a value outside the field's range is
//! clamped into it rather than rejected ([`parse_digits_clamped`],
//! [`parse_duration_parts`]). What commit can still reject is empty input,
//! which is why it returns a [`NumericParseError`].
//!
//! A keystroke the field will not take is refused out loud, not dropped:
//! [`apply_numeric_editor_input`] reports it as
//! [`TextInputOutcome::Rejected`] and the per-row error line says
//! [`NumericParseError::NotDigits`]. Both of the rules that can refuse one
//! matter to a player - the shared editor takes printable ASCII only, and
//! this field narrows that to digits - and neither is visible from the
//! keyboard: a full-width `3` from an IME and an Arabic-Indic digit both look
//! like the number that was typed.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use super::geometry::SettingsLayout;
use super::layout::{SettingsPlacement, placed};
use super::ring::{RingOrder, panel_focus_order};
use super::{
    ControlMappingState, GameInputError, GameInputValue, GameNumericInput, GameToggle,
    GameToggleValue, MappingTab, MatchMinimumLabel, SettingsValues, persist_current_settings,
};
use crate::audio::{AudioCueQueue, UiCue, UiSound};
use crate::match_settings::{
    MatchSettingField, MatchSettings, NumericParseError, duration_parts, field_minimum,
    parse_digits_clamped, parse_duration_parts,
};
use crate::presentation::{
    BLUE_BORDER, BUTTON_BACKGROUND, DANGER, SURFACE_BACKGROUND, TEXT_MUTED, TEXT_PRIMARY,
};
use crate::ui::focus::Focusable;
use crate::ui::text_input::{TextEditor, TextInputOutcome, apply_editor_input};

/// One numeric field being edited: which input it belongs to, and the caret
/// and text of the session in progress.
#[derive(Clone, Debug)]
pub(super) struct GameEditor {
    pub(super) input: GameNumericInput,
    pub(super) editor: TextEditor,
}

pub(super) fn game_input_text(input: GameNumericInput, settings: &MatchSettings) -> String {
    let value = settings.value(input.field());
    match input {
        GameNumericInput::RoundTimeMinutes | GameNumericInput::MatchTimeMinutes => {
            duration_parts(value).0.to_string()
        }
        GameNumericInput::RoundTimeSeconds | GameNumericInput::MatchTimeSeconds => {
            format!("{:02}", duration_parts(value).1)
        }
        _ => value.to_string(),
    }
}

/// Applies one key to a numeric field, narrowing the shared editor's rule to
/// digits.
///
/// The edit is tried on a clone so a keystroke this field will not take can be
/// refused without the editor ever holding it. Such a keystroke is
/// [`TextInputOutcome::Rejected`], the same outcome
/// [`apply_editor_input`] uses for a character with no printable ASCII form,
/// because the two are one thing to the player: the key did nothing and the
/// field owes an explanation.
fn apply_numeric_editor_input(
    editor: &mut TextEditor,
    key: &bevy::input::keyboard::Key,
    command_down: bool,
) -> TextInputOutcome {
    let mut candidate = editor.clone();
    let outcome = apply_editor_input(&mut candidate, key, command_down);
    if outcome == TextInputOutcome::Changed
        && !candidate.text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return TextInputOutcome::Rejected;
    }
    if matches!(
        outcome,
        TextInputOutcome::Changed | TextInputOutcome::Consumed
    ) {
        *editor = candidate;
    }
    outcome
}

/// The per-row error line for `input`, scoped by the setting's own label so
/// the message reads the same whether a commit or a single keystroke produced
/// it.
fn game_error_message(input: GameNumericInput, error: NumericParseError) -> String {
    format!("{}: {error}.", input.field().label())
}

fn commit_game_editor(
    match_settings: &mut MatchSettings,
    mapping: &mut ControlMappingState,
) -> Result<bool, NumericParseError> {
    let Some(active) = mapping.game_editor.take() else {
        return Ok(false);
    };
    let field = active.input.field();
    let value = match active.input {
        GameNumericInput::RoundTimeMinutes | GameNumericInput::MatchTimeMinutes => {
            let (_, seconds) = duration_parts(match_settings.value(field));
            parse_duration_parts(
                &active.editor.text,
                &seconds.to_string(),
                field_minimum(field, match_settings),
            )
        }
        GameNumericInput::RoundTimeSeconds | GameNumericInput::MatchTimeSeconds => {
            let (minutes, _) = duration_parts(match_settings.value(field));
            parse_duration_parts(
                &minutes.to_string(),
                &active.editor.text,
                field_minimum(field, match_settings),
            )
        }
        _ => parse_digits_clamped(
            &active.editor.text,
            field_minimum(field, match_settings),
            field.maximum(),
        ),
    };
    match value {
        Ok(value) => {
            let before = match_settings.clone();
            match_settings.set_value(field, u64::from(value));
            if mapping
                .game_error
                .as_ref()
                .is_some_and(|(input, _)| input.field() == field)
            {
                mapping.game_error = None;
            }
            Ok(*match_settings != before)
        }
        Err(error) => {
            mapping.game_error = Some((active.input, game_error_message(active.input, error)));
            mapping.game_editor = Some(active);
            Err(error)
        }
    }
}

pub(super) fn commit_active_game_editor(
    settings: &mut SettingsValues,
    mapping: &mut ControlMappingState,
) {
    if matches!(
        commit_game_editor(&mut settings.match_settings, mapping),
        Ok(true)
    ) {
        persist_current_settings(settings, mapping);
    }
}

pub(super) fn capture_game_numeric_input(
    mut keyboard_events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    chord: Option<Res<crate::app::window::FullscreenChord>>,
    mut settings: SettingsValues,
    mut mapping: ResMut<ControlMappingState>,
    mut audio: ResMut<AudioCueQueue>,
    mut active: Local<Option<GameNumericInput>>,
) {
    let editing = if mapping.reset_confirmation_open {
        None
    } else {
        mapping.game_editor.as_ref().map(|editor| editor.input)
    };
    // The rising-edge guard `capture_controller_mapping` keeps: the frame
    // this editor opens, the messages already in flight - including the
    // Enter press that a keyboard-focus activation used to open it, which
    // would otherwise commit and close the session immediately - are
    // drained unread. A pointer click sends no keyboard message; the one
    // pointer-visible shift is that a keystroke from the click's own frame,
    // or buffered unread just before it, no longer lands in a field it
    // predates. Every close reaches this edge: a commit re-reads the
    // session before this system leaves (below), and Escape closes it in
    // `handle_mapping_escape`, which runs *before* this system in the chain,
    // so the close is already visible here on the frame it happens.
    let just_armed = editing.is_some() && *active != editing;
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
        if mapping.game_editor.is_none() {
            break;
        }
        // The F of Control-Command-F, on a Mac, is the fullscreen chord's, and
        // so are its repeats once Control and Command are let go
        // (`app::window`): no character for the field, and no error.
        if event.state != ButtonState::Pressed
            || matches!(event.logical_key, bevy::input::keyboard::Key::Escape)
            || chord
                .as_ref()
                .is_some_and(|chord| chord.takes(event.key_code, &keys))
        {
            continue;
        }
        let outcome = {
            let active = mapping.game_editor.as_mut().expect("checked above");
            apply_numeric_editor_input(&mut active.editor, &event.logical_key, command_down)
        };
        match outcome {
            TextInputOutcome::Ignored | TextInputOutcome::Consumed => {}
            TextInputOutcome::Rejected => {
                let active_input = mapping.game_editor.as_ref().map(|active| active.input);
                if let Some(input) = active_input {
                    mapping.game_error = Some((
                        input,
                        game_error_message(input, NumericParseError::NotDigits),
                    ));
                }
            }
            TextInputOutcome::Changed => {
                let active_field = mapping
                    .game_editor
                    .as_ref()
                    .map(|active| active.input.field());
                if mapping
                    .game_error
                    .as_ref()
                    .is_some_and(|(input, _)| Some(input.field()) == active_field)
                {
                    mapping.game_error = None;
                }
            }
            TextInputOutcome::Submit => {
                match commit_game_editor(&mut settings.match_settings, &mut mapping) {
                    Ok(changed) => {
                        if changed {
                            persist_current_settings(&mut settings, &mut mapping);
                        }
                        audio.ui(UiCue::Confirm);
                    }
                    Err(_) => audio.ui(UiCue::Cancel),
                }
                break;
            }
            TextInputOutcome::Cancel => unreachable!("Escape is handled by navigation"),
        }
    }
    // Re-read the session before leaving: a Submit above can close the
    // editor mid-loop, and if the next frame's `handle_mapping_buttons`
    // reopens the same input, a stale `Some` here would skip that arming
    // frame and let the reopening key press straight into the fresh editor.
    *active = mapping.game_editor.as_ref().map(|editor| editor.input);
}

/// Spawns the Match-rules rows: one per optional limit, then the
/// capture-points switch.
///
/// Each limit is a checkbox plus its value, and a duration is two fields
/// sharing one [`MatchSettingField`], which is why the row helpers below are
/// parameterised by [`GameNumericInput`] rather than by field: the editing
/// session above is per *input*, not per setting. The ring walks each row
/// left to right - checkbox, then its field or minutes-and-seconds pair -
/// and the rows top to bottom, one [`RingOrder`] threaded through the lot.
/// The rows are as tall as `layout` makes them: closer together in the
/// sidebar, so that the tab fits the smallest window (see
/// [`super::geometry`]).
pub(super) fn spawn_game_panel(panel: &mut ChildSpawnerCommands, layout: &SettingsLayout) {
    let mut ring = RingOrder::starting_at(panel_focus_order(MappingTab::Game));
    panel
        .spawn((
            Node {
                width: px(760),
                max_height: px(535),
                padding: UiRect::all(px(14)),
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SURFACE_BACKGROUND),
        ))
        .with_children(|game| {
            spawn_integer_game_row(
                game,
                layout,
                MatchSettingField::RoundLimit,
                GameNumericInput::RoundLimit,
                "End after this many ordinary rounds.",
                &mut ring,
            );
            spawn_duration_game_row(
                game,
                layout,
                GameNumericInput::RoundTimeMinutes,
                GameNumericInput::RoundTimeSeconds,
                "Maximum time for each ordinary round.",
                false,
                &mut ring,
            );
            spawn_integer_game_row(
                game,
                layout,
                MatchSettingField::NoStoneRoundLimit,
                GameNumericInput::NoStoneRoundLimit,
                "End after consecutive ordinary rounds where the board does not change.",
                &mut ring,
            );
            spawn_duration_game_row(
                game,
                layout,
                GameNumericInput::MatchTimeMinutes,
                GameNumericInput::MatchTimeSeconds,
                "Maximum total match time.",
                true,
                &mut ring,
            );
            spawn_integer_game_row(
                game,
                layout,
                MatchSettingField::PointTarget,
                GameNumericInput::PointTarget,
                "End when either player reaches this score.",
                &mut ring,
            );
            spawn_capture_points_row(game, layout, &mut ring);
        });
}

fn spawn_integer_game_row(
    parent: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    field: MatchSettingField,
    input: GameNumericInput,
    description: &'static str,
    ring: &mut RingOrder,
) {
    spawn_game_row(
        parent,
        layout,
        GameToggle::Limit(field),
        field.label(),
        description,
        ring,
        |controls, ring| {
            spawn_numeric_input(controls, input, 92.0, ring.next());
        },
    );
}

/// A limit set as minutes and seconds: the row for the field the two inputs
/// edit.
fn spawn_duration_game_row(
    parent: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    minutes: GameNumericInput,
    seconds: GameNumericInput,
    description: &'static str,
    show_minimum: bool,
    ring: &mut RingOrder,
) {
    let field = minutes.field();
    debug_assert_eq!(seconds.field(), field, "both inputs edit one field");
    spawn_game_row(
        parent,
        layout,
        GameToggle::Limit(field),
        field.label(),
        description,
        ring,
        |controls, ring| {
            spawn_numeric_input(controls, minutes, 62.0, ring.next());
            controls.spawn((
                Text::new(":"),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(TEXT_PRIMARY),
            ));
            spawn_numeric_input(controls, seconds, 62.0, ring.next());
            let mut label = controls.spawn((
                Text::new(if show_minimum { "" } else { "MM:SS" }),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            if show_minimum {
                label.insert(MatchMinimumLabel);
            }
        },
    );
}

fn spawn_game_row(
    parent: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    toggle: GameToggle,
    label: &'static str,
    description: &'static str,
    ring: &mut RingOrder,
    spawn_controls: impl FnOnce(&mut ChildSpawnerCommands, &mut RingOrder),
) {
    parent
        .spawn(placed(
            layout,
            SettingsPlacement::GameRow,
            Node {
                width: percent(100),
                padding: UiRect::axes(px(10), px(5)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: px(455),
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(17.0),
                        ..default()
                    },
                    TextColor(TEXT_PRIMARY),
                ));
                copy.spawn((
                    Text::new(description),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                ));
                copy.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(DANGER),
                    GameInputError(match toggle {
                        GameToggle::Limit(MatchSettingField::RoundLimit) => {
                            GameNumericInput::RoundLimit
                        }
                        GameToggle::Limit(MatchSettingField::RoundTime) => {
                            GameNumericInput::RoundTimeMinutes
                        }
                        GameToggle::Limit(MatchSettingField::NoStoneRoundLimit) => {
                            GameNumericInput::NoStoneRoundLimit
                        }
                        GameToggle::Limit(MatchSettingField::MatchTime) => {
                            GameNumericInput::MatchTimeMinutes
                        }
                        GameToggle::Limit(MatchSettingField::PointTarget) => {
                            GameNumericInput::PointTarget
                        }
                        GameToggle::CapturePoints => unreachable!(),
                    }),
                ));
            });
            // The controls never give way: a duration row's are wider than
            // the row leaves beside the copy once its caption reads
            // `Min 60:00`, and flexbox would squeeze every child to fit,
            // wrapping the caption onto two lines. The copy wraps instead,
            // which it can do gracefully.
            row.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(7),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|controls| {
                spawn_game_checkbox(controls, toggle, ring.next());
                spawn_controls(controls, ring);
            });
        });
}

fn spawn_game_checkbox(parent: &mut ChildSpawnerCommands, toggle: GameToggle, focus: Focusable) {
    parent
        .spawn((
            Button,
            toggle,
            UiSound::new(UiCue::Neutral),
            focus,
            Node {
                width: px(36),
                height: px(36),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BLUE_BORDER),
        ))
        .with_child((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            GameToggleValue(toggle),
        ));
}

fn spawn_numeric_input(
    parent: &mut ChildSpawnerCommands,
    input: GameNumericInput,
    width: f32,
    focus: Focusable,
) {
    parent
        .spawn((
            Button,
            input,
            UiSound::new(UiCue::Neutral),
            focus,
            Node {
                width: px(width),
                height: px(36),
                padding: UiRect::horizontal(px(8)),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BLUE_BORDER),
        ))
        .with_child((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(17.0),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            GameInputValue(input),
        ));
}

fn spawn_capture_points_row(
    parent: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    ring: &mut RingOrder,
) {
    parent
        .spawn(placed(
            layout,
            SettingsPlacement::LastGameRow,
            Node {
                width: percent(100),
                padding: UiRect::axes(px(10), px(5)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn((
                    Text::new("Count Captured Stones as Points"),
                    TextFont {
                        font_size: FontSize::Px(17.0),
                        ..default()
                    },
                    TextColor(TEXT_PRIMARY),
                ));
                copy.spawn((
                    Text::new("Add captured stones to each player's score."),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                ));
            });
            spawn_game_checkbox(row, GameToggle::CapturePoints, ring.next());
        });
}

#[cfg(test)]
mod tests {
    use crate::ui::button::{UiButtonPlugin, trigger_activation};
    use bevy::input::keyboard::{Key, KeyboardInput};

    use super::super::display::update_game_settings_display;
    use super::super::{MappingTab, SettingsStore};
    use super::*;
    use crate::input::GameAction;
    use crate::input::controller::ControllerBindings;
    use crate::input::keyboard::{KeyboardBindings, KeyboardKey};
    use crate::platform::{SettingsStorage, SettingsStorageError};
    use crate::presentation::{BUTTON_DISABLED, TEXT_PRIMARY};
    use crate::settings::{ControlsConfig, GamePreferences};

    /// A settings backend for tests that only need the resource to exist.
    /// A refused keystroke never commits, so nothing here is ever called with
    /// anything to keep.
    struct DiscardingSettingsStorage;

    impl SettingsStorage for DiscardingSettingsStorage {
        fn load(&self) -> Result<Option<String>, SettingsStorageError> {
            Ok(None)
        }

        fn save(&self, _json: &str) -> Result<(), SettingsStorageError> {
            Ok(())
        }
    }

    /// Writes one logical key press and runs a frame.
    ///
    /// `crate::test_support::send_keyboard_event` cannot serve here: it always
    /// sends `Key::Character("a")`, and the logical key is the whole point of
    /// these tests.
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

    #[test]
    fn numeric_editor_accepts_digits_and_keeps_caret_editing_semantics() {
        let mut editor = TextEditor::new("13".into());
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::ArrowLeft, false),
            TextInputOutcome::Consumed
        );
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Character("2".into()), false),
            TextInputOutcome::Changed
        );
        assert_eq!(editor.text, "123");
        assert_eq!(editor.caret, 2);
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Home, false),
            TextInputOutcome::Consumed
        );
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Delete, false),
            TextInputOutcome::Changed
        );
        assert_eq!(editor.text, "23");
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Character("4x".into()), false),
            TextInputOutcome::Rejected
        );
        assert_eq!(editor.text, "23");
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::End, false),
            TextInputOutcome::Consumed
        );
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Backspace, false),
            TextInputOutcome::Changed
        );
        assert_eq!(editor.text, "2");
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Enter, false),
            TextInputOutcome::Submit
        );
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Escape, false),
            TextInputOutcome::Cancel
        );
    }

    /// Both ways this field can refuse a keystroke, and the reason they share
    /// one outcome. The shared editor rejects anything that is not printable
    /// ASCII; this field then rejects printable ASCII that is not a digit.
    /// The second is the pedestrian one - a letter in a number field - but the
    /// first is where a keystroke can look valid and do nothing, because a
    /// full-width or Arabic-Indic digit reads as the number the player typed.
    #[test]
    fn numeric_editor_rejects_non_ascii_and_non_digit_input_alike() {
        for text in [
            // Non-ASCII: refused by the shared editor.
            "\u{ff13}", "\u{0663}", "\u{e9}",
            // Printable ASCII that is not a digit: refused by this field.
            "x", ".", ":", "-",
        ] {
            let mut editor = TextEditor::new("12".into());

            let outcome =
                apply_numeric_editor_input(&mut editor, &Key::Character(text.into()), false);

            assert_eq!(outcome, TextInputOutcome::Rejected, "{text:?}");
            assert!(outcome.consumed());
            assert_eq!(editor.text, "12");
            assert_eq!(editor.caret, 2);
        }

        // A space is the same refusal by a different key.
        let mut editor = TextEditor::new("12".into());
        assert_eq!(
            apply_numeric_editor_input(&mut editor, &Key::Space, false),
            TextInputOutcome::Rejected
        );
        assert_eq!(editor.text, "12");
    }

    /// The feedback itself, through the real keystroke system and the real
    /// display pass: a refused character has to reach the player as the row's
    /// error line, and the next accepted digit has to take it away again.
    #[test]
    fn a_rejected_keystroke_shows_the_rows_error_line_until_a_digit_clears_it() {
        let mut mapping = ControlMappingState::new(None);
        mapping.tab = MappingTab::Game;
        mapping.game_editor = Some(GameEditor {
            input: GameNumericInput::PointTarget,
            editor: TextEditor::new("1".into()),
        });
        let mut settings = MatchSettings::default();
        settings.set_enabled(MatchSettingField::PointTarget, true);

        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(settings)
            .insert_resource(SettingsStore::from_storage(DiscardingSettingsStorage))
            .insert_resource(mapping)
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(
                Update,
                (capture_game_numeric_input, update_game_settings_display).chain(),
            );
        let error_text = app
            .world_mut()
            .spawn((
                Text::new(""),
                Node::default(),
                GameInputError(GameNumericInput::PointTarget),
            ))
            .id();

        // One frame to arm the capture: the frame an editor opens ignores
        // messages already in flight (see the rising-edge guard), and this
        // test seeds the editor as already open.
        app.update();

        // A full-width digit: it reads as the number the player typed.
        send_logical_key(&mut app, Key::Character("\u{ff13}".into()));

        assert_eq!(
            app.world().get::<Text>(error_text).unwrap().as_str(),
            "Point Target: use digits only."
        );
        assert_eq!(
            app.world().get::<Node>(error_text).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .as_ref()
                .unwrap()
                .editor
                .text,
            "1",
            "a refused keystroke must not reach the editor"
        );

        send_logical_key(&mut app, Key::Character("3".into()));

        assert!(
            app.world()
                .resource::<ControlMappingState>()
                .game_error
                .is_none()
        );
        assert_eq!(app.world().get::<Text>(error_text).unwrap().as_str(), "");
        assert_eq!(
            app.world().get::<Node>(error_text).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .as_ref()
                .unwrap()
                .editor
                .text,
            "13"
        );
    }

    /// On a Mac the F of Control-Command-F is the fullscreen chord's
    /// (`app::window`), and so are its repeats once Control and Command are let
    /// go: a numeric field takes no character from them and shows no error,
    /// where an F alone, with no chord on the platform, is refused as not
    /// digits, as ever.
    #[test]
    fn the_fullscreen_chords_f_is_no_input_for_a_numeric_field() {
        use crate::app::window::FullscreenChord;

        let mut settings = MatchSettings::default();
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(settings)
            .insert_resource(SettingsStore::from_storage(DiscardingSettingsStorage))
            .insert_resource(crate::input::controller::ControllerAssignments::default())
            .insert_resource(ControlMappingState::new(None))
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (
                    super::super::handle_mapping_buttons,
                    capture_game_numeric_input,
                )
                    .chain(),
            );
        app.update();
        let pressed = app.world_mut().spawn(GameNumericInput::RoundLimit).id();
        trigger_activation(&mut app, pressed);
        app.update();
        let f_repeat = |app: &mut App| {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyF,
                logical_key: Key::Character("f".into()),
                state: ButtonState::Pressed,
                text: None,
                repeat: true,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        };
        let state = |app: &App| {
            let mapping = app.world().resource::<ControlMappingState>();
            (
                mapping
                    .game_editor
                    .as_ref()
                    .expect("the editor stays open")
                    .editor
                    .text
                    .clone(),
                mapping.game_error.is_some(),
            )
        };

        // The chord's F, still held after Control and Command were let go.
        app.insert_resource(FullscreenChord::holding_f());
        f_repeat(&mut app);
        f_repeat(&mut app);
        assert_eq!(state(&app), ("100".to_string(), false));

        // No chord here: an F is refused as it always was.
        app.world_mut().remove_resource::<FullscreenChord>();
        f_repeat(&mut app);
        assert_eq!(state(&app), ("100".to_string(), true));
    }

    /// The frame-sharing defect the rising-edge guard exists for, on this
    /// capture: `handle_mapping_buttons` opens the editor and this system
    /// reads the same frame's `KeyboardInput` messages one system later in
    /// the same chain, so the Enter press that activated the field
    /// (reachable once the focus core can press buttons) would arrive as
    /// `Submit` and close the session the instant it opened. The arming
    /// frame also drains messages left unread from earlier frames, which
    /// would otherwise leak into a field that was not open when they were
    /// typed.
    #[test]
    fn the_key_press_that_opens_a_numeric_editor_never_reaches_it() {
        let mut settings = MatchSettings::default();
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(settings)
            .insert_resource(SettingsStore::from_storage(DiscardingSettingsStorage))
            .insert_resource(crate::input::controller::ControllerAssignments::default())
            .insert_resource(ControlMappingState::new(None))
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (
                    super::super::handle_mapping_buttons,
                    capture_game_numeric_input,
                )
                    .chain(),
            );

        // A stray keystroke from before the field existed, never read
        // because no editor was open.
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Digit9,
            logical_key: Key::Character("9".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();

        // The activation frame: the field's button fires and the Enter that
        // fired it is still in this frame's messages.
        let pressed = app.world_mut().spawn(GameNumericInput::RoundLimit).id();
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

        let mapping = app.world().resource::<ControlMappingState>();
        let editor = mapping
            .game_editor
            .as_ref()
            .expect("the editor must survive the frame that opened it");
        assert_eq!(
            editor.editor.text, "100",
            "the stray keystroke and the opening Enter must both be dropped"
        );

        // From the next frame on the session behaves normally: digits land
        // and Enter commits.
        send_logical_key(&mut app, Key::Character("7".into()));
        assert_eq!(
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .as_ref()
                .unwrap()
                .editor
                .text,
            "1007"
        );
        send_logical_key(&mut app, Key::Enter);
        assert!(
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .is_none(),
            "a fresh Enter still commits"
        );
    }

    /// Every way a session can close must re-arm the guard for the next
    /// one: a commit closes mid-run (the `Local` is re-read before the
    /// system leaves), and Escape closes in `handle_mapping_escape`, which
    /// runs before this system in the production chain - the order this
    /// test uses - so the close is visible here the same frame. Either way,
    /// reopening the same input the very next frame is a fresh session, and
    /// its opening key press must hit an armed guard.
    #[test]
    fn a_committed_editor_reopened_the_next_frame_still_arms() {
        let mut settings = MatchSettings::default();
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let mut app = App::new();
        app.add_plugins(UiButtonPlugin);
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<crate::app::AppScreen>()
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(settings)
            .insert_resource(SettingsStore::from_storage(DiscardingSettingsStorage))
            .insert_resource(crate::input::controller::ControllerAssignments::default())
            .insert_resource(ControlMappingState::new(None))
            .insert_resource(AudioCueQueue::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::diagnostics::Diagnostics>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (
                    super::super::handle_mapping_escape,
                    super::super::handle_mapping_buttons,
                    capture_game_numeric_input,
                )
                    .chain(),
            );

        // Open, arm, and commit a session.
        let pressed = app.world_mut().spawn(GameNumericInput::RoundLimit).id();
        trigger_activation(&mut app, pressed);
        app.update();
        send_logical_key(&mut app, Key::Enter);
        assert!(
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .is_none(),
            "the commit closes the session"
        );

        // Reopen the same input on the very next frame, its activating
        // Enter still in the same frame's messages.
        let pressed = app.world_mut().spawn(GameNumericInput::RoundLimit).id();
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
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .is_some(),
            "the fresh session must arm; a stale guard would let its own \
             opening Enter commit it instantly"
        );

        // Now the other close: Escape abandons the session before this
        // system's turn in the chain, and the reopen still has to arm.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Escape);
            keys.clear();
        }
        assert!(
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .is_none(),
            "Escape abandons the session"
        );

        let pressed = app.world_mut().spawn(GameNumericInput::RoundLimit).id();
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
            app.world()
                .resource::<ControlMappingState>()
                .game_editor
                .is_some(),
            "an Escape-closed session reopened the next frame must arm too"
        );
    }

    #[test]
    fn invalid_commit_keeps_saved_value_and_valid_commit_clamps_visible_value() {
        let mut settings = MatchSettings::default();
        settings.set_enabled(MatchSettingField::RoundLimit, true);
        let mut mapping = ControlMappingState::new(None);
        mapping.game_editor = Some(GameEditor {
            input: GameNumericInput::RoundLimit,
            editor: TextEditor::new(String::new()),
        });

        assert_eq!(
            commit_game_editor(&mut settings, &mut mapping),
            Err(NumericParseError::Empty)
        );
        assert_eq!(settings.value(MatchSettingField::RoundLimit), 100);
        assert!(
            mapping
                .game_error
                .as_ref()
                .unwrap()
                .1
                .contains("whole number")
        );
        assert!(mapping.game_editor.is_some());

        mapping.game_editor.as_mut().unwrap().editor = TextEditor::new("0".into());
        assert_eq!(commit_game_editor(&mut settings, &mut mapping), Ok(true));
        assert_eq!(settings.value(MatchSettingField::RoundLimit), 1);
        assert_eq!(
            game_input_text(GameNumericInput::RoundLimit, &settings),
            "1"
        );
        assert!(mapping.game_editor.is_none());
        assert!(mapping.game_error.is_none());
    }

    #[test]
    fn round_time_commit_normalizes_and_raises_match_time() {
        let mut settings = MatchSettings::default();
        settings.set_enabled(MatchSettingField::RoundTime, true);
        settings.set_enabled(MatchSettingField::MatchTime, true);
        settings.set_value(MatchSettingField::MatchTime, 60);
        let mut mapping = ControlMappingState::new(None);
        mapping.game_editor = Some(GameEditor {
            input: GameNumericInput::RoundTimeSeconds,
            editor: TextEditor::new("90".into()),
        });

        assert_eq!(commit_game_editor(&mut settings, &mut mapping), Ok(true));
        assert_eq!(settings.value(MatchSettingField::RoundTime), 150);
        assert_eq!(settings.value(MatchSettingField::MatchTime), 150);
        assert_eq!(
            game_input_text(GameNumericInput::RoundTimeMinutes, &settings),
            "2"
        );
        assert_eq!(
            game_input_text(GameNumericInput::RoundTimeSeconds, &settings),
            "30"
        );
    }

    #[test]
    fn game_toggles_round_trip_and_control_reset_is_isolated() {
        let mut match_settings = MatchSettings::default();
        match_settings.toggle_enabled(MatchSettingField::NoStoneRoundLimit);
        match_settings.set_value(MatchSettingField::NoStoneRoundLimit, 17);
        match_settings.toggle_capture_points();
        let expected = match_settings.clone();
        let mut config = ControlsConfig {
            match_settings,
            ..ControlsConfig::default()
        };
        config
            .keyboard
            .assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        config.keyboard.reset_to_defaults();
        config.controller.reset_to_defaults();

        let (loaded, warning) = ControlsConfig::from_json(&config.to_json().unwrap()).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded.match_settings, expected);
    }

    #[test]
    fn game_display_dims_disabled_inputs_and_shows_contextual_errors() {
        let mut app = App::new();
        app.insert_resource(MatchSettings::default())
            .insert_resource(ControlMappingState::new(None))
            .add_systems(Update, update_game_settings_display);
        let input_text = app
            .world_mut()
            .spawn((
                Text::new(""),
                TextColor(TEXT_PRIMARY),
                GameInputValue(GameNumericInput::PointTarget),
            ))
            .id();
        let input_button = app
            .world_mut()
            .spawn((
                GameNumericInput::PointTarget,
                BackgroundColor(BUTTON_BACKGROUND),
                BorderColor::all(BLUE_BORDER),
                crate::audio::UiSound::new(UiCue::Neutral),
            ))
            .id();
        let error_text = app
            .world_mut()
            .spawn((
                Text::new(""),
                Node::default(),
                GameInputError(GameNumericInput::PointTarget),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().get::<TextColor>(input_text).unwrap().0,
            TEXT_MUTED
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(input_button).unwrap().0,
            BUTTON_DISABLED
        );

        app.world_mut()
            .resource_mut::<MatchSettings>()
            .set_enabled(MatchSettingField::PointTarget, true);
        app.world_mut()
            .resource_mut::<ControlMappingState>()
            .game_error = Some((
            GameNumericInput::PointTarget,
            "Point Target: enter a whole number.".into(),
        ));
        app.update();
        assert_eq!(
            app.world().get::<TextColor>(input_text).unwrap().0,
            TEXT_PRIMARY
        );
        assert_eq!(
            app.world().get::<Text>(error_text).unwrap().as_str(),
            "Point Target: enter a whole number."
        );
        assert_eq!(
            app.world().get::<Node>(error_text).unwrap().display,
            Display::Flex
        );
    }

    /// The Game tab at its most crowded, laid out with its text by `bevy_ui`
    /// itself at every size the screen's real-layout tests run at, once for
    /// every error a row can show: every limit enabled, Round Time at its
    /// maximum so that the Match Time row's caption is its longest, `Min
    /// 60:00`, and the screen's own error line showing too, which on top
    /// takes a line above the tabs.
    ///
    /// Three things have to hold, and a node's declared size shows none of
    /// them. The panel clips what does not fit under its `max_height` rather
    /// than growing, and when its parent is shorter than its rows it is
    /// squeezed to the parent's height and clips them there; either way a
    /// row pushed too far down is cut off with nothing visibly overflowing -
    /// and an error line that wraps is the thing that pushes, the longest
    /// wrapping at 14px. So the rows must end inside the panel's padding,
    /// each at least its own height. And a row's controls keep their own
    /// width: the Match Time row's controls and its 455px of copy are wider
    /// together than the row, which flexbox resolves by squeezing every
    /// child, and squeezing a caption wraps it. The 12px caption was
    /// already drawn as `Min` above `60:00` this way before the text floor
    /// (2026-10-07) widened it; the controls now refuse to shrink and the
    /// copy gives way instead. And the panel itself must stand inside the
    /// room the frame leaves a tab, which a panel taller than that room
    /// runs out of: in the sidebar of the smallest window the rows fit it
    /// only at their closer heights.
    #[test]
    fn the_game_tab_fits_its_panel_at_its_most_crowded() {
        use bevy::text::TextLayoutInfo;
        use bevy::ui::ComputedNode;

        use super::super::geometry::settings_layout;
        use super::super::layout::SettingsPlacement;
        use super::super::layout::tests::{
            contents_room, laid_out_settings, node_rect, placed_entity, real_layout_sizes,
        };
        use super::super::{MappingPanel, SETTINGS_NOT_SAVED_WARNING};
        use crate::match_settings::DURATION_MAX_SECONDS;

        fn line_count(layout: &TextLayoutInfo) -> usize {
            layout
                .glyphs
                .iter()
                .map(|glyph| glyph.line_index as usize + 1)
                .max()
                .unwrap_or(0)
        }

        let inputs = [
            GameNumericInput::RoundLimit,
            GameNumericInput::RoundTimeMinutes,
            GameNumericInput::RoundTimeSeconds,
            GameNumericInput::NoStoneRoundLimit,
            GameNumericInput::MatchTimeMinutes,
            GameNumericInput::MatchTimeSeconds,
            GameNumericInput::PointTarget,
        ];
        for window in real_layout_sizes() {
            for input in inputs {
                for error in [NumericParseError::Empty, NumericParseError::NotDigits] {
                    let message = game_error_message(input, error);
                    let context = format!("{window}, {message:?}");
                    let mut settings = MatchSettings::default();
                    for field in [
                        MatchSettingField::RoundLimit,
                        MatchSettingField::RoundTime,
                        MatchSettingField::NoStoneRoundLimit,
                        MatchSettingField::MatchTime,
                        MatchSettingField::PointTarget,
                    ] {
                        settings.set_enabled(field, true);
                    }
                    settings.set_value(MatchSettingField::RoundTime, DURATION_MAX_SECONDS.into());
                    let mut mapping = ControlMappingState::new(None);
                    mapping.tab = MappingTab::Game;
                    mapping.game_error = Some((input, message.clone()));
                    mapping.error = SETTINGS_NOT_SAVED_WARNING.into();
                    let mut app = laid_out_settings(window, mapping, settings);
                    let world = app.world_mut();

                    // The error is really on screen, or a pass would say
                    // nothing.
                    let mut texts = world.query::<(&Text, &TextLayoutInfo)>();
                    let shown = texts
                        .iter(world)
                        .find(|(text, _)| text.0 == message)
                        .map(|(_, layout)| line_count(layout));
                    assert!(shown.is_some_and(|lines| lines > 0), "{context}: not drawn");

                    let (panel, surface) = world
                        .query::<(Entity, &MappingPanel, &Children)>()
                        .iter(world)
                        .find(|(_, panel, _)| panel.0 == MappingTab::Game)
                        .map(|(entity, _, children)| (entity, children[0]))
                        .expect("the Game panel holds its surface");
                    let rows: Vec<Entity> =
                        world.get::<Children>(surface).unwrap().iter().collect();
                    let mut surfaces = world.query::<(&Node, &ComputedNode)>();
                    let (declared, laid_out) = surfaces.get(world, surface).unwrap();
                    let Val::Px(max_height) = declared.max_height else {
                        panic!("the Game panel's surface has a fixed maximum height");
                    };
                    let Val::Px(padding) = declared.padding.bottom else {
                        panic!("the Game panel's surface has a fixed padding");
                    };
                    let height = laid_out.size.y * laid_out.inverse_scale_factor;
                    assert!(
                        height < max_height - 0.5,
                        "{context}: the rows need {height}px or more of a surface that shows \
                         {max_height}px and clips the rest"
                    );
                    let min_heights: Vec<f32> = rows
                        .iter()
                        .map(|row| {
                            let (declared, _) = surfaces.get(world, *row).unwrap();
                            let Val::Px(min) = declared.min_height else {
                                panic!("a Game tab row has a fixed least height");
                            };
                            min
                        })
                        .collect();

                    let mut controls = world.query_filtered::<(&Node, &ComputedNode), (
                        With<Button>,
                        Or<(With<GameToggle>, With<GameNumericInput>)>,
                    )>();
                    for (declared, laid_out) in controls.iter(world) {
                        let Val::Px(width) = declared.width else {
                            panic!("a Game tab control has a fixed width");
                        };
                        let drawn = laid_out.size.x * laid_out.inverse_scale_factor;
                        assert!(
                            (drawn - width).abs() < 0.5,
                            "{context}: a {width}px control is drawn {drawn}px wide"
                        );
                    }

                    let mut minimum =
                        world.query_filtered::<(&Text, &TextLayoutInfo), With<MatchMinimumLabel>>();
                    let (minimum, minimum_layout) = minimum.single(world).unwrap();
                    assert_eq!(minimum.0, "Min 60:00", "the caption at its longest");
                    let mut captions = vec![("Min 60:00", line_count(minimum_layout))];
                    captions.extend(
                        texts
                            .iter(world)
                            .filter(|(text, _)| text.0 == "MM:SS")
                            .map(|(_, layout)| ("MM:SS", line_count(layout))),
                    );
                    assert_eq!(captions.len(), 2, "both captions are on the tab");
                    for (caption, lines) in captions {
                        assert_eq!(lines, 1, "{context}: {caption:?} is drawn on {lines} lines");
                    }

                    // No row squeezed below its own height, the last ending
                    // inside the surface's padding, and the surface inside
                    // its panel.
                    let surface_rect = node_rect(&app, surface);
                    for (row, min) in rows.iter().zip(min_heights) {
                        let rect = node_rect(&app, *row);
                        assert!(
                            rect.height() >= min - 0.5,
                            "{context}: a row is {} tall of its {min}",
                            rect.height()
                        );
                    }
                    let last = node_rect(&app, *rows.last().unwrap());
                    assert!(
                        last.max.y <= surface_rect.max.y - padding + 0.5,
                        "{context}: the last row ends at {} in a surface ending at {}",
                        last.max.y,
                        surface_rect.max.y
                    );
                    let panel_rect = node_rect(&app, panel);
                    assert!(
                        panel_rect.inflate(0.5).contains(surface_rect.min)
                            && panel_rect.inflate(0.5).contains(surface_rect.max),
                        "{context}: the surface {surface_rect:?} leaves {panel_rect:?}"
                    );
                    // And the panel inside the room the frame gives a tab,
                    // which a taller tab would grow the contents past: they
                    // end where the room ends, and on top start lower by the
                    // screen's error line, which takes its line from them.
                    let contents = placed_entity(&mut app, SettingsPlacement::Contents);
                    let contents = node_rect(&app, contents);
                    let room = contents_room(&settings_layout(window));
                    assert!(
                        (contents.max - room.max).abs().max_element() <= 1.0
                            && (contents.min.x - room.min.x).abs() <= 1.0
                            && contents.min.y >= room.min.y - 1.0,
                        "{context}: the contents {contents:?} leave their room {room:?}"
                    );
                    assert!(
                        contents.inflate(0.5).contains(panel_rect.min)
                            && contents.inflate(0.5).contains(panel_rect.max),
                        "{context}: the panel {panel_rect:?} leaves the contents {contents:?}"
                    );
                }
            }
        }
    }
}
