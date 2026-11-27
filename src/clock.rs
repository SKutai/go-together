//! A precise, monotonic time vocabulary for authoritative Match logic.
//!
//! [`MatchInstant`] (a point on the authoritative Match clock) and
//! [`MatchDuration`] (a non-negative span of that clock) are whole
//! microseconds, not `f64` seconds. Only the authoritative Match clock -
//! never Bevy's per-frame `Time`, and never a Replay's persisted seconds -
//! decides official outcomes: simultaneous-versus-sequential resolution,
//! Round Time, Match Time, Round 0 countdown timing, and hold-to-end/restart
//! durations. Comparing two [`MatchInstant`]s or [`MatchDuration`]s is
//! always an exact integer comparison, so - unlike the `f64`-seconds
//! representation this replaces - a boundary like "exactly 0.1 seconds
//! apart" never needs an epsilon fudge to survive floating-point rounding.
//!
//! # Where these types come from
//!
//! Nothing in this module reads a clock. The Bevy client is the sole
//! producer of elapsed-time [`MatchDuration`] deltas today, converting
//! Bevy's `Time::delta()` (already an exact `std::time::Duration`, so no
//! floating-point round-trip) once per frame at each of its two boundaries:
//! `crate::gameplay::countdown::advance_match_clock`, whose budget drives
//! the Match clock, and `crate::gameplay::holds::handle_hold_gestures`, the
//! restart hold's time base. A future server would produce them from its
//! own tick the same way, without changing anything downstream.
//! [`MatchDuration::from_secs_f64`] and [`MatchInstant::from_secs_f64`] are
//! the deliberate conversion point to and from Replay's persisted
//! `f64`-seconds timestamps and durations, and Bevy presentation code
//! converts back with `as_secs_f64` to format the HUD's elapsed Match timer
//! or a hold-progress bar. The Round Time clock is read both ways: its
//! *countdown text* takes whole seconds through
//! [`MatchDuration::as_secs_ceil`], while the round-time bar below the board
//! is sized from
//! [`crate::authority::MatchState::round_time_remaining_fraction`], which
//! divides in `f64` - the one such conversion that happens inside
//! `authority` rather than in Bevy code, so that the bar and the countdown
//! read the same subtraction. Replay *playback*
//! position is a third, separate clock
//! ([`crate::replay::PlaybackController`]): it scrubs a finished Replay's
//! own timeline and never advances a live [`MatchInstant`].
//!
//! # Monotonicity and overflow
//!
//! Authoritative time must never move backward, overflow, or produce a
//! negative duration. Every arithmetic operation here is saturating rather
//! than panicking or wrapping:
//!
//! - [`MatchDuration::from_secs_f64`] and [`MatchInstant::from_secs_f64`]
//!   clamp a NaN or negative input to `0`, and an infinite or otherwise
//!   too-large one to `u64::MAX` microseconds ([`MatchDuration::MAX`]):
//!   Rust's `f64 as u64` cast is already total, so neither ever panics.
//!   Per-frame deltas never take this path - they arrive as an exact
//!   `std::time::Duration` through the `From` impl below, which can be
//!   neither NaN nor negative - and `authority`'s `bounded_delta` caps any
//!   oversized budget at the Replay-duration limit, so neither a corrupt
//!   persisted value nor an implausible frame delta can carry the Match
//!   clock past what a Replay can hold.
//! - [`MatchInstant::saturating_add`] and [`MatchDuration::saturating_add`]
//!   clamp at `u64::MAX` microseconds (about 584,942 years of Match time)
//!   instead of wrapping.
//! - [`MatchInstant::saturating_duration_since`] clamps to
//!   [`MatchDuration::ZERO`] if `earlier` is actually later than `self`.
//!   This should never happen for an authoritative clock that only ever
//!   advances, but a defensive clock does not panic if it does.

use std::fmt;

const MICROS_PER_SEC: u64 = 1_000_000;

