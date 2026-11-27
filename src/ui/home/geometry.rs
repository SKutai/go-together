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
//! * **The hidden marimba** stands under the panel, where it has always
//!   been, wherever it is clear there of both the panel and the diamond.
//!   In a shorter window it would cover them - the diamond's bottom corner
//!   first, since the diamond reaches lower than the panel - so there it
//!   stands upright beside the panel instead, on the right, clear of the
//!   diamond's corner: its low bars at the bottom, the way the upright
//!   instrument would stand, and centred on the panel. [`home_layout`]
//!   works out the height it changes at from the panel's real size and the
//!   diamond's: about 885 px, since in the opening window the marimba
//!   stands only 7.7 px below the diamond's bottom corner. The tests hold
//!   it to both.

use bevy::prelude::*;

use crate::app::visuals::WorldFraming;
use crate::ui::text_metrics::text_height;

/// The margin the diamond's corners keep from the window's edges.
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
/// window and its depth, and how far above the window's bottom it stands.
/// Upright, the two lengths swap.
pub(super) const MARIMBA_LENGTH: f32 = 350.0;
pub(super) const MARIMBA_DEPTH: f32 = 92.0;
pub(super) const MARIMBA_BOTTOM: f32 = 28.0;
/// The gap the upright marimba keeps from the panel and the diamond beside
/// it: the one it keeps from the window's bottom under the panel.
const MARIMBA_GAP: f32 = MARIMBA_BOTTOM;

/// Where the marimba stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MarimbaStance {
    /// Across the window under the panel, as in the opening window.
    UnderPanel,
    /// Upright beside the panel, its node's top-left corner here.
    Upright(Vec2),
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
    let panel_bottom = centre.y + PANEL_HEIGHT / 2.0;
    let diamond_bottom = centre.y + (DIAMOND_HALF_DIAGONAL - DIAMOND_CENTER_Y) * zoom;
    let under_top = window.y - MARIMBA_BOTTOM - MARIMBA_DEPTH;
    let marimba = if under_top >= panel_bottom.max(diamond_bottom) {
        MarimbaStance::UnderPanel
    } else {
        let beside = (PANEL_WIDTH / 2.0).max(DIAMOND_HALF_DIAGONAL * zoom) + MARIMBA_GAP;
        MarimbaStance::Upright(Vec2::new(
            centre.x + beside,
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
    /// top-left.
    #[cfg(test)]
    pub(super) fn marimba_rect(&self) -> Rect {
        match self.marimba {
            MarimbaStance::UnderPanel => Rect::from_center_size(
                Vec2::new(
                    self.window.x / 2.0,
                    self.window.y - MARIMBA_BOTTOM - MARIMBA_DEPTH / 2.0,
                ),
                Vec2::new(MARIMBA_LENGTH, MARIMBA_DEPTH),
            ),
            MarimbaStance::Upright(top_left) => Rect::from_corners(
                top_left,
                top_left + Vec2::new(MARIMBA_DEPTH, MARIMBA_LENGTH),
            ),
        }
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

    /// Whether the marimba's node overlaps the diamond itself rather than
    /// only the box around it: the diamond is a square turned on its
    /// corners, so a box's corner nearest the diamond's centre is inside it
    /// exactly when its distance in x and y, added, is under the
    /// half-diagonal.
    #[cfg(test)]
    pub(super) fn marimba_meets_the_diamond(&self) -> bool {
        let diamond = self.diamond_rect();
        let marimba = self.marimba_rect();
        let centre = diamond.center();
        let nearest = centre.clamp(marimba.min, marimba.max);
        let reach = (nearest - centre).abs();
        reach.x + reach.y < DIAMOND_HALF_DIAGONAL * self.zoom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::visuals::{NAMED_WINDOW_SIZES, WindowSize, layout_sweep};

    /// In the opening window Home is what it always was: the diamond at its
    /// own size about the origin, and the marimba under the panel, clear of
    /// the diamond's bottom corner by the few pixels it always had.
    #[test]
    fn the_opening_window_keeps_homes_diamond_and_marimba_as_they_were() {
        let opening = home_layout(WindowSize::OPENING.0);
        assert_eq!(opening.zoom, 1.0);
        assert_eq!(opening.framing(), WorldFraming::IDENTITY);
        assert_eq!(opening.marimba, MarimbaStance::UnderPanel);
        assert_eq!(
            opening.marimba_rect(),
            Rect::new(345.0, 780.0, 695.0, 872.0)
        );
        let below_the_corner = opening.marimba_rect().min.y - opening.diamond_rect().max.y;
        assert!((7.0..8.0).contains(&below_the_corner), "{below_the_corner}");
    }

    /// At every size the diamond's corners stand inside the window, and the
    /// diamond is never drawn smaller than the panel is tall, nor larger
    /// than its own size; the marimba stands inside the window, clear of the
    /// panel and of the diamond; and both stances occur, upright only in the
    /// shorter windows.
    #[test]
    fn the_diamond_and_the_marimba_keep_inside_the_window_and_clear_of_the_panel() {
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
            let marimba = layout.marimba_rect();
            assert!(
                window.contains(marimba.min) && window.contains(marimba.max),
                "{size}: the marimba {marimba:?}"
            );
            assert!(
                marimba.intersect(layout.panel_rect()).is_empty(),
                "{size}: the marimba {marimba:?} meets the panel {:?}",
                layout.panel_rect()
            );
            assert!(
                !layout.marimba_meets_the_diamond(),
                "{size}: the marimba {marimba:?} meets the diamond {diamond:?}"
            );
            stances[usize::from(matches!(layout.marimba, MarimbaStance::Upright(_)))] = true;
            if size.y >= WindowSize::OPENING.0.y {
                assert_eq!(layout.marimba, MarimbaStance::UnderPanel, "{size}");
            }
        }
        assert_eq!(stances, [true, true], "both stances occur");
        for size in NAMED_WINDOW_SIZES {
            let upright = matches!(home_layout(size).marimba, MarimbaStance::Upright(_));
            assert_eq!(upright, size.y < 885.0, "{size}");
        }
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
