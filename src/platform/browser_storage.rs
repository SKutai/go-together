//! Pure browser Replay catalog format shared by the WASM adapter and native
//! contract tests.
//!
//! The entire catalog is replaced with one `localStorage.setItem`, so quota or
//! access failures leave the previous value intact. Each record is keyed by its
//! immutable canonical UUID, while labels remain ordinary validated data.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ReplayStorageWarning;
use crate::replay::{Replay, ReplayError, canonical_replay_id};

const BROWSER_REPLAY_STORAGE_VERSION: u32 = 1;
const MAX_BROWSER_CATALOG_RECORDS: usize = 64;
pub(super) const MAX_BROWSER_REPLAY_BYTES: usize = 800 * 1024;
pub(super) const MAX_BROWSER_CATALOG_BYTES: usize = 2_500_000;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BrowserReplayCatalog {
    version: u32,
    records: BTreeMap<String, serde_json::Value>,
}

impl BrowserReplayCatalog {
    pub(super) fn empty() -> Self {
        Self {
            version: BROWSER_REPLAY_STORAGE_VERSION,
            records: BTreeMap::new(),
        }
    }

    pub(super) fn from_json(json: &str) -> Result<Self, ReplayError> {
        if json.len() > MAX_BROWSER_CATALOG_BYTES {
            return Err(ReplayError::DataTooLarge {
                item: "browser Replay catalog",
                max_bytes: MAX_BROWSER_CATALOG_BYTES,
            });
        }
        let catalog: Self = serde_json::from_str(json).map_err(ReplayError::Json)?;
        if catalog.version != BROWSER_REPLAY_STORAGE_VERSION {
            return Err(ReplayError::InvalidData(format!(
                "unsupported browser Replay storage version {}",
                catalog.version
            )));
        }
        if catalog.records.len() > MAX_BROWSER_CATALOG_RECORDS {
            return Err(ReplayError::InvalidData(format!(
                "browser Replay catalog has more than {MAX_BROWSER_CATALOG_RECORDS} records"
            )));
        }
        Ok(catalog)
    }

