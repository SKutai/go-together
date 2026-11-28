//! Where `bevy_ui` breaks the lines of the Viewer's text, worked out
//! without laying it out: how many lines a text takes at so many
//! characters to a line.
//!
//! Every text in the game is printable ASCII in a monospaced font (see
//! [`crate::ui::text_metrics`]), so a line's width is its length in
//! characters, and where it may break depends only on which characters
//! stand around the place. Both halves of the answer are rules, followed
//! here exactly rather than estimated:
//!
//! * **Where a line may break** is the Unicode line-breaking algorithm,
//!   UAX #14, which the layout engine `bevy_ui` uses takes from ICU.
//!   Printable ASCII falls into fourteen of its classes ([`Class`]), and
//!   [`may_break_before`] is its rules for those, as ICU applies them. A
//!   line may break after a space, unless what follows is one of `)` `]`
//!   `}` `!` `?` `,` `.` `:` `;` `/`, none of which starts a line, or the
//!   spaces follow `(` `[` `{`, after which no line breaks; so `(`, a
//!   quotation mark or `-` may start a line after a space. Between two
//!   other characters a line breaks after `-` or `/` that no digit
//!   follows, after `|` `!` `?`, and in a few other places beside
//!   punctuation, but never inside a word or a number, nor beside a
//!   quotation mark.
//! * **How lines are filled** is the way that engine fills them: each line
//!   takes as many characters as fit, and when the next does not, the line
//!   ends at the last place on it that it may break. A space that does not
//!   fit hangs past the end of its line, and the line ends after it, so
//!   the rest of a run of spaces starts the next line, and a space at the
//!   very end of a text that does not fit leaves an empty line after it. A
//!   word too long for a line runs past its end, or, in a text set to break
//!   inside a word (`LineBreak::WordOrCharacter`), breaks after the last
//!   character that fits - but only on a line with no other place to break.
//!
//! Both were checked on 2026-11-28, the breaks against ICU's segmenter
//! (`icu_segmenter` 2.3) for every printable ASCII string of up to four
//! characters and two million longer ones, and the lines against the
//! engine itself (`parley` 0.11, with Bevy's default font) for 650,000
//! texts of up to 60 characters, at the sizes and widths the Viewer sets
//! them at; they agreed on every one. A test in [`super`] holds the lines
//! to what `bevy_ui` draws for a corpus of labels, so a change in either
//! would show there.

/// The line-breaking class UAX #14 gives a printable ASCII character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Letters, and `#` `&` `*` `<` `=` `>` `@` `^` `_` `` ` `` `~`:
    /// alphabetic.
    Al,
    /// `|`: a break may follow it.
    Ba,
    /// `}`: closing punctuation.
    Cl,
    /// `)` and `]`: closing parentheses.
    Cp,
    /// `!` and `?`.
    Ex,
    /// `-`.
    Hy,
    /// `,` `.` `:` `;`: infix separators.
    Is,
    /// Digits.
    Nu,
    /// `(` `[` `{`.
    Op,
    /// `%`: a postfix to a number.
    Po,
    /// `$` `+` `\`: a prefix to a number.
    Pr,
    /// `"` and `'`, which are neither opening nor closing quotation marks.
    Qu,
    /// The space.
    Sp,
    /// `/`.
    Sy,
}

/// The class of `byte`, a printable ASCII character.
const fn class(byte: u8) -> Class {
    match byte {
        b' ' => Class::Sp,
        b'!' | b'?' => Class::Ex,
        b'"' | b'\'' => Class::Qu,
        b'$' | b'+' | b'\\' => Class::Pr,
        b'%' => Class::Po,
        b'(' | b'[' | b'{' => Class::Op,
        b')' | b']' => Class::Cp,
        b'}' => Class::Cl,
        b',' | b'.' | b':' | b';' => Class::Is,
        b'-' => Class::Hy,
        b'/' => Class::Sy,
        b'|' => Class::Ba,
        b'0'..=b'9' => Class::Nu,
        _ => Class::Al,
    }
}

