//! Where Home's diamond and its marimba stand, for a window of any size.
//!
//! Home is centred, as it has always been: its panel stands in the middle
//! of the window at its own size, whatever the window does. Two things
//! answer to the window's size, and [`home_layout`] decides both as a pure
//! function of it:
//!
//! * **The diamond** behind the panel is world-space decoration, drawn by
//!   the main camera, so a window shorter than the opening one shrinks it
//!   through the camera's zoom ([`HomeLayout::framing`]) until its corners
//!   stay [`EDGE`] inside the window - but never below the panel's own
//!   height, so it still frames the panel. It never grows past its own
//!   size.
//! * **The hidden marimba** stands under the panel where it always has
//!   beside the panel and the diamond: its bars 17.7 px under the
//!   diamond's bottom corner, which is where the opening window draws
//!   them (measured under `bevy_ui`'s layout on 2026-11-27). The panel and the diamond are centred on the window, so in a
//!   taller window the three move together, and the marimba keeps that
//!   place in every window whose bottom leaves its bars [`EDGE`] to
//!   spare there - every window at least 848 px tall, which [`home_layout`]
//!   works out from where the bars are drawn in the instrument
//!   ([`MARIMBA_DRAWN_INSET`]), not from the instrument's node, which draws
//!   nothing. In a shorter window its bars would reach the bottom edge, so
//!   there it stands upright beside the panel instead, on the right, its
//!   bars [`UPRIGHT_GAP`] clear of the panel or of the diamond's right
//!   corner, whichever reaches further: its low bars at the bottom, the way
//!   the upright instrument would stand, and centred on the panel. The
//!   tests hold the switch to 848 px, and the inset to what `bevy_ui` lays
//!   out.

use bevy::prelude::*;

use crate::app::visuals::WorldFraming;
use crate::ui::text_metrics::text_height;

/// The margin the diamond's corners keep from the window's edges, and the
/// marimba's bars, under the panel, from its bottom.
const EDGE: f32 = 12.0;

/// The diamond: two squares turned on their corners, the orange frame and
/// the panel-coloured face inside it, both centred this far above the
/// window's centre, in world units.
pub(super) const DIAMOND_SIDE: f32 = 470.0;
pub(super) const DIAMOND_FACE_SIDE: f32 = 430.0;
pub(super) const DIAMOND_CENTER_Y: f32 = 10.0;
/// From the diamond's centre to any of its corners.
const DIAMOND_HALF_DIAGONAL: f32 = DIAMOND_SIDE * std::f32::consts::FRAC_1_SQRT_2;

/// The panel and what it holds, top to bottom: the title, the tagline with
/// its margin below, the four menu buttons and the subtitle with its margin
/// above, with the panel's gap between each two and its padding and border
/// around them. Its height is these added up; a test lays the real panel
/// out and holds the sum to it.
pub(super) const PANEL_WIDTH: f32 = 590.0;
pub(super) const PANEL_PADDING: Vec2 = Vec2::new(54.0, 46.0);
pub(super) const PANEL_BORDER: f32 = 2.0;
pub(super) const PANEL_GAP: f32 = 16.0;
pub(super) const TITLE_SIZE: f32 = 68.0;
pub(super) const TAGLINE_SIZE: f32 = 15.0;
pub(super) const TAGLINE_MARGIN_BELOW: f32 = 20.0;
pub(super) const SUBTITLE_SIZE: f32 = 14.0;
pub(super) const SUBTITLE_MARGIN_ABOVE: f32 = 12.0;
/// Each menu button's height, from `ui::widgets`.
const MENU_BUTTON_HEIGHT: f32 = 54.0;
const MENU_BUTTONS: usize = 4;
pub(super) const PANEL_HEIGHT: f32 = 2.0 * (PANEL_PADDING.y + PANEL_BORDER)
    + text_height(1, TITLE_SIZE)
    + text_height(1, TAGLINE_SIZE)
    + TAGLINE_MARGIN_BELOW
    + MENU_BUTTONS as f32 * MENU_BUTTON_HEIGHT
    + SUBTITLE_MARGIN_ABOVE
    + text_height(1, SUBTITLE_SIZE)
    + (MENU_BUTTONS + 2) as f32 * PANEL_GAP;

