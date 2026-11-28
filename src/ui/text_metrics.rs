//! How long and how tall a line of the game's text is drawn, known without
//! laying it out.
//!
//! Every text in the game is set in Bevy's bundled default font, which is
//! monospaced, and in printable ASCII, which `crate::architecture` holds
//! every production string to. So a line's advance is its length in
//! characters times one advance, and a line box is a fixed multiple of the
//! font's size. A layout that has to know before `bevy_ui` runs whether a
//! line fits, or how tall it stands, reads these: the Match screen's title
//! row, which chooses between one line and two for its clocks; Home,
//! Settings and the Replay Library, which leave room for their titles and
//! notes; the Library's search field and Settings' error line, which cut a
//! text to the characters their lines hold; the Replay Viewer's header,
//! which keeps room for as many lines as its label takes; and the Help
//! modal, whose two columns hold a known number of characters and lines.
//! `bevy_ui` is still what lays the text out and draws it, and the test
//! here holds the two to each other.
//!
//! A different font, or Bevy laying text out differently, is what would
//! make these untrue, and the test is what would say so.

/// The font's advance, as a share of its size: 600 of the 1000 units of
/// its em.
const ADVANCE_PER_PX: f32 = 0.6;
/// The height of one line box, as a share of the font's size: Bevy's
/// default line height.
const LINE_HEIGHT_PER_PX: f32 = 1.2;

/// The advance of a line of `chars` characters at `font_px`, in logical
/// pixels: the width `bevy_ui` gives its node.
pub(crate) const fn line_width(chars: usize, font_px: f32) -> f32 {
    chars as f32 * ADVANCE_PER_PX * font_px
}

/// The height of `lines` lines at `font_px`, in logical pixels: the height
/// `bevy_ui` gives their node.
pub(crate) const fn text_height(lines: usize, font_px: f32) -> f32 {
    lines as f32 * LINE_HEIGHT_PER_PX * font_px
}

/// What marks text cut away to fit a line: three full stops, since the
/// game's own strings are printable ASCII (`crate::architecture`) and the
/// font draws anything else, an ellipsis among them, as a missing glyph.
pub(crate) const CUT_MARK: &str = "...";

/// Wraps `text` at `chars` characters a line, greedily, as `bevy_ui` wraps
/// at spaces: each line takes as many whole words as fit, a word is split
/// only where it is longer than a line, and the text's own line breaks are
/// kept, so a blank line between paragraphs stays one blank line. It counts
/// characters rather than bytes, for a text that quotes what the player
/// wrote. For a layout that has to know how many lines a text takes, or
/// sets them itself, before `bevy_ui` lays it out.
pub(crate) fn wrap_lines(text: &str, chars: usize) -> Vec<String> {
    let chars = chars.max(1);
    let mut lines = Vec::new();
    for source in text.split('\n') {
        let mut line = String::new();
        let mut length = 0;
        for word in source.split(' ').filter(|word| !word.is_empty()) {
            let mut word = word;
            let mut word_length = word.chars().count();
            if length > 0 && length + 1 + word_length <= chars {
                line.push(' ');
                line.push_str(word);
                length += 1 + word_length;
                continue;
            }
            if length > 0 {
                lines.push(std::mem::take(&mut line));
            }
            while word_length > chars {
                let at = word
                    .char_indices()
                    .nth(chars)
                    .map_or(word.len(), |(index, _)| index);
                let (head, tail) = word.split_at(at);
                lines.push(head.to_owned());
                word = tail;
                word_length -= chars;
            }
            line.push_str(word);
            length = word_length;
        }
        lines.push(line);
    }
    lines
}