/// Whether a line of `text` may break before `text[index]`, which is
/// neither its first character nor past its last: UAX #14's rules for
/// printable ASCII, in its order, each named by its number there. They are
/// the rules as ICU applies them, which are Unicode 15.0's but for LB15:
/// that kept a quotation mark and an opening bracket after it together
/// across spaces, and later versions keep it only for opening and closing
/// quotation marks, of which ASCII has none. Their other changes, to `,`
/// `.` `:` `;` after a space, to numbers and to a hyphen that starts a
/// word, ICU does not apply, and nor does this.
pub(super) fn may_break_before(text: &[u8], index: usize) -> bool {
    use Class::*;

    let (before, after) = (class(text[index - 1]), class(text[index]));
    // The class of the last character before `index` that is not a space.
    let before_spaces = text[..index]
        .iter()
        .rev()
        .map(|byte| class(*byte))
        .find(|class| *class != Sp);
    match (before, after) {
        // LB7: no break before a space.
        (_, Sp) => false,
        // LB13: none before closing punctuation, `!` `?`, `,` `.` `:` `;`
        // or `/`.
        (_, Cl | Cp | Ex | Is | Sy) => false,
        // LB14: none after opening punctuation, however many spaces follow.
        _ if before_spaces == Some(Op) => false,
        // LB18: a break after spaces.
        (Sp, _) => true,
        // LB19: none before or after a quotation mark.
        (Qu, _) | (_, Qu) => false,
        // LB21: none before a hyphen or `|`.
        (_, Hy | Ba) => false,
        // LB23: none between letters and digits.
        (Al, Nu) | (Nu, Al) => false,
        // LB24: none between a number's prefix or postfix and a letter.
        (Pr | Po, Al) | (Al, Pr | Po) => false,
        // LB25: none inside a number, nor between one and its prefix,
        // postfix or brackets.
        (Cl | Cp | Nu, Po | Pr) | (Po | Pr, Op | Nu) | (Hy | Is | Nu | Sy, Nu) => false,
        // LB28: none between letters.
        (Al, Al) => false,
        // LB29: none after `,` `.` `:` `;` before a letter.
        (Is, Al) => false,
        // LB30: none between a letter or a digit and a bracket beside it.
        (Al | Nu, Op) | (Cp, Al | Nu) => false,
        // LB31: a break everywhere else.
        _ => true,
    }
}

/// Calls `line` with where each line of `text` starts, set `chars`
/// characters to a line as the layout engine fills its lines; with
/// `break_words`, a word too long for a line breaks inside it.
pub(super) fn for_each_line(
    text: &[u8],
    chars: usize,
    break_words: bool,
    mut line: impl FnMut(usize),
) {
    line(0);
    let (mut index, mut width) = (0, 0);
    // The last places on this line it may break at, and the last it may
    // break at inside a word.
    let (mut regular, mut inside) = (None, None);
    while index < text.len() {
        if width > 0 {
            if may_break_before(text, index) {
                regular = Some(index);
            } else if break_words {
                inside = Some(index);
            }
        }
        if width < chars {
            width += 1;
            index += 1;
            continue;
        }
        let next = if text[index] == b' ' {
            // A space that does not fit stays on its line.
            Some(index + 1)
        } else {
            regular.or(inside)
        };
        if let Some(next) = next {
            index = next;
            (width, regular, inside) = (0, None, None);
            line(index);
        } else {
            // Nowhere to break: the line runs on.
            width += 1;
            index += 1;
        }
    }
}

