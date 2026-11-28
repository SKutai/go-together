//! Where everything on the Match screen stands, for a window of any size.
//!
//! Nothing here touches the ECS: [`match_layout`] is a pure function of the
//! window's size, which is what lets the tests run it at every size from
//! the minimum to 2560x1440 in steps of 20 px, at the named sizes, at a few
//! that a fractional scale factor gives and at a few larger windows, and
//! check that nothing overlaps and nothing leaves the window.
//! [`super::layout`] spawns the furniture where this says, and moves it
//! whenever the window's size changes.
//!
//! # Two arrangements
//!
//! Text never changes size. The board is the one thing that grows and
//! shrinks, with everything drawn on it, from 0.70 of its own size at the
//! smallest window up to twice it, and the camera does that
//! ([`MatchLayout::framing`]), so the board's own geometry is the same at
//! every size. The interface finds its room by moving:
//!
//! * **Tall** is the 1040x900 layout: the heading across the top with the
//!   Match timer and the round panel under it, each player's stack in a top
//!   corner, and the end-gesture line and the two bars under the board.
//!   Laid out in that window it is exactly the screen as it has always
//!   been, pixel for pixel.
//! * **Wide** puts things side by side. The board fills the height between
//!   a title row and the band under the board; each player's stack - name,
//!   Home or Help, capture count, Round 0 dots and control hints - stands
//!   beside the board, from the title row down; the round panel moves to
//!   the heading's left and the Match timer to its right; and the line of
//!   end reasons stays under the heading. The end-gesture line and both bars
//!   stay under the board, where the Help pages say the bar is.
//!
//! Either arrangement keeps the same room around its board at every size,
//! and its stacks the same distance from it. In a window that leaves more
//! room than that - one taller than tall needs, a wide one whose width
//! limits the board, or one past the size where the board reaches twice
//! its own - the whole of it, board and interface together, stands centred
//! down the window as one piece.
//!
//! Wide is used only where it would draw the board more than [`WIDE_GAIN`]
//! times as large as tall would - 4% larger - so a window about the opening
//! one's shape, or taller, keeps the familiar layout: 1040x900 is tall. The
//! choice is made from the room each arrangement leaves the board before
//! the board's cap, so it follows the window's shape alone: a window keeps
//! its arrangement as it grows past the size where both boards would reach
//! twice their size, and 2560x1600 and 3840x2160 are wide, as 2560x1440 is.
//! Near the boundary a slightly larger window can get a board up to 4%
//! smaller, as it crosses back into tall; the margin is what keeps the
//! change of arrangement worth making.
//!
//! # Making room
//!
//! A short window has less room above and beside the board than it seems,
//! because nothing but the board may shrink. Three things follow, each held
//! by the tests:
//!
//! * The line of end reasons is never wrapped. Its longest - every
//!   automatic limit at once, [`END_REASONS_MAX_CHARS`] characters - shares
//!   its band with the Home and Help buttons in wide, so in the smallest
//!   windows the two stacks stand further from the board than their usual
//!   [`STACK_GAP`], far enough to keep that line clear of the buttons.
//! * The round panel stands beside the heading on one line where its
//!   longest - a four-digit round with an hour of Round Time left - fits
//!   there, and otherwise on two, the round over its countdown. Which is
//!   [`MatchLayout::round_lines`], and the display pass writes the text to
//!   match. The Match timer, past 999 minutes at its longest, fits on one
//!   line on its side at every size, and the tests hold it to that.
//! * The band under the board holds two lines of end-gesture text or one
//!   line and a bar, never both at once: a hold in progress replaces the
//!   text with its short label, so the long restart line, wrapped by a
//!   Replay-save notice, is only ever shown with no bar under it.
//!
//! The widths come from [`crate::ui::text_metrics`], for the longest text
//! each place can hold. Tests in `super` hold each budget to the text the
//! screen really writes, and its real-layout tests hold the whole layout to
//! what `bevy_ui` draws.

use bevy::prelude::*;

use super::layout::{
    BAR_TRACK_HEIGHT, BOARD_OUTER_SIZE, BOTTOM_OVERLAY_ROW_GAP, CONTROL_HINT_WIDTH,
    END_REASONS_SIZE, GESTURE_SIZE, HEADING_SIZE, HUD_BUTTON_SIZE, PLAYER_HEADING_SIZE,
    ROUND_PANEL_SIZE, UNDER_HEADING_ROW_GAP,
};
use crate::app::visuals::{BOARD_CENTER_Y, WorldFraming};
use crate::ui::text_metrics::{line_width, text_height};

/// How much more room wide must give the board than tall before it is
/// used: 4%.
pub(super) const WIDE_GAIN: f32 = 1.04;
/// The most the board grows: twice its own size.
pub(super) const MAX_ZOOM: f32 = 2.0;

