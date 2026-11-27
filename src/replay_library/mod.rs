//! The client's on-disk Replay collection: [`ReplayLibrary`], the in-memory
//! catalog the UI reads and mutates, the [`ReplayCatalogEntry`] projection it
//! holds instead of complete Replays, and the search/sort helpers
//! ([`SortField`], [`SortOrder`], [`filter_and_sort_replays`]) the Replay
//! Library screen presents that collection with.
//!
//! This is the client half of what used to live inside [`crate::replay`],
//! and the reason it is a module of its own: [`crate::replay`] is wholly
//! headless core, and everything here needs the `client` feature - a Bevy
//! `Resource`, the `platform::ReplayStorage` adapter, and the local
//! wall-clock formatting a Replay's metadata is displayed with. It stands to
//! `crate::ui::replay::library` (the screen) the way [`crate::settings`]
//! stands to `crate::ui::settings`: the model the screen renders, owned
//! outside the screen because `gameplay` writes to it (a completed Match's
//! save) and `app` constructs it before any screen exists.
//!
//! Every mutation goes through the platform [`ReplayStorage`] adapter first;
//! [`ReplayLibrary`] then updates its own in-memory copy - wholesale, via
//! [`ReplayStorageUpdate::authoritative_entries`], when the adapter reports
//! one, as the browser adapter does on every mutation since retention and
//! reconciliation happen storage-side, and otherwise by applying the same
//! single insert, replace, or remove itself, which is what the native
//! adapter's `None` means.

mod catalog;

#[cfg(test)]
pub(crate) mod test_support;

use bevy::prelude::Resource;

use crate::platform::{
    ReplayMutation, ReplayStorage, ReplayStorageUpdate, ReplayStorageWarning, replay_storage,
};
use crate::replay::{Replay, ReplayError};

pub(crate) use catalog::ReplayCatalogEntry;

/// The real native backend, under the name the tests outside this module
/// know it by. Only tests construct one directly - production goes through
/// `platform::replay_storage`.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use crate::platform::NativeReplayStorage as ReplayStore;

/// A timestamp in the local time zone, in the form Replay metadata is shown
/// and default labels are built from.
///
/// The platform adapter reports `None` when the instant has no single local
/// representation - natively an out-of-range or ambiguous millisecond value
/// (a DST fold, through `chrono`'s `single()`), in the browser a negative or
/// non-finite one; "Unknown" is what a player then sees in place of a date,
/// rather than a missing line or a raw epoch value.
pub(crate) fn format_local_timestamp(timestamp_ms: i64) -> String {
    crate::platform::format_local_timestamp(timestamp_ms).unwrap_or_else(|| "Unknown".into())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SortField {
    Name,
    #[default]
    CreatedDate,
}

impl SortField {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::CreatedDate => "Created Date",
        }
    }

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Name => Self::CreatedDate,
            Self::CreatedDate => Self::Name,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SortOrder {
    Ascending,
    #[default]
    Descending,
}

impl SortOrder {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Ascending => "Ascending",
            Self::Descending => "Descending",
        }
    }

    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

pub(crate) fn filter_and_sort_replays<'a>(
    replays: &'a [ReplayCatalogEntry],
    search: &str,
    field: SortField,
    order: SortOrder,
) -> Vec<&'a ReplayCatalogEntry> {
    let needle = search.to_lowercase();
    let mut matches: Vec<_> = replays
        .iter()
        .filter(|replay| replay.label.to_lowercase().contains(&needle))
        .collect();
    matches.sort_by(|left, right| {
        let ascending = match field {
            SortField::Name => left
                .label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.completed_at_ms.cmp(&right.completed_at_ms))
                .then_with(|| left.id.cmp(&right.id)),
            SortField::CreatedDate => left
                .completed_at_ms
                .cmp(&right.completed_at_ms)
                .then_with(|| left.id.cmp(&right.id)),
        };
        match order {
            SortOrder::Ascending => ascending,
            SortOrder::Descending => ascending.reverse(),
        }
    });
    matches
}