    /// Decodes every record that can be read, and warns about - without
    /// removing - every record that cannot: a non-canonical UUID key, a
    /// record over `MAX_BROWSER_REPLAY_BYTES`, failed deserialization, a key
    /// that disagrees with the decoded Replay's own ID, or failed validation.
    ///
    /// Skipping is all that happens, and that is the policy rather than an
    /// oversight. `replay_eviction_ids` computes the browser adapter's
    /// evictions from the *valid* list this returns, so a skipped record is
    /// never chosen for deletion and holds its space for good. Native behaves
    /// identically: `NativeReplayStorage::load_catalog` skips a file it
    /// cannot read and never deletes it either. Apart from that retention
    /// eviction of readable Replays, the only thing either adapter removes on
    /// its own initiative is native's catalog *cache*, which is explicitly
    /// disposable and rebuildable. Neither ever destroys a Replay the
    /// application cannot read.
    ///
    /// **A skipped record is not necessarily corrupt.** The `ReplayFile`
    /// shadow struct in `src/replay/compat.rs` - the one every Replay is
    /// deserialized through - carries `#[serde(deny_unknown_fields)]`, and
    /// [`Replay::validate`](crate::replay::Replay::validate) accepts only the
    /// schema versions this build knows, so a Replay written by a *future*
    /// version of the application lands in exactly the same skipped bucket a
    /// corrupt one does. In a browser that is the realistic way unreadable
    /// records accumulate at all - not corruption. The release is a plain
    /// static site with no service worker, but ordinary HTTP caching can
    /// still serve a stale bundle after a deploy, and that older copy of the
    /// application then meets records the current copy wrote.
    ///
    /// **The known cost is quota.** Unreadable records still occupy their
    /// bytes and still count toward `MAX_BROWSER_CATALOG_RECORDS`. A large
    /// enough accumulation makes [`Self::to_json`] exceed
    /// `MAX_BROWSER_CATALOG_BYTES` or that record count, and since
    /// [`Self::to_json`] runs before every write, the failure mode is that
    /// saving stops working entirely while the readable Replays keep
    /// listing. That is the accepted worst case. It is also self-limiting in
    /// one direction: because [`Self::to_json`] enforces both caps, this
    /// build can never grow the stored catalog past them itself - whatever
    /// it cannot read was put there by something else, a newer version of
    /// the application or any other script on the origin.
    ///
    /// **Automatic eviction was considered and rejected.** Deleting skipped
    /// records would reclaim that space, but because the future-version case
    /// and the corrupt case are indistinguishable here, it would mean an old
    /// cached tab silently destroying the Replays the current version just
    /// wrote - trading a rare, bounded quota problem for permanent data loss
    /// in the more likely case. If reclaiming the space is ever wanted, the
    /// shape that does not break the policy is a player-initiated "clear
    /// unreadable Replays" action, where the deletion is someone's explicit
    /// choice rather than the adapter's.
    pub(super) fn valid_replays(&self) -> (Vec<Replay>, Vec<ReplayStorageWarning>) {
        let mut replays = Vec::new();
        let mut warnings = Vec::new();
        for (record_id, value) in &self.records {
            if canonical_replay_id(record_id).is_err() {
                warnings.push(ReplayStorageWarning::SkippedReplay(
                    "Skipped a browser Replay record with an invalid ID.".into(),
                ));
                continue;
            }
            let encoded = match serde_json::to_vec(value) {
                Ok(encoded) => encoded,
                Err(error) => {
                    warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                        "Skipped corrupt browser Replay {record_id}: {error}"
                    )));
                    continue;
                }
            };
            if encoded.len() > MAX_BROWSER_REPLAY_BYTES {
                warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                    "Skipped corrupt browser Replay {record_id}: record exceeds the storage limit"
                )));
                continue;
            }
            let replay = match Replay::parse_value(value.clone()) {
                Ok(replay) => replay,
                Err(error) => {
                    warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                        "Skipped corrupt browser Replay {record_id}: {error}"
                    )));
                    continue;
                }
            };
            if replay.id != *record_id {
                warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                    "Skipped corrupt browser Replay {record_id}: record key does not match Replay ID"
                )));
                continue;
            }
            match replay.validate() {
                Ok(()) => replays.push(replay),
                Err(error) => warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                    "Skipped corrupt browser Replay {record_id}: {error}"
                ))),
            }
        }

        (replays, warnings)
    }

    pub(super) fn contains_record(&self, id: &str) -> bool {
        self.records.contains_key(id)
    }

    /// Loads and fully validates exactly one record, for the Replay Viewer
    /// opening a single Replay - the same validation `valid_replays` applies
    /// to every record, just without decoding the rest of the catalog.
    pub(super) fn valid_replay(&self, id: &str) -> Result<Replay, ReplayError> {
        canonical_replay_id(id)?;
        let value = self
            .records
            .get(id)
            .ok_or_else(|| ReplayError::Storage("Replay no longer exists.".into()))?;
        let encoded = serde_json::to_vec(value).map_err(ReplayError::Json)?;
        if encoded.len() > MAX_BROWSER_REPLAY_BYTES {
            return Err(ReplayError::DataTooLarge {
                item: "browser Replay",
                max_bytes: MAX_BROWSER_REPLAY_BYTES,
            });
        }
        let replay = Replay::parse_value(value.clone()).map_err(ReplayError::Json)?;
        if replay.id != id {
            return Err(ReplayError::InvalidData(
                "record key does not match Replay ID".into(),
            ));
        }
        replay.validate()?;
        Ok(replay)
    }

    pub(super) fn apply(
        &mut self,
        upsert: Option<&Replay>,
        delete_ids: &[String],
    ) -> Result<(), ReplayError> {
        if let Some(replay) = upsert {
            let value = replay.to_storage_value()?;
            let encoded = serde_json::to_vec(&value).map_err(ReplayError::Json)?;
            if encoded.len() > MAX_BROWSER_REPLAY_BYTES {
                return Err(ReplayError::DataTooLarge {
                    item: "browser Replay",
                    max_bytes: MAX_BROWSER_REPLAY_BYTES,
                });
            }
            self.records.insert(replay.id.clone(), value);
        }
        for id in delete_ids {
            canonical_replay_id(id)?;
            self.records.remove(id);
        }
        Ok(())
    }

    pub(super) fn to_json(&self) -> Result<String, ReplayError> {
        if self.records.len() > MAX_BROWSER_CATALOG_RECORDS {
            return Err(ReplayError::InvalidData(format!(
                "browser Replay catalog has more than {MAX_BROWSER_CATALOG_RECORDS} records"
            )));
        }
        let json = serde_json::to_string(self).map_err(ReplayError::Json)?;
        if json.len() > MAX_BROWSER_CATALOG_BYTES {
            return Err(ReplayError::DataTooLarge {
                item: "browser Replay catalog",
                max_bytes: MAX_BROWSER_CATALOG_BYTES,
            });
        }
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Shared `replay` fixtures rather than local copies - see `native`'s
    // equivalent test module.
    use crate::replay::test_support::{assert_storage_schema, literal_replay_value};
    use crate::replay::{REPLAY_SCHEMA_VERSION, ReplayEvent, ReplayEventKind, ReplayFrame};
    use crate::rules::{Board, Score};

    fn replay(id: &str, completed_at_ms: i64) -> Replay {
        let frame = ReplayFrame {
            current_round: 1,
            ..Default::default()
        };
        let final_frame = ReplayFrame {
            phase: crate::replay::ReplayPhase::Ended,
            ..frame.clone()
        };
        Replay {
            version: REPLAY_SCHEMA_VERSION,
            id: id.into(),
            label: "Browser Match".into(),
            started_at_ms: 1,
            completed_at_ms,
            duration_seconds: 1.0,
            score: Score::default(),
            round_count: 0,
            opening_completed: false,
            match_settings: crate::match_settings::MatchSettings::default(),
            end_reasons: vec![crate::replay::EndReason::Manual],
            final_board: Board::default(),
            events: vec![
                ReplayEvent {
                    at_seconds: 0.0,
                    kind: ReplayEventKind::MatchStart,
                    frame: frame.clone(),
                },
                ReplayEvent {
                    at_seconds: 1.0,
                    kind: ReplayEventKind::MatchEnd,
                    frame: final_frame,
                },
            ],
        }
    }

    #[test]
    fn browser_catalog_round_trips_uuid_keyed_records_and_label_updates() {
        let id = "00000000-0000-4000-8000-000000000001";
        let mut catalog = BrowserReplayCatalog::empty();
        let mut stored = replay(id, 2);
        catalog.apply(Some(&stored), &[]).unwrap();
        stored.label = "Changed Label".into();
        catalog.apply(Some(&stored), &[]).unwrap();

        let encoded = catalog.to_json().unwrap();
        let decoded = BrowserReplayCatalog::from_json(&encoded).unwrap();
        let (replays, warnings) = decoded.valid_replays();

        assert!(warnings.is_empty());
        assert_eq!(replays, vec![stored.clone()]);
        assert_eq!(decoded.records.len(), 1);
        assert!(decoded.contains_record(id));
        assert_eq!(decoded.valid_replay(id).unwrap(), stored);
    }

    #[test]
    fn valid_replay_loads_exactly_one_record_without_decoding_the_rest() {
        let valid = replay("00000000-0000-4000-8000-000000000001", 2);
        let mut catalog = BrowserReplayCatalog::empty();
        catalog.apply(Some(&valid), &[]).unwrap();
        catalog.records.insert(
            "00000000-0000-4000-8000-000000000002".into(),
            serde_json::json!({"version": 3, "id": "wrong"}),
        );

        assert_eq!(catalog.valid_replay(&valid.id).unwrap(), valid);
        assert!(matches!(
            catalog.valid_replay("00000000-0000-4000-8000-000000000002"),
            Err(ReplayError::Json(_))
        ));
        assert!(matches!(
            catalog.valid_replay("00000000-0000-4000-8000-000000000099"),
            Err(ReplayError::Storage(_))
        ));
        assert!(matches!(
            catalog.valid_replay("../outside"),
            Err(ReplayError::InvalidId)
        ));
    }

    #[test]
    fn browser_updates_preserve_each_replay_schema() {
        for version in 1..=REPLAY_SCHEMA_VERSION {
            let id = format!("00000000-0000-4000-8000-{version:012}");
            // See `native`'s copy of this test for why the id is patched
            // into the JSON rather than built into a fixture.
            let mut literal = literal_replay_value(version);
            literal["id"] = serde_json::json!(id);
            let mut catalog = BrowserReplayCatalog::empty();
            catalog.records.insert(id.clone(), literal.clone());

            let (mut loaded, warnings) = catalog.valid_replays();
            assert!(warnings.is_empty());
            assert_eq!(loaded.len(), 1);
            let label = format!("Renamed Schema {version}");
            loaded[0].rename(label.clone()).unwrap();
            catalog.apply(Some(&loaded[0]), &[]).unwrap();

            let stored = catalog.records[&id].clone();
            assert_storage_schema(&stored, version);
            let mut expected = literal;
            expected["label"] = serde_json::json!(label);
            assert_eq!(stored, expected);

            let decoded = BrowserReplayCatalog::from_json(&catalog.to_json().unwrap()).unwrap();
            let (reloaded, warnings) = decoded.valid_replays();
            assert!(warnings.is_empty());
            assert_eq!(reloaded.len(), 1);
            assert_eq!(reloaded[0].version, version);
            assert_eq!(reloaded[0].id, id);
            assert_eq!(reloaded[0].label, format!("Renamed Schema {version}"));
        }
    }

    #[test]
    fn corrupt_browser_records_are_isolated_from_valid_replays() {
        let valid = replay("00000000-0000-4000-8000-000000000001", 2);
        let mut catalog = BrowserReplayCatalog::empty();
        catalog
            .records
            .insert(valid.id.clone(), valid.to_in_memory_value().unwrap());
        catalog.records.insert(
            "00000000-0000-4000-8000-000000000002".into(),
            serde_json::json!({"version": 3, "id": "wrong"}),
        );
        catalog
            .records
            .insert("../outside".into(), serde_json::json!({"label": "bad"}));

        let (replays, warnings) = catalog.valid_replays();

        assert_eq!(replays, vec![valid]);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn browser_catalog_bounds_and_uuid_deletes_protect_the_namespace() {
        let oversized = " ".repeat(MAX_BROWSER_CATALOG_BYTES + 1);
        assert!(matches!(
            BrowserReplayCatalog::from_json(&oversized),
            Err(ReplayError::DataTooLarge { .. })
        ));

        let valid = replay("00000000-0000-4000-8000-000000000001", 2);
        let mut catalog = BrowserReplayCatalog::empty();
        catalog.apply(Some(&valid), &[]).unwrap();
        assert!(matches!(
            catalog.apply(None, &["../outside".into()]),
            Err(ReplayError::InvalidId)
        ));
        assert!(catalog.records.contains_key(&valid.id));

        for sequence in 2..=MAX_BROWSER_CATALOG_RECORDS + 1 {
            catalog.records.insert(
                format!("00000000-0000-4000-8000-{sequence:012}"),
                serde_json::json!({}),
            );
        }
        assert!(matches!(
            catalog.to_json(),
            Err(ReplayError::InvalidData(ref error))
                if error.contains("more than 64 records")
        ));
    }

    #[test]
    fn hundreds_of_full_frame_events_fit_the_browser_replay_budget() {
        let mut replay = replay("00000000-0000-4000-8000-000000000001", 2);
        let repeated = ReplayEvent {
            at_seconds: 0.5,
            kind: ReplayEventKind::CursorMoved {
                player: crate::rules::Player::One,
                from: crate::rules::Point::new(7, 4),
                to: crate::rules::Point::new(6, 4),
            },
            frame: ReplayFrame {
                current_round: 1,
                round_elapsed_seconds: 0.5,
                ..Default::default()
            },
        };
        for _ in 0..750 {
            replay
                .events
                .insert(replay.events.len() - 1, repeated.clone());
        }
        replay.validate().unwrap();

        // Measured on the uncompacted in-memory shape rather than the
        // compacted record `apply` actually writes: these 750 events all
        // repeat one board, so the record would be a fraction of the size
        // and would stop bounding the full-frame case this test is named
        // for. Fitting here means fitting once written.
        let encoded = serde_json::to_vec(&replay.to_in_memory_value().unwrap()).unwrap();
        assert!(
            encoded.len() < MAX_BROWSER_REPLAY_BYTES,
            "{} bytes exceeded the browser Replay budget",
            encoded.len()
        );
    }
}
