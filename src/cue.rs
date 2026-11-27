//! [`GameCue`]: the persisted semantic-sound vocabulary shared by the
//! headless [`crate::replay`] schema and the Bevy client's `audio` playback
//! (behind the `client` feature). Kept in its own module, independent of
//! both, so a Replay's `MatchSound` event can be represented without pulling
//! audio playback (or Bevy) into the headless core.
//!
//! This module is part of the crate's headless public API because a Replay
//! exposes cues: [`crate::replay::ReplayEventKind::MatchSound`] carries one,
//! and [`crate::replay::PlaybackController::advance_with_cues`] returns the
//! cues playback just crossed. A headless consumer receives cues and decides
//! what, if anything, to do with them; producing one from a live round's
//! [`RoundSound`] stays crate-internal (`pub(crate)`): only the recording
//! layer (behind `client`) creates `MatchSound` events, and the one caller
//! outside it - `replay`'s best-effort cue derivation for Replays below
//! [`crate::replay::EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION`] - is headless
//! and inside this crate. Neither constructor is gated behind `client`, and
//! neither may be.

use serde::{Deserialize, Serialize};

use crate::rules::{Player, RoundSound};

/// Persisted semantic sounds produced by live Matches and Replay playback.
///
/// The `snake_case` variant names are part of the Replay on-disk format from
/// schema v3 on (see `crate::replay`), so a variant is never renamed or
/// reordered. New sounds have been added between schema versions before
/// (`OpeningDuplicate` arrived with v4), so the enum is `#[non_exhaustive]`:
/// a consumer matching on cues needs a wildcard arm, and a future sound is
/// then simply one it does not react to rather than a compile error.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GameCue {
    Countdown3,
    Countdown2,
    Countdown1,
    Go,
    BlackCapture,
    WhiteCapture,
    BothCapture,
    GrayCapture,
    SamePointGray,
    OpeningDuplicate,
    BlackTeleport,
    WhiteTeleport,
    MatchEnd,
    MatchRestart,
    Preview,
}

impl GameCue {
    pub(crate) const fn from_round_sound(sound: RoundSound) -> Option<Self> {
        if sound.gray_capture {
            return Some(Self::GrayCapture);
        }
        if sound.same_point_gray {
            return Some(Self::SamePointGray);
        }
        match (sound.black_capture, sound.white_capture) {
            (true, true) => Some(Self::BothCapture),
            (true, false) => Some(Self::BlackCapture),
            (false, true) => Some(Self::WhiteCapture),
            (false, false) => None,
        }
    }

    pub(crate) const fn teleport(player: Player) -> Self {
        match player {
            Player::One => Self::BlackTeleport,
            Player::Two => Self::WhiteTeleport,
        }
    }
}
