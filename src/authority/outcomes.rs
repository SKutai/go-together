//! The vocabulary every command answers in: what a move, a selection, a
//! resolved round, or a step of the Match clock actually did.
//!
//! These types are one module because they exist for one reason. A command
//! on [`super::MatchState`] returns a description of what happened rather
//! than reaching out to record a Replay event, queue a sound, or finish a
//! Match itself, so the orchestration layer - the Bevy client today, a
//! headless host tomorrow - decides what that means. Grouping them here
//! keeps that contract one readable list rather than a property you have to
//! infer from method signatures scattered across the module. [`super`]'s
//! "Commands and outcomes" section is the argument for it; this is its
//! vocabulary.
//!
//! Nothing here reaches back into [`super::state`]: an outcome is a plain
//! value, and depends only on [`super::values`] and the rules types the
//! Match is played with.

use crate::clock::MatchDuration;
use crate::opening::OpeningCountdownStage;
use crate::replay::EndReason;
use crate::rules::{Player, Point, RemovedStone, ResolutionKind, RoundSound};

use super::values::OrdinaryCompletion;

/// A cursor actually moved from one point to another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorMove {
    pub player: Player,
    pub from: Point,
    pub to: Point,
}

/// A player's cursor actually jumped to a different point via its marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TeleportJump {
    pub player: Player,
    pub from: Point,
    pub to: Point,
}

/// The result of one Round 0 selection attempt.
pub enum OpeningSelectOutcome {
    /// Not in an opening phase; nothing happened.
    Inactive,
    /// A new point was added to the player's rolling window.
    Added {
        point: Point,
        evicted: Option<Point>,
        /// `true` when this selection completed both players' windows and
        /// started the closing countdown.
        countdown_started: bool,
    },
    /// The player re-selected a point already in their window.
    Duplicate { point: Point },
    /// The cursor was outside the board (should not happen in practice).
    OutOfBounds,
}

/// The result of one ordinary hidden-selection attempt.
///
/// [`Self::Unchanged`] is separated from [`Self::Locked`] because the two
/// differ in what the caller should record, not in what the player is
/// allowed to see: a re-press on the point a player already holds leaves the
/// authoritative selection byte-for-byte as it was, timestamp included, so
/// there is no change for a Replay to record. It must stay as silent as
/// [`Self::Rejected`] - see [`super::MatchState::try_lock`] for why every
/// outcome here is silent.
pub enum LockOutcome {
    /// Occupied, reserved, or both players already have a selection locked in.
    Rejected,
    /// The selection was recorded, replacing any selection this player held.
    Locked { point: Point },
    /// The player re-selected the point they already hold. Nothing changed,
    /// including the timestamp the round's timing is resolved from.
    Unchanged { point: Point },
}

/// One authoritative ordinary round's result, ready for orchestration to
/// record, cue, and act on.
///
/// `sound` is the pure capture/Gray summary the rules produced; mapping it to
/// an actual audio cue is the caller's concern (the Bevy client turns it into a
/// [`crate::cue::GameCue`]), not something this authoritative core depends on.
pub struct RoundOutcome {
    /// How the round resolved - the [`ResolutionKind`] a Replay persists.
    pub kind: ResolutionKind,
    /// Stones credited as captures to Black and White by this round, indexed
    /// by [`super::player_index`]; see
    /// [`crate::rules::RoundResolution::capture_credits`] for what counts.
    /// Already folded into [`super::MatchProgress::capture_totals`].
    pub capture_credits: [usize; 2],
    /// Which players held a hidden selection when the round completed, read
    /// before the selections were resolved and cleared. This is what a
    /// `RoundTimedOut` Replay event records.
    pub submitted: [bool; 2],
    pub sound: RoundSound,
    /// Automatic end reasons crossed by this round, including a forced
    /// [`EndReason::MatchTimeLimit`] when `completion` was a Match-time
    /// timeout. Empty when play continues.
    pub end_reasons: Vec<EndReason>,
    /// Every stone the round took off the board, with its owner - what the
    /// credits above count. Also kept on the state as
    /// [`super::MatchProgress::last_removals`], for a reader that was not
    /// the caller.
    pub removed: Vec<RemovedStone>,
}

