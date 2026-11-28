//! How the Replay Library is laid out for a window of any size: how many
//! cards a page holds, in how many columns, and how its header stands.
//!
//! Nothing here touches the ECS: [`library_layout`] is a pure function of
//! the window's size and of how many lines of notes stand above the list,
//! which is what lets the tests run it at every size the layouts are checked
//! at. [`super::layout`] builds the screen from it - from what
//! [`LibraryBuild`] keeps of it, taking the window's width by flex - and
//! [`super`] pages and scrolls by it.
//!
//! # As many cards as fit
//!
//! Text never changes size, and neither does a card: every card is
//! [`CARD_HEIGHT`] tall with [`CARD_GAP`] between two, so a page holds as
//! many as the list's height has room for, and the rest of the screen - its
//! header, its toolbar, the notes above the list and the paging row under
//! it - is fixed height around the list. A note is a line of 14 px text:
//! the retention note a browser build always shows, and the line that
//! counts Replays that could not be read, whenever there are any. Both are
//! counted wherever they are shown, so a browser page lays out as a native
//! window of the same size does, notes and all.
//!
//! * **Padding.** The screen keeps [`TALL_PADDING`] round its contents, as
//!   it did before it was laid out for any size, in every window at least
//!   [`SHORT_BELOW`] px tall, and [`SHORT_PADDING`] in a shorter one. Where
//!   the change falls hardly matters to the count: anywhere from 804 to
//!   929 px tall, a window shows the same number of cards with either
//!   padding. So it falls at 860 px, 40 px below the opening window, which
//!   keeps the margins it always had.
//! * **One header row.** The title stands on a row of its own, centred
//!   over the toolbar, with Home at its left. In a window at least
//!   [`ONE_ROW_HEADER_FROM`] px wide, Home, the title and the toolbar share
//!   one row whenever that makes room for one more card - always at the
//!   short padding, with the search control narrowed from
//!   [`SEARCH_CONTROL_WIDTH`] to [`ONE_ROW_SEARCH_CONTROL_WIDTH`], which is
//!   what lets the row fit that window.
//! * **Columns.** The list is held to [`ONE_COLUMN_MAX_WIDTH`] and centred,
//!   so a card's buttons stay near its words, with the header and the
//!   paging row held to the same width over and under it. That width is
//!   the one header row's, 1126 px, rather than a rounder 1100, because the
//!   row needs 1120.4 and the list stands as wide as the header over it in
//!   either arrangement - so where the list is held to it, from 1174 px
//!   wide or in a window shorter than [`SHORT_BELOW`] with its short
//!   padding either way, the list keeps its width when the header moves
//!   between one row and two. A window at least [`TWO_COLUMNS_FROM`] px
//!   wide shows two columns side by side, held to
//!   [`TWO_COLUMN_MAX_WIDTH`], each card half the list less half the gap
//!   between them; a page is then as many rows of two as fit.
//!
//! So a page holds two cards at the 1040x600 minimum, three in a
//! 1280x720 laptop's maximised window, four at 1040x900, at 1366x768 - where
//! the one header row gains the fourth - and in a maximised 1440x900 Mac,
//! twelve at 1920x1080 in two columns of six, and sixteen at 2560x1440. The
//! tests hold each count on both sides of the height where it changes.

use bevy::prelude::*;

use crate::ui::text_metrics::text_height;

