//! Time as a command: [`MatchState::advance_clock`], the per-phase advance
//! it dispatches to, the two closing-countdown readers
//! ([`MatchState::opening_countdown_stage`] and
//! [`MatchState::opening_go_remaining`]), and the deadline arithmetic behind
//! all of them.
//!
//! This is the one place the Match's *own* timing lives - which closing
//! countdown stage comes next and when, the atomic reveal, the Go card
//! clearing, the Round Time and Match Time deadlines, and the cap that keeps
//! `elapsed` within what a Replay can persist. A caller supplies only how
//! much time passed; what that time *means* is decided here, in whole
//! integer microseconds, so no host re-derives Match timing and no `f64`
//! takes part in an authoritative decision. The arithmetic that implements
//! that - `bounded_delta`, `match_time_remaining`, `round_time_remaining`,
//! and the `match_time_expired`/`round_time_expired` predicates - is private
//! to this module for exactly that reason, and the public surface is
//! [`MatchState::advance_clock`] plus the two readers. The two Round Time
//! readers outside this module, [`MatchState::round_time_remaining_seconds`]
//! and [`MatchState::round_time_remaining_fraction`] in [`super::state`],
//! are display values - whole seconds, rounded up, for the HUD's countdown,
//! and the share of the round still to run, for the bar below the board -
//! computed from the same subtraction, and no authoritative decision reads
//! either.
//!
//! These are inherent methods on [`MatchState`], which [`super::state`]
//! declares; this module adds to the same type rather than wrapping it. See
//! [`super`]'s "Commands and outcomes" and "Clocks" sections for where a
//! step deliberately stops and what it leaves to the caller, and
//! [`super::outcomes::ClockBoundary`] for the per-boundary obligations.

use crate::clock::{MatchDuration, MatchInstant};
use crate::match_settings::MatchSettingField;
use crate::opening::{
    OPENING_COUNTDOWN_STAGE, OPENING_COUNTDOWN_TOTAL, OPENING_REVEAL, OpeningCountdownStage,
};

use super::outcomes::{ClockBoundary, ClockStep};
use super::state::MatchState;
use super::values::MatchPhase;

impl MatchState {
    /// Advances the Match clock by up to `budget`, stopping at the first
    /// authoritative boundary it reaches.
    ///
    /// This is the one place the Match's own timing lives: which closing
    /// countdown stage comes next and when, the atomic reveal at
    /// [`crate::opening::OPENING_REVEAL`], the Go card clearing, and the
    /// Round Time and Match Time deadlines, all in whole microseconds. A
    /// caller drives a Match through time by calling this repeatedly - each
    /// call with the previous [`ClockStep::remaining`] - and acting on each
    /// [`ClockBoundary`] before the next call, until a step crosses nothing.
    /// A step never spans two boundaries: one unusually long budget still
    /// stops at every countdown stage and every deadline in turn, each at
    /// its exact Match-clock instant, so a caller recording the state after
    /// each step gets the same timeline a series of short budgets would
    /// produce. Nothing here consults a real clock, records anything, or
    /// finishes a Match - see [`ClockBoundary`] for exactly where a step
    /// stops and what it leaves to the caller.
    ///
    /// A budget of zero is a valid question: it consumes nothing and reports
    /// a deadline that has already landed and is still waiting to be acted
    /// on, or `None`.
    pub fn advance_clock(&mut self, budget: MatchDuration) -> ClockStep {
        let budget = self.bounded_delta(budget);
        let (consumed, crossed) = match self.phase {
            MatchPhase::Opening => self.advance_opening(budget),
            MatchPhase::OpeningCountdown => self.advance_opening_countdown(budget),
            MatchPhase::Playing => self.advance_playing(budget),
            MatchPhase::Ended => (MatchDuration::ZERO, None),
        };
        ClockStep {
            consumed,
            remaining: budget.saturating_sub(consumed),
            crossed,
        }
    }