/// The marimba's node, standing under the panel: its length across the
/// window and its depth. Upright, the two lengths swap.
pub(super) const MARIMBA_LENGTH: f32 = 350.0;
pub(super) const MARIMBA_DEPTH: f32 = 92.0;
/// Its bars, one to a note.
pub(super) const HOME_MARIMBA_BAR_COUNT: usize = 11;
/// A bar's breadth, and its length: the lowest bar's, less this much for
/// each step up the scale.
pub(super) const BAR_BREADTH: f32 = 23.0;
pub(super) const LOWEST_BAR_LENGTH: f32 = 72.0;
pub(super) const BAR_LENGTH_STEP: f32 = 3.2;
/// The gap between two bars, and the node's padding around them: along
/// the instrument, and across it.
pub(super) const BAR_GAP: f32 = 5.0;
pub(super) const PADDING_ALONG: f32 = 9.0;
pub(super) const PADDING_ACROSS: f32 = 8.0;
/// How far inside the node, across the instrument, its bars are drawn:
/// the node centres them, and the lowest bar is the longest. So under the
/// panel the bars start this far below the node's top and end this far
/// above its bottom, and upright this far in from its sides.
pub(super) const MARIMBA_DRAWN_INSET: f32 = (MARIMBA_DEPTH - LOWEST_BAR_LENGTH) / 2.0;
const _: () = assert!(
    MARIMBA_DRAWN_INSET >= PADDING_ACROSS,
    "the lowest bar fits inside the padding, so the node centres it"
);
/// Under the panel, how far below the window's centre the node's top
/// stands: where the opening window has always put it, 780 px down a
/// 900 px window.
const UNDER_PANEL_TOP: f32 = 330.0;
/// Upright, the gap between its bars and whatever of the panel and the
/// diamond reaches further towards it: enough that the instrument reads as
/// standing beside the panel rather than against its border.
pub(super) const UPRIGHT_GAP: f32 = 38.0;

/// Where the marimba stands, and its node's top-left corner there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MarimbaStance {
    /// Across the window under the panel, as in the opening window.
    UnderPanel(Vec2),
    /// Upright beside the panel.
    Upright(Vec2),
}

impl MarimbaStance {
    /// The node's top-left corner, in logical pixels from the window's
    /// top-left.
    pub(super) fn top_left(self) -> Vec2 {
        match self {
            Self::UnderPanel(top_left) | Self::Upright(top_left) => top_left,
        }
    }
}

/// Where Home's diamond and marimba stand in one window; every length in
/// logical pixels from the window's top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HomeLayout {
    /// The size laid out for: the window's, raised to the minimum.
    pub(super) window: Vec2,
    /// How large the diamond is drawn against its own size.
    pub(super) zoom: f32,
    pub(super) marimba: MarimbaStance,
}

/// Lays Home out for a window of `window` logical pixels, which the caller
/// has already raised to the minimum with `WindowSize::layout`.
pub(super) fn home_layout(window: Vec2) -> HomeLayout {
    let corners_inside = (window.y / 2.0 - EDGE) / (DIAMOND_HALF_DIAGONAL + DIAMOND_CENTER_Y);
    let zoom = corners_inside.clamp(PANEL_HEIGHT / (2.0 * DIAMOND_HALF_DIAGONAL), 1.0);
    let centre = window / 2.0;
    let under_panel = Vec2::new(centre.x - MARIMBA_LENGTH / 2.0, centre.y + UNDER_PANEL_TOP);
    let bars_bottom = under_panel.y + MARIMBA_DEPTH - MARIMBA_DRAWN_INSET;
    let marimba = if bars_bottom + EDGE <= window.y {
        MarimbaStance::UnderPanel(under_panel)
    } else {
        let reach = (PANEL_WIDTH / 2.0).max(DIAMOND_HALF_DIAGONAL * zoom);
        MarimbaStance::Upright(Vec2::new(
            centre.x + reach + UPRIGHT_GAP - MARIMBA_DRAWN_INSET,
            centre.y - MARIMBA_LENGTH / 2.0,
        ))
    };
    HomeLayout {
        window,
        zoom,
        marimba,
    }
}