/// In-memory Replay catalog and isolated load warnings used by the UI.
///
/// Holds only lightweight [`ReplayCatalogEntry`] values, never a complete
/// `Replay`'s event timeline - see [`Self::load_full`] for loading one
/// Replay's complete, validated content on demand, and [`catalog`]
/// for why the catalog entry omits `Replay::events`.
#[derive(Resource)]
pub(crate) struct ReplayLibrary {
    storage: Box<dyn ReplayStorage>,
    pub(crate) entries: Vec<ReplayCatalogEntry>,
    /// Everything that could not be read: what the storage adapter reported
    /// from `load_catalog` and from each mutation, a reconcile that failed at
    /// load, and a Replay the Library screen could not open on `Play` (it
    /// pushes a `SkippedReplay` here so its count includes that one). Typed
    /// so the Library screen can count the Replays that are actually missing
    /// without rendering any of this text - see [`ReplayStorageWarning`].
    pub(crate) warnings: Vec<ReplayStorageWarning>,
}

impl ReplayLibrary {
    /// Loads the catalog through the platform's storage backend.
    ///
    /// Every warning this collects into [`Self::warnings`] is also the
    /// technical detail someone would want in a bug report, but construction
    /// is not where it can be logged: the only production caller is
    /// [`crate::app::run`], which builds this before `LogPlugin` installs a
    /// `tracing` subscriber, so a `warn!` from here would be discarded. `run`
    /// relays [`Self::warnings`] into
    /// [`crate::app::startup_log::DeferredStartupWarnings`] instead, which
    /// logs them from a `Startup` system. [`Self::apply_storage_update`]
    /// keeps its own `warn!` because its other callers - rename, delete, a
    /// completed Match's save - run mid-session, where logging works.
    pub(crate) fn load_user() -> Self {
        Self::from_boxed_storage(replay_storage())
    }

    #[cfg(test)]
    pub(crate) fn from_store(storage: impl ReplayStorage + 'static) -> Self {
        Self::from_boxed_storage(Box::new(storage))
    }

    fn from_boxed_storage(storage: Box<dyn ReplayStorage>) -> Self {
        let (entries, warnings) = storage.load_catalog();
        let mut library = Self {
            storage,
            entries,
            warnings,
        };
        match library.storage.apply(ReplayMutation::Reconcile) {
            Ok(update) => {
                library.apply_storage_update(update);
            }
            Err(error) => {
                library.warnings.push(ReplayStorageWarning::Storage(format!(
                    "Could not reconcile Replay storage: {error}"
                )));
            }
        }
        library
    }

    /// Returns the underlying [`ReplayError`] rather than a formatted
    /// message: callers - all of them Bevy systems with their own
    /// user-facing text and [`crate::diagnostics::Diagnostics`] access - are
    /// the boundary that turns this into both a log line and a player-safe
    /// message. See `gameplay::save_completed_replay` and
    /// `ui::replay::library`'s rename/delete confirmation.
    pub(crate) fn save_completed(&mut self, replay: Replay) -> Result<(), ReplayError> {
        replay.validate()?;
        if self.entries.iter().any(|existing| existing.id == replay.id) {
            return Ok(());
        }
        let entry = ReplayCatalogEntry::from_replay(&replay);
        let update = self.storage.apply(ReplayMutation::Insert(&replay))?;
        if !self.apply_storage_update(update) {
            self.entries.push(entry);
        }
        Ok(())
    }

    /// Loads the complete Replay, applies the rename, and persists it. This
    /// is one of only two operations (with [`Self::load_full`]) that reads a
    /// complete Replay rather than just its catalog entry - renaming needs
    /// the full record to write back, but never touches any other Replay.
    pub(crate) fn rename(&mut self, id: &str, label: String) -> Result<(), ReplayError> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| ReplayError::Storage("Replay no longer exists.".into()))?;
        let mut renamed = self.storage.load_full(id)?;
        renamed.rename(label)?;
        renamed.validate()?;
        let entry = ReplayCatalogEntry::from_replay(&renamed);
        let update = self.storage.apply(ReplayMutation::Update(&renamed))?;
        if !self.apply_storage_update(update) {
            self.entries[index] = entry;
        }
        Ok(())
    }

    pub(crate) fn delete(&mut self, id: &str) -> Result<(), ReplayError> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| ReplayError::Storage("Replay no longer exists.".into()))?;
        let update = self.storage.apply(ReplayMutation::Delete(id))?;
        if !self.apply_storage_update(update) {
            self.entries.remove(index);
        }
        Ok(())
    }

    pub(crate) fn get(&self, id: &str) -> Option<&ReplayCatalogEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Loads and fully validates one complete Replay by ID. With
    /// [`Self::rename`], which needs the full record to write back, this is
    /// one of only two Library operations that read a complete event timeline
    /// rather than a catalog entry - and the only one that reads it for
    /// display, for the Replay a player actually asked to open in the Viewer.
    pub(crate) fn load_full(&self, id: &str) -> Result<Replay, ReplayError> {
        self.storage.load_full(id)
    }

    pub(crate) fn retention_limit(&self) -> Option<usize> {
        self.storage.retention_limit()
    }

    /// The `warn!` here is for the mid-session callers - rename, delete, a
    /// completed Match's save - where a subscriber exists. Reached during
    /// [`Self::load_user`] it is discarded like any other pre-`LogPlugin`
    /// log call, but the copy it pushes onto [`Self::warnings`] is what
    /// `app::run` relays into the deferred startup log, so a startup-time
    /// reconcile warning still reaches a log exactly once.
    fn apply_storage_update(&mut self, update: ReplayStorageUpdate) -> bool {
        for warning in &update.warnings {
            bevy::log::warn!("{warning}");
        }
        self.warnings.extend(update.warnings);
        if let Some(entries) = update.authoritative_entries {
            self.entries = entries;
            true
        } else {
            false
        }
    }
}