    /// Interactive Round 0 before the countdown: only Match Time can end it.
    fn advance_opening(&mut self, budget: MatchDuration) -> (MatchDuration, Option<ClockBoundary>) {
        let advance = budget.min(self.match_time_remaining().unwrap_or(MatchDuration::MAX));
        self.elapsed += advance;
        if self.match_time_expired() {
            self.expire_match_time_in_round_zero();
            return (advance, Some(ClockBoundary::MatchTimeExpiredInRoundZero));
        }
        (advance, None)
    }

    /// The closing 3-2-1 countdown, stepped one stage at a time.
    fn advance_opening_countdown(
        &mut self,
        budget: MatchDuration,
    ) -> (MatchDuration, Option<ClockBoundary>) {
        let two_stages = OPENING_COUNTDOWN_STAGE.saturating_add(OPENING_COUNTDOWN_STAGE);
        let (next_threshold, stage) = if self.countdown_elapsed < OPENING_COUNTDOWN_STAGE {
            (OPENING_COUNTDOWN_STAGE, OpeningCountdownStage::Two)
        } else if self.countdown_elapsed < two_stages {
            (two_stages, OpeningCountdownStage::One)
        } else {
            (OPENING_REVEAL, OpeningCountdownStage::Go)
        };
        let to_threshold = next_threshold.saturating_sub(self.countdown_elapsed);
        let advance = budget
            .min(to_threshold)
            .min(self.match_time_remaining().unwrap_or(MatchDuration::MAX));
        self.countdown_elapsed += advance;
        self.elapsed += advance;

        if self.match_time_expired() {
            self.expire_match_time_in_round_zero();
            return (advance, Some(ClockBoundary::MatchTimeExpiredInRoundZero));
        }
        if self.countdown_elapsed >= next_threshold {
            if stage == OpeningCountdownStage::Go {
                self.reveal_round_zero();
            }
            return (advance, Some(ClockBoundary::OpeningCountdown(stage)));
        }
        (advance, None)
    }

    /// Ordinary play: the Go card clearing, then the two deadlines, Match
    /// Time taking precedence over Round Time when both land at once.
    fn advance_playing(&mut self, budget: MatchDuration) -> (MatchDuration, Option<ClockBoundary>) {
        let to_match = self.match_time_remaining().unwrap_or(MatchDuration::MAX);
        let to_round = self.round_time_remaining().unwrap_or(MatchDuration::MAX);
        let to_go_finish = self.opening_go_remaining();
        let advance = budget
            .min(to_match)
            .min(to_round)
            .min(to_go_finish.unwrap_or(MatchDuration::MAX));
        self.elapsed += advance;
        self.progress.round_elapsed += advance;
        if to_go_finish.is_some() {
            self.countdown_elapsed += advance;
            if self.countdown_elapsed >= OPENING_COUNTDOWN_TOTAL {
                self.countdown_elapsed = OPENING_COUNTDOWN_TOTAL;
                return (advance, Some(ClockBoundary::OpeningCountdownFinished));
            }
        }
        if self.match_time_expired() {
            self.clamp_match_time();
            return (advance, Some(ClockBoundary::MatchTimeExpired));
        }
        if self.round_time_expired() {
            return (advance, Some(ClockBoundary::RoundTimeExpired));
        }
        (advance, None)
    }

    /// Match Time landed before ordinary play began: pin the clock and
    /// reveal whatever Round 0 holds so there is a board to score.
    fn expire_match_time_in_round_zero(&mut self) {
        self.clamp_match_time();
        self.reveal_round_zero();
    }