/// The room the 1040x900 layout keeps around its board: above it for the
/// heading and the column under it, below it for the end-gesture line and
/// the bars, and on each side for a player's stack. Tall keeps exactly this
/// room around a board of any size.
const TALL_TOP: f32 = 145.0;
const TALL_BOTTOM: f32 = 101.0;
const TALL_SIDE: f32 = 193.0;
/// Tops in tall, from the top of its piece, which is the top of the
/// window whenever the window is no taller than the piece.
const TALL_HEADING_TOP: f32 = 14.0;
const TALL_COLUMN_TOP: f32 = 54.0;
/// How far the bottom overlay stands above the bottom of the tall piece.
const TALL_OVERLAY_BOTTOM: f32 = 18.0;

/// The margin wide keeps from the top and bottom of its piece, which are
/// the window's whenever the board fills the height, and the least it keeps
/// at the sides.
const EDGE: f32 = 12.0;
/// The title row: the heading's one line, at the window's top margin.
pub(super) const TITLE_ROW_HEIGHT: f32 = text_height(1, HEADING_SIZE);
const END_REASONS_LINE: f32 = text_height(1, END_REASONS_SIZE);
/// The gap between the board and the bands of interface above and below it.
const BOARD_GAP: f32 = 8.0;
const GESTURE_LINE: f32 = text_height(1, GESTURE_SIZE);
/// The bottom overlay's row gap above a bar, and the bar's track.
const BAR_SLOT: f32 = BOTTOM_OVERLAY_ROW_GAP + BAR_TRACK_HEIGHT;
/// The height the band under the board needs: two lines of end-gesture
/// text, or one and the slot the bars share - whichever is taller. The
/// module docs say why never both.
const BOTTOM_CONTENT: f32 = if 2.0 * GESTURE_LINE > GESTURE_LINE + BAR_SLOT {
    2.0 * GESTURE_LINE
} else {
    GESTURE_LINE + BAR_SLOT
};
const _: () = assert!(
    BOTTOM_CONTENT >= GESTURE_LINE + BAR_SLOT && BOTTOM_CONTENT >= 2.0 * GESTURE_LINE,
    "the band under the board holds either of its two loads"
);
/// The room wide keeps above and below the board.
const WIDE_TOP: f32 =
    EDGE + TITLE_ROW_HEIGHT + UNDER_HEADING_ROW_GAP + END_REASONS_LINE + BOARD_GAP;
const WIDE_BOTTOM: f32 = EDGE + BOTTOM_CONTENT + BOARD_GAP;

/// A player's stack, from its tops in the 1040x900 layout: name, Home or
/// Help, capture count, Round 0 dots, control hints. The count and the dots
/// are never shown at once: a count shows only from the first round, and
/// the dots only in Round 0.
const STACK_TOPS: [f32; 5] = [58.0, 94.0, 140.0, 174.0, 214.0];
/// How far a wide stack rises from those tops, so that its first line, the
/// player's name, stands at its piece's top margin, level with the heading.
const WIDE_STACK_RISE: f32 = STACK_TOPS[0] - EDGE;
/// From a stack's outer edge to where its name and its button start.
const STACK_INSET: f32 = 10.0;
/// The gap between a stack's inner edge - the edge of its control-hint
/// card - and the board.
pub(super) const STACK_GAP: f32 = 19.0;
/// From a stack's outer edge to the board, as the 1040x900 corners stand.
const STACK_FROM_BOARD: f32 = CONTROL_HINT_WIDTH + STACK_GAP;
/// The least room beside a wide board: a stack, and the margin outside it.
const WIDE_SIDE: f32 = STACK_FROM_BOARD + EDGE;
/// The least gap this layout leaves between two things it sets side by side.
const CLEARANCE: f32 = 12.0;
/// The gap between the heading and the round panel or the timer beside it.
const TITLE_GAP: f32 = 24.0;

/// The longest text each place holds, in characters of the game's
/// monospaced font; `super`'s tests hold each to the text written there.
/// The heading: `GO TOGETHER`, or a winner, `White Wins!`.
pub(super) const HEADING_MAX_CHARS: usize = 11;
/// A corner's name with a final score of up to three digits: `WHITE 789`.
pub(super) const PLAYER_HEADING_MAX_CHARS: usize = 9;
/// Every automatic end reason at once.
pub(super) const END_REASONS_MAX_CHARS: usize = 66;
/// The round panel with a four-digit round and an hour of Round Time left:
/// on one line, and the longer of its two lines.
pub(super) const ROUND_PANEL_MAX_CHARS: [usize; 2] = [25, 10];
/// The Match timer past 999 minutes, which the tests hold to fitting on
/// one line beside the heading at every size.
#[cfg(test)]
pub(super) const TIMER_MAX_CHARS: usize = 12;

/// Which of the two arrangements a window gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Arrangement {
    Tall,
    Wide,
}

/// How many lines a readout beside the heading is written on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lines {
    One,
    Two,
}

