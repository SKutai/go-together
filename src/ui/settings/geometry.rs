//! Where the Settings screen's frame stands, for a window of any size.
//!
//! Nothing here touches the ECS: [`settings_layout`] is a pure function of
//! the window's size, which is what lets the tests run it at every size the
//! layouts are checked at and hold that nothing overlaps and nothing leaves
//! the window. [`super::layout`] spawns the frame where this says, and moves
//! it whenever the window's size changes.
//!
//! # Two arrangements
//!
//! Text never changes size, and nothing on this screen is scaled: the
//! screen finds its room by moving.
//!
//! * **Tabs on top** is the screen as it was first laid out: a 930x830
//!   panel in the middle of the window, with the title and the instruction
//!   at its top, the four tabs in a row under them, the tab's contents, and
//!   Reset Controls and Back at its foot. Laid out in the opening window it
//!   is exactly the screen it has always been. It is used in every window
//!   tall enough to show that panel with [`EDGE`] above and below it: every
//!   window at least [`SIDEBAR_BELOW`] px tall, 854. A taller or wider
//!   window leaves the panel at its size, in the middle.
//! * **Sidebar** is for every shorter window. The panel takes the window,
//!   less a margin, up to [`SIDEBAR_PANEL_MAX_WIDTH`] wide; the title, the
//!   instruction, Reset Controls and Back share one row at its top; the
//!   four tabs stand in a column down its left side; and the tab's contents
//!   fill the rest. Two tabs are arranged differently there, by
//!   [`super::layout`]: Preferences sets its three switches side by side as
//!   cards above the two volume rows, and Keyboard puts the shared Restart
//!   Match binding on one line beside its heading. The Game tab's rows stand
//!   closer together, at [`SettingsLayout::game_row_heights`].
//!
//! Which arrangement is used depends on the window's height alone: the
//! tabs-on-top panel is 930 px wide, which every window from the minimum
//! up has room for.
//!
//! # The binding columns
//!
//! The Keyboard and Controller tabs show one column per player, side by
//! side and centred together, never at the panel's far edges. Each is
//! [`PLAYER_COLUMN_WIDTH`] wide where there is room, as it always was, and
//! narrower in the narrowest sidebars. The longest row the columns show
//! with the default bindings is the Controller tab's `Select / Revise
//! Selection` with `South Button`, [`LONGEST_DEFAULT_ROW_CHARS`]
//! characters, and the sidebar keeps at least [`ROW_HALVES_GAP`] between its
//! two halves at every width. That takes the outer margins: the tabs are
//! [`SIDEBAR_TAB_WIDTH`] wide, as on top, wherever that leaves the columns
//! room for the row, and [`NARROW_TAB_WIDTH`] - still wide enough for
//! `Preferences` - in a window narrower than about 1083 px; and below about
//! 1058 px even that is not enough, so the margins on each side of the
//! panel and inside it narrow, all four by the same whole number of pixels,
//! just far enough. At 1040 px each gives up 5 px. The gap is reckoned
//! from the font's advance, 12.8 px there; as drawn, its glyphs stand
//! 12.6 px apart, measured under `bevy_ui`'s layout on 2026-11-28, and on
//! top 14.6, as they always were.
//!
//! A row whose binding is longer than the default's - a stick direction
//! bound to Select - can be wider than its column; there the label wraps
//! onto two lines inside the row's height, as it always has on top.

use bevy::prelude::*;

use crate::ui::text_metrics::line_width;
#[cfg(test)]
use crate::ui::text_metrics::text_height;

/// The tabs-on-top panel, as the screen was first laid out, and the padding
/// around its contents. The screen's root pads the panel off the window's
/// edges by as much again where the window has that much room around it,
/// and by what it has where it has less: `bevy_ui` does not centre a child
/// taller than the room inside its parent's padding, but stands it at the
/// padding's edge, so a 28 px root would push the panel 16 px down a
/// window 854 px tall and its foot out of the window.
pub(super) const TABS_ON_TOP_PANEL: Vec2 = Vec2::new(930.0, 830.0);
pub(super) const TABS_ON_TOP_PADDING: f32 = 28.0;
/// The room the tabs-on-top panel keeps above and below it in the shortest
/// window it is used in.
pub(super) const EDGE: f32 = 12.0;
/// The shortest window the tabs-on-top arrangement is used in; every
/// shorter one gets the sidebar.
pub(super) const SIDEBAR_BELOW: f32 = TABS_ON_TOP_PANEL.y + 2.0 * EDGE;