/// An authoritative boundary the Match clock stopped at while advancing -
/// see [`super::MatchState::advance_clock`].
///
/// Every variant is reported *after* the transition it names has been
/// applied to the state, and before anything the caller owns: the closing
/// countdown has already moved on to `stage`, Round 0 has already been
/// revealed at `Go` and on a Round 0 Match Time expiry, the Go card has
/// already cleared. What has deliberately *not* happened yet is anything a
/// caller interposes on - the round a timeout ends is not resolved, and a
/// Match that ran out of time is not finished - because the Bevy client
/// records Replay events between exactly those steps and, for a Round Time
/// timeout, may defer the automatic end to an in-flight end gesture. A Match
/// Time timeout is never deferred: it ends the gesture and finishes at once.
/// The two timeout variants therefore carry an obligation: the caller
/// resolves (see [`ClockBoundary::completion`]) or finishes before advancing
/// again, and until it does, every further
/// [`super::MatchState::advance_clock`] call consumes nothing and reports the
/// same boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockBoundary {
    /// The closing countdown reached `stage` - `Two`, `One`, or `Go`, never
    /// `Three`, which [`super::MatchState::start_opening_countdown`] enters. At
    /// `Go` the six Round 0 slots have been revealed and the Match is
    /// `Playing` Round 1; the caller records the reveal and the stage, in
    /// that order, if it records anything.
    OpeningCountdown(OpeningCountdownStage),
    /// The Go card's display time ended
    /// ([`crate::opening::OPENING_GO_DISPLAY`] after the reveal), with
    /// ordinary play already under way beneath it.
    OpeningCountdownFinished,
    /// Round Time ran out for the current ordinary round. Nothing has been
    /// resolved: the caller resolves it with
    /// [`OrdinaryCompletion::RoundTimeout`], then finishes the Match if the
    /// outcome carries end reasons.
    RoundTimeExpired,
    /// Match Time ran out during ordinary play. The clock is pinned at the
    /// limit and nothing has been resolved: the caller resolves the current
    /// round with [`OrdinaryCompletion::MatchTimeTimeout`], whose outcome
    /// always carries [`EndReason::MatchTimeLimit`], then finishes.
    MatchTimeExpired,
    /// Match Time ran out before ordinary play began. Round 0's selections,
    /// complete or not, have been revealed as they stood - a partial or
    /// empty opening is still a scoreable board - so the Match is `Playing`
    /// Round 1 with no round to resolve; the caller finishes it with
    /// [`EndReason::MatchTimeLimit`].
    MatchTimeExpiredInRoundZero,
}

impl ClockBoundary {
    /// The completion the caller resolves the current round with when this
    /// boundary demands one - the mapping a headless host needs so it never
    /// re-derives which timeout it is looking at.
    pub const fn completion(self) -> Option<OrdinaryCompletion> {
        match self {
            Self::RoundTimeExpired => Some(OrdinaryCompletion::RoundTimeout),
            Self::MatchTimeExpired => Some(OrdinaryCompletion::MatchTimeTimeout),
            Self::OpeningCountdown(_)
            | Self::OpeningCountdownFinished
            | Self::MatchTimeExpiredInRoundZero => None,
        }
    }
}

/// What one [`super::MatchState::advance_clock`] call did with its budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockStep {
    /// How much Match time actually passed: the whole budget when nothing
    /// was crossed, otherwise exactly the time up to `crossed`.
    pub consumed: MatchDuration,
    /// What is left of the budget after this step. This is the budget capped
    /// at what a Replay can still persist
    /// ([`crate::replay::MAX_REPLAY_DURATION_SECONDS`] less the time already
    /// elapsed), minus `consumed`, so feeding it back into the next call
    /// keeps a single oversized budget from carrying time past that cap.
    pub remaining: MatchDuration,
    /// The boundary this step stopped at, or `None` when the budget ran out
    /// first - or when there was nothing left to advance, which is how an
    /// `Ended` Match or an exhausted clock answers.
    pub crossed: Option<ClockBoundary>,
}