/// Converts seconds to whole microseconds, saturating at the `u64` bounds.
///
/// Relies on `f64 as u64`'s total (never-panicking) cast semantics: NaN and
/// negative inputs become `0`; inputs at or beyond `u64::MAX` microseconds
/// become `u64::MAX`.
fn seconds_to_micros(seconds: f64) -> u64 {
    (seconds * MICROS_PER_SEC as f64) as u64
}

fn micros_to_seconds(micros: u64) -> f64 {
    micros as f64 / MICROS_PER_SEC as f64
}

/// A point on the authoritative Match clock: whole microseconds since a
/// Match's Round 0 began. Never negative.
#[derive(Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct MatchInstant(u64);

impl MatchInstant {
    /// The Match clock's origin: Round 0 has just begun.
    pub const ZERO: Self = Self(0);

    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub const fn as_micros(self) -> u64 {
        self.0
    }

    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(MICROS_PER_SEC))
    }

    /// Converts from Replay's persisted `f64` seconds, or from a display or
    /// test value in seconds. Bevy frame deltas do not take this path: they
    /// convert through [`MatchDuration`]'s `From<std::time::Duration>` impl
    /// instead. See the module documentation for how NaN, negative, and
    /// out-of-range inputs are handled.
    pub fn from_secs_f64(seconds: f64) -> Self {
        Self(seconds_to_micros(seconds))
    }

    pub fn as_secs_f64(self) -> f64 {
        micros_to_seconds(self.0)
    }

    /// Advances by `duration`, saturating instead of overflowing.
    pub const fn saturating_add(self, duration: MatchDuration) -> Self {
        Self(self.0.saturating_add(duration.0))
    }

    /// The non-negative span from `earlier` to `self`, saturating to
    /// [`MatchDuration::ZERO`] if `earlier` is actually later than `self`.
    pub const fn saturating_duration_since(self, earlier: Self) -> MatchDuration {
        MatchDuration(self.0.saturating_sub(earlier.0))
    }
}

impl fmt::Debug for MatchInstant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MatchInstant({}s)", self.as_secs_f64())
    }
}

impl std::ops::Add<MatchDuration> for MatchInstant {
    type Output = Self;

    fn add(self, duration: MatchDuration) -> Self {
        self.saturating_add(duration)
    }
}

impl std::ops::AddAssign<MatchDuration> for MatchInstant {
    fn add_assign(&mut self, duration: MatchDuration) {
        *self = *self + duration;
    }
}

/// A non-negative span of authoritative Match time, in whole microseconds.
#[derive(Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct MatchDuration(u64);

impl MatchDuration {
    pub const ZERO: Self = Self(0);
    /// About 584,942 years of Match time; used as an "unbounded"/"no
    /// deadline" sentinel where the original `f64` code used `INFINITY`.
    pub const MAX: Self = Self(u64::MAX);

    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub const fn as_micros(self) -> u64 {
        self.0
    }

    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.saturating_mul(1_000))
    }

    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(MICROS_PER_SEC))
    }

    /// Converts from Replay's persisted `f64` seconds, or from a display or
    /// test value in seconds. Bevy frame deltas do not take this path: they
    /// convert through [`MatchDuration`]'s `From<std::time::Duration>` impl
    /// instead. See the module documentation for how NaN, negative, and
    /// out-of-range inputs are handled.
    pub fn from_secs_f64(seconds: f64) -> Self {
        Self(seconds_to_micros(seconds))
    }

    pub fn as_secs_f64(self) -> f64 {
        micros_to_seconds(self.0)
    }

    /// Whole seconds, rounded up: the unit the player-facing Round Time
    /// countdown displays. Its one caller is
    /// [`crate::authority::MatchState::round_time_remaining_seconds`].
    pub const fn as_secs_ceil(self) -> u64 {
        self.0.div_ceil(MICROS_PER_SEC)
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub const fn min(self, other: Self) -> Self {
        if self.0 < other.0 { self } else { other }
    }
}

impl fmt::Debug for MatchDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MatchDuration({}s)", self.as_secs_f64())
    }
}

impl std::ops::Add for MatchDuration {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.saturating_add(other)
    }
}

impl std::ops::AddAssign for MatchDuration {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl std::ops::Sub for MatchDuration {
    type Output = Self;