impl HomeLayout {
    /// The main camera's framing: the diamond at its zoom, about the
    /// world's origin, which the window's centre stays on.
    pub(super) fn framing(&self) -> WorldFraming {
        WorldFraming {
            zoom: self.zoom,
            center: Vec2::ZERO,
        }
    }

    /// The marimba node's box, in logical pixels from the window's
    /// top-left. It draws nothing, but it is a node, so it must not cover
    /// a button.
    #[cfg(test)]
    pub(super) fn marimba_rect(&self) -> Rect {
        let top_left = self.marimba.top_left();
        let size = match self.marimba {
            MarimbaStance::UnderPanel(_) => Vec2::new(MARIMBA_LENGTH, MARIMBA_DEPTH),
            MarimbaStance::Upright(_) => Vec2::new(MARIMBA_DEPTH, MARIMBA_LENGTH),
        };
        Rect::from_corners(top_left, top_left + size)
    }

    /// The box the marimba's bars are drawn in: its node's, less
    /// [`MARIMBA_DRAWN_INSET`] across the instrument and, along it, the
    /// room its row of bars leaves at each end.
    #[cfg(test)]
    pub(super) fn marimba_bars_rect(&self) -> Rect {
        let bars = HOME_MARIMBA_BAR_COUNT as f32;
        let along = (MARIMBA_LENGTH - bars * BAR_BREADTH - (bars - 1.0) * BAR_GAP) / 2.0;
        let inset = match self.marimba {
            MarimbaStance::UnderPanel(_) => Vec2::new(along, MARIMBA_DRAWN_INSET),
            MarimbaStance::Upright(_) => Vec2::new(MARIMBA_DRAWN_INSET, along),
        };
        let node = self.marimba_rect();
        Rect::from_corners(node.min + inset, node.max - inset)
    }

    /// The panel's box: centred in the window at its own size.
    #[cfg(test)]
    pub(super) fn panel_rect(&self) -> Rect {
        Rect::from_center_size(self.window / 2.0, Vec2::new(PANEL_WIDTH, PANEL_HEIGHT))
    }

    /// The box around the diamond's four corners, where it is drawn.
    #[cfg(test)]
    pub(super) fn diamond_rect(&self) -> Rect {
        let reach = DIAMOND_HALF_DIAGONAL * self.zoom;
        let centre = self.window / 2.0 - Vec2::new(0.0, DIAMOND_CENTER_Y * self.zoom);
        Rect::from_center_half_size(centre, Vec2::splat(reach))
    }