/// The panel's border, in either arrangement.
pub(super) const PANEL_BORDER: f32 = 2.0;
/// The gap between the panel's rows - the top row, the tabs, the tab's
/// contents and, on top, the footer - in either arrangement.
pub(super) const PANEL_GAP: f32 = 12.0;

/// The sidebar's margins: around the panel, and inside it, before any
/// narrowing.
const SIDEBAR_ROOT_PADDING: f32 = 10.0;
const SIDEBAR_PANEL_PADDING: f32 = 16.0;
pub(super) const SIDEBAR_PANEL_MAX_WIDTH: f32 = 1400.0;
/// The sidebar's gaps: between the title, the instruction and the two
/// buttons in the top row, and between the tabs' column and the tab's
/// contents.
pub(super) const SIDEBAR_HEAD_GAP: f32 = 24.0;
pub(super) const SIDEBAR_BODY_GAP: f32 = 18.0;

/// A tab's width, on top and in the sidebar where there is room, and in a
/// narrow sidebar; and the gap between tabs, in a row and in a column.
pub(super) const SIDEBAR_TAB_WIDTH: f32 = 155.0;
pub(super) const NARROW_TAB_WIDTH: f32 = 130.0;
pub(super) const TAB_HEIGHT: f32 = 42.0;
pub(super) const TAB_ROW_GAP: f32 = 18.0;
pub(super) const TAB_COLUMN_GAP: f32 = 12.0;
/// The tab labels' size: `Preferences`, the longest, is what
/// [`NARROW_TAB_WIDTH`] has to hold.
pub(super) const TAB_LABEL_SIZE: f32 = 17.0;

/// The title's size, and the instruction's.
pub(super) const SETTINGS_TITLE_SIZE: f32 = 38.0;
pub(super) const INSTRUCTION_SIZE: f32 = 16.0;
/// Reset Controls and Back: their widths, the gap between them and their
/// height, from `ui::widgets`.
pub(super) const RESET_BUTTON_WIDTH: f32 = 170.0;
pub(super) const BACK_BUTTON_WIDTH: f32 = 110.0;
pub(super) const FOOTER_GAP: f32 = 18.0;
#[cfg(test)]
const FOOTER_BUTTON_HEIGHT: f32 = 36.0;
#[cfg(test)]
const FOOTER_WIDTH: f32 = RESET_BUTTON_WIDTH + FOOTER_GAP + BACK_BUTTON_WIDTH;

/// A player's binding column at its widest, and the gap between the two:
/// on top they fill the panel's 870 px between them, as they always have.
pub(super) const PLAYER_COLUMN_WIDTH: f32 = 420.0;
const TABS_ON_TOP_COLUMN_GAP: f32 = 30.0;
const SIDEBAR_COLUMN_GAP: f32 = 20.0;
/// What a binding row holds: its text, the padding and border each side of
/// the row, and the column's padding each side of the row.
pub(super) const BINDING_TEXT_SIZE: f32 = 16.0;
pub(super) const BINDING_ROW_PADDING: f32 = 12.0;
pub(super) const BINDING_ROW_BORDER: f32 = 1.0;
pub(super) const PLAYER_COLUMN_PADDING: f32 = 12.0;
/// The longest row the binding columns show with the default bindings, in
/// characters: `Select / Revise Selection` and `South Button`, on the
/// Controller tab with no controller assigned. A test holds it to the
/// labels the tabs really draw.
pub(super) const LONGEST_DEFAULT_ROW_CHARS: usize = 37;
/// The least room the sidebar leaves between that row's label and its
/// binding.
pub(super) const ROW_HALVES_GAP: f32 = 12.0;
/// The narrowest a binding column may be: that row, the gap between its
/// halves, and the row's and the column's insets.
const PLAYER_COLUMN_MIN_WIDTH: f32 = line_width(LONGEST_DEFAULT_ROW_CHARS, BINDING_TEXT_SIZE)
    + ROW_HALVES_GAP
    + 2.0 * (BINDING_ROW_PADDING + BINDING_ROW_BORDER + PLAYER_COLUMN_PADDING);