    /// Saturates at [`MatchDuration::ZERO`] rather than panicking, matching
    /// the module's monotonic, never-negative invariant.
    fn sub(self, other: Self) -> Self {
        self.saturating_sub(other)
    }
}

impl From<std::time::Duration> for MatchDuration {
    /// The Bevy client's per-frame adapter: `Time::delta()` is already an
    /// exact, nanosecond-precision `std::time::Duration` with no floating
    /// point involved, so converting it directly (rather than through
    /// `delta_secs_f64`) avoids a float round-trip for the conversions that
    /// run every single frame. Sub-microsecond precision is truncated,
    /// never rounded up, so this can only ever under-report elapsed time.
    fn from(duration: std::time::Duration) -> Self {
        Self(duration.as_micros().min(u128::from(u64::MAX)) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_round_trip_for_ordinary_values() {
        for seconds in [0.0, 0.1, 1.0, 1.05, 1.1, 59.999, 600.0] {
            let instant = MatchInstant::from_secs_f64(seconds);
            assert!((instant.as_secs_f64() - seconds).abs() < 1e-9);
            let duration = MatchDuration::from_secs_f64(seconds);
            assert!((duration.as_secs_f64() - seconds).abs() < 1e-9);
        }
    }

    #[test]
    fn exact_tenth_second_window_matches_one_hundred_thousand_micros() {
        assert_eq!(
            MatchDuration::from_secs_f64(0.1).as_micros(),
            100_000,
            "0.1s must convert to exactly the documented 100ms simultaneous window"
        );
    }

    #[test]
    fn invalid_or_backward_inputs_clamp_to_zero_instead_of_panicking() {
        assert_eq!(MatchDuration::from_secs_f64(f64::NAN), MatchDuration::ZERO);
        assert_eq!(MatchDuration::from_secs_f64(-5.0), MatchDuration::ZERO);
        assert_eq!(MatchInstant::from_secs_f64(f64::NAN), MatchInstant::ZERO);
        assert_eq!(MatchInstant::from_secs_f64(-1.0), MatchInstant::ZERO);

        let earlier = MatchInstant::from_secs(10);
        let later = MatchInstant::from_secs(5);
        assert_eq!(
            later.saturating_duration_since(earlier),
            MatchDuration::ZERO,
            "time moving backward must not produce a negative duration"
        );
    }

    #[test]
    fn overflow_saturates_instead_of_wrapping() {
        let near_max = MatchInstant::from_micros(u64::MAX - 1);
        assert_eq!(
            near_max.saturating_add(MatchDuration::from_secs(1_000_000_000)),
            MatchInstant::from_micros(u64::MAX)
        );

        let huge = MatchDuration::from_secs_f64(f64::MAX);
        assert_eq!(huge, MatchDuration::MAX);

        let infinite = MatchDuration::from_secs_f64(f64::INFINITY);
        assert_eq!(infinite, MatchDuration::MAX);
    }

    #[test]
    fn as_secs_ceil_rounds_up_to_whole_seconds() {
        assert_eq!(MatchDuration::from_secs_f64(0.0).as_secs_ceil(), 0);
        assert_eq!(MatchDuration::from_secs_f64(0.001).as_secs_ceil(), 1);
        assert_eq!(MatchDuration::from_secs(1).as_secs_ceil(), 1);
        assert_eq!(MatchDuration::from_secs_f64(59.01).as_secs_ceil(), 60);
    }

    #[test]
    fn duration_arithmetic_saturates_at_zero() {
        let small = MatchDuration::from_secs(1);
        let big = MatchDuration::from_secs(2);
        assert_eq!(small.saturating_sub(big), MatchDuration::ZERO);
        assert_eq!(small - big, MatchDuration::ZERO);
        assert_eq!(small.min(big), small);
    }

    #[test]
    fn converts_exactly_from_a_std_duration_with_no_float_rounding() {
        let duration = std::time::Duration::from_nanos(100_000_999);
        assert_eq!(
            MatchDuration::from(duration).as_micros(),
            100_000,
            "sub-microsecond precision truncates; it never rounds a frame delta up"
        );
    }
}