impl Lines {
    /// Which of a `[one line, two lines]` pair this is.
    #[cfg(test)]
    pub(super) fn pick<T: Copy>(self, pair: [T; 2]) -> T {
        match self {
            Self::One => pair[0],
            Self::Two => pair[1],
        }
    }
}

/// One of the five things in a player's stack, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StackItem {
    Name,
    Button,
    CaptureCount,
    OpeningDots,
    ControlHint,
}

impl StackItem {
    fn opening_top(self) -> f32 {
        STACK_TOPS[match self {
            Self::Name => 0,
            Self::Button => 1,
            Self::CaptureCount => 2,
            Self::OpeningDots => 3,
            Self::ControlHint => 4,
        }]
    }
}

/// Where everything on the Match screen stands in one window. Every
/// length is in logical pixels from the window's top-left; the module docs
/// give the rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MatchLayout {
    /// The size laid out for: the window's, raised to the minimum.
    pub(super) window: Vec2,
    pub(super) arrangement: Arrangement,
    /// The board's wood, edge to edge, where it is drawn.
    pub(super) board: Rect,
    /// How far down the window the piece - board and interface together -
    /// stands: half the height the window has beyond what the piece needs,
    /// and zero wherever the board fills the height.
    drop: f32,
    /// How far each stack's outer edge stands from its side of the window.
    stack_edge: f32,
    /// How the round panel is written; always one line in tall, under the
    /// heading.
    pub(super) round_lines: Lines,
}

/// Lays the Match screen out for a window of `window` logical pixels,
/// which the caller has already raised to the minimum with
/// `WindowSize::layout`.
///
/// The arrangement is chosen from the room each leaves the board before
/// the board is capped at twice its size, so that the choice follows the
/// window's shape; the cap is applied to the one chosen.
pub(super) fn match_layout(window: Vec2) -> MatchLayout {
    let (tall, wide) = (tall_room(window), wide_room(window));
    let capped = |room: f32| room.min(BOARD_OUTER_SIZE * MAX_ZOOM);
    if wide > tall * WIDE_GAIN {
        wide_layout(window, capped(wide))
    } else {
        tall_layout(window, capped(tall))
    }
}

/// The board tall would draw in `window`, before the cap: the room it
/// keeps around a board, taken from the window.
fn tall_room(window: Vec2) -> f32 {
    (window.x - 2.0 * TALL_SIDE).min(window.y - TALL_TOP - TALL_BOTTOM)
}

/// The board wide would draw in `window`, before the cap: the height
/// between the title row and the band under the board, or the width
/// between the least room for the two stacks, whichever is less.
fn wide_room(window: Vec2) -> f32 {
    (window.y - WIDE_TOP - WIDE_BOTTOM).min(window.x - 2.0 * WIDE_SIDE)
}

fn tall_layout(window: Vec2, size: f32) -> MatchLayout {
    let drop = (window.y - (TALL_TOP + size + TALL_BOTTOM)) / 2.0;
    let left = (window.x - size) / 2.0;
    let top = drop + TALL_TOP;
    MatchLayout {
        window,
        arrangement: Arrangement::Tall,
        board: Rect::new(left, top, left + size, top + size),
        drop,
        stack_edge: left - STACK_FROM_BOARD,
        round_lines: Lines::One,
    }
}

fn wide_layout(window: Vec2, size: f32) -> MatchLayout {
    let drop = (window.y - (WIDE_TOP + size + WIDE_BOTTOM)) / 2.0;
    let left = (window.x - size) / 2.0;
    let top = drop + WIDE_TOP;
    // The stacks stand their usual gap from the board, unless the line of
    // end reasons needs them further out: in wide it shares its band with
    // the Home and Help buttons, which start a stack's inset in.
    let buttons_clear = window.x / 2.0
        - wide_end_reasons_width() / 2.0
        - CLEARANCE
        - HUD_BUTTON_SIZE.x
        - STACK_INSET;
    let stack_edge = (left - STACK_FROM_BOARD).min(buttons_clear);
    let one_line = line_width(ROUND_PANEL_MAX_CHARS[0], ROUND_PANEL_SIZE);
    MatchLayout {
        window,
        arrangement: Arrangement::Wide,
        board: Rect::new(left, top, left + size, top + size),
        drop,
        stack_edge,
        round_lines: if one_line <= title_slot_width(window, stack_edge) {
            Lines::One
        } else {
            Lines::Two
        },
    }
}

/// The width the end-reason line is held to in wide: its longest line, to
/// the whole pixel above.
fn wide_end_reasons_width() -> f32 {
    line_width(END_REASONS_MAX_CHARS, END_REASONS_SIZE).ceil()
}

/// The room on each side of the heading in wide's title row: from the
/// heading, less its gap, out to the longest name a stack can show, less
/// the clearance.
fn title_slot_width(window: Vec2, stack_edge: f32) -> f32 {
    let beside_heading = window.x / 2.0 - heading_width() / 2.0 - TITLE_GAP;
    let after_name = stack_edge + STACK_INSET + player_heading_width() + CLEARANCE;
    beside_heading - after_name
}