/// The narrowest the sidebar's contents may be: two columns that narrow
/// and the gap between them.
const SIDEBAR_CONTENTS_MIN_WIDTH: f32 = 2.0 * PLAYER_COLUMN_MIN_WIDTH + SIDEBAR_COLUMN_GAP;

/// The Game tab's rows: a limit's, and the last, Count Captured Stones as
/// Points, which has no field beside its checkbox. On top they keep the
/// height they always had; in the sidebar they close up to the height of
/// the tallest row's own contents - End After Rounds Without a Board
/// Change, whose description takes two lines - so that the tab fits the
/// smallest window. A row whose error line is showing grows past either.
pub(super) const GAME_ROW_HEIGHTS: [f32; 2] = [77.0, 65.0];
pub(super) const COMPACT_GAME_ROW_HEIGHTS: [f32; 2] = [66.0, 56.0];

/// The arrangement a window gets; see the module docs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettingsArrangement {
    TabsOnTop,
    Sidebar,
}

/// Where the Settings screen's frame stands in one window, and how wide its
/// parts are; every length in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SettingsLayout {
    /// The size laid out for: the window's, raised to the minimum.
    pub(super) window: Vec2,
    pub(super) arrangement: SettingsArrangement,
    /// The screen's padding around the panel, across and down, each side.
    pub(super) root_padding: Vec2,
    /// The panel's outer size, border included.
    pub(super) panel_size: Vec2,
    /// The panel's padding inside its border, across and down, each side.
    pub(super) panel_padding: Vec2,
    pub(super) tab_width: f32,
    /// Each player's binding column, and the gap between the two.
    pub(super) player_column_width: f32,
    pub(super) player_column_gap: f32,
}

/// Lays Settings out for a window of `window` logical pixels, which the
/// caller has already raised to the minimum with `WindowSize::layout`.
pub(super) fn settings_layout(window: Vec2) -> SettingsLayout {
    if window.y >= SIDEBAR_BELOW {
        return SettingsLayout {
            window,
            arrangement: SettingsArrangement::TabsOnTop,
            root_padding: ((window - TABS_ON_TOP_PANEL) / 2.0)
                .min(Vec2::splat(TABS_ON_TOP_PADDING)),
            panel_size: TABS_ON_TOP_PANEL,
            panel_padding: Vec2::splat(TABS_ON_TOP_PADDING),
            tab_width: SIDEBAR_TAB_WIDTH,
            player_column_width: PLAYER_COLUMN_WIDTH,
            player_column_gap: TABS_ON_TOP_COLUMN_GAP,
        };
    }
    // The contents' width with the sidebar's own margins, for a tab width.
    let contents_width = |root: f32, panel: f32, tab: f32| {
        (window.x - 2.0 * root).min(SIDEBAR_PANEL_MAX_WIDTH)
            - 2.0 * (PANEL_BORDER + panel)
            - tab
            - SIDEBAR_BODY_GAP
    };
    let tab_width = if contents_width(
        SIDEBAR_ROOT_PADDING,
        SIDEBAR_PANEL_PADDING,
        SIDEBAR_TAB_WIDTH,
    ) >= SIDEBAR_CONTENTS_MIN_WIDTH
    {
        SIDEBAR_TAB_WIDTH
    } else {
        NARROW_TAB_WIDTH
    };
    // Each of the four margins across - outside and inside the panel, on
    // either side - gives up a quarter of what the contents still lack,
    // rounded up to a whole pixel: `bevy_ui` rounds a fractional margin, and
    // two of them nested would put the contents a pixel or more from where
    // this says they are.
    let shortfall = SIDEBAR_CONTENTS_MIN_WIDTH
        - contents_width(SIDEBAR_ROOT_PADDING, SIDEBAR_PANEL_PADDING, tab_width);
    let narrowing = (shortfall / 4.0).max(0.0).ceil();
    let root_padding = Vec2::new(SIDEBAR_ROOT_PADDING - narrowing, SIDEBAR_ROOT_PADDING);
    let panel_padding = Vec2::new(SIDEBAR_PANEL_PADDING - narrowing, SIDEBAR_PANEL_PADDING);
    let contents = contents_width(root_padding.x, panel_padding.x, tab_width);
    SettingsLayout {
        window,
        arrangement: SettingsArrangement::Sidebar,
        root_padding,
        panel_size: Vec2::new(
            (window.x - 2.0 * root_padding.x).min(SIDEBAR_PANEL_MAX_WIDTH),
            window.y - 2.0 * root_padding.y,
        ),
        panel_padding,
        tab_width,
        player_column_width: ((contents - SIDEBAR_COLUMN_GAP) / 2.0).min(PLAYER_COLUMN_WIDTH),
        player_column_gap: SIDEBAR_COLUMN_GAP,
    }
}