/// The IDs the browser policy evicts to bring `replays` down to `limit`: the
/// oldest by `completed_at_ms`, ties broken by `id`, so two tabs holding the
/// same catalog evict the same records. Labels play no part - a renamed
/// Replay is still evicted by its completion time.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn replay_eviction_ids(replays: &[Replay], limit: usize) -> Vec<String> {
    let overflow = replays.len().saturating_sub(limit);
    if overflow == 0 {
        return Vec::new();
    }
    let mut ordered = replays.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.completed_at_ms
            .cmp(&right.completed_at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    ordered
        .into_iter()
        .take(overflow)
        .map(|replay| replay.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::Ordering as AtomicOrdering;

    use uuid::Uuid;

    use super::test_support::*;
    use super::*;
    use crate::replay::test_support::*;
    use crate::replay::{
        LEGACY_REPLAY_SCHEMA_VERSION, PlaybackController, REPLAY_SCHEMA_VERSION,
        default_replay_label, validate_label,
    };
    use crate::rules::Score;

    /// Projects a mock storage's full `Replay` list into the catalog entries
    /// `ReplayLibrary` would hold, for comparing against `library.entries`.
    fn entries_of(replays: &[Replay]) -> Vec<ReplayCatalogEntry> {
        replays
            .iter()
            .map(ReplayCatalogEntry::from_replay)
            .collect()
    }

    /// `recording`'s own tests check the default label's shape against a
    /// fixed timestamp text; this is the half only the client can check -
    /// that the text this module actually produces is short enough for the
    /// longest possible label to stay inside the 60-character rule.
    #[test]
    fn a_real_local_timestamp_keeps_the_longest_default_label_valid() {
        let started = format_local_timestamp(1_704_067_200_000);
        assert_eq!(started.chars().count(), 16, "{started}");

        let maximum_normal_score = default_replay_label(
            Score {
                black: 81,
                white: 81,
            },
            &started,
        );
        assert!(maximum_normal_score.is_ascii());
        assert!(validate_label(&maximum_normal_score).is_ok());
    }

    #[test]
    fn mock_persistence_renames_every_schema_without_hybrid_fields() {
        for version in LEGACY_REPLAY_SCHEMA_VERSION..=REPLAY_SCHEMA_VERSION {
            let literal = literal_replay_value(version);
            let id = literal["id"].as_str().unwrap().to_string();
            let storage = MockJsonReplayStorage::from_record(literal.clone());
            let persisted = Arc::clone(&storage.records);
            let mut library = ReplayLibrary::from_store(storage.clone());
            assert!(library.warnings.is_empty());

            let label = format!("Renamed Schema {version}");
            library.rename(&id, label.clone()).unwrap();

            let stored = persisted.lock().unwrap()[0].clone();
            assert_storage_schema(&stored, version);
            let mut expected = literal;
            expected["label"] = serde_json::json!(label);
            assert_eq!(stored, expected);

            let reloaded = ReplayLibrary::from_store(storage);
            assert!(reloaded.warnings.is_empty());
            assert_eq!(reloaded.entries.len(), 1);
            assert_eq!(reloaded.entries[0].version, version);
            assert_eq!(reloaded.entries[0].id, id);
            assert_eq!(
                reloaded.entries[0].label,
                format!("Renamed Schema {version}")
            );
        }
    }

    #[test]
    fn persistence_round_trip_rename_delete_and_corrupt_isolation() {
        let root = std::env::temp_dir().join(format!("go-together-replays-{}", Uuid::new_v4()));
        let store = ReplayStore::new(root.clone());
        let replay = sample_replay(1, 2, "Stored Match");
        store.save(&replay).unwrap();
        fs::write(root.join("corrupt.json"), "{ nope").unwrap();

        let (loaded, warnings) = store.load_catalog();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, replay.id);
        assert_eq!(warnings.len(), 1);
        let full = store.load_full(&replay.id).unwrap();
        assert_eq!(full, replay);

        let renamed = store.rename(&full, "Changed Label".into()).unwrap();
        assert_eq!(renamed.id, replay.id);
        assert_eq!(store.load_catalog().0[0].label, "Changed Label");

        store.delete(&replay.id).unwrap();
        assert!(store.load_catalog().0.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn punctuation_label_round_trips_in_json_without_affecting_storage_path() {
        let root = std::env::temp_dir().join(format!("go-together-replays-{}", Uuid::new_v4()));
        let store = ReplayStore::new(root.clone());
        let replay = sample_replay(1, 2, "A \"quote\" \\ slash / :; [] {} | ~");
        let expected_path = root.join(format!("{}.json", replay.id));

        store.save(&replay).unwrap();

        let entries = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![expected_path.clone()]);
        let json = fs::read_to_string(expected_path).unwrap();
        assert!(json.contains("\\\"quote\\\""));
        assert!(json.contains("\\\\ slash /"));
        assert_eq!(store.load_full(&replay.id).unwrap(), replay);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_is_case_insensitive_and_label_only() {
        let first = sample_replay(1, 20, "Alpha Match");
        let mut second = sample_replay(2, 30, "Beta Game");
        second.id = Uuid::new_v4().to_string();
        let first = ReplayCatalogEntry::from_replay(&first);
        let second = ReplayCatalogEntry::from_replay(&second);
        assert_eq!(
            filter_and_sort_replays(
                &[first.clone(), second.clone()],
                "ALPHA",
                SortField::Name,
                SortOrder::Ascending
            )[0]
            .id,
            first.id
        );
        assert!(
            filter_and_sort_replays(
                &[first, second],
                "black",
                SortField::Name,
                SortOrder::Ascending
            )
            .is_empty()
        );
    }

    #[test]
    fn punctuation_labels_preserve_case_insensitive_search_and_name_sorting() {
        let colon = sample_replay(1, 10, "Alpha: Match");
        let mut slash = sample_replay(2, 20, "alpha/Match");
        slash.id = Uuid::new_v4().to_string();
        let mut semicolon = sample_replay(3, 30, "Beta; Match");
        semicolon.id = Uuid::new_v4().to_string();
        let colon = ReplayCatalogEntry::from_replay(&colon);
        let slash = ReplayCatalogEntry::from_replay(&slash);
        let semicolon = ReplayCatalogEntry::from_replay(&semicolon);
        let replays = vec![semicolon.clone(), slash.clone(), colon.clone()];

        assert_eq!(
            filter_and_sort_replays(&replays, ": mAtCh", SortField::Name, SortOrder::Ascending),
            vec![&colon]
        );
        assert_eq!(
            filter_and_sort_replays(&replays, "", SortField::Name, SortOrder::Ascending),
            vec![&slash, &colon, &semicolon]
        );
    }

    #[test]
    fn every_sort_mode_is_deterministic() {
        let alpha_old = sample_replay(1, 10, "alpha");
        let mut alpha_new = sample_replay(2, 30, "ALPHA");
        alpha_new.id = Uuid::new_v4().to_string();
        let mut beta = sample_replay(3, 20, "Beta");
        beta.id = Uuid::new_v4().to_string();
        let alpha_old = ReplayCatalogEntry::from_replay(&alpha_old);
        let alpha_new = ReplayCatalogEntry::from_replay(&alpha_new);
        let beta = ReplayCatalogEntry::from_replay(&beta);
        let replays = vec![alpha_old.clone(), alpha_new.clone(), beta.clone()];

        let ids = |field, order| {
            filter_and_sort_replays(&replays, "", field, order)
                .into_iter()
                .map(|replay| replay.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(SortField::Name, SortOrder::Ascending),
            vec![alpha_old.id.clone(), alpha_new.id.clone(), beta.id.clone()]
        );
        assert_eq!(
            ids(SortField::Name, SortOrder::Descending),
            vec![beta.id.clone(), alpha_new.id.clone(), alpha_old.id.clone()]
        );
        assert_eq!(
            ids(SortField::CreatedDate, SortOrder::Ascending),
            vec![alpha_old.id.clone(), beta.id.clone(), alpha_new.id.clone()]
        );
        assert_eq!(
            ids(SortField::CreatedDate, SortOrder::Descending),
            vec![alpha_new.id, beta.id, alpha_old.id]
        );
    }

    #[test]
    fn sort_order_labels_use_full_words() {
        assert_eq!(SortOrder::Ascending.label(), "Ascending");
        assert_eq!(SortOrder::Descending.label(), "Descending");
    }

    #[test]
    fn sort_defaults_to_newest_created_date_and_cycles_only_two_orders() {
        assert_eq!(SortField::default(), SortField::CreatedDate);
        assert_eq!(SortOrder::default(), SortOrder::Descending);
        assert_eq!(SortOrder::Ascending.next(), SortOrder::Descending);
        assert_eq!(SortOrder::Descending.next(), SortOrder::Ascending);
    }

    #[test]
    fn sort_ties_are_deterministic_and_descending_is_the_reverse_of_ascending() {
        let mut first = sample_replay(1, 10, "same");
        first.id = "00000000-0000-4000-8000-000000000001".into();
        let mut second = sample_replay(2, 10, "SAME");
        second.id = "00000000-0000-4000-8000-000000000002".into();
        let first = ReplayCatalogEntry::from_replay(&first);
        let second = ReplayCatalogEntry::from_replay(&second);
        let replays = vec![second.clone(), first.clone()];

        for field in [SortField::Name, SortField::CreatedDate] {
            assert_eq!(
                filter_and_sort_replays(&replays, "", field, SortOrder::Ascending),
                vec![&first, &second]
            );
            assert_eq!(
                filter_and_sort_replays(&replays, "", field, SortOrder::Descending),
                vec![&second, &first]
            );
        }
    }

    #[test]
    fn completed_replay_saves_once_and_playback_never_writes() {
        let root = std::env::temp_dir().join(format!("go-together-replays-{}", Uuid::new_v4()));
        let store = ReplayStore::new(root.clone());
        let mut library = ReplayLibrary::from_store(store);
        let replay = sample_replay(1, 2, "One Save");

        library.save_completed(replay.clone()).unwrap();
        library.save_completed(replay.clone()).unwrap();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(
            fs::read_dir(&root).unwrap().filter_map(Result::ok).count(),
            1
        );

        let mut playback = PlaybackController::default();
        playback.toggle(&replay);
        playback.advance(1.0, replay.duration_seconds);
        playback.jump_forward(&replay);
        assert_eq!(
            fs::read_dir(&root).unwrap().filter_map(Result::ok).count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// Neither storage adapter validates any more (see `Replay::validate`'s
    /// ownership policy), so these pin the invariant at the layer that does:
    /// an invalid `Replay` never reaches storage through either write path.
    #[test]
    fn the_write_path_rejects_an_invalid_replay_and_persists_nothing() {
        let storage = MockReplayStorage::new(None);
        let persisted = Arc::clone(&storage.replays);
        let mut library = ReplayLibrary::from_store(storage);

        let mut invalid = sample_replay(1, 2, "Forged");
        invalid.duration_seconds = -1.0;
        assert!(matches!(
            library.save_completed(invalid),
            Err(ReplayError::InvalidData(ref message))
                if message == "invalid replay duration"
        ));
        assert!(library.entries.is_empty());
        assert!(persisted.lock().unwrap().is_empty());

        // A backend that hands back something invalid cannot get it written
        // back either, even though only the label was meant to change.
        let storage = MockReplayStorage::new(None);
        let persisted = Arc::clone(&storage.replays);
        let mut smuggled = sample_replay(1, 2, "Smuggled");
        smuggled.duration_seconds = -1.0;
        let id = smuggled.id.clone();
        persisted.lock().unwrap().push(smuggled.clone());
        let mut library = ReplayLibrary::from_store(storage);
        assert_eq!(library.entries, entries_of(&[smuggled.clone()]));
        assert!(matches!(
            library.rename(&id, "Renamed".into()),
            Err(ReplayError::InvalidData(ref message))
                if message == "invalid replay duration"
        ));
        assert_eq!(persisted.lock().unwrap().as_slice(), [smuggled]);
    }

    #[test]
    fn browser_policy_retains_exactly_newest_three_with_deterministic_ties() {
        let storage = MockReplayStorage::new(Some(3));
        let persisted = Arc::clone(&storage.replays);
        let mut library = ReplayLibrary::from_store(storage);
        for sequence in 1..=4 {
            library
                .save_completed(replay_with_id(sequence, sequence as i64 * 10))
                .unwrap();
        }
        let mut ids = library
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(
            ids,
            (2..=4)
                .map(|sequence| format!("00000000-0000-4000-8000-{sequence:012}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(entries_of(&persisted.lock().unwrap()), library.entries);

        let tied = MockReplayStorage::new(Some(3));
        let mut tied_library = ReplayLibrary::from_store(tied);
        for sequence in [4, 2, 3, 1] {
            tied_library
                .save_completed(replay_with_id(sequence, 10))
                .unwrap();
        }
        let mut tied_ids = tied_library
            .entries
            .iter()
            .map(|replay| replay.id.clone())
            .collect::<Vec<_>>();
        tied_ids.sort();
        assert_eq!(
            tied_ids,
            (2..=4)
                .map(|sequence| format!("00000000-0000-4000-8000-{sequence:012}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn browser_retention_ignores_labels_and_repairs_oversized_loaded_catalogs() {
        let storage = MockReplayStorage::new(Some(3));
        {
            let mut persisted = storage.replays.lock().unwrap();
            persisted
                .extend((1..=4).map(|sequence| replay_with_id(sequence, sequence as i64 * 10)));
            persisted[0].label = "Renamed But Still Oldest".into();
        }
        let persisted = Arc::clone(&storage.replays);

        let library = ReplayLibrary::from_store(storage);

        assert_eq!(library.entries.len(), 3);
        assert!(
            library
                .entries
                .iter()
                .all(|entry| entry.label != "Renamed But Still Oldest")
        );
        assert_eq!(entries_of(&persisted.lock().unwrap()), library.entries);
    }

    #[test]
    fn browser_mutations_reconcile_stale_tabs_without_exceeding_three() {
        let storage = MockReplayStorage::new(Some(3));
        let mut first_tab = ReplayLibrary::from_store(storage.clone());
        let mut stale_tab = ReplayLibrary::from_store(storage.clone());
        for sequence in 1..=3 {
            first_tab
                .save_completed(replay_with_id(sequence, sequence as i64 * 10))
                .unwrap();
        }

        stale_tab.save_completed(replay_with_id(4, 40)).unwrap();
        first_tab.save_completed(replay_with_id(5, 50)).unwrap();

        let persisted = storage.replays.lock().unwrap().clone();
        assert_eq!(persisted.len(), 3);
        assert_eq!(first_tab.entries, entries_of(&persisted));
        assert_eq!(
            persisted
                .iter()
                .map(|replay| replay.completed_at_ms)
                .collect::<Vec<_>>(),
            vec![30, 40, 50]
        );
    }

    #[test]
    fn replay_quota_failure_keeps_memory_and_persistence_coherent() {
        let storage = MockReplayStorage::new(Some(3));
        let persisted = Arc::clone(&storage.replays);
        let fail_apply = Arc::clone(&storage.fail_apply);
        let mut library = ReplayLibrary::from_store(storage);
        for sequence in 1..=3 {
            library
                .save_completed(replay_with_id(sequence, sequence as i64 * 10))
                .unwrap();
        }
        let memory_before = library.entries.clone();
        let persisted_before = persisted.lock().unwrap().clone();
        fail_apply.store(true, AtomicOrdering::Relaxed);

        assert_eq!(
            library
                .save_completed(replay_with_id(4, 40))
                .unwrap_err()
                .to_string(),
            "mock quota exceeded"
        );
        assert_eq!(library.entries, memory_before);
        assert_eq!(*persisted.lock().unwrap(), persisted_before);
    }

    #[test]
    fn native_policy_keeps_completed_replays_unlimited_until_manual_delete() {
        let storage = MockReplayStorage::new(None);
        let mut library = ReplayLibrary::from_store(storage);
        for sequence in 1..=5 {
            library
                .save_completed(replay_with_id(sequence, sequence as i64 * 10))
                .unwrap();
        }
        assert_eq!(library.retention_limit(), None);
        assert_eq!(library.entries.len(), 5);
    }
}