/// The leading above a line's em box and below it, in logical pixels: half
/// of what a line box adds to the font's size. Two line boxes may meet by
/// this much without any of their glyphs doing so.
#[cfg(test)]
pub(crate) const fn half_leading(font_px: f32) -> f32 {
    (LINE_HEIGHT_PER_PX - 1.0) / 2.0 * font_px
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use bevy::ui::ComputedNode;

    /// The greedy wrap, on lines whose breaks are worked out by hand: words
    /// fill a line to its last character and no further, the text's own
    /// line breaks and blank lines stay, a word longer than a line is split,
    /// and characters are counted, not bytes.
    #[test]
    fn the_wrap_fills_each_line_with_whole_words() {
        assert_eq!(
            wrap_lines("one two three four", 9),
            ["one two", "three", "four"]
        );
        assert_eq!(wrap_lines("one two three", 13), ["one two three"]);
        assert_eq!(wrap_lines("one two three", 12), ["one two", "three"]);
        assert_eq!(wrap_lines("a b\n\nc d\ne", 3), ["a b", "", "c d", "e"]);
        assert_eq!(wrap_lines("abcdefgh ij", 3), ["abc", "def", "gh", "ij"]);
        assert_eq!(
            wrap_lines("caf\u{e9} \u{e9}t\u{e9}", 4),
            ["caf\u{e9}", "\u{e9}t\u{e9}"]
        );
        assert_eq!(
            wrap_lines("\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}", 2),
            ["\u{e9}\u{e9}", "\u{e9}\u{e9}", "\u{e9}"]
        );
    }

    /// Every size the game sets text at that a layout reads these for,
    /// laid out by `bevy_ui` with the game's font and read back: each node
    /// is as wide as the estimate and as tall, to the pixel the layout
    /// rounds to. The texts are the longest the Match screen's title row
    /// and corners are made to hold, two of them on two lines; the titles
    /// Home, Settings, the Replay Library and the Help modal leave room
    /// for, the Help modal's the longest, at their sizes; and the longest
    /// label the Replay Viewer's header holds, on one.
    #[test]
    fn the_estimates_are_what_bevy_ui_lays_the_game_font_out_at() {
        use crate::app::visuals::WindowSize;
        use crate::ui::focus::real_layout_app;

        let texts: [(&str, f32); 13] = [
            ("Go Together", 68.0),
            ("Match Replays", 38.0),
            ("Terms: Gray, Suicide, Ko & Territory", 30.0),
            (
                "Saturday afternoon rematch against my brother, best of three",
                26.0,
            ),
            ("GO TOGETHER", 34.0),
            ("WHITE 789", 24.0),
            ("TIME  999:59", 18.0),
            ("TIME\n999:59", 18.0),
            ("Round 9999  |  60:00 left", 17.0),
            ("Round 9999\n60:00 left", 17.0),
            (
                "Match Time Limit, Round Limit, No Board Change Limit, Point Target",
                16.0,
            ),
            ("Captured 81", 15.0),
            ("Select: Down", 14.0),
        ];
        let mut app = real_layout_app(WindowSize::OPENING.0);
        for (content, size) in texts {
            app.world_mut().spawn((
                Text::new(content),
                TextFont {
                    font_size: FontSize::Px(size),
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
            ));
        }
        app.update();
        app.update();

        let world = app.world_mut();
        let mut nodes = world.query::<(&Text, &ComputedNode)>();
        let mut measured = 0;
        for (text, node) in nodes.iter(world) {
            let Some((content, size)) = texts.iter().find(|(content, _)| *content == text.0) else {
                continue;
            };
            let lines: Vec<&str> = content.lines().collect();
            let longest = lines.iter().map(|line| line.len()).max().unwrap();
            let drawn = node.size() * node.inverse_scale_factor;
            let expected = Vec2::new(line_width(longest, *size), text_height(lines.len(), *size));
            assert!(
                (drawn - expected).abs().max_element() < 1.0,
                "{content:?} at {size} px is laid out {drawn}, estimated {expected}"
            );
            measured += 1;
        }
        assert_eq!(measured, texts.len(), "every text was laid out and read");
    }
}
