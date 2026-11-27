//! Mock [`ReplayStorage`] backends shared by the Library's own tests and by
//! the `gameplay` and `ui` tests that need a Library backed by something
//! other than a real filesystem.
//!
//! These live here rather than in [`crate::replay::test_support`] because
//! each one `impl`s [`ReplayStorage`], a trait that exists only with the
//! `client` feature. Keeping them apart from the Replay *fixtures* is what
//! lets those fixtures - and with them every in-module test of `schema`,
//! `compat`, `validation`, `playback`, and `recording` - compile in a
//! headless build. See `docs/ARCHITECTURE.md`'s "Headless core and Bevy
//! client" section.

use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use crate::platform::{ReplayMutation, ReplayStorage, ReplayStorageUpdate, ReplayStorageWarning};
use crate::replay::test_support::sample_replay;
use crate::replay::{Replay, ReplayError};

use super::{ReplayCatalogEntry, replay_eviction_ids};

/// A Replay with a predictable ID and completion time, for the retention and
/// sort-order tests that need several Replays whose relative age is fixed.
pub(crate) fn replay_with_id(sequence: u64, completed_at_ms: i64) -> Replay {
    let mut replay = sample_replay(1, completed_at_ms, &format!("Match {sequence}"));
    replay.id = format!("00000000-0000-4000-8000-{sequence:012}");
    replay
}

/// A `ReplayError::Storage` in the shape a failed native save can actually
/// produce: `atomic_write` passes the `atomicwrites` error through unchanged,
/// and when the write cannot create the scratch directory it needs, the
/// `tempfile` crate underneath appends the path it failed on to the OS error,
/// so a real failure reads like this rather than like a tidy sentence.
pub(crate) const LEAKED_REPLAY_STORAGE_ERROR: &str = "Permission denied (os error 13) at path \"/Users/name/Library/Application Support/Go Together/replays/.atomicwriteA1b2C3\"";

/// Fails `load_full` and every mutation with [`LEAKED_REPLAY_STORAGE_ERROR`];
/// `load_catalog` returns an empty catalog with no warnings, so a Library
/// built on this starts clean and every later failure is that text.
///
/// [`MockReplayStorage`] keeps its terse "mock quota exceeded" for tests that
/// only need *a* failure; this one exists for the tests that pin what the
/// diagnostics boundary shows a player, where a mock whose own text is
/// already clean would let a call site leaking its technical message pass
/// unnoticed.
#[derive(Clone, Copy)]
pub(crate) struct PathLeakingReplayStorage;

impl ReplayStorage for PathLeakingReplayStorage {
    fn load_catalog(&self) -> (Vec<ReplayCatalogEntry>, Vec<ReplayStorageWarning>) {
        (Vec::new(), Vec::new())
    }

    fn load_full(&self, _id: &str) -> Result<Replay, ReplayError> {
        Err(ReplayError::Storage(LEAKED_REPLAY_STORAGE_ERROR.into()))
    }

    fn apply(&self, _mutation: ReplayMutation<'_>) -> Result<ReplayStorageUpdate, ReplayError> {
        Err(ReplayError::Storage(LEAKED_REPLAY_STORAGE_ERROR.into()))
    }

    fn retention_limit(&self) -> Option<usize> {
        None
    }
}

/// Holds `Replay` values, and takes the shape of whichever adapter a test
/// needs: with a retention limit it behaves like the browser one, evicting
/// through [`replay_eviction_ids`] and returning `authoritative_entries` on
/// every mutation; with `None` it behaves like the native one, unlimited and
/// leaving the Library to apply its own single change. `fail_apply` makes
/// every mutation fail with "mock quota exceeded".
#[derive(Clone)]
pub(crate) struct MockReplayStorage {
    pub(crate) replays: Arc<Mutex<Vec<Replay>>>,
    retention_limit: Option<usize>,
    pub(crate) fail_apply: Arc<AtomicBool>,
}