fn heading_width() -> f32 {
    line_width(HEADING_MAX_CHARS, HEADING_SIZE)
}

fn player_heading_width() -> f32 {
    line_width(PLAYER_HEADING_MAX_CHARS, PLAYER_HEADING_SIZE)
}

impl MatchLayout {
    /// How large the board is drawn against its own size.
    pub(super) fn zoom(&self) -> f32 {
        self.board.width() / BOARD_OUTER_SIZE
    }

    /// The main camera's framing that draws the board where this layout
    /// puts it: the board's centre, world-space `(0, BOARD_CENTER_Y)`, at
    /// the centre of [`Self::board`], at [`Self::zoom`].
    pub(super) fn framing(&self) -> WorldFraming {
        WorldFraming::placing(
            Vec2::new(0.0, BOARD_CENTER_Y),
            self.board.center(),
            self.window,
            self.zoom(),
        )
    }

    /// The heading's top: in wide, the top of the title row.
    pub(super) fn heading_top(&self) -> f32 {
        self.drop
            + match self.arrangement {
                Arrangement::Tall => TALL_HEADING_TOP,
                Arrangement::Wide => EDGE,
            }
    }

    /// The top of the column under the heading, and the room it keeps at
    /// its own top. In tall the column holds the end reasons, the timer and
    /// the round panel, one under another; in wide it starts at the title
    /// row, which it leaves to the timer and the round panel standing in
    /// it, so the end reasons start under the row.
    pub(super) fn column_top_and_padding(&self) -> (f32, f32) {
        match self.arrangement {
            Arrangement::Tall => (self.drop + TALL_COLUMN_TOP, 0.0),
            Arrangement::Wide => (self.drop + EDGE, TITLE_ROW_HEIGHT + UNDER_HEADING_ROW_GAP),
        }
    }

    /// The most the line of end reasons is held to: the board's own width
    /// in tall, and its own longest line in wide, where the buttons beside
    /// it are kept clear of exactly that.
    pub(super) fn end_reasons_max_width(&self) -> f32 {
        match self.arrangement {
            Arrangement::Tall => BOARD_OUTER_SIZE,
            Arrangement::Wide => wide_end_reasons_width(),
        }
    }

    /// Whether the round panel and the timer stand in a title row beside
    /// the heading, as they do in wide, rather than under it.
    pub(super) fn has_title_row(&self) -> bool {
        self.arrangement == Arrangement::Wide
    }

    /// In wide's title row, the round panel's right edge - the heading's
    /// left, less its gap - and, mirrored, the timer's left edge.
    pub(super) fn title_inner_edges(&self) -> (f32, f32) {
        let half = heading_width() / 2.0 + TITLE_GAP;
        (self.window.x / 2.0 - half, self.window.x / 2.0 + half)
    }

    /// The width each side of the title row leaves the round panel and
    /// the timer: the timer's whole slot is its node's width.
    pub(super) fn title_slot_width(&self) -> f32 {
        title_slot_width(self.window, self.stack_edge)
    }

    /// The top of one item in each player's stack.
    pub(super) fn stack_top(&self, item: StackItem) -> f32 {
        self.drop
            + match self.arrangement {
                Arrangement::Tall => item.opening_top(),
                Arrangement::Wide => item.opening_top() - WIDE_STACK_RISE,
            }
    }

    /// How far one item in a stack stands in from its side of the window:
    /// `left` for Player Two's stack, `right` for Player One's.
    pub(super) fn stack_inset(&self, item: StackItem) -> f32 {
        match item {
            StackItem::Name | StackItem::Button => self.stack_edge + STACK_INSET,
            StackItem::CaptureCount | StackItem::OpeningDots | StackItem::ControlHint => {
                self.stack_edge
            }
        }
    }

