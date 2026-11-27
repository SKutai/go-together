//! A [`Replay`] with fixed metadata, used by both replay screens' tests.
//!
//! This is [`crate::replay::test_support::sample_replay`] - the same
//! [`crate::replay::MatchRecorder`]-built, `validate`-clean fixture the
//! `replay` module's own tests use - with its id and label overridden. The
//! id, label and timestamps are therefore constant, so tests can assert on
//! rendered metadata strings without depending on the clock or on a
//! generated UUID, while the rest of the value stays a Replay that could
//! really exist in storage.
//!
//! Building on the shared fixture rather than a struct literal is what keeps
//! it that way: a schema change that the recorder honors reaches these tests
//! automatically instead of leaving a hand-written literal behind. See
//! `the_screens_fixture_is_a_replay_that_could_exist_in_storage` below, which
//! pins that.

use crate::replay::Replay;

/// The shared recorder-built Replay with a fixed id and label.
pub(super) fn fixed_metadata_replay() -> Replay {
    let mut replay = crate::replay::test_support::sample_replay(
        1_704_067_200_000,
        1_704_067_201_000,
        "Sample Replay",
    );
    replay.id = "00000000-0000-4000-8000-000000000000".into();
    replay
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The screens' fixture must stay a Replay that could exist in storage.
    ///
    /// The predecessor of this fixture was a struct literal with no events,
    /// which `validate` rejects outright, so the replay screens were tested
    /// against a value no `ReplayStore` could ever hand them. Delegating to
    /// the recorder-built fixture fixed that; this test keeps it fixed.
    #[test]
    fn the_screens_fixture_is_a_replay_that_could_exist_in_storage() {
        assert!(fixed_metadata_replay().validate().is_ok());
    }
}
