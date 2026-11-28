//! How the Help modal stands in a window of any size, and how a page is
//! set in two columns where the modal cannot have its own size.
//!
//! Nothing here touches the ECS: [`help_layout`] is a pure function of the
//! window's size, and [`HelpLayout::modal_offset`] of that and of what the
//! Match draws where the modal opens over it, which is what lets the tests
//! check every size the layouts are checked at. [`super`] spawns the modal
//! where this says and moves it when the window's size or the screen under
//! it changes; `ui::hud` writes the page in whichever arrangement this says.
//!
//! # Two arrangements
//!
//! Text never changes size, and nothing in the modal is scaled.
//!
//! * **One column** is the modal as it always was: [`ONE_COLUMN_MODAL`],
//!   with its page in one column that `bevy_ui` wraps at the column's width.
//!   It is used in every window with room for it and [`MARGIN`] round it:
//!   at least 844x784.
//! * **Two columns** are for every other window. The modal takes the window
//!   less [`MARGIN`] round it - held to [`TWO_COLUMN_MAX_WIDTH`], so that a
//!   column's line is never longer than the one column's - with tighter
//!   padding and gaps, and its page in two columns [`COLUMN_GAP`] apart;
//!   Close moves onto the pagination row, at its right end. A column holds
//!   [`HelpLayout::column_chars`] characters and
//!   [`HelpLayout::column_lines`] lines, both known before `bevy_ui` lays
//!   anything out because the font is monospaced (`ui::text_metrics`), and
//!   the game wraps the page itself ([`wrap_lines`]) and divides it between
//!   the columns ([`split_page`]). At the 1040x600 minimum a column holds 43
//!   characters and 19 lines.
//!
//! # Where the one column stands
//!
//! Over Home, in the middle of the window, as it always has. Over the
//! Match it keeps four promises, the earlier giving way to none of the
//! later: it stands inside the window, with [`MARGIN`] round it; the
//! heading's line stays whole, wholly clear of the modal's top edge or
//! wholly under the modal; the board is covered, or where it is larger than
//! the modal framed evenly round it, rather than showing a sliver on one
//! side and a band on the other; and a heading the modal stands under is
//! cleared by [`HEADING_GAP`], as in the opening window.
//!
//! The modal stands at the place nearest the middle of the window, under
//! the heading, that keeps all four. Where no place under the heading does,
//! it stands at the nearest over it, its top edge level with the heading's
//! top or above it, covering the heading. Where neither does - a wide Match
//! 831 to 839 px tall, and up to 848 px at 1063 px wide, whose board is a
//! little smaller than the modal - the gap gives way: the modal stands as
//! low as the window allows, its top edge 6.2 to 15 px under the heading's
//! line, still covering the board. In the opening window the nearest place
//! is the middle, the modal's top 70 px down as it always was.
//!
//! So a change of height of one pixel moves the modal by a pixel or less,
//! except where the Match changes arrangement and at one height for each
//! width, where the room under the heading runs out and the modal passes
//! over the heading's line in one step of 47 to 56 px: between 871 and
//! 872 px tall at 1040 px wide, where the Match is wide, between 849 and 850
//! at 1062, and between 830 and 831 from 1063 px wide on. Any place in
//! between would cross the heading's line, so no rule that keeps it whole
//! avoids that step. The tests hold the four promises at every size with
//! one column, and the step at every whole-pixel height of every width from
//! 1040 to 2560 px.

use bevy::prelude::*;

use crate::ui::text_metrics::{line_width, text_height, wrap_lines};

/// The modal as it always was, and its padding and the gap between its
/// parts.
pub(crate) const ONE_COLUMN_MODAL: Vec2 = Vec2::new(820.0, 760.0);
pub(crate) const ONE_COLUMN_PADDING: f32 = 32.0;
pub(crate) const ONE_COLUMN_GAP: f32 = 18.0;
/// The room the modal keeps from the window's edges: the one column needs
/// that much round it to be used, and two columns take the window less it.
pub(crate) const MARGIN: f32 = 12.0;
/// Two columns' padding, the gap between the modal's parts, and the gap
/// between the columns.
pub(crate) const TWO_COLUMN_PADDING: f32 = 24.0;
pub(crate) const TWO_COLUMN_GAP: f32 = 12.0;
pub(crate) const COLUMN_GAP: f32 = 32.0;
/// The modal's border.
pub(crate) const BORDER: f32 = 2.0;
/// The sizes of the title and the page.
pub(crate) const HELP_TITLE_SIZE: f32 = 30.0;
pub(crate) const BODY_SIZE: f32 = 18.0;
/// The pagination row's height, and Close's.
pub(crate) const PAGINATION_HEIGHT: f32 = 42.0;
pub(crate) const CLOSE_HEIGHT: f32 = 36.0;
/// The one column's width for the page: the modal's, less its padding and
/// border. No column of two is wider, so no line is longer.
pub(crate) const ONE_COLUMN_BODY_WIDTH: f32 =
    ONE_COLUMN_MODAL.x - 2.0 * (ONE_COLUMN_PADDING + BORDER);
