//! A minimal caret-aware single-line text editor shared by UI features.
//!
//! Bevy has no text input widget, so this is the smallest thing that behaves
//! correctly: a `String` plus a caret measured in **characters** - a `char`
//! index, never a byte offset - which [`TextEditor::byte_index`] converts to a
//! byte offset at every edit, so multi-byte input can never split a
//! character. Only printable ASCII is accepted, matching the replay label rule
//! enforced by [`crate::replay::validate_label`].
//!
//! # Typing is not always ASCII
//!
//! A player's keyboard produces characters that rule cannot hold, and the two
//! ways that happens deserve different answers:
//!
//! * A few typographic forms have an exact ASCII equivalent, and the player
//!   did not ask for them - macOS smart quotes and Dictation both produce
//!   U+2019 where the player typed an apostrophe. [`printable_ascii_form`]
//!   substitutes those, so the keystroke types the character it stands for.
//! * Everything else - an accented letter, a non-Latin script, a control
//!   character - is reported as [`TextInputOutcome::Rejected`] rather than
//!   dropped, so the caller can say why the keystroke did nothing. A silently
//!   swallowed character is indistinguishable from a dead key, a full field,
//!   or a broken keyboard.
//!
//! Neither widens what is legal: every accepted character still satisfies
//! [`crate::replay::is_printable_ascii`].
//!
//! [`apply_editor_input`] reports, via [`TextInputOutcome`], whether a key was
//! consumed, whether the text changed, and whether a character was refused.
//! Callers use that to decide whether to swallow the key (so arrows and space
//! never leak to the screen underneath), whether to re-filter the list, and
//! whether to show their error line.

use bevy::input::keyboard::Key;

use crate::replay::is_printable_ascii;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextEditor {
    pub(crate) text: String,
    pub(crate) caret: usize,
}

impl TextEditor {
    pub(crate) fn new(text: String) -> Self {
        let caret = text.chars().count();
        Self { text, caret }
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    pub(crate) fn insert(&mut self, text: &str) {
        let byte_index = self.byte_index();
        self.text.insert_str(byte_index, text);
        self.caret += text.chars().count();
    }

    pub(crate) fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        let end = self.byte_index();
        self.caret -= 1;
        let start = self.byte_index();
        self.text.replace_range(start..end, "");
        true
    }

    pub(crate) fn delete(&mut self) -> bool {
        if self.caret == self.text.chars().count() {
            return false;
        }
        let start = self.byte_index();
        let end = self
            .text
            .char_indices()
            .nth(self.caret + 1)
            .map_or(self.text.len(), |(index, _)| index);
        self.text.replace_range(start..end, "");
        true
    }

    pub(crate) fn move_left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
    }

    pub(crate) fn move_right(&mut self) {
        self.caret = (self.caret + 1).min(self.text.chars().count());
    }

    pub(crate) fn move_home(&mut self) {
        self.caret = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.caret = self.text.chars().count();
    }

    pub(crate) fn display_text(&self) -> String {
        let mut displayed = self.text.clone();
        displayed.insert(self.byte_index(), '|');
        displayed
    }

    pub(crate) fn byte_index(&self) -> usize {
        self.text
            .char_indices()
            .nth(self.caret)
            .map_or(self.text.len(), |(index, _)| index)
    }
}

/// What [`apply_editor_input`] did with one key. Every variant but
/// [`Self::Ignored`] means the editor took the key - see [`Self::consumed`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextInputOutcome {
    Ignored,
    Consumed,
    /// A character the field cannot hold, so nothing was typed.
    ///
    /// It is consumed like every other key the editor took; what separates it
    /// from [`Self::Consumed`] is that the caller is expected to *say so*,
    /// because a character that vanishes reads as a dead key rather than as a
    /// refusal. A caller with nowhere to put a message may treat it exactly
    /// like [`Self::Consumed`], which is what the Library's search field does.
    Rejected,
    Changed,
    Submit,
    Cancel,
}

impl TextInputOutcome {
    pub(crate) const fn consumed(self) -> bool {
        !matches!(self, Self::Ignored)
    }
}

