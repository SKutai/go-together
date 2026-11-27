//! Shared fixtures used across `authority`'s own submodule test suites.
//!
//! Every submodule's `#[cfg(test)] mod tests` imports from here with
//! `use crate::authority::test_support::*;` instead of redefining fixtures,
//! mirroring the same pattern in `crate::rules::test_support` and
//! `crate::replay::test_support`. Fixtures only one submodule uses stay in
//! that submodule's own test module.

use super::state::{MatchProgress, MatchState};
use super::values::MatchPhase;

/// Constructs an active `MatchState` directly, with no Round 0 timeline
/// and no Bevy `App` - proving the authoritative state is a plain,
/// independently testable value.
pub(crate) fn active_state() -> MatchState {
    MatchState {
        phase: MatchPhase::Playing,
        progress: MatchProgress {
            round: 1,
            ..MatchProgress::default()
        },
        ..MatchState::default()
    }
}
