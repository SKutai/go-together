//! Browser storage, JavaScript clock, relative assets, and canvas adapter.
//!
//! Both stores are single same-origin `localStorage` values:
//! `SETTINGS_STORAGE_KEY` holds the raw settings document, and
//! `REPLAY_STORAGE_KEY` one serialized `BrowserReplayCatalog` holding every
//! retained Replay - which is why a mutation re-reads and rewrites the whole
//! catalog. Each key carries its own `.v1` suffix, a separate mechanism from
//! the catalog's `BROWSER_REPLAY_STORAGE_VERSION`: a new key orphans the old
//! value outright, while a version mismatch inside the JSON is rejected by
//! `BrowserReplayCatalog::from_json`.
//!
//! A store that cannot be reached at all - no window, storage disabled - is
//! `SettingsStorageError::Unavailable` or `ReplayError::StorageUnavailable`;
//! one that rejects a write, typically over quota, is
//! `SettingsStorageError::Failed` or `ReplayError::Storage`.

use js_sys::Date;
use wasm_bindgen::JsValue;
use web_sys::Storage;

use super::browser_storage::BrowserReplayCatalog;
use super::{
    BROWSER_REPLAY_LIMIT, ReplayMutation, ReplayStorage, ReplayStorageUpdate, ReplayStorageWarning,
    SettingsStorage, SettingsStorageError, WindowPlatformConfig,
};
use crate::replay::{Replay, ReplayError};
use crate::replay_library::{ReplayCatalogEntry, replay_eviction_ids};
use crate::settings::MAX_SETTINGS_JSON_BYTES;

const SETTINGS_STORAGE_KEY: &str = "io.github.skutai.go-together.settings.v1";
const REPLAY_STORAGE_KEY: &str = "io.github.skutai.go-together.replays.v1";

#[derive(Default)]
struct BrowserSettingsStorage;

impl SettingsStorage for BrowserSettingsStorage {
    fn load(&self) -> Result<Option<String>, SettingsStorageError> {
        local_storage()
            .map_err(SettingsStorageError::Unavailable)?
            .get_item(SETTINGS_STORAGE_KEY)
            .map_err(|error| {
                SettingsStorageError::Failed(browser_error(
                    "Could not read browser settings",
                    error,
                ))
            })
    }

    fn save(&self, json: &str) -> Result<(), SettingsStorageError> {
        if json.len() > MAX_SETTINGS_JSON_BYTES {
            return Err(SettingsStorageError::Failed(format!(
                "Settings exceed the {MAX_SETTINGS_JSON_BYTES}-byte browser limit"
            )));
        }
        local_storage()
            .map_err(SettingsStorageError::Unavailable)?
            .set_item(SETTINGS_STORAGE_KEY, json)
            .map_err(|error| {
                SettingsStorageError::Failed(browser_error(
                    "Could not save browser settings",
                    error,
                ))
            })
    }
}

#[derive(Default)]
struct BrowserReplayStorage;

impl BrowserReplayStorage {
    fn read_catalog(&self) -> Result<(Storage, BrowserReplayCatalog), ReplayError> {
        let storage = local_storage().map_err(ReplayError::StorageUnavailable)?;
        let catalog = storage
            .get_item(REPLAY_STORAGE_KEY)
            .map_err(|error| {
                ReplayError::Storage(browser_error(
                    "Could not read browser Replay storage",
                    error,
                ))
            })?
            .map_or_else(
                || Ok(BrowserReplayCatalog::empty()),
                |json| BrowserReplayCatalog::from_json(&json),
            )?;
        Ok((storage, catalog))
    }

