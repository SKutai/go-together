//! [`ReplayCatalogEntry`]: the lightweight, catalog-only projection of a
//! [`Replay`] the Library uses to list, search, sort, rename, and delete
//! Replays without loading every complete event timeline into memory.
//!
//! A catalog entry carries everything a Replay card renders - label, start
//! and completion time, duration, final score, completed round count, Round 0
//! status, end reasons, and the final board for the card's small preview -
//! but never `Replay::events`, the one field whose size scales with how long
//! a Match ran rather than with how many Replays exist. Building an entry
//! from a full, already-validated `Replay` is a pure, cheap projection;
//! nothing here reads or writes storage. See `crate::platform::native` for
//! the rebuildable on-disk cache built from these entries, and
//! [`super`] for how the Library loads a complete `Replay` only when a
//! player actually opens one.

use serde::{Deserialize, Serialize};

use crate::replay::{EndReason, Replay, ReplayWinner};
use crate::rules::{Board, Score};

/// Everything the Replay Library needs to render, search, and sort one card.
///
/// Field names are a persisted compatibility surface for the native catalog
/// cache (see `crate::platform::native`) - unlike `Replay`'s own schema,
/// though, this cache is a disposable, rebuildable index rather than the
/// source of truth, so an incompatible or corrupt cache is simply rebuilt
/// from the real Replay files rather than needing its own migration path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ReplayCatalogEntry {
    pub(crate) version: u32,
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) started_at_ms: i64,
    pub(crate) completed_at_ms: i64,
    pub(crate) duration_seconds: f64,
    pub(crate) score: Score,
    pub(crate) round_count: usize,
    pub(crate) opening_completed: bool,
    pub(crate) end_reasons: Vec<EndReason>,
    pub(crate) final_board: Board,
}

impl ReplayCatalogEntry {
    /// Projects a complete, already-validated `Replay` into its catalog
    /// entry. `round_count` is `Replay::completed_round_count()`, not the raw
    /// `round_count` field, so a legacy Replay's derived-from-events count is
    /// computed once here rather than on every future catalog read.
    pub(crate) fn from_replay(replay: &Replay) -> Self {
        Self {
            version: replay.version,
            id: replay.id.clone(),
            label: replay.label.clone(),
            started_at_ms: replay.started_at_ms,
            completed_at_ms: replay.completed_at_ms,
            duration_seconds: replay.duration_seconds,
            score: replay.score,
            round_count: replay.completed_round_count(),
            opening_completed: replay.opening_completed(),
            end_reasons: replay.end_reasons.clone(),
            final_board: replay.final_board.clone(),
        }
    }

    pub(crate) fn winner(&self) -> ReplayWinner {
        ReplayWinner::from_score(self.score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::test_support::sample_replay;

    #[test]
    fn entry_matches_the_replay_it_was_built_from() {
        let replay = sample_replay(1_000, 2_000, "Catalog Match");
        let entry = ReplayCatalogEntry::from_replay(&replay);

        assert_eq!(entry.version, replay.version);
        assert_eq!(entry.id, replay.id);
        assert_eq!(entry.label, replay.label);
        assert_eq!(entry.started_at_ms, replay.started_at_ms);
        assert_eq!(entry.completed_at_ms, replay.completed_at_ms);
        assert_eq!(entry.duration_seconds, replay.duration_seconds);
        assert_eq!(entry.score, replay.score);
        assert_eq!(entry.round_count, replay.completed_round_count());
        assert_eq!(entry.opening_completed, replay.opening_completed());
        assert_eq!(entry.end_reasons, replay.end_reasons);
        assert_eq!(entry.final_board, replay.final_board);
        assert_eq!(entry.winner(), replay.winner());
    }
}