/// The padding round the screen's contents in a tall window and in a short
/// one, and the height that divides them.
pub(super) const TALL_PADDING: f32 = 24.0;
pub(super) const SHORT_PADDING: f32 = 12.0;
pub(super) const SHORT_BELOW: f32 = 860.0;
/// The gap between the screen's rows: the header, the toolbar, each note,
/// the list and the paging row.
pub(super) const ROW_GAP: f32 = 10.0;
/// The title's size, the toolbar's height, which is also the one header
/// row's, the paging row's height, and a note's size.
pub(super) const LIBRARY_TITLE_SIZE: f32 = 38.0;
pub(super) const TOOLBAR_HEIGHT: f32 = 48.0;
pub(super) const PAGINATION_ROW_HEIGHT: f32 = 40.0;
pub(super) const NOTE_SIZE: f32 = 14.0;
/// A card's height, and the gap between two cards, down a column and
/// across two.
pub(super) const CARD_HEIGHT: f32 = 142.0;
pub(super) const CARD_GAP: f32 = 8.0;
/// The widest the list stands in one column - the width the one header row
/// has in the narrowest window that shows it - and in two, and the
/// narrowest window that gets two.
pub(super) const ONE_COLUMN_MAX_WIDTH: f32 = ONE_ROW_HEADER_FROM - 2.0 * SHORT_PADDING;
pub(super) const TWO_COLUMN_MAX_WIDTH: f32 = 2200.0;
pub(super) const TWO_COLUMNS_FROM: f32 = 1700.0;
/// The narrowest window whose header can stand on one row. Its row needs
/// 1120.4 px with every gap at its least, and the short padding either
/// side, 1144.4 px in all; the rest is room for `bevy_ui`, which lays a text
/// box out up to a pixel wider than its advance. A test holds the sum to
/// the controls' widths.
pub(super) const ONE_ROW_HEADER_FROM: f32 = 1150.0;

/// The toolbar's controls: the search field and its Clear, the sort field
/// and the sort order, the gap inside each pair, and the least room the
/// flexible spacer between the two pairs keeps.
pub(super) const SEARCH_CONTROL_WIDTH: f32 = 500.0;
pub(super) const ONE_ROW_SEARCH_CONTROL_WIDTH: f32 = 250.0;
pub(super) const CLEAR_CONTROL_WIDTH: f32 = 74.0;
pub(super) const SORT_FIELD_CONTROL_MIN_WIDTH: f32 = 210.0;
pub(super) const SORT_ORDER_CONTROL_WIDTH: f32 = 126.0;
pub(super) const TOOLBAR_COLUMN_GAP: f32 = 10.0;
pub(super) const TOOLBAR_MIN_SPACER_WIDTH: f32 = 14.0;
/// The Home button's width, and in the one header row the gaps after it
/// and after the title.
pub(super) const HOME_BUTTON_WIDTH: f32 = 90.0;
pub(super) const HOME_TITLE_GAP: f32 = 16.0;
pub(super) const TITLE_TOOLBAR_GAP: f32 = 24.0;
/// The title's text.
pub(super) const LIBRARY_TITLE: &str = "Match Replays";

/// The toolbar's least width with the search control `search` px wide.
#[cfg(test)]
pub(super) fn toolbar_min_width(search: f32) -> f32 {
    search
        + TOOLBAR_COLUMN_GAP
        + CLEAR_CONTROL_WIDTH
        + TOOLBAR_MIN_SPACER_WIDTH
        + SORT_FIELD_CONTROL_MIN_WIDTH
        + TOOLBAR_COLUMN_GAP
        + SORT_ORDER_CONTROL_WIDTH
}

/// What the one header row holds at its least: Home, the title and the
/// toolbar with the narrowed search control.
#[cfg(test)]
pub(super) fn one_row_header_contents_width() -> f32 {
    HOME_BUTTON_WIDTH
        + HOME_TITLE_GAP
        + crate::ui::text_metrics::line_width(LIBRARY_TITLE.len(), LIBRARY_TITLE_SIZE)
        + TITLE_TOOLBAR_GAP
        + toolbar_min_width(ONE_ROW_SEARCH_CONTROL_WIDTH)
}

/// The height `bevy_ui` lays one line of `font_px` text out at: its line
/// box, rounded up to a whole pixel.
fn laid_out_line(font_px: f32) -> f32 {
    text_height(1, font_px).ceil()
}

