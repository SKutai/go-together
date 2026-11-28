//! Where `bevy_ui` breaks the lines of the game's text, worked out
//! without laying it out: how many characters a line holds, how many lines
//! a text takes at so many characters to a line, and whether it fits a box
//! of so many lines. The Replay Viewer reads it to give its label the lines
//! it takes, and the Replay Library's search field and Settings' error line
//! to know whether a text fits them whole.
//!
//! The game's own strings, the Replay labels and the search queries are
//! printable ASCII, in a monospaced font (see [`crate::ui::text_metrics`]),
//! so a line's width is its length in characters, and where it may break
//! depends only on which characters stand around the place. Settings'
//! error line can carry anything, since it quotes a path or a setting's
//! value from the settings file, and [`fits`] takes a text with anything
//! else in it on one line only. For printable ASCII, all three parts of the
//! answer are rules, followed here exactly rather than estimated:
//!
//! * **How many characters a line holds** is what parley, the layout
//!   engine `bevy_ui` uses, counts: it adds the font's advance, as it
//!   scales it, one character at a time in single precision, and a
//!   character fits while the sum is no more than the line's width
//!   ([`chars_per_line`]). So a width that is an exact number of advances
//!   may hold one character fewer: thirty characters at 26 px add up to
//!   a little over 468 px, and 468 px holds twenty-nine.
//! * **Where a line may break** is the Unicode line-breaking algorithm,
//!   UAX #14, which parley takes from ICU. Printable ASCII falls into
//!   fourteen of its classes ([`Class`]), and [`may_break_before`] is its
//!   rules for those, as ICU applies them. A line may break after a space,
//!   unless what follows is one of `)` `]` `}` `!` `?` `,` `.` `:` `;` `/`,
//!   none of which starts a line, or the spaces follow `(` `[` `{`, after
//!   which no line breaks; so `(`, a quotation mark or `-` may start a line
//!   after a space. Between two other characters a line breaks after `-`
//!   or `/` that no digit follows, after `|` `!` `?`, and in a few other
//!   places beside punctuation, but never inside a word or a number, nor
//!   beside a quotation mark.
//! * **How lines are filled** is the way parley fills them: each line
//!   takes as many characters as fit, and when the next does not, the line
//!   ends at the last place on it that it may break. A space that does not
//!   fit hangs past the end of its line, and the line ends after it, so
//!   the rest of a run of spaces starts the next line, and a space at the
//!   very end of a text that does not fit leaves an empty line after it. A
//!   word too long for a line runs past its end, or, in a text set to break
//!   inside a word (`LineBreak::WordOrCharacter`), breaks after the last
//!   character that fits - but only on a line with no other place to break.
//!
//! The three were checked on 2026-11-28 against what they follow: the
//! breaks against ICU's segmenter (`icu_segmenter` 2.3) for every
//! printable ASCII string of up to four characters and two million longer
//! ones; the lines against parley 0.11 itself, with Bevy's default font,
//! for 650,000 texts of up to 60 characters at the sizes and widths the
//! Viewer sets them at; and the count of characters against parley at
//! 32,636 widths, every exact fit among them. They agreed on every one.
//! In the game, a test in `ui::replay::viewer` lays a corpus of labels out
//! with `bevy_ui` at several widths and holds where each of their lines
//! starts to this, character for character, which a change to any one rule
//! here fails. The search field's test holds its fit to what `bevy_ui`
//! draws both ways, laying each query it cuts out whole as well; the error
//! line's tests hold what it draws to its place.

/// The font's advance at `font_px`, in pixels at a scale factor of 1, as
/// parley scales it: 600 units of a 1000-unit em, the size divided by the
/// em before it multiplies the units, in single precision. At some sizes
/// that is a unit in the last place away from `0.6 * font_px`.
fn advance(font_px: f32) -> f32 {
    600.0 * (font_px / 1000.0)
}