    /// How far the bottom overlay - the end-gesture line and the bars'
    /// slot - stands above the window's bottom.
    pub(super) fn overlay_bottom(&self) -> f32 {
        self.drop
            + match self.arrangement {
                Arrangement::Tall => TALL_OVERLAY_BOTTOM,
                Arrangement::Wide => EDGE,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{NAMED_WINDOW_SIZES, WindowSize, layout_sweep};
    use crate::rules::Player;
    use crate::ui::match_screen::capture::CAPTURE_COUNT_FONT_PX;
    use crate::ui::match_screen::layout::{
        CONTROL_HINT_MIN_HEIGHT, END_REASONS_GAP_BELOW, MATCH_COUNTDOWN_CARD, TIMER_SIZE,
        match_countdown_top,
    };
    use crate::ui::match_screen::opening_dots::ROW_HEIGHT;
    use crate::ui::text_metrics::{half_leading, text_height};

    fn layout(width: f32, height: f32) -> MatchLayout {
        match_layout(WindowSize(Vec2::new(width, height)).layout())
    }

    const STACK: [StackItem; 5] = [
        StackItem::Name,
        StackItem::Button,
        StackItem::CaptureCount,
        StackItem::OpeningDots,
        StackItem::ControlHint,
    ];

    /// A line of text at its largest, as a box to check for overlaps: the
    /// em box of its lines, which is its line box less the leading, so two
    /// lines whose boxes meet only by their leading - the heading and the
    /// first line under it do by 0.8 px - do not count as meeting.
    fn text_box(left: f32, top: f32, chars: usize, lines: usize, font_px: f32) -> Rect {
        let leading = half_leading(font_px);
        Rect::new(
            left,
            top + leading,
            left + line_width(chars, font_px),
            top + text_height(lines, font_px) - leading,
        )
    }

    /// Every box `layout` makes room for, at its largest, labelled: the
    /// longest heading, end reasons, readouts and names; the column under a
    /// tall heading with every line in it shown; the stacks with both a
    /// capture count and the dots, which are never shown together; the
    /// countdown card; and the band under the board.
    fn boxes(layout: &MatchLayout) -> Vec<(String, Rect)> {
        let centre = layout.window.x / 2.0;
        let centred = |chars: usize, lines: usize, font_px: f32, top: f32| {
            text_box(
                centre - line_width(chars, font_px) / 2.0,
                top,
                chars,
                lines,
                font_px,
            )
        };
        let mut boxes = vec![
            ("board".to_string(), layout.board),
            (
                "heading".into(),
                centred(HEADING_MAX_CHARS, 1, HEADING_SIZE, layout.heading_top()),
            ),
        ];
        let (column_top, padding) = layout.column_top_and_padding();
        let reasons_top = column_top + padding;
        boxes.push((
            "end reasons".into(),
            centred(END_REASONS_MAX_CHARS, 1, END_REASONS_SIZE, reasons_top),
        ));
        let round_lines = layout.round_lines.pick([1, 2]);
        let round_chars = layout.round_lines.pick(ROUND_PANEL_MAX_CHARS);
        let (timer_lines, timer_chars) = (1, TIMER_MAX_CHARS);
        if layout.has_title_row() {
            let (round_right, timer_left) = layout.title_inner_edges();
            let middle = layout.heading_top() + TITLE_ROW_HEIGHT / 2.0;
            let round_height = text_height(round_lines, ROUND_PANEL_SIZE);
            let timer_height = text_height(timer_lines, TIMER_SIZE);
            boxes.push((
                "round panel".into(),
                text_box(
                    round_right - line_width(round_chars, ROUND_PANEL_SIZE),
                    middle - round_height / 2.0,
                    round_chars,
                    round_lines,
                    ROUND_PANEL_SIZE,
                ),
            ));
            boxes.push((
                "timer".into(),
                text_box(
                    timer_left,
                    middle - timer_height / 2.0,
                    timer_chars,
                    timer_lines,
                    TIMER_SIZE,
                ),
            ));
        } else {
            let timer_top =
                reasons_top + END_REASONS_LINE + END_REASONS_GAP_BELOW + UNDER_HEADING_ROW_GAP;
            let round_top =
                timer_top + text_height(timer_lines, TIMER_SIZE) + UNDER_HEADING_ROW_GAP;
            boxes.push((
                "timer".into(),
                centred(timer_chars, timer_lines, TIMER_SIZE, timer_top),
            ));
            boxes.push((
                "round panel".into(),
                centred(round_chars, round_lines, ROUND_PANEL_SIZE, round_top),
            ));
        }
        for player in Player::ALL {
            let at_side = |rect: Rect| match player {
                Player::Two => rect,
                Player::One => Rect::new(
                    layout.window.x - rect.max.x,
                    rect.min.y,
                    layout.window.x - rect.min.x,
                    rect.max.y,
                ),
            };
            for item in STACK {
                let (inset, top) = (layout.stack_inset(item), layout.stack_top(item));
                let rect = match item {
                    StackItem::Name => {
                        text_box(inset, top, PLAYER_HEADING_MAX_CHARS, 1, PLAYER_HEADING_SIZE)
                    }
                    StackItem::Button => Rect::new(
                        inset,
                        top,
                        inset + HUD_BUTTON_SIZE.x,
                        top + HUD_BUTTON_SIZE.y,
                    ),
                    StackItem::CaptureCount => Rect::new(
                        inset,
                        top,
                        inset + CONTROL_HINT_WIDTH,
                        top + text_height(1, CAPTURE_COUNT_FONT_PX),
                    ),
                    StackItem::OpeningDots => {
                        Rect::new(inset, top, inset + CONTROL_HINT_WIDTH, top + ROW_HEIGHT)
                    }
                    StackItem::ControlHint => Rect::new(
                        inset,
                        top,
                        inset + CONTROL_HINT_WIDTH,
                        top + CONTROL_HINT_MIN_HEIGHT,
                    ),
                };
                boxes.push((format!("{player:?}'s {item:?}"), at_side(rect)));
            }
        }
        let card = MATCH_COUNTDOWN_CARD.size;
        let card_top = match_countdown_top(layout);
        boxes.push((
            "countdown card".into(),
            Rect::new(
                centre - card.x / 2.0,
                card_top,
                centre + card.x / 2.0,
                card_top + card.y,
            ),
        ));
        let overlay_bottom = layout.window.y - layout.overlay_bottom();
        boxes.push((
            "end-gesture line and bars".into(),
            Rect::new(
                0.0,
                overlay_bottom - BOTTOM_CONTENT,
                layout.window.x,
                overlay_bottom,
            ),
        ));
        boxes
    }

    /// In the opening window the layout is the one the screen has always
    /// had, number for number: the board where it was, drawn at its own
    /// size by a camera at a scale of 1 over the origin, and every piece of
    /// interface where it was spawned before the layout adapted. The
    /// numbers are written out, not this module's constants.
    #[test]
    fn the_opening_window_is_laid_out_exactly_as_it_always_was() {
        let opening = match_layout(WindowSize::OPENING.0);
        assert_eq!(opening.arrangement, Arrangement::Tall);
        assert_eq!(opening.board, Rect::new(193.0, 145.0, 847.0, 799.0));
        assert_eq!(opening.zoom(), 1.0);
        assert_eq!(opening.framing(), WorldFraming::IDENTITY);
        assert_eq!(opening.heading_top(), 14.0);
        assert_eq!(opening.column_top_and_padding(), (54.0, 0.0));
        assert_eq!(opening.end_reasons_max_width(), 654.0);
        assert!(!opening.has_title_row());
        assert_eq!(opening.round_lines, Lines::One);
        let stack: Vec<(f32, f32)> = STACK
            .iter()
            .map(|item| (opening.stack_top(*item), opening.stack_inset(*item)))
            .collect();
        assert_eq!(
            stack,
            [
                (58.0, 28.0),
                (94.0, 28.0),
                (140.0, 18.0),
                (174.0, 18.0),
                (214.0, 18.0)
            ]
        );
        assert_eq!(opening.overlay_bottom(), 18.0);
        assert_eq!(match_countdown_top(&opening), 250.0);
    }

    /// Every window gets a board between the smallest window's and twice its
    /// own size: inside the window, centred across it, square, and clear of
    /// the bands above and below it. Wide is used exactly where the room it
    /// leaves the board, before the cap, is more than [`WIDE_GAIN`] times
    /// tall's, and the board is that room, capped; so a larger window never
    /// gets a board smaller by more than that. The two rooms are worked out
    /// here from the margins, not by the functions under test, so that a
    /// slip in either changes what the test expects of the other.
    #[test]
    fn the_board_grows_with_the_window_from_the_smallest_up_to_twice_its_size() {
        let smallest = match_layout(WindowSize::MINIMUM.0).zoom();
        assert!(
            (0.695..0.705).contains(&smallest),
            "the smallest window's board is 0.70 of its own size: {smallest}"
        );
        let mut both = [false, false];
        for size in layout_sweep() {
            let layout = match_layout(size);
            let (board, zoom) = (layout.board, layout.zoom());
            assert!(
                zoom >= smallest - 1e-5 && zoom <= MAX_ZOOM + 1e-5,
                "{size}: {zoom}"
            );
            assert!(
                (board.width() - board.height()).abs() < 1e-3,
                "{size}: {board:?}"
            );
            assert!((board.center().x - size.x / 2.0).abs() < 1e-3, "{size}");
            let (top, bottom) = match layout.arrangement {
                Arrangement::Tall => (TALL_TOP, TALL_BOTTOM),
                Arrangement::Wide => (WIDE_TOP, WIDE_BOTTOM),
            };
            assert!(
                board.min.y >= top - 1e-3 && board.max.y <= size.y - bottom + 1e-3,
                "{size}: {board:?}"
            );
            let tall_room = (size.x - 2.0 * TALL_SIDE).min(size.y - TALL_TOP - TALL_BOTTOM);
            let wide_room = (size.y - WIDE_TOP - WIDE_BOTTOM).min(size.x - 2.0 * WIDE_SIDE);
            let wide = wide_room > tall_room * WIDE_GAIN;
            assert_eq!(
                layout.arrangement == Arrangement::Wide,
                wide,
                "{size}: rooms {tall_room} and {wide_room}"
            );
            let room = if wide { wide_room } else { tall_room };
            assert!(
                (board.width() - room.min(2.0 * BOARD_OUTER_SIZE)).abs() < 1e-3,
                "{size}: {board:?} for a room of {room}"
            );
            both[usize::from(wide)] = true;
            for larger in [size + Vec2::new(20.0, 0.0), size + Vec2::new(0.0, 20.0)] {
                assert!(
                    match_layout(larger).zoom() * WIDE_GAIN >= zoom - 1e-5,
                    "{size} -> {larger}"
                );
            }
        }
        assert_eq!(both, [true, true], "the sweep reaches both arrangements");
    }

    /// The figures this layout was designed to on 2026-11-27: at each named
    /// size, its arrangement and the board's zoom, which the layout holds
    /// to within 0.01 - tall at its own size in the opening window, and
    /// wide everywhere else, from 0.70 at the minimum to 1.98 at 2560x1440.
    #[test]
    fn each_named_window_gets_the_arrangement_and_zoom_it_was_designed_for() {
        let expected = [
            (Arrangement::Tall, 1.00),
            (Arrangement::Wide, 0.70),
            (Arrangement::Wide, 0.76),
            (Arrangement::Wide, 0.88),
            (Arrangement::Wide, 0.83),
            (Arrangement::Wide, 0.95),
            (Arrangement::Wide, 1.12),
            (Arrangement::Wide, 1.43),
            (Arrangement::Wide, 1.98),
        ];
        for (size, (arrangement, zoom)) in NAMED_WINDOW_SIZES.into_iter().zip(expected) {
            let layout = match_layout(size);
            assert_eq!(layout.arrangement, arrangement, "{size}");
            assert!(
                (layout.zoom() - zoom).abs() <= 0.01,
                "{size}: {} against {zoom}",
                layout.zoom()
            );
        }
    }

    /// Past the sweep's largest window, the board stops at exactly twice
    /// its size, and each window keeps the arrangement its shape gives it
    /// before that: a window of a wide shape stays wide however large it
    /// grows, as 2560x1440 is, and a portrait one stays tall, whether its
    /// board reaches the cap or not.
    #[test]
    fn the_board_stops_at_twice_its_size_and_the_window_keeps_its_arrangement() {
        for (size, arrangement, capped) in [
            (Vec2::new(2560.0, 1440.0), Arrangement::Wide, false),
            (Vec2::new(2560.0, 1600.0), Arrangement::Wide, true),
            (Vec2::new(3840.0, 2160.0), Arrangement::Wide, true),
            (Vec2::new(1440.0, 2560.0), Arrangement::Tall, false),
            (Vec2::new(2160.0, 3840.0), Arrangement::Tall, true),
        ] {
            let layout = match_layout(size);
            assert_eq!(layout.arrangement, arrangement, "{size}");
            if capped {
                assert_eq!(layout.zoom(), MAX_ZOOM, "{size}");
            } else {
                assert!(layout.zoom() < MAX_ZOOM - 0.01, "{size}: {}", layout.zoom());
            }
        }
        assert_eq!(MAX_ZOOM, 2.0);
    }

    /// The heart of it: at every size, everything the screen can show at its
    /// largest stands inside the window and at least a pixel clear of
    /// everything else. The countdown card is the one thing meant to stand
    /// over something - the board - and it does, at every size. The
    /// positive half is the count: every size lays out every box.
    #[test]
    fn nothing_overlaps_and_nothing_leaves_the_window_at_any_size() {
        let card_and_board =
            |a: &str, b: &str| [a, b].contains(&"board") && [a, b].contains(&"countdown card");
        for size in layout_sweep() {
            let layout = match_layout(size);
            let boxes = boxes(&layout);
            assert_eq!(boxes.len(), 17, "{size}: {boxes:?}");
            let window = Rect::from_corners(Vec2::ZERO, size);
            for (name, rect) in &boxes {
                assert!(
                    rect.width() > 0.0 && rect.height() > 0.0,
                    "{size}: {name} {rect:?}"
                );
                assert!(
                    window.contains(rect.min) && window.contains(rect.max),
                    "{size}: {name} {rect:?} leaves the window"
                );
            }
            for (index, (name, rect)) in boxes.iter().enumerate() {
                for (other, other_rect) in &boxes[index + 1..] {
                    if card_and_board(name, other) {
                        let card = if name == "board" { other_rect } else { rect };
                        assert!(
                            layout.board.contains(card.min) && layout.board.contains(card.max),
                            "{size}: the card {card:?} stands over the board {:?}",
                            layout.board
                        );
                        continue;
                    }
                    let met = rect.inflate(0.5).intersect(other_rect.inflate(0.5));
                    assert!(
                        met.is_empty(),
                        "{size} ({:?}): {name} {rect:?} meets {other} {other_rect:?}",
                        layout.arrangement
                    );
                }
            }
        }
    }

    /// Beside the heading, the round panel written on two lines fits its
    /// side of the title row at every size, so a form always fits, and one
    /// line is chosen wherever its longest form fits; both forms occur, so
    /// the choice is really made. The timer fits on one line everywhere.
    #[test]
    fn the_round_panel_takes_two_lines_only_where_one_does_not_fit() {
        let mut seen = std::collections::BTreeSet::new();
        for size in layout_sweep() {
            let layout = match_layout(size);
            if !layout.has_title_row() {
                assert_eq!(layout.round_lines, Lines::One, "{size}");
                continue;
            }
            let slot = layout.title_slot_width();
            let round =
                |lines: Lines| line_width(lines.pick(ROUND_PANEL_MAX_CHARS), ROUND_PANEL_SIZE);
            assert!(round(layout.round_lines) <= slot, "{size}");
            assert_eq!(
                layout.round_lines == Lines::One,
                round(Lines::One) <= slot,
                "{size}"
            );
            assert!(
                line_width(TIMER_MAX_CHARS, TIMER_SIZE) <= slot,
                "{size}: {slot}"
            );
            seen.insert(format!("{:?}", layout.round_lines));
        }
        assert_eq!(seen.len(), 2, "both forms occur: {seen:?}");
        assert_eq!(
            match_layout(Vec2::new(2560.0, 1440.0)).round_lines,
            Lines::One
        );
        assert_eq!(match_layout(WindowSize::MINIMUM.0).round_lines, Lines::Two);
    }

    /// The stacks keep their gap from the board wherever the line of end
    /// reasons leaves them room, and only the smallest wide windows move
    /// them out: at 1040x600 the line needs it, at 1280x641 it does not.
    #[test]
    fn a_wide_stack_leaves_the_board_only_as_far_as_the_end_reasons_need() {
        for size in layout_sweep() {
            let layout = match_layout(size);
            let from_board = layout.board.min.x - layout.stack_edge;
            match layout.arrangement {
                Arrangement::Tall => assert_eq!(from_board, STACK_FROM_BOARD, "{size}"),
                Arrangement::Wide => assert!(from_board >= STACK_FROM_BOARD - 1e-3, "{size}"),
            }
        }
        let smallest = match_layout(WindowSize::MINIMUM.0);
        assert!(smallest.board.min.x - smallest.stack_edge > STACK_FROM_BOARD + 1.0);
        let laptop = match_layout(Vec2::new(1280.0, 641.0));
        assert!((laptop.board.min.x - laptop.stack_edge - STACK_FROM_BOARD).abs() < 1e-3);
    }

    /// A window smaller than the minimum in either direction is laid out as
    /// the minimum would be in that direction.
    #[test]
    fn a_window_below_the_minimum_is_laid_out_at_the_minimum() {
        assert_eq!(layout(800.0, 500.0), match_layout(WindowSize::MINIMUM.0));
        assert_eq!(layout(1600.0, 400.0), layout(1600.0, 600.0));
        assert_eq!(layout(900.0, 900.0), layout(1040.0, 900.0));
    }

    /// A window with more room than the piece needs keeps the piece whole
    /// and centres it, in either arrangement: the heading, the stacks, the
    /// board and the band under it move down together, by half the height
    /// added. In tall, a window taller than the opening one; in wide, one
    /// past the size where the board reaches its cap, and one whose width
    /// limits the board, where the piece stands centred from the first.
    #[test]
    fn a_window_with_room_to_spare_centres_the_piece_in_either_arrangement() {
        let moved_down = |from: &MatchLayout, to: &MatchLayout, by: f32| {
            let context = format!("{} -> {}", from.window, to.window);
            assert_eq!(from.arrangement, to.arrangement, "{context}");
            assert_eq!(from.board.size(), to.board.size(), "{context}");
            let mut moves = vec![
                to.heading_top() - from.heading_top(),
                to.column_top_and_padding().0 - from.column_top_and_padding().0,
                to.board.min.y - from.board.min.y,
                to.overlay_bottom() - from.overlay_bottom(),
            ];
            moves.extend(STACK.map(|item| to.stack_top(item) - from.stack_top(item)));
            assert!(
                moves.iter().all(|moved| (moved - by).abs() < 1e-3),
                "{context}: {moves:?}"
            );
        };
        let opening = layout(1040.0, 900.0);
        let taller = layout(1040.0, 1100.0);
        assert_eq!(taller.arrangement, Arrangement::Tall);
        assert_eq!(taller.zoom(), 1.0);
        moved_down(&opening, &taller, 100.0);

        // 2560x1450.4 is the window whose height the capped board and its
        // margins fill exactly.
        let filled = layout(2560.0, 1450.4);
        assert_eq!(filled.arrangement, Arrangement::Wide);
        assert!((filled.heading_top() - EDGE).abs() < 1e-3);
        let past_the_cap = layout(2560.0, 1650.4);
        assert_eq!(past_the_cap.zoom(), MAX_ZOOM);
        moved_down(&filled, &past_the_cap, 100.0);

        // At 1040x880 the width limits a wide board to 666 px, which leaves
        // 35.8 px above the heading and below the band under the board.
        let narrow = layout(1040.0, 880.0);
        assert_eq!(narrow.arrangement, Arrangement::Wide);
        assert!((narrow.heading_top() - EDGE - 35.8).abs() < 1e-3);
        assert!((narrow.overlay_bottom() - EDGE - 35.8).abs() < 1e-3);
    }
}