/// How the Library stands in one window; every length in logical pixels.
/// The screen is built from its [`LibraryLayout::build`] alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LibraryLayout {
    /// The size laid out for: the window's, raised to the minimum.
    pub(super) window: Vec2,
    /// The screen's padding round its contents: across, each side, and
    /// down, above and below.
    pub(super) padding: Vec2,
    /// Whether Home, the title and the toolbar share one row.
    pub(super) one_row_header: bool,
    /// Cards across the list, and down each column.
    pub(super) columns: usize,
    pub(super) rows: usize,
    pub(super) search_width: f32,
}

/// Lays the Library out for a window of `window` logical pixels, which the
/// caller has already raised to the minimum with `WindowSize::layout`, with
/// `notes` lines of notes standing above the list.
pub(super) fn library_layout(window: Vec2, notes: usize) -> LibraryLayout {
    let notes_height = notes as f32 * (laid_out_line(NOTE_SIZE) + ROW_GAP);
    let rows_under = |chrome: f32| {
        let room = window.y - chrome - notes_height + CARD_GAP;
        ((room / (CARD_HEIGHT + CARD_GAP)).floor() as usize).max(1)
    };
    let padding = if window.y >= SHORT_BELOW {
        TALL_PADDING
    } else {
        SHORT_PADDING
    };
    // Above and below the list: the padding, and every row with the gap
    // under it, the paging row with the gap over it.
    let two_rows = rows_under(
        2.0 * padding
            + laid_out_line(LIBRARY_TITLE_SIZE)
            + ROW_GAP
            + TOOLBAR_HEIGHT
            + ROW_GAP
            + ROW_GAP
            + PAGINATION_ROW_HEIGHT,
    );
    let one_row = rows_under(
        2.0 * SHORT_PADDING + TOOLBAR_HEIGHT + ROW_GAP + ROW_GAP + PAGINATION_ROW_HEIGHT,
    );
    let one_row_header = window.x >= ONE_ROW_HEADER_FROM && one_row > two_rows;
    let padding = if one_row_header {
        SHORT_PADDING
    } else {
        padding
    };
    let columns = if window.x >= TWO_COLUMNS_FROM { 2 } else { 1 };
    let side = ((window.x - max_width(columns)) / 2.0).max(padding);
    LibraryLayout {
        window,
        padding: Vec2::new(side, padding),
        one_row_header,
        columns,
        rows: if one_row_header { one_row } else { two_rows },
        search_width: if one_row_header {
            ONE_ROW_SEARCH_CONTROL_WIDTH
        } else {
            SEARCH_CONTROL_WIDTH
        },
    }
}

/// The widest the list stands in `columns` columns, and the header and
/// the paging row with it.
fn max_width(columns: usize) -> f32 {
    if columns > 1 {
        TWO_COLUMN_MAX_WIDTH
    } else {
        ONE_COLUMN_MAX_WIDTH
    }
}

/// What the screen is built from: everything in a [`LibraryLayout`] but the
/// window's size and the side padding that follows from it. The screen
/// takes those from the window by flex, its contents held to the list's
/// widest and centred and two columns of cards a grid of two equal tracks,
/// so two layouts that build alike lay out alike, and a change of size
/// between them rebuilds nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LibraryBuild {
    padding: f32,
    one_row_header: bool,
    columns: usize,
    rows: usize,
    search_width: f32,
}

impl LibraryLayout {
    /// What the screen is built from; see [`LibraryBuild`].
    pub(super) fn build(&self) -> LibraryBuild {
        LibraryBuild {
            padding: self.padding.y,
            one_row_header: self.one_row_header,
            columns: self.columns,
            rows: self.rows,
            search_width: self.search_width,
        }
    }

    /// How many cards a page holds.
    pub(super) fn cards_per_page(&self) -> usize {
        self.rows * self.columns
    }

    /// The widest the screen's contents stand: the list's widest, in one
    /// column or two. In a narrower window they take the window less the
    /// padding, which is never less than `padding.y` at the sides.
    pub(super) fn max_width(&self) -> f32 {
        max_width(self.columns)
    }