/// Applies one logical key to `editor`.
///
/// `command_down` is whether a Control or Super key is held. With it down,
/// printable keys are reported [`TextInputOutcome::Ignored`] instead of being
/// typed, so a shortcut chord falls through to the screen rather than landing
/// in the field. `Ignored` is the only outcome callers should let through;
/// every other outcome - including [`TextInputOutcome::Submit`] (Enter),
/// [`TextInputOutcome::Cancel`] (Escape), and [`TextInputOutcome::Rejected`]
/// for a character the field cannot hold - means the key was for the editor.
pub(crate) fn apply_editor_input(
    editor: &mut TextEditor,
    key: &Key,
    command_down: bool,
) -> TextInputOutcome {
    match key {
        Key::Escape => TextInputOutcome::Cancel,
        Key::Enter => TextInputOutcome::Submit,
        Key::Backspace => {
            if editor.backspace() {
                TextInputOutcome::Changed
            } else {
                TextInputOutcome::Consumed
            }
        }
        Key::Delete => {
            if editor.delete() {
                TextInputOutcome::Changed
            } else {
                TextInputOutcome::Consumed
            }
        }
        Key::ArrowLeft => {
            editor.move_left();
            TextInputOutcome::Consumed
        }
        Key::ArrowRight => {
            editor.move_right();
            TextInputOutcome::Consumed
        }
        Key::Home => {
            editor.move_home();
            TextInputOutcome::Consumed
        }
        Key::End => {
            editor.move_end();
            TextInputOutcome::Consumed
        }
        Key::Space if !command_down => {
            editor.insert(" ");
            TextInputOutcome::Changed
        }
        Key::Character(text) if !command_down => match printable_ascii_form(text) {
            // Bevy should never report a character event carrying no
            // characters, and an error line about nothing typed would be
            // worse than silence, so an empty one is merely consumed.
            Some(accepted) if accepted.is_empty() => TextInputOutcome::Consumed,
            Some(accepted) => {
                editor.insert(&accepted);
                TextInputOutcome::Changed
            }
            None => TextInputOutcome::Rejected,
        },
        _ => TextInputOutcome::Ignored,
    }
}

/// What to tell a player whose keystroke was [`TextInputOutcome::Rejected`].
///
/// It names what the field does accept rather than only that something
/// failed, because the player's next move is to type something else. A caller
/// whose own rule is narrower - the Settings screen's numeric fields take
/// digits alone - shows its own message instead.
pub(crate) const REJECTED_CHARACTER_MESSAGE: &str =
    "That character cannot be used. Use unaccented letters, numbers and punctuation.";

/// The printable-ASCII text `input` stands for, or `None` if any of its
/// characters stands for nothing.
///
/// All or nothing on purpose: a character event can carry more than one
/// character, and typing the acceptable half of one would leave the player
/// with text they did not write.
fn printable_ascii_form(input: &str) -> Option<String> {
    input.chars().map(printable_ascii_substitute).collect()
}