/// The widest the two-column modal stands: two columns as wide as the one
/// column's page, and the padding, border and gap round them.
pub(crate) const TWO_COLUMN_MAX_WIDTH: f32 =
    2.0 * ONE_COLUMN_BODY_WIDTH + COLUMN_GAP + 2.0 * (TWO_COLUMN_PADDING + BORDER);
/// How far below the Match's heading line the one-column modal's top stands
/// in the opening window, to the pixel's fifth: what it keeps where it
/// moves down to clear the heading.
pub(crate) const HEADING_GAP: f32 = 15.0;

/// The arrangement a window gets; see the module docs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HelpArrangement {
    OneColumn,
    TwoColumns,
}

/// How the Help modal stands in one window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HelpLayout {
    /// The size laid out for: the window's, raised to the minimum.
    pub(crate) window: Vec2,
    pub(crate) arrangement: HelpArrangement,
    /// The modal's outer size, border included.
    pub(crate) modal_size: Vec2,
    /// In two columns: the width of each, how many characters a line of it
    /// holds, and how many lines it holds. In one column, the one column's
    /// width, and the characters and lines it is never asked to hold.
    pub(crate) column_width: f32,
    pub(crate) column_chars: usize,
    pub(crate) column_lines: usize,
}

/// Lays the Help modal out for a window of `window` logical pixels, which
/// the caller has already raised to the minimum with `WindowSize::layout`.
pub(crate) fn help_layout(window: Vec2) -> HelpLayout {
    let advance = line_width(1, BODY_SIZE);
    if window.cmpge(ONE_COLUMN_MODAL + 2.0 * MARGIN).all() {
        return HelpLayout {
            window,
            arrangement: HelpArrangement::OneColumn,
            modal_size: ONE_COLUMN_MODAL,
            column_width: ONE_COLUMN_BODY_WIDTH,
            column_chars: (ONE_COLUMN_BODY_WIDTH / advance).floor() as usize,
            column_lines: 0,
        };
    }
    let modal_size = Vec2::new(
        (window.x - 2.0 * MARGIN).min(TWO_COLUMN_MAX_WIDTH),
        window.y - 2.0 * MARGIN,
    );
    let inside = modal_size - 2.0 * Vec2::splat(TWO_COLUMN_PADDING + BORDER);
    let column_width = (inside.x - COLUMN_GAP) / 2.0;
    // The columns' room: the modal's inside less the title, the pagination
    // row and the gap under and over the columns.
    let room =
        inside.y - text_height(1, HELP_TITLE_SIZE) - PAGINATION_HEIGHT - 2.0 * TWO_COLUMN_GAP;
    HelpLayout {
        window,
        arrangement: HelpArrangement::TwoColumns,
        modal_size,
        column_width,
        column_chars: (column_width / advance).floor() as usize,
        column_lines: lines_in(room),
    }
}

/// How many lines of the page a column `room` px tall holds: as many as
/// `bevy_ui` lays out no taller than the room, a text box being rounded up
/// to a whole pixel.
fn lines_in(room: f32) -> usize {
    let line = text_height(1, BODY_SIZE);
    let mut lines = (room / line).floor() as usize;
    while lines > 0 && (lines as f32 * line).ceil() > room {
        lines -= 1;
    }
    lines
}

impl HelpLayout {
    pub(crate) fn two_columns(&self) -> bool {
        self.arrangement == HelpArrangement::TwoColumns
    }

