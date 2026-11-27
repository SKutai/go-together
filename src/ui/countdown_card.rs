//! The Round 0 countdown card, which the Match screen and the Replay Viewer
//! each draw over their own board, built and placed by one rule.
//!
//! # Where a card stands
//!
//! A card is centred across the window, which is where both boards are
//! centred, and its centre stands [`COUNTDOWN_CARD_RISE`] above its board's
//! centre, scaled by that board's grid spacing - so on the Viewer's smaller
//! board its centre lands on the same point of the grid as on the live one. The
//! rise is the Match screen's, where the card was placed by eye with its top
//! 250 px down the window. Until 2026-10-05 the Viewer had a `top` of its
//! own, 268 px, which stood its card 29 px - nearly half a row of its grid -
//! higher on its board than this rule puts it.
//!
//! # Why the digit is a child of the card
//!
//! In `bevy_ui`, a node's `justify_content` and `align_items` place its
//! *children*. A [`Text`] node's glyphs are not children: they are laid out
//! from the top-left corner of the node's own content box. `TextLayout`'s
//! justification can move them across inside that box, but nothing in
//! Bevy moves them down it - measured on 0.19.1 and again on 0.20.0, where
//! a sized `Text` asking for centre, end or start alignment drew its glyphs
//! at the top of its box all the same. So a `Text` that is also a sized,
//! bordered card draws its digit in the card's top-left corner, as ordinary
//! text would sit, whatever alignment the card asks for. That is what the
//! Viewer's card did as far back as this repository's history goes, until
//! 2026-10-05: its `3` sat 56 px left of the card's centre and 15 px above
//! it, while the card itself stood exactly where it was meant to. The card
//! is therefore a plain node, and the digit a `Text` child that its own
//! glyphs size, which the card's alignment then centres. `docs/TESTING.md`
//! carries the same warning for any other overlay, under "A Text node does
//! not centre its own text".

use bevy::prelude::*;

use crate::app::visuals::{GRID_SPACING, WINDOW_HEIGHT};
use crate::presentation::{BLUE_BORDER, TEXT_HEADING};

/// How far a countdown card's centre stands above its board's centre, in
/// logical pixels on a board of the Match screen's [`GRID_SPACING`]. A board
/// of another spacing scales it: see [`CountdownCard::center_y`].
const COUNTDOWN_CARD_RISE: f32 = 157.0;
const COUNTDOWN_CARD_BORDER: f32 = 3.0;

/// One screen's countdown card: the board it stands over, its own size, its
/// digit's size, and its fill.
pub(super) struct CountdownCard {
    /// The board's centre as its sprite is placed: world-space y, upward
    /// from the middle of the window.
    pub(super) board_center_y: f32,
    pub(super) grid_spacing: f32,
    pub(super) size: Vec2,
    pub(super) font_size: FontSize,
    pub(super) background: Color,
}

impl CountdownCard {
    /// The card's centre, in logical pixels down from the top of the window.
    pub(super) fn center_y(&self) -> f32 {
        let board_center = WINDOW_HEIGHT as f32 / 2.0 - self.board_center_y;
        board_center - COUNTDOWN_CARD_RISE * self.grid_spacing / GRID_SPACING
    }

    /// The full-width row the card is centred in. Its `top` is measured
    /// from whatever it is laid out against, so it must be spawned as a
    /// root node or as the child of one that covers the whole window.
    pub(super) fn overlay_node(&self) -> Node {
        Node {
            position_type: PositionType::Absolute,
            top: px(self.center_y() - self.size.y / 2.0),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        }
    }

    /// Spawns the card into `overlay`, an entity spawned with
    /// [`Self::overlay_node`], and its digit as the card's child, carrying
    /// `digit` - the marker the screen finds it by to write the stage.
    pub(super) fn spawn(&self, overlay: &mut ChildSpawnerCommands, digit: impl Bundle) {
        overlay
            .spawn((
                Node {
                    width: px(self.size.x),
                    height: px(self.size.y),
                    border: UiRect::all(px(COUNTDOWN_CARD_BORDER)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(self.background),
                BorderColor::all(BLUE_BORDER),
            ))
            .with_child((
                Text::new(""),
                TextFont {
                    font_size: self.font_size,
                    ..default()
                },
                TextColor(TEXT_HEADING),
                digit,
            ));
    }
}

/// Where a countdown card and its digit landed, read from a laid-out
/// screen by [`measure_countdown_card`].
#[cfg(test)]
#[derive(Debug)]
pub(super) struct CountdownLanding {
    /// The card's border box.
    pub(super) card: Rect,
    /// The inked extent of the digit's glyphs.
    pub(super) digit: Rect,
}

/// Reads the card and digit of the one countdown on screen, in logical
/// pixels from the window's top-left, on a
/// [`crate::ui::focus::real_layout_app`], which lays out text as well as
/// nodes. The digit is the `Text` whose content is `stage`.
///
/// The digit is measured by its glyphs and the card is the nearest node that
/// draws a fill, starting from the digit's own, which is what
/// [`crate::ui::focus::measure_drawn_text`] reads - so a digit that is itself
/// the card is measured against itself, as the player sees it.
#[cfg(test)]
pub(super) fn measure_countdown_card(app: &mut App, stage: &str) -> CountdownLanding {
    let drawn = crate::ui::focus::measure_drawn_text(app, stage);
    CountdownLanding {
        card: drawn.surface,
        digit: drawn.ink,
    }
}

/// Asserts what both screens owe their countdown: the digit centred on the
/// card, and the card centred across the board and standing at the shared
/// height above its centre. Returns the card's top, for a screen that also
/// pins its own.
#[cfg(test)]
pub(super) fn assert_countdown_card_lands_on_board(
    landing: &CountdownLanding,
    board_center_y: f32,
    grid_spacing: f32,
) -> f32 {
    use crate::app::visuals::WINDOW_WIDTH;

    // A glyph's ink is not quite centred in its advance and line, so the
    // digit is allowed a few pixels; the defect was 56 left and 15 up.
    const DIGIT_TOLERANCE: f32 = 3.0;
    let (card, digit) = (landing.card, landing.digit);
    let offset = digit.center() - card.center();
    assert!(
        offset.x.abs() <= DIGIT_TOLERANCE && offset.y.abs() <= DIGIT_TOLERANCE,
        "the digit {digit:?} is {offset:?} from the centre of its card {card:?}"
    );
    assert!(
        card.contains(digit.min) && card.contains(digit.max),
        "the digit {digit:?} spills out of its card {card:?}"
    );

    let board_center = Vec2::new(
        WINDOW_WIDTH as f32 / 2.0,
        WINDOW_HEIGHT as f32 / 2.0 - board_center_y,
    );
    assert!(
        (card.center().x - board_center.x).abs() < 0.5,
        "the card {card:?} is not centred across its board at x {}",
        board_center.x
    );
    let rise_in_rows = (board_center.y - card.center().y) / grid_spacing;
    let expected_rows = COUNTDOWN_CARD_RISE / GRID_SPACING;
    assert!(
        (rise_in_rows - expected_rows).abs() < 0.01,
        "the card stands {rise_in_rows} rows above its board's centre, not {expected_rows}"
    );
    card.min.y
}