    /// The width the header, the toolbar, the list and the paging row are
    /// held to, centred in the window.
    #[cfg(test)]
    pub(super) fn content_width(&self) -> f32 {
        self.window.x - 2.0 * self.padding.x
    }

    /// One card's width: the list's, or in two columns half of it less half
    /// the gap between them.
    #[cfg(test)]
    pub(super) fn card_width(&self) -> f32 {
        if self.columns == 1 {
            self.content_width()
        } else {
            (self.content_width() - CARD_GAP) / 2.0
        }
    }

    /// The first card a page may start at, in a list of `count`: its own
    /// index rounded down to a row's first card, and no further down the
    /// list than the page that ends with the last card, so that a scroll
    /// leaves no room for cards the list does not have - but for one place:
    /// in two columns the page moves a row of two at a time, so its last
    /// row may hold one card beside an empty place.
    pub(super) fn first_card(&self, wanted: usize, count: usize) -> usize {
        let last = count
            .saturating_sub(self.cards_per_page())
            .next_multiple_of(self.columns);
        (wanted - wanted % self.columns).min(last)
    }

    /// The list's height, between the rows above it and the paging row.
    #[cfg(test)]
    pub(super) fn list_height(&self, notes: usize) -> f32 {
        let above = if self.one_row_header {
            TOOLBAR_HEIGHT + ROW_GAP
        } else {
            laid_out_line(LIBRARY_TITLE_SIZE) + ROW_GAP + TOOLBAR_HEIGHT + ROW_GAP
        };
        self.window.y
            - 2.0 * self.padding.y
            - above
            - notes as f32 * (laid_out_line(NOTE_SIZE) + ROW_GAP)
            - ROW_GAP
            - PAGINATION_ROW_HEIGHT
    }