impl SettingsLayout {
    pub(super) fn sidebar(&self) -> bool {
        self.arrangement == SettingsArrangement::Sidebar
    }

    /// The Game tab's row heights here: a limit's, and the last row's.
    pub(super) fn game_row_heights(&self) -> [f32; 2] {
        if self.sidebar() {
            COMPACT_GAME_ROW_HEIGHTS
        } else {
            GAME_ROW_HEIGHTS
        }
    }

    /// The panel's box, border included: centred in the window.
    #[cfg(test)]
    pub(super) fn panel_rect(&self) -> Rect {
        Rect::from_center_size(self.window / 2.0, self.panel_size)
    }

    /// The box inside the panel's border and padding.
    #[cfg(test)]
    pub(super) fn content_rect(&self) -> Rect {
        let panel = self.panel_rect();
        let inset = self.panel_padding + Vec2::splat(PANEL_BORDER);
        Rect::from_corners(panel.min + inset, panel.max - inset)
    }

    /// The height of the sidebar's top row: the title's line, which is the
    /// tallest thing in it while the instruction, or the message standing
    /// in its place, takes two lines or fewer.
    #[cfg(test)]
    pub(super) fn head_height() -> f32 {
        text_height(1, SETTINGS_TITLE_SIZE)
    }

    /// The title's box in the sidebar's top row, at its left.
    #[cfg(test)]
    pub(super) fn sidebar_title_rect(&self) -> Rect {
        let content = self.content_rect();
        let title = Vec2::new(
            line_width(super::SETTINGS_TITLE.len(), SETTINGS_TITLE_SIZE),
            text_height(1, SETTINGS_TITLE_SIZE),
        );
        Rect::from_corners(content.min, content.min + title)
    }

    /// Reset Controls and Back together, in the sidebar's top row, at its
    /// right and centred down it.
    #[cfg(test)]
    pub(super) fn sidebar_footer_rect(&self) -> Rect {
        let content = self.content_rect();
        let top = content.min.y + (Self::head_height() - FOOTER_BUTTON_HEIGHT) / 2.0;
        Rect::new(
            content.max.x - FOOTER_WIDTH,
            top,
            content.max.x,
            top + FOOTER_BUTTON_HEIGHT,
        )
    }

    /// The room between the title and the two buttons in the sidebar's top
    /// row, where the instruction, or a message in its place, wraps.
    #[cfg(test)]
    pub(super) fn sidebar_notes_rect(&self) -> Rect {
        let title = self.sidebar_title_rect();
        let footer = self.sidebar_footer_rect();
        Rect::new(
            title.max.x + SIDEBAR_HEAD_GAP,
            title.min.y,
            footer.min.x - SIDEBAR_HEAD_GAP,
            title.max.y,
        )
    }

    /// The sidebar's column of tabs, each tab's box in tab order.
    #[cfg(test)]
    pub(super) fn sidebar_tab_rects(&self) -> [Rect; 4] {
        let content = self.content_rect();
        let top = content.min.y + Self::head_height() + PANEL_GAP;
        std::array::from_fn(|index| {
            let y = top + index as f32 * (TAB_HEIGHT + TAB_COLUMN_GAP);
            Rect::new(
                content.min.x,
                y,
                content.min.x + self.tab_width,
                y + TAB_HEIGHT,
            )
        })
    }