    /// Which closing-countdown card is showing, if any: the numbered stages
    /// while the countdown runs, and `Go` for
    /// [`crate::opening::OPENING_GO_DISPLAY`] after the reveal while ordinary
    /// play is already active beneath it.
    pub fn opening_countdown_stage(&self) -> Option<OpeningCountdownStage> {
        match self.phase {
            MatchPhase::OpeningCountdown => match self.countdown_elapsed {
                elapsed if elapsed < OPENING_COUNTDOWN_STAGE => Some(OpeningCountdownStage::Three),
                elapsed
                    if elapsed
                        < OPENING_COUNTDOWN_STAGE.saturating_add(OPENING_COUNTDOWN_STAGE) =>
                {
                    Some(OpeningCountdownStage::Two)
                }
                _ => Some(OpeningCountdownStage::One),
            },
            MatchPhase::Playing if self.opening_go_remaining().is_some() => {
                Some(OpeningCountdownStage::Go)
            }
            MatchPhase::Opening | MatchPhase::Playing | MatchPhase::Ended => None,
        }
    }

    /// How much longer the Go card stays up, or `None` once it has cleared
    /// (or never showed: a Match that skipped the countdown, or has not
    /// reached it).
    pub fn opening_go_remaining(&self) -> Option<MatchDuration> {
        (self.phase == MatchPhase::Playing
            && self.countdown_elapsed >= OPENING_REVEAL
            && self.countdown_elapsed < OPENING_COUNTDOWN_TOTAL)
            .then(|| OPENING_COUNTDOWN_TOTAL.saturating_sub(self.countdown_elapsed))
    }

    /// Caps `delta` so `elapsed` cannot advance past what a Replay can ever
    /// persist. `delta` is already non-negative and finite by construction
    /// (see [`crate::clock`]), so this is the only remaining clamp.
    fn bounded_delta(&self, delta: MatchDuration) -> MatchDuration {
        let max_replay_duration =
            MatchDuration::from_secs_f64(crate::replay::MAX_REPLAY_DURATION_SECONDS);
        let remaining_capacity = max_replay_duration
            .saturating_sub(self.elapsed.saturating_duration_since(MatchInstant::ZERO));
        delta.min(remaining_capacity)
    }

    fn match_time_remaining(&self) -> Option<MatchDuration> {
        self.match_rules
            .enabled(MatchSettingField::MatchTime)
            .then(|| {
                MatchDuration::from_secs(u64::from(
                    self.match_rules.value(MatchSettingField::MatchTime),
                ))
                .saturating_sub(self.elapsed.saturating_duration_since(MatchInstant::ZERO))
            })
    }

    fn round_time_remaining(&self) -> Option<MatchDuration> {
        self.match_rules
            .enabled(MatchSettingField::RoundTime)
            .then(|| {
                MatchDuration::from_secs(u64::from(
                    self.match_rules.value(MatchSettingField::RoundTime),
                ))
                .saturating_sub(self.progress.round_elapsed)
            })
    }

    /// Exact: `remaining` is a saturating subtraction, so it is precisely
    /// [`MatchDuration::ZERO`] once the deadline has passed - no epsilon
    /// fudge is needed the way `f64` seconds required one.
    fn match_time_expired(&self) -> bool {
        self.match_time_remaining()
            .is_some_and(MatchDuration::is_zero)
    }

    /// Pins `elapsed` to the Match Time limit. The clock only ever advances
    /// by at most [`Self::match_time_remaining`], so the integer arithmetic
    /// already lands exactly on the limit; this keeps that a guarantee
    /// rather than a property of the arithmetic.
    fn clamp_match_time(&mut self) {
        self.elapsed = MatchInstant::ZERO
            + MatchDuration::from_secs(u64::from(
                self.match_rules.value(MatchSettingField::MatchTime),
            ));
    }