    /// The height a page of cards takes, down one column.
    #[cfg(test)]
    pub(super) fn page_height(&self) -> f32 {
        self.rows as f32 * CARD_HEIGHT + (self.rows - 1) as f32 * CARD_GAP
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{NAMED_WINDOW_SIZES, WindowSize, layout_sweep};

    /// The opening window is the Library as it always was: four cards in
    /// one column, two header rows, 24 px round everything and the list
    /// filling the 992 px between, and the search control 500 px wide.
    #[test]
    fn the_opening_window_keeps_the_library_as_it_was() {
        for notes in [0, 1, 2] {
            let opening = library_layout(WindowSize::OPENING.0, notes);
            assert_eq!(opening.padding, Vec2::splat(24.0), "{notes}");
            assert!(!opening.one_row_header, "{notes}");
            assert_eq!(opening.columns, 1, "{notes}");
            assert_eq!(opening.content_width(), 992.0, "{notes}");
            assert_eq!(opening.search_width, 500.0, "{notes}");
        }
        assert_eq!(library_layout(WindowSize::OPENING.0, 0).cards_per_page(), 4);
        assert_eq!(
            library_layout(WindowSize::OPENING.0, 0).list_height(0),
            688.0
        );
    }

    /// How many cards the named sizes show, with no notes: two
    /// cards at the minimum, three on the 1280x720 laptop's screens and in a
    /// maximised 1366x768 one, four at 1040x900, at 1366x768 and at
    /// 1440x844, twelve at 1920x1080 and sixteen at 2560x1440; and which
    /// gain theirs from the one header row.
    #[test]
    fn the_named_sizes_show_as_many_cards_as_they_have_room_for() {
        let expected = [
            ("1040x900", 4, 1, false),
            ("1040x600", 2, 1, false),
            ("1280x641", 3, 1, false),
            ("1280x720", 3, 1, false),
            ("1366x689", 3, 1, false),
            ("1366x768", 4, 1, true),
            ("1440x844", 4, 1, false),
            ("1920x1080", 12, 2, true),
            ("2560x1440", 16, 2, false),
        ];
        for (size, (name, cards, columns, one_row)) in NAMED_WINDOW_SIZES.iter().zip(expected) {
            assert_eq!(format!("{}x{}", size.x, size.y), name);
            let layout = library_layout(*size, 0);
            assert_eq!(layout.cards_per_page(), cards, "{name}");
            assert_eq!(layout.columns, columns, "{name}");
            assert_eq!(layout.one_row_header, one_row, "{name}");
        }
        // A browser's retention note takes a line: on the laptop's
        // maximised page the one header row keeps its third card.
        let browser = library_layout(Vec2::new(1280.0, 641.0), 1);
        assert_eq!(browser.cards_per_page(), 3);
        assert!(browser.one_row_header);
    }

    /// At every size, with no notes, one or both: the page fills the list
    /// without spilling, and one more row would not fit; the content stands
    /// centred and inside the window, at its padding or held to its widest;
    /// the toolbar and, in one row, the whole header fit across it; and the
    /// one header row is used exactly where it gains a card.
    #[test]
    fn every_window_shows_as_many_cards_as_fit() {
        for size in layout_sweep() {
            for notes in [0, 1, 2] {
                let layout = library_layout(size, notes);
                let context = format!("{size} with {notes} notes");
                let room = layout.list_height(notes);
                assert!(
                    layout.page_height() <= room + 1e-3,
                    "{context}: {} rows need {}, the list has {room}",
                    layout.rows,
                    layout.page_height()
                );
                assert!(
                    layout.page_height() + CARD_GAP + CARD_HEIGHT > room,
                    "{context}: another row fits"
                );
                assert!(layout.rows >= 1);
                let max = if layout.columns == 2 {
                    TWO_COLUMN_MAX_WIDTH
                } else {
                    ONE_COLUMN_MAX_WIDTH
                };
                let width = layout.content_width();
                assert!(width <= max + 1e-3, "{context}: {width}");
                assert!(
                    layout.padding.x >= layout.padding.y - 1e-3,
                    "{context}: {:?}",
                    layout.padding
                );
                assert!(
                    (width - max).abs() < 1e-3
                        || (layout.padding.x - layout.padding.y).abs() < 1e-3,
                    "{context}: held to its widest or at its padding"
                );
                let toolbar = toolbar_min_width(layout.search_width);
                assert!(toolbar <= width, "{context}: the toolbar needs {toolbar}");
                if layout.one_row_header {
                    assert!(size.x >= ONE_ROW_HEADER_FROM, "{context}");
                    assert!(
                        one_row_header_contents_width() <= width,
                        "{context}: the one header row needs {}",
                        one_row_header_contents_width()
                    );
                    assert_eq!(layout.padding.y, SHORT_PADDING, "{context}");
                }
                // Where the one row is allowed, it is used exactly when it
                // gains a card over two rows at this height's padding.
                if size.x >= ONE_ROW_HEADER_FROM {
                    let two_rows = LibraryLayout {
                        one_row_header: false,
                        padding: Vec2::splat(if size.y >= SHORT_BELOW {
                            TALL_PADDING
                        } else {
                            SHORT_PADDING
                        }),
                        ..layout
                    };
                    let two_rows_fit = ((two_rows.list_height(notes) + CARD_GAP)
                        / (CARD_HEIGHT + CARD_GAP))
                        .floor() as usize;
                    assert_eq!(
                        layout.one_row_header,
                        layout.rows > two_rows_fit,
                        "{context}"
                    );
                }
            }
        }
    }

    /// Each count of cards a page holds, on both sides of the height it
    /// starts at, which the sweep's 20 px steps cannot pin. In the opening
    /// window's width, at the short padding, two cards fit up to 629 px
    /// tall and three from 630, three up to 779 and four from 780; the
    /// padding widens at 860 without changing the count; and five fit from
    /// 954. Where one header row may stand, it gains a fifth card from
    /// 874, and at 1920 px wide a sixth row a column, twelve cards, from
    /// 1024.
    #[test]
    fn each_count_starts_where_its_cards_first_fit() {
        let cards = |width: f32, height: f32| library_layout(Vec2::new(width, height), 0);
        for (height, rows) in [
            (779.0, 3),
            (779.9, 3),
            (780.0, 4),
            (803.0, 4),
            (859.0, 4),
            (860.0, 4),
            (953.0, 4),
            (954.0, 5),
        ] {
            let layout = cards(1040.0, height);
            assert_eq!(layout.rows, rows, "1040x{height}");
            assert!(!layout.one_row_header);
        }
        // 779 and 780 decide between the short padding's third and fourth
        // card; the tall padding's fourth begins at 804 had it been used.
        assert_eq!(cards(1040.0, 629.0).rows, 2);
        assert_eq!(cards(1040.0, 630.0).rows, 3);
        // Where one header row may stand it gains a card: from 874 a fifth,
        // and from 1024 a sixth, at 1920 px six rows a column, twelve cards.
        assert_eq!(cards(1366.0, 873.0).cards_per_page(), 4);
        assert_eq!(cards(1366.0, 874.0).cards_per_page(), 5);
        assert!(cards(1366.0, 874.0).one_row_header);
        assert_eq!(cards(1920.0, 1023.0).cards_per_page(), 10);
        assert_eq!(cards(1920.0, 1024.0).cards_per_page(), 12);
    }

    /// The one header row's threshold, a pixel either side: at 1150 px wide
    /// the row is used where it gains a card, and its contents fit; at 1149
    /// it is not used. Two columns, likewise, from 1700 px.
    #[test]
    fn one_header_row_and_two_columns_start_where_they_fit() {
        assert!((one_row_header_contents_width() - 1120.4).abs() < 1e-3);
        let at = library_layout(Vec2::new(1150.0, 768.0), 0);
        assert!(at.one_row_header);
        assert!(one_row_header_contents_width() + 2.0 * SHORT_PADDING <= 1150.0);
        assert!(!library_layout(Vec2::new(1149.0, 768.0), 0).one_row_header);
        assert_eq!(library_layout(Vec2::new(1700.0, 900.0), 0).columns, 2);
        assert_eq!(library_layout(Vec2::new(1699.0, 900.0), 0).columns, 1);
    }

    /// The padding changes at 860 px tall, to the pixel: 12 px round the
    /// contents at 1040x859, 24 at 1040x860, as at 1040x900.
    #[test]
    fn the_padding_changes_at_860_px_tall() {
        assert_eq!(
            library_layout(Vec2::new(1040.0, 859.0), 0).padding,
            Vec2::splat(SHORT_PADDING)
        );
        assert_eq!(
            library_layout(Vec2::new(1040.0, 860.0), 0).padding,
            Vec2::splat(TALL_PADDING)
        );
        assert_eq!((SHORT_PADDING, TALL_PADDING), (12.0, 24.0));
    }

    /// The first card a page starts at: the card asked for, unless that
    /// leaves the page short of the list's end, and in two columns the
    /// first of its row of two. A list no longer than a page always starts
    /// at its first card.
    #[test]
    fn the_first_card_keeps_the_page_full_and_rows_whole() {
        let one = library_layout(WindowSize::OPENING.0, 0);
        assert_eq!(one.cards_per_page(), 4);
        assert_eq!(one.first_card(6, 10), 6);
        assert_eq!(one.first_card(7, 10), 6);
        assert_eq!(one.first_card(3, 3), 0);
        assert_eq!(one.first_card(5, 0), 0);
        let two = library_layout(Vec2::new(1920.0, 1080.0), 0);
        assert_eq!(two.cards_per_page(), 12);
        assert_eq!(two.first_card(7, 30), 6);
        assert_eq!(two.first_card(6, 13), 2);
        assert_eq!(two.first_card(1, 13), 0);
        assert_eq!(two.first_card(18, 30), 18);
        assert_eq!(two.first_card(20, 30), 18);
    }
}
