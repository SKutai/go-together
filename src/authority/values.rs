//! Fundamental value types shared by the rest of `authority`: the Match life
//! cycle ([`MatchPhase`]), the immutable [`MatchRules`] snapshot a Match is
//! played under, why an ordinary round completed ([`OrdinaryCompletion`]),
//! the per-player indexing every array on [`super::MatchState`] uses, and
//! the records of what the last resolution removed ([`RoundRemovals`]) and
//! what each player's last Round 0 selection did ([`OpeningAttempt`]).
//!
//! Nothing here depends on [`super::MatchState`] or on the outcome
//! vocabulary in [`super::outcomes`] - these are the values the rest of the
//! module is built from. See [`super`]'s module docs for the design they
//! serve.

use crate::match_settings::{MatchSettingField, MatchSettings};
use crate::rules::{Player, Point, RemovedStone};

/// Starting cursors, indexed by [`player_index`]: each player begins on their
/// own side of the board rather than on top of the other.
pub const PLAYER_STARTS: [Point; 2] = [Point::new(7, 4), Point::new(1, 4)];

/// Index of a player in the per-player arrays on [`super::MatchState`].
pub const fn player_index(player: Player) -> usize {
    match player {
        Player::One => 0,
        Player::Two => 1,
    }
}

/// The stones the most recent resolution took off the board.
///
/// This is the record of a capture outside the renderer. A capture is public
/// information - it resolves only after both players have committed, so
/// announcing it reveals nothing - and so [`crate::view::PublicMatchView`]
/// publishes it as it stands here: the live board draws from it, and a
/// screen reader can read what a round took without re-deriving it from
/// two boards.
///
/// `round` names the resolution: `0` for the Round 0 reveal sweep, and `N`
/// for ordinary round `N`. Every resolution writes a fresh record, so an
/// empty `stones` after a round means that round removed nothing, and the
/// record stays what the last round did until the next one replaces it. It
/// lives on [`super::MatchProgress`] so that a completed end hold rolls it
/// back with the round that produced it.
///
/// `Serialize`-only, like [`MatchPhase`]: a view embeds it, and nothing
/// reconstructs one from untrusted input.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct RoundRemovals {
    /// The round that removed the stones: `0` for the reveal sweep.
    pub round: usize,
    /// Every stone removed, with its owner, sorted by cell index; see
    /// [`crate::rules::RoundResolution::removed`] for exactly what is
    /// listed.
    pub stones: Vec<RemovedStone>,
}

/// What one player's most recent Round 0 selection did to their window -
/// never which point it named, nor which held point a duplicate matched.
///
/// This is the record of a Round 0 selection outside the renderer, kept
/// for the same reason as [`RoundRemovals`]: the live dot rows draw from
/// it, and a screen reader can say "that selection changed nothing"
/// without re-deriving it. Unlike a capture, the opening is private, so
/// what this may carry is argued rather than assumed - see
/// `docs/SECURITY.md`'s "Match state privacy" section. In short, every
/// field is something the shared screen already shows with motion allowed:
/// the dot count shows an [`OpeningAttemptOutcome::Added`], the clack and
/// the refusal mark show a [`OpeningAttemptOutcome::Duplicate`], and the
/// dot row's roll-forward shows an [`OpeningAttemptOutcome::RolledForward`].
///
/// `number` is what tells two identical outcomes apart: a second duplicate
/// in a row leaves the window, the count and the outcome exactly as the
/// first did, and a reader that announces each one needs to see that it
/// happened. It counts this player's selections that reached their window
/// in this Round 0, from 1, and wraps rather than overflowing.
///
/// `Serialize`-only, like [`RoundRemovals`]: a view embeds it, and nothing
/// reconstructs one from untrusted input.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OpeningAttempt {
    /// This player's selections in this Round 0 so far, this one included.
    pub number: u32,
    pub outcome: OpeningAttemptOutcome,
}

/// The three things a Round 0 selection can do, named for their effect on
/// the window; see `docs/GAME_RULES.md` for the rules themselves.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningAttemptOutcome {
    /// A distinct point joined a window holding fewer than three.
    Added,
    /// A distinct point joined a full window, and the oldest point left it.
    RolledForward,
    /// The point was already held, so nothing changed: the clack.
    Duplicate,
}

/// Where a match is in its life cycle.
///
/// Round 0 and its closing countdown are both interactive; ordinary move rules
/// begin only after the atomic reveal enters `Playing`.
///
/// `Serialize`-only (no `Deserialize`): [`crate::view`]'s live views embed it
/// for an authorized viewer to read, but a phase is never something an
/// untrusted client submits or a Replay reconstructs through this type - see
/// [`crate::replay::ReplayPhase`] for the persisted, independently-versioned
/// equivalent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum MatchPhase {
    /// Interactive Round 0, with no countdown running yet.
    Opening,
    /// The closing 3-2-1 countdown, with Round 0 still interactive.
    OpeningCountdown,
    /// Ordinary rounds, from the atomic reveal until
    /// [`super::MatchState::finish`].
    Playing,
    /// Over: territory filled and the score final.
    Ended,
}

/// Immutable rules copied from persisted settings when a fresh Match starts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MatchRules {
    settings: MatchSettings,
}

impl MatchRules {
    /// Copies the preset a Match is starting under. The copy never changes
    /// again: a preset is a proposal, while this snapshot is the rules the
    /// Match was actually played under. See `docs/SECURITY.md`.
    pub fn snapshot(settings: &MatchSettings) -> Self {
        Self {
            settings: settings.clone(),
        }
    }

    /// The whole snapshot, for a caller that needs more than the accessors
    /// below - the Replay recorder persisting the rules the Match was played
    /// under, for one.
    pub const fn settings(&self) -> &MatchSettings {
        &self.settings
    }

    /// Whether `field`'s rule is in force for this Match.
    pub const fn enabled(&self, field: MatchSettingField) -> bool {
        self.settings.enabled(field)
    }

    /// `field`'s configured value - whole seconds for Round Time and Match
    /// Time, a round count or a point total for the rest - readable whether
    /// or not the rule is [`Self::enabled`].
    pub const fn value(&self, field: MatchSettingField) -> u16 {
        self.settings.value(field)
    }

    /// Whether captured stones count toward the score
    /// ([`super::MatchState::current_score`]).
    pub const fn capture_points(&self) -> bool {
        self.settings.capture_points()
    }
}

/// Why an ordinary round is completing, and whether that was a timeout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrdinaryCompletion {
    /// Both hidden selections were filled.
    Normal,
    /// Round Time ran out before the caller resolved the round;
    /// [`super::RoundOutcome::submitted`] reports who had locked in.
    RoundTimeout,
    /// Match Time ran out, so [`super::RoundOutcome::end_reasons`] always
    /// carries [`crate::replay::EndReason::MatchTimeLimit`].
    MatchTimeTimeout,
}

impl OrdinaryCompletion {
    /// The persisted [`crate::replay::TimeoutCause`] a Replay reports for
    /// this completion, or `None` when the round completed normally - the
    /// mapping the client uses when it records a `RoundTimedOut` event, and
    /// the one a headless host needs to compare a live completion with what
    /// a loaded Replay says about the same round.
    pub const fn timeout_cause(self) -> Option<crate::replay::TimeoutCause> {
        match self {
            Self::Normal => None,
            Self::RoundTimeout => Some(crate::replay::TimeoutCause::RoundTime),
            Self::MatchTimeTimeout => Some(crate::replay::TimeoutCause::MatchTime),
        }
    }
}