    /// Where the sidebar shows a tab's contents: right of the tabs, under
    /// the top row, to the panel's padding.
    #[cfg(test)]
    pub(super) fn sidebar_contents_rect(&self) -> Rect {
        let content = self.content_rect();
        Rect::new(
            content.min.x + self.tab_width + SIDEBAR_BODY_GAP,
            content.min.y + Self::head_height() + PANEL_GAP,
            content.max.x,
            content.max.y,
        )
    }

    /// The two binding columns, side by side and centred together in the
    /// tab's contents: White's, then Black's.
    #[cfg(test)]
    pub(super) fn player_column_spans(&self, contents: Rect) -> [(f32, f32); 2] {
        let both = 2.0 * self.player_column_width + self.player_column_gap;
        let left = contents.center().x - both / 2.0;
        let right = left + self.player_column_width + self.player_column_gap;
        [
            (left, left + self.player_column_width),
            (right, right + self.player_column_width),
        ]
    }

    /// The room left between the halves of the longest default row in a
    /// binding column this wide.
    #[cfg(test)]
    pub(super) fn longest_row_gap(&self) -> f32 {
        self.player_column_width
            - 2.0 * (BINDING_ROW_PADDING + BINDING_ROW_BORDER + PLAYER_COLUMN_PADDING)
            - line_width(LONGEST_DEFAULT_ROW_CHARS, BINDING_TEXT_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{NAMED_WINDOW_SIZES, WindowSize, layout_sweep};

    /// In the opening window Settings is what it always was: the 930x830
    /// panel in the middle, 55 px in from either side and 35 down, the
    /// tabs 155 px wide, and the two binding columns 420 px wide and 30
    /// apart, filling the panel's 870 px between them.
    #[test]
    fn the_opening_window_keeps_the_tabs_on_top_panel_as_it_was() {
        let opening = settings_layout(WindowSize::OPENING.0);
        assert_eq!(opening.arrangement, SettingsArrangement::TabsOnTop);
        assert_eq!(opening.panel_rect(), Rect::new(55.0, 35.0, 985.0, 865.0));
        assert_eq!(opening.content_rect(), Rect::new(85.0, 65.0, 955.0, 835.0));
        assert_eq!(opening.root_padding, Vec2::splat(28.0));
        assert_eq!(opening.panel_padding, Vec2::splat(28.0));
        assert_eq!(opening.tab_width, 155.0);
        assert_eq!(opening.player_column_width, 420.0);
        assert_eq!(opening.player_column_gap, 30.0);
        assert_eq!(
            opening.player_column_spans(opening.content_rect()),
            [(85.0, 505.0), (535.0, 955.0)]
        );
        assert_eq!(opening.game_row_heights(), [77.0, 65.0]);
        let gap = opening.longest_row_gap();
        assert!((14.7..14.9).contains(&gap), "{gap}");
    }

    /// The switch, a pixel either side of it, which the sweep's 20 px steps
    /// cannot pin: a window 854 px tall has the tabs-on-top panel with
    /// exactly 12 px above and below it, and one 853 px tall, at any width,
    /// has the sidebar.
    #[test]
    fn the_sidebar_is_used_below_854_px_and_tabs_on_top_from_there() {
        assert_eq!(SIDEBAR_BELOW, 854.0);
        for width in [1040.0, 1440.0, 2560.0] {
            let at = settings_layout(Vec2::new(width, 854.0));
            assert_eq!(at.arrangement, SettingsArrangement::TabsOnTop, "{width}");
            let panel = at.panel_rect();
            assert_eq!(panel.min.y, 12.0, "{width}");
            assert_eq!(854.0 - panel.max.y, 12.0, "{width}");
            assert_eq!(at.root_padding.y, 12.0, "{width}");
            for height in [853.0, 853.99] {
                let below = settings_layout(Vec2::new(width, height));
                assert_eq!(
                    below.arrangement,
                    SettingsArrangement::Sidebar,
                    "{width}x{height}"
                );
            }
        }
    }

    /// The layouts are pure: at every size the panel stands inside the
    /// window, on top with at least 12 px above and below it and in the
    /// sidebar with its margins; and in the sidebar the title, the room for
    /// the instruction and the two buttons share the top row without
    /// meeting, the tabs stand in their column under it and clear of the
    /// tab's contents, the binding columns fit the contents side by side
    /// and centred, and the longest default row keeps at least 12 px
    /// between its halves. Both arrangements occur.
    #[test]
    fn every_window_keeps_the_panel_inside_and_its_parts_apart() {
        let mut arrangements = [false, false];
        for size in layout_sweep() {
            let layout = settings_layout(size);
            let window = Rect::from_corners(Vec2::ZERO, size);
            let panel = layout.panel_rect();
            assert!(
                window.contains(panel.min) && window.contains(panel.max),
                "{size}: the panel {panel:?}"
            );
            // Centred: the same room either side, and above and below.
            assert!(
                (panel.min.x - (size.x - panel.max.x)).abs() < 1e-3,
                "{size}"
            );
            assert!(
                (panel.min.y - (size.y - panel.max.y)).abs() < 1e-3,
                "{size}"
            );
            match layout.arrangement {
                SettingsArrangement::TabsOnTop => {
                    arrangements[0] = true;
                    assert!(panel.min.y >= EDGE, "{size}: {panel:?}");
                    assert_eq!(panel.size(), TABS_ON_TOP_PANEL, "{size}");
                    assert_eq!(layout.player_column_width, PLAYER_COLUMN_WIDTH);
                    // The root's padding never reaches past the panel's
                    // edge, so `bevy_ui` centres the panel as this does.
                    let room = (size - TABS_ON_TOP_PANEL) / 2.0;
                    assert!(layout.root_padding.cmple(room).all(), "{size}");
                    assert!(layout.root_padding.cmpge(Vec2::splat(EDGE)).all(), "{size}");
                }
                SettingsArrangement::Sidebar => {
                    arrangements[1] = true;
                    assert!(size.y < SIDEBAR_BELOW, "{size}");
                    // The margins narrow across only, and never past half.
                    assert_eq!(layout.root_padding.y, SIDEBAR_ROOT_PADDING, "{size}");
                    assert_eq!(layout.panel_padding.y, SIDEBAR_PANEL_PADDING, "{size}");
                    assert!(
                        layout.root_padding.x >= SIDEBAR_ROOT_PADDING / 2.0,
                        "{size}"
                    );
                    assert!(
                        layout.panel_padding.x >= SIDEBAR_PANEL_PADDING / 2.0,
                        "{size}"
                    );
                    assert!(panel.width() <= SIDEBAR_PANEL_MAX_WIDTH, "{size}");

                    let content = layout.content_rect();
                    let parts = [
                        ("title", layout.sidebar_title_rect()),
                        ("instruction", layout.sidebar_notes_rect()),
                        ("buttons", layout.sidebar_footer_rect()),
                    ];
                    for (name, rect) in parts {
                        assert!(
                            rect.width() > 0.0
                                && content.contains(rect.min)
                                && content.contains(rect.max),
                            "{size}: the {name} {rect:?} leaves {content:?}"
                        );
                    }
                    for (index, (a_name, a)) in parts.iter().enumerate() {
                        for (b_name, b) in &parts[index + 1..] {
                            let gap = b.min.x - a.max.x;
                            assert!(
                                gap >= SIDEBAR_HEAD_GAP - 1e-3,
                                "{size}: the {a_name} and the {b_name} are {gap} apart"
                            );
                        }
                    }
                    let contents = layout.sidebar_contents_rect();
                    for (index, tab) in layout.sidebar_tab_rects().iter().enumerate() {
                        assert!(
                            content.contains(tab.min) && content.contains(tab.max),
                            "{size}: tab {index} {tab:?}"
                        );
                        assert!(tab.max.x < contents.min.x, "{size}: tab {index}");
                        assert!(tab.min.y >= parts[0].1.max.y, "{size}: tab {index}");
                    }
                    let spans = layout.player_column_spans(contents);
                    assert!(spans[0].0 >= contents.min.x - 1e-3, "{size}: {spans:?}");
                    assert!(spans[1].1 <= contents.max.x + 1e-3, "{size}: {spans:?}");
                    assert!(
                        (spans[0].0 - contents.min.x - (contents.max.x - spans[1].1)).abs() < 1e-3,
                        "{size}: the columns are centred together"
                    );
                    let gap = layout.longest_row_gap();
                    assert!(gap >= ROW_HALVES_GAP - 1e-3, "{size}: {gap}");
                }
            }
        }
        assert_eq!(arrangements, [true, true], "both arrangements occur");
    }

    /// The margins narrow only where the row would otherwise have less than
    /// 12 px between its halves, and then by the fewest whole pixels that
    /// keep it 12: up to about 1058 px wide, with 5 px from each at the
    /// minimum, where the row keeps 12.8. From there the sidebar has its own
    /// 10 px round the panel and 16 inside it; the tabs are 130 px wide below
    /// about 1083 px and 155 from there, where 155 still leaves the row its
    /// 12.
    #[test]
    fn the_sidebar_narrows_its_margins_only_where_the_longest_row_needs_it() {
        let smallest = settings_layout(WindowSize::MINIMUM.0);
        assert_eq!(smallest.tab_width, NARROW_TAB_WIDTH);
        assert_eq!(smallest.root_padding, Vec2::new(5.0, 10.0), "{smallest:?}");
        assert_eq!(
            smallest.panel_padding,
            Vec2::new(11.0, 16.0),
            "{smallest:?}"
        );
        let gap = smallest.longest_row_gap();
        assert!((gap - 12.8).abs() < 1e-3, "{gap}");
        // A pixel less narrowing would leave the row under 12.
        let column = |narrowing: f32| {
            let panel = 1040.0 - 2.0 * (SIDEBAR_ROOT_PADDING - narrowing);
            let contents = panel
                - 2.0 * (PANEL_BORDER + SIDEBAR_PANEL_PADDING - narrowing)
                - NARROW_TAB_WIDTH
                - SIDEBAR_BODY_GAP;
            (contents - SIDEBAR_COLUMN_GAP) / 2.0
        };
        assert_eq!(column(5.0), smallest.player_column_width);
        let fewer = SettingsLayout {
            player_column_width: column(4.0),
            ..smallest
        };
        assert!(fewer.longest_row_gap() < ROW_HALVES_GAP, "{fewer:?}");

        // The first width with no narrowing, a pixel either side of it.
        let full = 2.0 * (SIDEBAR_ROOT_PADDING + PANEL_BORDER + SIDEBAR_PANEL_PADDING)
            + NARROW_TAB_WIDTH
            + SIDEBAR_BODY_GAP
            + SIDEBAR_CONTENTS_MIN_WIDTH;
        assert_eq!(full.round(), 1058.0, "{full}");
        let at = settings_layout(Vec2::new(full, 600.0));
        assert_eq!(at.root_padding.x, SIDEBAR_ROOT_PADDING);
        assert_eq!(at.panel_padding.x, SIDEBAR_PANEL_PADDING);
        let short = settings_layout(Vec2::new(full - 1.0, 600.0));
        assert_eq!(
            short.root_padding.x,
            SIDEBAR_ROOT_PADDING - 1.0,
            "{short:?}"
        );
        assert!(short.longest_row_gap() >= ROW_HALVES_GAP, "{short:?}");

        // The wider tabs, a pixel either side of where they fit.
        let wide_tabs = full - NARROW_TAB_WIDTH + SIDEBAR_TAB_WIDTH;
        assert_eq!(wide_tabs.round(), 1083.0, "{wide_tabs}");
        assert_eq!(
            settings_layout(Vec2::new(wide_tabs, 700.0)).tab_width,
            SIDEBAR_TAB_WIDTH
        );
        assert_eq!(
            settings_layout(Vec2::new(wide_tabs - 1.0, 700.0)).tab_width,
            NARROW_TAB_WIDTH
        );
        for size in layout_sweep() {
            let layout = settings_layout(size);
            if layout.sidebar() && size.x >= full {
                assert_eq!(layout.root_padding.x, SIDEBAR_ROOT_PADDING, "{size}");
                assert_eq!(layout.panel_padding.x, SIDEBAR_PANEL_PADDING, "{size}");
            }
        }
    }

    /// In a wide sidebar the columns keep their 420 px and stand together
    /// in the middle of the contents, 20 px apart, rather than at its edges;
    /// the panel stops at 1400 px wide and stands in the middle of the
    /// window. The named sizes get these arrangements, a maximised
    /// 1440x900 Mac being taken as 1440x848.
    #[test]
    fn the_named_sizes_get_their_arrangements_and_wide_columns_stand_together() {
        let expected = [
            ("1040x900", SettingsArrangement::TabsOnTop),
            ("1040x600", SettingsArrangement::Sidebar),
            ("1280x641", SettingsArrangement::Sidebar),
            ("1280x720", SettingsArrangement::Sidebar),
            ("1366x689", SettingsArrangement::Sidebar),
            ("1366x768", SettingsArrangement::Sidebar),
            ("1440x848", SettingsArrangement::Sidebar),
            ("1920x1080", SettingsArrangement::TabsOnTop),
            ("2560x1440", SettingsArrangement::TabsOnTop),
        ];
        for (size, (name, arrangement)) in NAMED_WINDOW_SIZES.iter().zip(expected) {
            assert_eq!(format!("{}x{}", size.x, size.y), name);
            assert_eq!(settings_layout(*size).arrangement, arrangement, "{name}");
        }

        let wide = settings_layout(Vec2::new(2560.0, 800.0));
        assert_eq!(wide.panel_size.x, SIDEBAR_PANEL_MAX_WIDTH);
        assert_eq!(wide.panel_rect().min.x, 580.0);
        assert_eq!(wide.player_column_width, PLAYER_COLUMN_WIDTH);
        let contents = wide.sidebar_contents_rect();
        let spans = wide.player_column_spans(contents);
        assert_eq!(spans[1].0 - spans[0].1, SIDEBAR_COLUMN_GAP);
        assert!(
            spans[0].0 - contents.min.x > 100.0,
            "{spans:?} in {contents:?}"
        );
    }

    /// The longest default row is the one the tabs really draw: every
    /// action's label beside its default binding, on both tabs, for every
    /// kind of controller - and the longest is the Controller tab's Select
    /// with no controller assigned.
    #[test]
    fn the_longest_default_row_is_the_rows_the_tabs_draw() {
        use crate::input::GameAction;
        use crate::input::controller::{ControllerBindings, DeviceFamily, ProfileAction};
        use crate::input::keyboard::KeyboardBindings;
        use crate::rules::Player;

        let keyboard = KeyboardBindings::default();
        let controller = ControllerBindings::default();
        let mut rows: Vec<String> = GameAction::PLAYER_ONE
            .iter()
            .chain(&GameAction::PLAYER_TWO)
            .chain(&GameAction::SHARED)
            .map(|action| format!("{}{}", action.label(), keyboard.key(*action).display_name()))
            .collect();
        for family in [
            DeviceFamily::Unknown,
            DeviceFamily::Xbox,
            DeviceFamily::PlayStation,
            DeviceFamily::Nintendo,
        ] {
            for player in [Player::One, Player::Two] {
                for action in ProfileAction::ALL {
                    rows.push(format!(
                        "{}{}",
                        action.label(),
                        controller.input(player, action).display_name(family)
                    ));
                }
            }
        }
        let longest = rows.iter().map(String::len).max().unwrap();
        assert_eq!(longest, LONGEST_DEFAULT_ROW_CHARS);
        assert!(rows.contains(&"Select / Revise SelectionSouth Button".to_string()));
    }

    /// A window below the minimum is laid out at it.
    #[test]
    fn a_window_below_the_minimum_is_laid_out_at_the_minimum() {
        assert_eq!(
            settings_layout(WindowSize(Vec2::new(800.0, 480.0)).layout()),
            settings_layout(WindowSize::MINIMUM.0)
        );
    }
}