    /// The modal's padding and the gap between its parts here.
    pub(crate) fn padding_and_gap(&self) -> (f32, f32) {
        if self.two_columns() {
            (TWO_COLUMN_PADDING, TWO_COLUMN_GAP)
        } else {
            (ONE_COLUMN_PADDING, ONE_COLUMN_GAP)
        }
    }

    /// How far below the middle of the window the modal stands: `over_match`
    /// is where the Match's heading line and board are, if the modal is open
    /// over the Match. See the module docs for the rule, which only the one
    /// column has: two columns take the whole window.
    pub(crate) fn modal_offset(&self, over_match: Option<(Rect, Rect)>) -> f32 {
        let Some((heading, board)) = over_match.filter(|_| !self.two_columns()) else {
            return 0.0;
        };
        let height = self.modal_size.y;
        let centred = (self.window.y - height) / 2.0;
        top_over_match(self.window.y, height, heading, board) - centred
    }

    /// The modal's box, border included, in logical pixels from the window's
    /// top-left: centred across the window, and down it by
    /// [`Self::modal_offset`].
    #[cfg(test)]
    pub(crate) fn modal_rect(&self, over_match: Option<(Rect, Rect)>) -> Rect {
        let centre = self.window / 2.0 + Vec2::new(0.0, self.modal_offset(over_match));
        Rect::from_center_size(centre, self.modal_size)
    }
}

/// Where the top edge of a modal `height` px tall stands over the Match, in
/// a window `window_height` px tall whose heading line and board are where
/// `heading` and `board` say: the place nearest the middle of the window
/// that keeps the module docs' four promises, under the heading or else
/// over it, and failing both as low as the window allows.
fn top_over_match(window_height: f32, height: f32, heading: Rect, board: Rect) -> f32 {
    let centred = (window_height - height) / 2.0;
    let lowest = window_height - MARGIN - height;
    // The tops that cover the board, from the lowest to the highest; over a
    // board larger than the modal, the one top that frames it evenly.
    let (covers_from, covers_to) = if board.height() <= height {
        (board.max.y - height, board.min.y)
    } else {
        let framed = board.center().y - height / 2.0;
        (framed, framed)
    };
    let from = covers_from.max(MARGIN);
    let to = covers_to.min(lowest);
    let nearest = |from: f32, to: f32| (from <= to).then(|| centred.clamp(from, to));
    nearest(from.max(heading.max.y + HEADING_GAP), to)
        .or_else(|| nearest(from, to.min(heading.min.y)))
        .unwrap_or(lowest)
}

/// Divides a page's wrapped `lines` between two columns of `room` lines
/// each. It breaks at a blank line between paragraphs where one keeps both
/// columns within the room - dropping the blank line - and, of those, at the
/// one that leaves the shorter tallest column, the first column the taller
/// where two leave the same. Where none does, it breaks within a paragraph,
/// after half the lines, the first column taking the odd one. The page fits
/// when both columns returned are within the room; a page longer than two
/// columns does not, and its tests say so.
pub(crate) fn split_page(lines: &[String], room: usize) -> (Vec<String>, Vec<String>) {
    let breaks = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.is_empty())
        .map(|(index, _)| (index, lines.len() - index - 1))
        .filter(|(left, right)| *left <= room && *right <= room);
    let best = breaks.min_by_key(|(left, right)| ((*left).max(*right), *right));
    match best {
        Some((at, _)) => (lines[..at].to_vec(), lines[at + 1..].to_vec()),
        None => {
            let half = lines.len().div_ceil(2);
            (lines[..half].to_vec(), lines[half..].to_vec())
        }
    }
}