/// How many characters at `font_px` a line `width` pixels wide holds, as
/// parley counts them at a scale factor of 1: the font's [`advance`] added
/// one character at a time, each character fitting while the sum is no
/// more than `width` - and at least one, which a line takes however narrow
/// it is.
pub(crate) fn chars_per_line(width: f32, font_px: f32) -> usize {
    let advance = advance(font_px);
    let (mut sum, mut chars) = (0.0_f32, 0);
    while sum + advance <= width {
        sum += advance;
        chars += 1;
    }
    chars.max(1)
}

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
pub(crate) fn may_break_before(text: &[u8], index: usize) -> bool {
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
pub(crate) fn for_each_line(
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
pub(crate) fn wrapped_lines(text: &str, chars: usize, break_words: bool) -> usize {
    let mut lines = 0;
    for_each_line(text.as_bytes(), chars, break_words, |_| lines += 1);
    lines
}

/// Whether `text`, set with `LineBreak::WordBoundary` at `chars` characters
/// to a line, takes no more than `lines` lines and runs past the end of
/// none of them, as a word too long for a line runs on past it. Only
/// printable ASCII is followed to its breaks: a text with anything else in
/// it is taken to fit only where it needs none, on one line of no more than
/// `chars` characters, each taken for one advance as
/// [`crate::ui::text_metrics`] takes them, and with no control character,
/// a line break of its own among them.
pub(crate) fn fits(text: &str, chars: usize, lines: usize) -> bool {
    let bytes = text.as_bytes();
    if !bytes.iter().all(|byte| (b' '..=b'~').contains(byte)) {
        return lines > 0 && text.chars().count() <= chars && !text.chars().any(char::is_control);
    }
    let mut starts = Vec::new();
    for_each_line(bytes, chars, false, |start| starts.push(start));
    if starts.len() > lines {
        return false;
    }
    starts.push(bytes.len());
    starts.windows(2).all(|line| {
        let line = &bytes[line[0]..line[1]];
        // A space that does not fit hangs past the line's end, unseen.
        let hanging = line.iter().rev().take_while(|byte| **byte == b' ').count();
        line.len() - hanging <= chars
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether a text fits a box of so many lines, on texts whose lines are
    /// worked out by hand, ten characters to a line: a hyphen and a slash
    /// give a word places to break; a space before `!` or a closing bracket
    /// is no place to break (LB13), so the word before it moves down and a
    /// second line can spill onto a third; a word too long for a line runs
    /// past it; a space that does not fit hangs unseen; and a text with
    /// anything but printable ASCII in it is taken to fit on one line only,
    /// and one with a line break of its own not at all.
    #[test]
    fn a_text_fits_where_bevy_ui_sets_it_inside_its_lines() {
        // "abcde-fgh-" and "ijklmn": a break after a hyphen.
        assert!(fits("abcde-fgh-ijklmn", 10, 2));
        assert!(fits("abcde/fgh/ijklmn", 10, 2));
        assert!(!fits("abcde-fgh-ijklmn", 10, 1));
        // Spaces alone would set "aaaa bbbb" and "! cc dd"; LB13 keeps
        // "bbbb" with the "!", so "aaaa ", "bbbb ! cc " and "dd".
        assert!(!fits("aaaa bbbb ! cc dd", 10, 2));
        assert!(fits("aaaa bbbb ! cc dd", 10, 3));
        assert!(!fits("aaaa bbbb ) cc dd", 10, 2));
        // A word longer than a line runs on past it.
        assert!(!fits("abcdefghijk", 10, 2));
        assert!(fits("abcdefghij", 10, 1));
        // "abcdefghij " - the space hangs past the line's end.
        assert!(fits("abcdefghij klmno", 10, 2));
        assert!(fits("caf\u{e9} au lait", 12, 1));
        assert!(!fits("caf\u{e9} au lait", 11, 2));
        assert!(!fits("one\ntwo", 10, 2));
    }

    /// How many characters a line holds, against counts parley gave on
    /// 2026-11-28 at widths an exact number of advances wide and a
    /// thousandth of a pixel either side: at 26 px, 16 px and 18 px an
    /// exact fit holds one character fewer than it seems to, because the
    /// sum of the advances comes out a little over it; at 14 px it holds
    /// them all, because the sum comes out a little under.
    #[test]
    fn a_line_holds_the_characters_whose_advances_fit_it() {
        for (font_px, width, chars) in [
            (26.0, 233.999, 14),
            (26.0, 234.0, 14),
            (26.0, 234.001, 15),
            (26.0, 468.0, 29),
            (26.0, 468.001, 30),
            (26.0, 498.0, 31),
            (26.0, 499.2, 31),
            (26.0, 499.201, 32),
            (16.0, 240.0, 24),
            (16.0, 240.001, 25),
            (18.0, 162.0, 14),
            (18.0, 162.001, 15),
            (14.0, 209.999, 24),
            (14.0, 210.0, 25),
        ] {
            assert_eq!(
                chars_per_line(width, font_px),
                chars,
                "{width} px at {font_px} px"
            );
        }
        assert_eq!(chars_per_line(1.0, 26.0), 1, "a line takes one at least");
        assert_eq!(advance(26.0), 0.6 * 26.0);
        assert_ne!(advance(18.0), 0.6 * 18.0, "a unit in the last place apart");
    }

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