    fn commit(&self, mutation: ReplayMutation<'_>) -> Result<ReplayStorageUpdate, ReplayError> {
        let (storage, mut catalog) = self.read_catalog()?;
        let mut changed = !matches!(mutation, ReplayMutation::Reconcile);
        match mutation {
            ReplayMutation::Reconcile => {}
            ReplayMutation::Insert(replay) => {
                if catalog.contains_record(&replay.id) {
                    return Err(ReplayError::InvalidData(
                        "Replay ID already exists in browser storage".into(),
                    ));
                }
                catalog.apply(Some(replay), &[])?;
            }
            ReplayMutation::Update(replay) => {
                if !catalog.contains_record(&replay.id) {
                    return Err(ReplayError::Storage("Replay no longer exists.".into()));
                }
                catalog.apply(Some(replay), &[])?;
            }
            ReplayMutation::Delete(id) => {
                if !catalog.contains_record(id) {
                    return Err(ReplayError::Storage("Replay no longer exists.".into()));
                }
                catalog.apply(None, &[id.to_string()])?;
            }
        }

        let (mut replays, warnings) = catalog.valid_replays();
        let delete_ids = replay_eviction_ids(&replays, BROWSER_REPLAY_LIMIT);
        if !delete_ids.is_empty() {
            catalog.apply(None, &delete_ids)?;
            replays.retain(|replay| !delete_ids.contains(&replay.id));
            changed = true;
        }
        if changed {
            let json = catalog.to_json()?;
            storage
                .set_item(REPLAY_STORAGE_KEY, &json)
                .map_err(|error| {
                    ReplayError::Storage(browser_error(
                        "Could not save browser Replay storage (storage may be full or disabled)",
                        error,
                    ))
                })?;
        }
        Ok(ReplayStorageUpdate {
            authoritative_entries: Some(
                replays
                    .iter()
                    .map(ReplayCatalogEntry::from_replay)
                    .collect(),
            ),
            warnings,
        })
    }
}

impl ReplayStorage for BrowserReplayStorage {
    fn load_catalog(&self) -> (Vec<ReplayCatalogEntry>, Vec<ReplayStorageWarning>) {
        match self.read_catalog() {
            Ok((_, catalog)) => {
                let (replays, warnings) = catalog.valid_replays();
                (
                    replays
                        .iter()
                        .map(ReplayCatalogEntry::from_replay)
                        .collect(),
                    warnings,
                )
            }
            Err(error) => (
                Vec::new(),
                vec![ReplayStorageWarning::Storage(format!(
                    "Browser Replay storage is unavailable: {error}"
                ))],
            ),
        }
    }

    fn load_full(&self, id: &str) -> Result<Replay, ReplayError> {
        let (_, catalog) = self.read_catalog()?;
        catalog.valid_replay(id)
    }

    fn apply(&self, mutation: ReplayMutation<'_>) -> Result<ReplayStorageUpdate, ReplayError> {
        self.commit(mutation)
    }

    fn retention_limit(&self) -> Option<usize> {
        Some(BROWSER_REPLAY_LIMIT)
    }
}

pub(crate) fn settings_storage() -> Box<dyn SettingsStorage> {
    Box::new(BrowserSettingsStorage)
}

pub(crate) fn replay_storage() -> Box<dyn ReplayStorage> {
    Box::new(BrowserReplayStorage)
}

pub(crate) fn now_millis() -> i64 {
    Date::now() as i64
}

pub(crate) fn format_local_timestamp(timestamp_ms: i64) -> Option<String> {
    if timestamp_ms < 0 {
        return None;
    }
    let date = Date::new(&JsValue::from_f64(timestamp_ms as f64));
    if !date.get_time().is_finite() {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date(),
        date.get_hours(),
        date.get_minutes()
    ))
}

pub(crate) fn settings_storage_note() -> &'static str {
    "Settings are stored in this browser."
}

/// Whether the browser reports `prefers-reduced-motion: reduce` - the same
/// media query `web/styles.css` gates the shell's loading pulse on. A
/// browser with no window or no media-query support answers no, as a
/// stylesheet would. Read by `app::motion` at startup and on entering a
/// Match, not polled, so a change of the operating-system setting applies
/// from the next Match.
pub(crate) fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|window| {
            window
                .match_media("(prefers-reduced-motion: reduce)")
                .ok()
                .flatten()
        })
        .is_some_and(|query| query.matches())
}

pub(crate) fn window_platform_config() -> WindowPlatformConfig {
    WindowPlatformConfig {
        canvas: Some("#go-together".into()),
        fit_canvas_to_parent: false,
        prevent_default_event_handling: true,
        resizable: false,
    }
}

/// The browser build serves assets from a fixed relative path and has no
/// executable to locate, so the warning half of the native signature (see
/// `native::asset_root`) is always `None` here.
pub(crate) fn asset_root() -> (String, Option<String>) {
    ("assets".into(), None)
}

fn local_storage() -> Result<Storage, String> {
    let window = web_sys::window().ok_or_else(|| "Browser window is unavailable".to_string())?;
    window
        .local_storage()
        .map_err(|error| browser_error("Browser storage access failed", error))?
        .ok_or_else(|| "Browser local storage is unavailable".to_string())
}

fn browser_error(context: &str, error: JsValue) -> String {
    match error.as_string() {
        Some(detail) if !detail.is_empty() => format!("{context}: {detail}"),
        _ => context.to_string(),
    }
}