/// The printable ASCII character `character` stands for.
///
/// Printable ASCII stands for itself. Past that this is a closed list of
/// typographic forms a player receives without asking for them - the smart
/// quotes macOS substitution and Dictation produce, the dashes an editor
/// autocorrects, the non-breaking space Option+Space types - each mapped to
/// the key the player meant to press. It is deliberately not Unicode
/// normalisation: a character not listed here stands for nothing and is
/// rejected rather than approximated, because guessing at `e` for `é` would
/// silently change what the player wrote.
fn printable_ascii_substitute(character: char) -> Option<char> {
    if is_printable_ascii(character) {
        return Some(character);
    }
    match character {
        // Left and right single quotation marks.
        '\u{2018}' | '\u{2019}' => Some('\''),
        // Left and right double quotation marks.
        '\u{201c}' | '\u{201d}' => Some('"'),
        // En dash and em dash.
        '\u{2013}' | '\u{2014}' => Some('-'),
        // Non-breaking space.
        '\u{a0}' => Some(' '),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::validate_label;

    #[test]
    fn replay_label_editor_inserts_spaces_at_end_and_middle() {
        let mut editor = TextEditor::new("Replay".into());
        editor.insert(" ");
        editor.insert("One");
        assert_eq!(editor.text, "Replay One");
        assert_eq!(editor.caret, 10);

        editor.move_home();
        for _ in 0..3 {
            editor.move_right();
        }
        editor.insert(" ");
        assert_eq!(editor.text, "Rep lay One");
        assert_eq!(editor.caret, 4);
    }

    #[test]
    fn replay_label_editor_clamps_left_and_right_movement() {
        let mut editor = TextEditor::new("Game".into());

        for _ in 0..10 {
            editor.move_left();
        }
        assert_eq!(editor.caret, 0);

        for _ in 0..10 {
            editor.move_right();
        }
        assert_eq!(editor.caret, 4);
    }

    #[test]
    fn replay_label_editor_inserts_and_deletes_around_caret() {
        let mut editor = TextEditor::new("Gme".into());
        editor.move_left();
        editor.move_left();
        editor.insert("a");
        assert_eq!(editor.text, "Game");
        assert_eq!(editor.caret, 2);

        assert!(editor.backspace());
        assert_eq!(editor.text, "Gme");
        assert_eq!(editor.caret, 1);

        assert!(editor.delete());
        assert_eq!(editor.text, "Ge");
        assert_eq!(editor.caret, 1);
    }

    #[test]
    fn replay_label_editor_preserves_printable_text_and_unicode_boundaries() {
        let label = "Game_1 - (final), wow! isn't it?";
        let mut editor = TextEditor::new(label.into());
        editor.move_home();
        editor.move_right();
        editor.insert("é");
        editor.backspace();

        assert_eq!(editor.text, label);
        assert_eq!(editor.display_text(), "G|ame_1 - (final), wow! isn't it?");
        assert!(validate_label(&editor.text).is_ok());
    }

    /// The rejection a caller has to be able to report. A character with no
    /// ASCII form leaves the text and caret exactly as they were, and says so
    /// with its own outcome rather than sharing `Consumed` with the caret
    /// keys - which is what made a refused character look like a dead key.
    #[test]
    fn character_without_an_ascii_form_is_rejected_and_changes_nothing() {
        for (text, description) in [
            ("\n", "a newline"),
            ("\t", "a tab"),
            ("\u{7f}", "delete, just past the printable range"),
            ("\u{e9}", "e with an acute accent"),
            ("\u{fc}", "u with a diaeresis"),
            ("\u{3042}", "hiragana a"),
            ("\u{0416}", "Cyrillic zhe"),
            ("\u{03b1}", "Greek alpha"),
            ("\u{0645}", "Arabic meem"),
            ("\u{ff13}", "a full-width 3"),
            (
                "\u{0301}",
                "a combining acute, the second half of a dead key",
            ),
        ] {
            let mut editor = TextEditor::new("Game".into());
            editor.move_home();

            let outcome = apply_editor_input(&mut editor, &Key::Character(text.into()), false);

            assert_eq!(
                outcome,
                TextInputOutcome::Rejected,
                "{description} ({text:?}) has no printable ASCII form"
            );
            assert!(
                outcome.consumed(),
                "a rejected character must not reach the screen underneath"
            );
            assert_eq!(editor.text, "Game");
            assert_eq!(editor.caret, 0);
        }
    }

    /// A character event carrying more than one character is all or nothing:
    /// the acceptable half of it must not land in the field on its own.
    #[test]
    fn a_mixed_character_event_is_rejected_whole() {
        let mut editor = TextEditor::new("Game".into());

        assert_eq!(
            apply_editor_input(&mut editor, &Key::Character("a\u{e9}b".into()), false),
            TextInputOutcome::Rejected
        );
        assert_eq!(editor.text, "Game");

        assert_eq!(
            apply_editor_input(&mut editor, &Key::Character("a\u{2019}b".into()), false),
            TextInputOutcome::Changed
        );
        assert_eq!(editor.text, "Gamea'b");
    }

    /// The substitutions exist so Dictation and macOS smart quotes type what
    /// the player meant instead of failing loudly. Each one is an exact ASCII
    /// equivalent, so the result is still a valid label.
    #[test]
    fn typographic_forms_are_typed_as_the_ascii_they_stand_for() {
        for (typed, expected) in [
            ("\u{2018}", "'"),
            ("\u{2019}", "'"),
            ("\u{201c}", "\""),
            ("\u{201d}", "\""),
            ("\u{2013}", "-"),
            ("\u{2014}", "-"),
            ("\u{a0}", " "),
        ] {
            let mut editor = TextEditor::new("Game".into());

            let outcome = apply_editor_input(&mut editor, &Key::Character(typed.into()), false);

            assert_eq!(outcome, TextInputOutcome::Changed, "{typed:?}");
            assert_eq!(editor.text, format!("Game{expected}"));
            assert_eq!(editor.caret, 5);
            assert!(validate_label(&editor.text).is_ok());
        }
    }

    /// The case the substitution table was added for: `Dad's game` typed on
    /// macOS, where the apostrophe arrives as U+2019.
    #[test]
    fn a_dictated_apostrophe_produces_a_valid_label() {
        let mut editor = TextEditor::new("Dad".into());
        for text in ["\u{2019}", "s"] {
            assert_eq!(
                apply_editor_input(&mut editor, &Key::Character(text.into()), false),
                TextInputOutcome::Changed
            );
        }
        assert_eq!(
            apply_editor_input(&mut editor, &Key::Space, false),
            TextInputOutcome::Changed
        );
        assert_eq!(
            apply_editor_input(&mut editor, &Key::Character("game".into()), false),
            TextInputOutcome::Changed
        );

        assert_eq!(editor.text, "Dad's game");
        assert!(validate_label(&editor.text).is_ok());
    }

    /// A command chord still falls through, and a character event with no
    /// characters is not a rejection to report.
    #[test]
    fn a_chord_is_ignored_and_an_empty_character_event_is_only_consumed() {
        let mut editor = TextEditor::new("Game".into());

        assert_eq!(
            apply_editor_input(&mut editor, &Key::Character("a".into()), true),
            TextInputOutcome::Ignored
        );
        assert_eq!(
            apply_editor_input(&mut editor, &Key::Character("".into()), false),
            TextInputOutcome::Consumed
        );
        assert_eq!(editor.text, "Game");
    }

    #[test]
    fn the_rejection_message_names_what_the_field_accepts() {
        assert_eq!(
            REJECTED_CHARACTER_MESSAGE,
            "That character cannot be used. Use unaccented letters, numbers and punctuation."
        );
        assert!(REJECTED_CHARACTER_MESSAGE.chars().all(is_printable_ascii));
    }
}