    fn round_time_expired(&self) -> bool {
        self.round_time_remaining()
            .is_some_and(MatchDuration::is_zero)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::test_support::*;
    use crate::authority::values::{MatchRules, OrdinaryCompletion};
    use crate::match_settings::MatchSettings;
    use crate::replay::EndReason;
    use crate::rules::{Cell, Player, Point};

    fn secs(seconds: f64) -> MatchDuration {
        MatchDuration::from_secs_f64(seconds)
    }

    fn settings_with(fields: &[(MatchSettingField, u64)]) -> MatchSettings {
        let mut settings = MatchSettings::default();
        for (field, value) in fields {
            settings.set_value(*field, *value);
            settings.set_enabled(*field, true);
        }
        settings
    }

    /// A Match in its closing countdown at elapsed zero, both windows full.
    fn counting_down(settings: &MatchSettings) -> MatchState {
        let mut state = MatchState::default();
        state.begin_round_zero(settings);
        for x in 0..3 {
            state.cursors[0] = Point::new(x, 0);
            state.try_opening_select(Player::One);
            state.cursors[1] = Point::new(x, 8);
            state.try_opening_select(Player::Two);
        }
        assert_eq!(state.phase, MatchPhase::OpeningCountdown);
        state
    }

    /// One oversized budget still stops at every countdown stage, each at
    /// its exact instant, with the reveal folded into the `Go` step.
    #[test]
    fn advance_clock_steps_the_countdown_one_stage_at_a_time() {
        let mut state = counting_down(&MatchSettings::default());

        let step = state.advance_clock(secs(10.0));
        assert_eq!(step.consumed, OPENING_COUNTDOWN_STAGE);
        assert_eq!(step.remaining, secs(9.0));
        assert_eq!(
            step.crossed,
            Some(ClockBoundary::OpeningCountdown(OpeningCountdownStage::Two))
        );
        assert_eq!(
            state.opening_countdown_stage(),
            Some(OpeningCountdownStage::Two)
        );

        let step = state.advance_clock(step.remaining);
        assert_eq!(step.consumed, OPENING_COUNTDOWN_STAGE);
        assert_eq!(
            step.crossed,
            Some(ClockBoundary::OpeningCountdown(OpeningCountdownStage::One))
        );
        assert_eq!(state.phase, MatchPhase::OpeningCountdown);

        let step = state.advance_clock(step.remaining);
        assert_eq!(step.consumed, OPENING_COUNTDOWN_STAGE);
        assert_eq!(
            step.crossed,
            Some(ClockBoundary::OpeningCountdown(OpeningCountdownStage::Go))
        );
        assert_eq!(state.phase, MatchPhase::Playing);
        assert_eq!(state.round, 1);
        assert_eq!(state.elapsed, MatchInstant::ZERO + OPENING_REVEAL);
        assert_eq!(state.board.get(Point::new(0, 0)), Cell::Black);
        assert_eq!(
            state.opening_go_remaining(),
            Some(crate::opening::OPENING_GO_DISPLAY)
        );

        let step = state.advance_clock(step.remaining);
        assert_eq!(step.consumed, crate::opening::OPENING_GO_DISPLAY);
        assert_eq!(step.crossed, Some(ClockBoundary::OpeningCountdownFinished));
        assert_eq!(state.opening_countdown_stage(), None);
        assert_eq!(state.round_elapsed, crate::opening::OPENING_GO_DISPLAY);

        let step = state.advance_clock(step.remaining);
        assert_eq!(
            step.consumed,
            secs(10.0).saturating_sub(OPENING_COUNTDOWN_TOTAL)
        );
        assert_eq!(step.remaining, MatchDuration::ZERO);
        assert_eq!(step.crossed, None);
        assert_eq!(state.elapsed, MatchInstant::from_secs(10));
    }

    /// Match Time landing at the same instant as Round Time is reported as
    /// Match Time, once, with the clock pinned exactly on the limit.
    #[test]
    fn advance_clock_reports_match_time_over_a_simultaneous_round_deadline() {
        let mut state = active_state();
        state.match_rules = MatchRules::snapshot(&settings_with(&[
            (MatchSettingField::RoundTime, 1),
            (MatchSettingField::MatchTime, 1),
        ]));

        let step = state.advance_clock(secs(2.5));
        assert_eq!(step.consumed, secs(1.0));
        assert_eq!(step.remaining, secs(1.5));
        assert_eq!(step.crossed, Some(ClockBoundary::MatchTimeExpired));
        assert_eq!(state.elapsed, MatchInstant::from_secs(1));
        assert_eq!(
            step.crossed.and_then(ClockBoundary::completion),
            Some(OrdinaryCompletion::MatchTimeTimeout)
        );

        let outcome = state
            .resolve_ordinary_round(OrdinaryCompletion::MatchTimeTimeout)
            .expect("a Match Time timeout always resolves");
        assert_eq!(outcome.end_reasons, vec![EndReason::MatchTimeLimit]);
        assert!(state.finish(outcome.end_reasons));
        assert_eq!(
            state.advance_clock(secs(1.0)),
            ClockStep {
                consumed: MatchDuration::ZERO,
                remaining: secs(1.0),
                crossed: None,
            },
            "an Ended Match neither advances nor reports anything"
        );
    }

    /// Match Time expiring during Round 0 reveals whatever was chosen and
    /// leaves the finish to the caller, in both interactive phases.
    #[test]
    fn advance_clock_reveals_round_zero_when_match_time_expires_before_play() {
        let settings = settings_with(&[(MatchSettingField::MatchTime, 2)]);

        let mut interactive = MatchState::default();
        interactive.begin_round_zero(&settings);
        interactive.cursors[1] = Point::new(4, 4);
        interactive.try_opening_select(Player::Two);
        let step = interactive.advance_clock(secs(5.0));
        assert_eq!(step.consumed, secs(2.0));
        assert_eq!(
            step.crossed,
            Some(ClockBoundary::MatchTimeExpiredInRoundZero)
        );
        assert_eq!(step.crossed.and_then(ClockBoundary::completion), None);
        assert_eq!(interactive.phase, MatchPhase::Playing);
        assert_eq!(interactive.round, 1);
        assert_eq!(interactive.board.get(Point::new(4, 4)), Cell::White);
        assert!(interactive.finish(vec![EndReason::MatchTimeLimit]));

        let mut mid_countdown = counting_down(&settings);
        assert_eq!(
            mid_countdown.advance_clock(secs(1.0)).crossed,
            Some(ClockBoundary::OpeningCountdown(OpeningCountdownStage::Two))
        );
        let step = mid_countdown.advance_clock(secs(5.0));
        assert_eq!(step.consumed, secs(1.0));
        assert_eq!(
            step.crossed,
            Some(ClockBoundary::MatchTimeExpiredInRoundZero)
        );
        assert_eq!(mid_countdown.phase, MatchPhase::Playing);
        assert_eq!(mid_countdown.elapsed, MatchInstant::from_secs(2));
        assert_eq!(mid_countdown.board.get(Point::new(2, 8)), Cell::White);
    }

    /// The clock never carries `elapsed` past what a Replay can persist,
    /// and `remaining` tells the caller so.
    #[test]
    fn advance_clock_caps_the_budget_at_the_replay_duration_limit() {
        let cap = MatchDuration::from_secs_f64(crate::replay::MAX_REPLAY_DURATION_SECONDS);
        let mut state = active_state();
        state.elapsed = MatchInstant::ZERO + cap.saturating_sub(secs(1.0));

        let step = state.advance_clock(secs(3.0));
        assert_eq!(step.consumed, secs(1.0));
        assert_eq!(step.remaining, MatchDuration::ZERO);
        assert_eq!(step.crossed, None);
        assert_eq!(state.elapsed, MatchInstant::ZERO + cap);

        let step = state.advance_clock(secs(3.0));
        assert_eq!(step.consumed, MatchDuration::ZERO);
        assert_eq!(step.remaining, MatchDuration::ZERO);
        assert_eq!(step.crossed, None);
        assert_eq!(state.elapsed, MatchInstant::ZERO + cap);
    }
}