/// A page's text set in two columns for `layout`: wrapped and divided, each
/// column's lines joined with its own line breaks.
pub(crate) fn page_columns(body: &str, layout: &HelpLayout) -> [String; 2] {
    let lines = wrap_lines(body, layout.column_chars);
    let (left, right) = split_page(&lines, layout.column_lines);
    [left.join("\n"), right.join("\n")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{NAMED_WINDOW_SIZES, WindowSize, layout_sweep};

    /// The opening window keeps the modal as it always was: 820x760 in the
    /// middle of the window, its page in one column 752 px wide.
    #[test]
    fn the_opening_window_keeps_the_one_column_modal() {
        let opening = help_layout(WindowSize::OPENING.0);
        assert_eq!(opening.arrangement, HelpArrangement::OneColumn);
        assert_eq!(opening.modal_size, Vec2::new(820.0, 760.0));
        assert_eq!(opening.column_width, 752.0);
        assert_eq!(opening.padding_and_gap(), (32.0, 18.0));
        assert_eq!(
            opening.modal_rect(None),
            Rect::new(110.0, 70.0, 930.0, 830.0)
        );
    }

    /// One column from 844x784, a pixel either side of both edges, which
    /// the sweep's 20 px steps cannot pin; two columns below either.
    #[test]
    fn one_column_needs_844_by_784() {
        let arrangement = |w: f32, h: f32| help_layout(Vec2::new(w, h)).arrangement;
        assert_eq!(arrangement(1040.0, 784.0), HelpArrangement::OneColumn);
        assert_eq!(arrangement(1040.0, 783.0), HelpArrangement::TwoColumns);
        assert_eq!(arrangement(1040.0, 783.9), HelpArrangement::TwoColumns);
        assert_eq!(arrangement(2560.0, 783.0), HelpArrangement::TwoColumns);
        // Below the minimum width only a browser page reaches; laid out at
        // the minimum, it is never narrow enough to matter, but the rule
        // reads both edges.
        assert_eq!(arrangement(844.0, 900.0), HelpArrangement::OneColumn);
        assert_eq!(arrangement(843.0, 900.0), HelpArrangement::TwoColumns);
    }

    /// The figures a test of the pages rests on: at the 1040x600 minimum a
    /// column holds 43 characters and 19 lines, and at 1280x641 54 and 21;
    /// a line of 43 characters is 464.4 px of the 466 px column. Worked from
    /// the modal's own numbers here rather than read back: the window less
    /// 12 px round it, 26 px of padding and border inside it, and a 32 px
    /// gap between the columns; the title's 36 px line, the 42 px pagination
    /// row and the two 12 px gaps under and over the columns.
    #[test]
    fn a_column_holds_the_characters_and_lines_its_room_allows() {
        for (window, chars, lines) in [
            (Vec2::new(1040.0, 600.0), 43, 19),
            (Vec2::new(1280.0, 641.0), 54, 21),
            (Vec2::new(1366.0, 689.0), 58, 23),
            (Vec2::new(1366.0, 768.0), 58, 27),
        ] {
            let layout = help_layout(window);
            assert!(layout.two_columns(), "{window}");
            let column = (window.x - 24.0 - 52.0 - 32.0) / 2.0;
            assert_eq!(layout.column_width, column, "{window}");
            assert_eq!(layout.column_chars, chars, "{window}");
            assert_eq!(layout.column_lines, lines, "{window}");
            let room = window.y - 24.0 - 52.0 - 36.0 - 42.0 - 24.0;
            assert!(
                (lines as f32 * 21.6).ceil() <= room && ((lines + 1) as f32 * 21.6).ceil() > room,
                "{window}: {lines} lines in {room}"
            );
        }
    }

    /// At every size, either arrangement: the modal stands inside the window
    /// with at least 12 px round it, and two columns never longer a line
    /// than the one column's. It is centred across the window by
    /// construction (`modal_rect`); the real-layout tests check where
    /// `bevy_ui` puts it.
    #[test]
    fn the_modal_stands_inside_every_window() {
        let mut arrangements = [false, false];
        for size in layout_sweep() {
            let layout = help_layout(size);
            arrangements[usize::from(layout.two_columns())] = true;
            for over in [None, Some(crate::ui::match_screen::heading_and_board(size))] {
                let modal = layout.modal_rect(over);
                let room = Rect::from_corners(Vec2::ZERO, size).inflate(-MARGIN + 1e-3);
                assert!(
                    room.contains(modal.min) && room.contains(modal.max),
                    "{size}: the modal {modal:?}"
                );
            }
            assert!(layout.column_chars <= 69, "{size}: {}", layout.column_chars);
            if layout.two_columns() {
                assert!(layout.column_lines >= 19, "{size}: {}", layout.column_lines);
            }
        }
        assert_eq!(arrangements, [true, true]);
    }

    /// Where the modal's top stands over the Match in a window of `size`,
    /// with where the Match's heading line and board stand there.
    fn over_match_at(size: Vec2) -> (f32, Rect, Rect) {
        use crate::ui::match_screen::heading_and_board;

        let (heading, board) = heading_and_board(size);
        let top = help_layout(size).modal_rect(Some((heading, board))).min.y;
        (top, heading, board)
    }

    /// Over the Match, at every whole-pixel size with one column from
    /// 1040x784 to 2560x1440, the four promises in the module docs: the
    /// modal inside the window with 12 px round it; the heading's line wholly
    /// clear of its top edge or wholly under it; the board under it, or
    /// framed evenly round it; and a heading it stands under cleared by
    /// 15 px - or, where the modal stands as low as the window allows, by
    /// 6.2 at the least. Over Home there is nothing to keep off: the middle
    /// of the window.
    #[test]
    fn over_the_match_the_modal_keeps_its_four_promises_at_every_size() {
        let mut framed = 0;
        let mut narrowed = 0;
        for width in 1040..=2560 {
            for height in 784..=1440 {
                let size = Vec2::new(width as f32, height as f32);
                let layout = help_layout(size);
                assert!(!layout.two_columns(), "{size}");
                let (top, heading, board) = over_match_at(size);
                let foot = top + layout.modal_size.y;
                let lowest = size.y - MARGIN - layout.modal_size.y;
                assert!(
                    top >= MARGIN - 1e-3 && top <= lowest + 1e-3,
                    "{size}: {top}"
                );
                let under = top >= heading.max.y - 1e-3;
                let over = top <= heading.min.y + 1e-3;
                assert!(under || over, "{size}: {top} crosses {heading:?}");
                let above = top - board.min.y;
                let below = board.max.y - foot;
                let covered = above <= 1e-3 && below <= 1e-3;
                let even = above > 0.0 && (above - below).abs() < 0.5;
                assert!(covered || even, "{size}: {above} above, {below} below");
                framed += usize::from(even);
                if under {
                    let gap = top - heading.max.y;
                    if gap < HEADING_GAP - 1e-3 {
                        assert!((top - lowest).abs() < 1e-3, "{size}: {gap} px under");
                        assert!(gap >= 6.2 - 1e-3, "{size}: {gap} px under");
                        narrowed += 1;
                    }
                }
            }
        }
        assert!(framed > 0, "some boards are larger than the modal");
        assert!(narrowed > 0, "the gap gives way somewhere");
        for size in NAMED_WINDOW_SIZES {
            let layout = help_layout(size);
            assert_eq!(layout.modal_rect(None).center(), size / 2.0, "{size}");
        }
    }

    /// The step a change of height of one pixel moves the modal by, over the
    /// Match, at every whole-pixel height from 784 to 1440 of every width from
    /// 1040 to 2560: a pixel or less, except where the Match changes
    /// arrangement - which moves its board by more than a pixel - and at
    /// exactly one height for each width, where the modal passes over the
    /// heading's line in one step, from over the heading to under it, of 47
    /// to 56.3 px: the heading's line and its gap, or the band's narrower
    /// gap.
    #[test]
    fn a_pixel_of_height_moves_the_modal_a_pixel_or_less_but_once() {
        for width in 1040..=2560 {
            let mut switches = Vec::new();
            let mut last: Option<(f32, Rect, Rect)> = None;
            for height in 784..=1440 {
                let size = Vec2::new(width as f32, height as f32);
                let now = over_match_at(size);
                if let Some((top, heading, board)) = last {
                    let step = (now.0 - top).abs();
                    let match_moved = (now.2.min - board.min).abs().max_element() > 1.0 + 1e-3
                        || (now.2.max - board.max).abs().max_element() > 1.0 + 1e-3;
                    if step > 1.0 + 1e-3 && !match_moved {
                        let was_over = top <= heading.min.y + 1e-3;
                        let is_under = now.0 >= now.1.max.y - 1e-3;
                        assert!(
                            was_over && is_under,
                            "{size}: a step of {step} px, from {top} to {}",
                            now.0
                        );
                        assert!(
                            (47.0 - 1e-3..=56.3 + 1e-3).contains(&step),
                            "{size}: a step of {step} px over the heading"
                        );
                        switches.push(height);
                    }
                }
                last = Some(now);
            }
            assert_eq!(switches.len(), 1, "{width}: {switches:?}");
        }
    }

    /// Both sides of every place the rule changes, to the pixel: the opening
    /// window and a pixel shorter; where the modal passes over the heading -
    /// 871 and 872 px tall at 1040 wide, 849 and 850 at 1062, and 830 and 831
    /// from 1063 wide on; and where the room under the heading runs out and
    /// the gap gives way - 848 and 849 at 1063 wide, 839 and 840 from 1072
    /// on - down to 6.2 px at 831. Over a board larger than the modal, as at
    /// 1920x1080, it frames the board evenly.
    #[test]
    fn the_rule_changes_where_the_module_docs_say() {
        let at = |w: f32, h: f32| over_match_at(Vec2::new(w, h));
        let top = |w: f32, h: f32| at(w, h).0;
        let gap = |w: f32, h: f32| {
            let (top, heading, _) = at(w, h);
            top - heading.max.y
        };
        let over = |w: f32, h: f32| {
            let (top, heading, _) = at(w, h);
            (top - heading.min.y).abs() < 1e-3
        };
        assert_eq!(top(1040.0, 900.0), 70.0);
        assert!((top(1040.0, 899.0) - 69.8).abs() < 1e-3);
        assert!((gap(1040.0, 899.0) - HEADING_GAP).abs() < 1e-3);
        for (width, last_over) in [
            (1040.0, 871.0),
            (1062.0, 849.0),
            (1063.0, 830.0),
            (1280.0, 830.0),
            (2560.0, 830.0),
        ] {
            assert!(over(width, last_over), "{width}x{last_over}");
            let first_under = last_over + 1.0;
            assert!(
                gap(width, first_under) >= 6.2 - 1e-3,
                "{width}x{first_under}"
            );
            let step = top(width, first_under) - top(width, last_over);
            assert!(
                (47.0 - 1e-3..=56.3 + 1e-3).contains(&step),
                "{width}x{first_under}: the one step, {step} px"
            );
        }
        assert!((gap(1280.0, 831.0) - 6.2).abs() < 1e-3);
        for (width, last_narrowed) in [(1063.0, 848.0), (1072.0, 839.0), (1280.0, 839.0)] {
            let lowest = last_narrowed - MARGIN - ONE_COLUMN_MODAL.y;
            assert!((top(width, last_narrowed) - lowest).abs() < 1e-3);
            assert!(
                gap(width, last_narrowed) < HEADING_GAP,
                "{width}x{last_narrowed}"
            );
            assert!(
                (gap(width, last_narrowed + 1.0) - HEADING_GAP).abs() < 1e-3,
                "{width}x{}",
                last_narrowed + 1.0
            );
        }
        let (top, _, board) = at(1920.0, 1080.0);
        let foot = top + ONE_COLUMN_MODAL.y;
        assert!(board.min.y < top && board.max.y > foot);
        assert!(((top - board.min.y) - (board.max.y - foot)).abs() < 0.5);
    }

    fn lines(counts: &[usize]) -> Vec<String> {
        // Paragraphs of the given line counts, a blank line between two.
        let mut out = Vec::new();
        for (index, count) in counts.iter().enumerate() {
            if index > 0 {
                out.push(String::new());
            }
            out.extend((0..*count).map(|line| format!("p{index} l{line}")));
        }
        out
    }

    /// The split, on pages whose best break is worked out by hand: the
    /// paragraph break that leaves the shorter tallest column; the first
    /// column the taller between two that tie; within a paragraph, after
    /// half its lines, only where no break fits.
    #[test]
    fn the_split_breaks_between_paragraphs_where_it_can() {
        // 5 + 6 + 7 lines: breaks give 5|14 and 12|7; only 12|7 fits 12.
        let (left, right) = split_page(&lines(&[5, 6, 7]), 12);
        assert_eq!((left.len(), right.len()), (12, 7));
        // 4 + 4 + 4: 4|9 and 9|4 tie on 9; the first column takes the 9.
        let (left, right) = split_page(&lines(&[4, 4, 4]), 10);
        assert_eq!((left.len(), right.len()), (9, 4));
        assert_eq!(right[0], "p2 l0", "the blank line is dropped");
        // 3 + 10 + 3 in columns of 9: 3|14 and 14|3 overflow, so the page
        // breaks within the middle paragraph after 9 of its 18 lines.
        let page = lines(&[3, 10, 3]);
        let (left, right) = split_page(&page, 9);
        assert_eq!((left.len(), right.len()), (9, 9));
        assert_eq!(left.last().unwrap(), "p1 l4");
        assert_eq!(right[0], "p1 l5");
    }
}