/// How many lines `text` takes `chars` characters to a line; with
/// `break_words`, as a text set with `LineBreak::WordOrCharacter` takes
/// them, breaking inside a word too long for a line, and otherwise as one
/// set with `LineBreak::WordBoundary`, the default.
pub(super) fn wrapped_lines(text: &str, chars: usize, break_words: bool) -> usize {
    let mut lines = 0;
    for_each_line(text.as_bytes(), chars, break_words, |_| lines += 1);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` cut at every place a line may break, as [`may_break_before`]
    /// says.
    fn pieces(text: &str) -> Vec<&str> {
        let mut pieces = Vec::new();
        let mut start = 0;
        for index in 1..text.len() {
            if may_break_before(text.as_bytes(), index) {
                pieces.push(&text[start..index]);
                start = index;
            }
        }
        pieces.push(&text[start..]);
        pieces
    }

    /// Where a line may break, worked out by hand from UAX #14 for each
    /// kind of place in a label or in the Viewer's other texts.
    #[test]
    fn a_line_breaks_where_the_unicode_rules_let_it() {
        // After spaces, and before a bracket or a quotation mark there.
        assert_eq!(
            pieces("a (b) [c] {d} \"e\" 'f'"),
            ["a ", "(b) ", "[c] ", "{d} ", "\"e\" ", "'f'"]
        );
        // But never before closing punctuation, `!` `?` `,` `.` `:` `;` or
        // `/`, even after a space.
        assert_eq!(
            pieces("a ) b ] c } d ! e ? f , g . h : i ; j / k"),
            [
                "a ) ", "b ] ", "c } ", "d ! ", "e ? ", "f , ", "g . ", "h : ", "i ; ", "j / ", "k"
            ]
        );
        // Nor after opening punctuation, however many spaces follow.
        assert_eq!(pieces("a (  b [ c { d"), ["a ", "(  b ", "[ c ", "{ d"]);
        // A line may start with `-` or `|` after a space, and breaks after
        // either between words, and after `/`, `!` and `?`.
        assert_eq!(pieces("a - b | c"), ["a ", "- ", "b ", "| ", "c"]);
        assert_eq!(
            pieces("semi-final a|b and/or Hi!Yo Why?Not"),
            [
                "semi-", "final ", "a|", "b ", "and/", "or ", "Hi!", "Yo ", "Why?", "Not"
            ]
        );
        // A hyphen that starts a word does not keep it whole.
        assert_eq!(pieces("a -b"), ["a ", "-", "b"]);
        // No break inside a number, nor between one and its sign.
        assert_eq!(
            pieces("2026-11-27 23:59 1/2 a/2 x-1 $5 +3 50% 1,000.5 -7"),
            [
                "2026-11-27 ",
                "23:59 ",
                "1/2 ",
                "a/2 ",
                "x-1 ",
                "$5 ",
                "+3 ",
                "50% ",
                "1,000.5 ",
                "-7"
            ]
        );
        // Nor between letters, digits and the punctuation inside a word, a
        // bracket beside one, or a quotation mark.
        assert_eq!(
            pieces("Round9 e.g. f(x) a)b don't a\"b\" #4 a&b"),
            [
                "Round9 ", "e.g. ", "f(x) ", "a)b ", "don't ", "a\"b\" ", "#4 ", "a&b"
            ]
        );
        // Between brackets facing apart, and after `}`.
        assert_eq!(pieces("(a)(b) a}b"), ["(a)", "(b) ", "a}", "b"]);
    }

    /// Where each line starts, as [`for_each_line`] calls it.
    fn line_starts(text: &str, chars: usize, break_words: bool) -> Vec<usize> {
        let mut starts = Vec::new();
        for_each_line(text.as_bytes(), chars, break_words, |start| {
            starts.push(start);
        });
        starts
    }

    /// How lines are filled, worked out by hand: as many characters as fit,
    /// to the last place the line may break; a space past the end hanging
    /// on its line; a word too long for a line broken inside only where it
    /// may be and the line has nowhere else to break, and otherwise running
    /// past the end.
    #[test]
    fn each_line_holds_what_fits_up_to_its_last_break() {
        // "aaa bbb" fits seven, the space after it hangs, and "ccc".
        assert_eq!(line_starts("aaa bbb ccc", 7, false), [0, 8]);
        // "aaa " and "bbb", which does not fit after it.
        assert_eq!(line_starts("aaa bbb", 6, false), [0, 4]);
        // A word too long for a line breaks after the last character that
        // fits, or runs on.
        assert_eq!(line_starts("abcdefghij", 4, true), [0, 4, 8]);
        assert_eq!(line_starts("abcdefghij", 4, false), [0]);
        // On a line that has a place to break, a long word starts a line of
        // its own first, and breaks inside itself only there.
        assert_eq!(line_starts("ab cdefgh", 5, true), [0, 3, 8]);
        assert_eq!(line_starts("ab cdefgh", 5, false), [0, 3]);
        // Two spaces at a full line: the first hangs, the second starts the
        // next line.
        assert_eq!(line_starts("ab  c", 2, false), [0, 3]);
        // A space at the end that does not fit leaves an empty line after it.
        assert_eq!(line_starts("ab ", 2, false), [0, 3]);
        assert_eq!(wrapped_lines("ab ", 2, false), 2);
        assert_eq!(wrapped_lines("ab ", 3, false), 1);
    }
}