impl MockReplayStorage {
    pub(crate) fn new(retention_limit: Option<usize>) -> Self {
        Self {
            replays: Arc::new(Mutex::new(Vec::new())),
            retention_limit,
            fail_apply: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ReplayStorage for MockReplayStorage {
    fn load_catalog(&self) -> (Vec<ReplayCatalogEntry>, Vec<ReplayStorageWarning>) {
        (
            self.replays
                .lock()
                .unwrap()
                .iter()
                .map(ReplayCatalogEntry::from_replay)
                .collect(),
            Vec::new(),
        )
    }

    fn load_full(&self, id: &str) -> Result<Replay, ReplayError> {
        self.replays
            .lock()
            .unwrap()
            .iter()
            .find(|replay| replay.id == id)
            .cloned()
            .ok_or_else(|| ReplayError::Storage("Replay no longer exists.".into()))
    }

    fn apply(&self, mutation: ReplayMutation<'_>) -> Result<ReplayStorageUpdate, ReplayError> {
        if self.fail_apply.load(AtomicOrdering::Relaxed) {
            return Err(ReplayError::Storage("mock quota exceeded".into()));
        }
        let mut next = self.replays.lock().unwrap().clone();
        match mutation {
            ReplayMutation::Reconcile => {}
            ReplayMutation::Insert(replay) => {
                if next.iter().any(|existing| existing.id == replay.id) {
                    return Err(ReplayError::InvalidData("Replay ID already exists".into()));
                }
                next.push(replay.clone());
            }
            ReplayMutation::Update(replay) => {
                let Some(index) = next.iter().position(|existing| existing.id == replay.id) else {
                    return Err(ReplayError::Storage("Replay no longer exists.".into()));
                };
                next[index] = replay.clone();
            }
            ReplayMutation::Delete(id) => {
                let before = next.len();
                next.retain(|replay| replay.id != id);
                if next.len() == before {
                    return Err(ReplayError::Storage("Replay no longer exists.".into()));
                }
            }
        }
        if let Some(limit) = self.retention_limit {
            let delete_ids = replay_eviction_ids(&next, limit);
            next.retain(|replay| !delete_ids.contains(&replay.id));
        }
        *self.replays.lock().unwrap() = next.clone();
        Ok(ReplayStorageUpdate {
            authoritative_entries: self
                .retention_limit
                .map(|_| next.iter().map(ReplayCatalogEntry::from_replay).collect()),
            warnings: Vec::new(),
        })
    }

    fn retention_limit(&self) -> Option<usize> {
        self.retention_limit
    }
}

/// Keeps raw storage records instead of `Replay` values - `serde_json::Value`
/// as `to_storage_value` writes them, parsed and validated on every read the
/// way a real file is. It exists so a test can assert what was *stored*, such
/// as a rename preserving the record's own schema version, which a mock
/// holding `Replay` values cannot show. Mutations answer like the native
/// adapter: no authoritative entries.
#[derive(Clone)]
pub(crate) struct MockJsonReplayStorage {
    pub(crate) records: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl MockJsonReplayStorage {
    pub(crate) fn from_record(record: serde_json::Value) -> Self {
        Self {
            records: Arc::new(Mutex::new(vec![record])),
        }
    }
}

impl MockJsonReplayStorage {
    fn valid_replays(&self) -> (Vec<Replay>, Vec<ReplayStorageWarning>) {
        let mut replays = Vec::new();
        let mut warnings = Vec::new();
        for value in self.records.lock().unwrap().iter().cloned() {
            match Replay::parse_value(value) {
                Ok(replay) => match replay.validate() {
                    Ok(()) => replays.push(replay),
                    Err(error) => {
                        warnings.push(ReplayStorageWarning::SkippedReplay(error.to_string()));
                    }
                },
                Err(error) => warnings.push(ReplayStorageWarning::SkippedReplay(error.to_string())),
            }
        }
        (replays, warnings)
    }
}

impl ReplayStorage for MockJsonReplayStorage {
    fn load_catalog(&self) -> (Vec<ReplayCatalogEntry>, Vec<ReplayStorageWarning>) {
        let (replays, warnings) = self.valid_replays();
        (
            replays
                .iter()
                .map(ReplayCatalogEntry::from_replay)
                .collect(),
            warnings,
        )
    }

    fn load_full(&self, id: &str) -> Result<Replay, ReplayError> {
        let (replays, _) = self.valid_replays();
        replays
            .into_iter()
            .find(|replay| replay.id == id)
            .ok_or_else(|| ReplayError::Storage("Replay no longer exists.".into()))
    }

    fn apply(&self, mutation: ReplayMutation<'_>) -> Result<ReplayStorageUpdate, ReplayError> {
        let mut records = self.records.lock().unwrap();
        match mutation {
            ReplayMutation::Reconcile => {}
            ReplayMutation::Insert(replay) => records.push(replay.to_storage_value()?),
            ReplayMutation::Update(replay) => {
                let Some(record) = records
                    .iter_mut()
                    .find(|record| record["id"].as_str() == Some(replay.id.as_str()))
                else {
                    return Err(ReplayError::Storage("Replay no longer exists.".into()));
                };
                *record = replay.to_storage_value()?;
            }
            ReplayMutation::Delete(id) => {
                records.retain(|record| record["id"].as_str() != Some(id));
            }
        }
        Ok(ReplayStorageUpdate::default())
    }

    fn retention_limit(&self) -> Option<usize> {
        None
    }
}