    /// Whether the marimba's bars meet the diamond itself rather than only
    /// the box around it: the diamond is a square turned on its corners, so
    /// a box's corner nearest the diamond's centre is inside it exactly when
    /// its distance in x and y, added, is under the half-diagonal.
    #[cfg(test)]
    pub(super) fn marimba_meets_the_diamond(&self) -> bool {
        let diamond = self.diamond_rect();
        let bars = self.marimba_bars_rect();
        let centre = diamond.center();
        let nearest = centre.clamp(bars.min, bars.max);
        let reach = (nearest - centre).abs();
        reach.x + reach.y < DIAMOND_HALF_DIAGONAL * self.zoom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{WindowSize, layout_sweep};

    /// In the opening window Home is what it always was: the diamond at its
    /// own size about the origin, and the marimba under the panel, where it
    /// was always spawned, its bars 17.7 px under the diamond's bottom
    /// corner.
    #[test]
    fn the_opening_window_keeps_homes_diamond_and_marimba_as_they_were() {
        let opening = home_layout(WindowSize::OPENING.0);
        assert_eq!(opening.zoom, 1.0);
        assert_eq!(opening.framing(), WorldFraming::IDENTITY);
        assert_eq!(
            opening.marimba,
            MarimbaStance::UnderPanel(Vec2::new(345.0, 780.0))
        );
        assert_eq!(
            opening.marimba_rect(),
            Rect::new(345.0, 780.0, 695.0, 872.0)
        );
        assert_eq!(
            opening.marimba_bars_rect(),
            Rect::new(368.5, 790.0, 671.5, 862.0)
        );
        let below_the_corner = opening.marimba_bars_rect().min.y - opening.diamond_rect().max.y;
        assert!(
            (17.6..17.7).contains(&below_the_corner),
            "{below_the_corner}"
        );
    }

    /// At every size the diamond's corners stand inside the window, and the
    /// diamond is never drawn smaller than the panel is tall, nor larger
    /// than its own size. The marimba's bars stand at least [`EDGE`] inside
    /// the window, clear of the panel and of the diamond, and its node clear
    /// of the panel. Under the panel it stands exactly where the opening
    /// window has it beside the panel and the diamond, which is in every
    /// window at least 848 px tall and in no other; upright, its bars stand
    /// [`UPRIGHT_GAP`] beside whichever of the two reaches further. Both
    /// stances occur.
    #[test]
    fn the_diamond_and_the_marimba_keep_inside_the_window_and_clear_of_the_panel() {
        let opening = home_layout(WindowSize::OPENING.0);
        let opening_place = opening.marimba_bars_rect().min - opening.window / 2.0;
        let mut stances = [false, false];
        for size in layout_sweep() {
            let layout = home_layout(size);
            let window = Rect::from_corners(Vec2::ZERO, size).inflate(-EDGE + 1e-3);
            let diamond = layout.diamond_rect();
            assert!(
                window.contains(diamond.min) && window.contains(diamond.max),
                "{size}: the diamond's corners {diamond:?}"
            );
            assert!(layout.zoom <= 1.0, "{size}");
            assert!(diamond.height() >= PANEL_HEIGHT - 1e-3, "{size}");
            let (node, bars, panel) = (
                layout.marimba_rect(),
                layout.marimba_bars_rect(),
                layout.panel_rect(),
            );
            assert!(
                window.contains(bars.min) && window.contains(bars.max),
                "{size}: the marimba's bars {bars:?}"
            );
            for (what, rect) in [("node", node), ("bars", bars)] {
                assert!(
                    rect.intersect(panel).is_empty(),
                    "{size}: the marimba's {what} {rect:?} meets the panel {panel:?}"
                );
            }
            assert!(
                !layout.marimba_meets_the_diamond(),
                "{size}: the marimba's bars {bars:?} meet the diamond {diamond:?}"
            );
            match layout.marimba {
                MarimbaStance::UnderPanel(_) => {
                    assert_eq!(layout.zoom, 1.0, "{size}: the diamond at its own size");
                    let place = bars.min - size / 2.0;
                    assert!(place.distance(opening_place) < 1e-3, "{size}: {place}");
                }
                MarimbaStance::Upright(_) => {
                    let reach = panel.max.x.max(diamond.max.x);
                    assert!((bars.min.x - reach - UPRIGHT_GAP).abs() < 1e-3, "{size}");
                    assert!((bars.center().y - panel.center().y).abs() < 1e-3, "{size}");
                }
            }
            let upright = matches!(layout.marimba, MarimbaStance::Upright(_));
            assert_eq!(upright, size.y < 848.0, "{size}");
            stances[usize::from(upright)] = true;
        }
        assert_eq!(stances, [true, true], "both stances occur");
    }

    /// The diamond shrinks only in a window shorter than the opening one,
    /// to 0.84 of its size in the smallest.
    #[test]
    fn the_diamond_shrinks_only_in_a_shorter_window() {
        let smallest = home_layout(WindowSize::MINIMUM.0).zoom;
        assert!((0.835..0.845).contains(&smallest), "{smallest}");
        let full_size_from = 2.0 * (DIAMOND_HALF_DIAGONAL + DIAMOND_CENTER_Y + EDGE);
        for size in layout_sweep() {
            let zoom = home_layout(size).zoom;
            if size.y >= full_size_from {
                assert_eq!(zoom, 1.0, "{size}");
            } else {
                assert!(zoom < 1.0, "{size}");
            }
        }
    }

    /// A window below the minimum is laid out at it.
    #[test]
    fn a_window_below_the_minimum_is_laid_out_at_the_minimum() {
        assert_eq!(
            home_layout(WindowSize(Vec2::new(800.0, 480.0)).layout()),
            home_layout(WindowSize::MINIMUM.0)
        );
    }
}
