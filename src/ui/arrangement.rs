//! The rule that chooses between a screen's two arrangements, which the
//! Match screen and the Replay Viewer both lay themselves out by, each from
//! its own margins, so that the two switch by one rule.
//!
//! # Tall and wide
//!
//! Each of the two screens draws a square board that is the one thing on it
//! to grow and shrink. *Tall* is the layout the screen was first made for,
//! with its interface above and below the board; *wide* sets things beside
//! the board instead. Either keeps fixed room around the board - above it,
//! below it and on each side ([`Margins`]) - so the board it would draw in a
//! window is the window less that room, in whichever direction is shorter
//! ([`Margins::room`]). The board stops growing at [`MAX_ZOOM`] times its
//! own size, the cap ([`Arrangements::cap`]).
//!
//! Wide is used only where it would draw the board more than [`WIDE_GAIN`]
//! times as large as tall would - 4% larger - so a window keeps the layout
//! it was first made for until the change is worth making, and near the
//! boundary a slightly larger window can get a board up to 4% smaller as it
//! crosses back into tall. The margins are fixed pixels, so below the cap
//! the point where wide takes over depends on the window's size as well as
//! its shape.
//!
//! Past the cap - in a window where the larger of the two rooms would draw
//! the board at more than its cap - the window's shape alone decides, so
//! that a window keeps its arrangement however large it grows: the choice is
//! the one the same shape gets at the size where that larger room just
//! reaches the cap ([`Arrangements::choose`]). At the cap itself the two
//! agree, so no window changes arrangement as it grows across it. Each
//! screen's own module says where its margins put the boundary.

use bevy::math::Vec2;

/// How much more room wide must give the board than tall before it is
/// used: 4%.
pub(super) const WIDE_GAIN: f32 = 1.04;
/// The most a board grows: twice its own size.
pub(super) const MAX_ZOOM: f32 = 2.0;

/// Which of the two arrangements a window gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Arrangement {
    Tall,
    Wide,
}

/// The room one arrangement keeps around its square board, in logical
/// pixels: above it, below it, and on each of its two sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Margins {
    pub(super) top: f32,
    pub(super) bottom: f32,
    pub(super) side: f32,
}

impl Margins {
    /// The board this arrangement would draw in `window`, before the cap:
    /// the window less the room it keeps, across or down, whichever leaves
    /// less.
    pub(super) fn room(self, window: Vec2) -> f32 {
        (window.x - 2.0 * self.side).min(window.y - self.top - self.bottom)
    }

    /// The factor that scales `window`, keeping its shape, to the size where
    /// [`Self::room`] is exactly `cap`. The room is the lesser of a width
    /// and a height, each the window's less fixed margins, so it reaches the
    /// cap at the scale where the later of those two does.
    fn scale_to_reach(self, window: Vec2, cap: f32) -> f32 {
        ((cap + 2.0 * self.side) / window.x).max((cap + (self.top + self.bottom)) / window.y)
    }
}

/// One screen's two arrangements and the most its board is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Arrangements {
    pub(super) tall: Margins,
    pub(super) wide: Margins,
    /// The board's largest size, in logical pixels: its own size times
    /// [`MAX_ZOOM`].
    pub(super) cap: f32,
}

impl Arrangements {
    /// The arrangement `window` gets. Below the cap, wide where its room is
    /// more than [`WIDE_GAIN`] times tall's. Past it, where the larger room
    /// would pass the cap, the same choice is made instead at the window
    /// scaled down, at the same shape, to the size where that room just
    /// reaches the cap, and kept for every larger window of that shape.
    pub(super) fn choose(&self, window: Vec2) -> Arrangement {
        let at = window * self.cap_scale(window).min(1.0);
        if self.wide.room(at) > self.tall.room(at) * WIDE_GAIN {
            Arrangement::Wide
        } else {
            Arrangement::Tall
        }
    }

    /// The margins of one arrangement.
    pub(super) fn margins(&self, arrangement: Arrangement) -> Margins {
        match arrangement {
            Arrangement::Tall => self.tall,
            Arrangement::Wide => self.wide,
        }
    }

    /// The board `arrangement` draws in `window`: its room, capped.
    pub(super) fn board(&self, window: Vec2, arrangement: Arrangement) -> f32 {
        self.margins(arrangement).room(window).min(self.cap)
    }

    /// The factor that scales `window`, keeping its shape, to the size where
    /// the larger of the two rooms is exactly the cap: the larger room is the
    /// one that reaches it first.
    fn cap_scale(&self, window: Vec2) -> f32 {
        self.tall
            .scale_to_reach(window, self.cap)
            .min(self.wide.scale_to_reach(window, self.cap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Margins with nothing to them but the numbers, so that the rule can
    /// be read off by hand: tall keeps 100 above and below and 50 a side,
    /// wide 20 above and below and 200 a side, and the cap is 1000.
    const SAMPLE: Arrangements = Arrangements {
        tall: Margins {
            top: 100.0,
            bottom: 100.0,
            side: 50.0,
        },
        wide: Margins {
            top: 20.0,
            bottom: 20.0,
            side: 200.0,
        },
        cap: 1000.0,
    };

    /// A room is the window less the margins, across or down, whichever is
    /// less; and the board is that room, capped.
    #[test]
    fn a_room_is_the_window_less_its_margins_in_the_shorter_direction() {
        let window = Vec2::new(900.0, 700.0);
        assert_eq!(SAMPLE.tall.room(window), 500.0, "700 less 200 down");
        assert_eq!(SAMPLE.wide.room(window), 500.0, "900 less 400 across");
        let window = Vec2::new(1000.0, 700.0);
        assert_eq!(SAMPLE.wide.room(window), 600.0);
        assert_eq!(
            SAMPLE.board(Vec2::new(5000.0, 5000.0), Arrangement::Wide),
            1000.0,
            "capped"
        );
        assert_eq!(SAMPLE.margins(Arrangement::Tall), SAMPLE.tall);
        assert_eq!(SAMPLE.margins(Arrangement::Wide), SAMPLE.wide);
    }

    /// Below the cap, wide is chosen exactly where its room is more than
    /// 4% larger than tall's: on the line where it is 4% larger, tall.
    #[test]
    fn below_the_cap_wide_needs_more_than_four_percent_more_room() {
        // Tall's room is 500 (height-limited); wide's is the width less 400.
        let height = 700.0;
        let even = 400.0 + 500.0 * WIDE_GAIN;
        assert_eq!(
            SAMPLE.choose(Vec2::new(even, height)),
            Arrangement::Tall,
            "exactly 4% more is not more"
        );
        assert_eq!(
            SAMPLE.choose(Vec2::new(even + 0.01, height)),
            Arrangement::Wide
        );
        assert_eq!(SAMPLE.choose(Vec2::new(900.0, height)), Arrangement::Tall);
    }

    /// Past the cap the shape alone decides: a shape keeps its arrangement
    /// at every size from the one where its larger room reaches the cap, and
    /// it is the arrangement it has there. Worked out by hand for these
    /// margins, the boundary past the cap is where wide's width-limited room
    /// is the cap and tall's height-limited room the cap over 1.04: a window
    /// 1400 wide by 1000 / 1.04 + 200 tall, a proportion of about 1.205.
    #[test]
    fn past_the_cap_the_shape_alone_decides() {
        let boundary = 1400.0 / (1000.0 / WIDE_GAIN + 200.0);
        for (aspect, arrangement) in [
            (boundary - 0.002, Arrangement::Tall),
            (boundary + 0.002, Arrangement::Wide),
        ] {
            for height in [1400.0, 2000.0, 4000.0, 40_000.0] {
                let window = Vec2::new(height * aspect, height);
                assert!(
                    SAMPLE.tall.room(window).max(SAMPLE.wide.room(window)) > SAMPLE.cap,
                    "{window} is past the cap"
                );
                assert_eq!(SAMPLE.choose(window), arrangement, "{window}");
            }
        }
        // Below the cap the same shape need not agree: the rule there is
        // the rooms', and they depend on the size.
        let small = Vec2::new(boundary + 0.01, 1.0) * 600.0;
        assert_eq!(SAMPLE.choose(small), Arrangement::Tall, "{small}");
    }

    /// Past the cap, shape by shape from 0.8 to 2 times as wide as tall,
    /// the choice is the one the rooms make at the size where the larger of
    /// them reaches the cap: found here by halving the window's scale until
    /// it is there, not by the closed form the rule uses.
    #[test]
    fn past_the_cap_each_shape_is_decided_where_its_larger_room_reaches_the_cap() {
        let larger = |window: Vec2| SAMPLE.tall.room(window).max(SAMPLE.wide.room(window));
        let mut both = [false, false];
        for step in 0..=2_400 {
            let aspect = 0.8 + step as f32 * 0.0005;
            for height in [2_000.0, 6_000.0] {
                let window = Vec2::new(height * aspect, height);
                assert!(larger(window) > SAMPLE.cap, "{window} is past the cap");
                let (mut low, mut high) = (0.0_f32, 1.0_f32);
                for _ in 0..48 {
                    let middle = (low + high) / 2.0;
                    if larger(window * middle) < SAMPLE.cap {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                let at = window * high;
                let wide = SAMPLE.wide.room(at) > SAMPLE.tall.room(at) * WIDE_GAIN;
                let expected = if wide {
                    Arrangement::Wide
                } else {
                    Arrangement::Tall
                };
                assert_eq!(SAMPLE.choose(window), expected, "{window}");
                both[usize::from(wide)] = true;
            }
        }
        assert_eq!(both, [true, true], "the shapes reach both");
    }
}
