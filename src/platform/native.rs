//! Native filesystem, clock, executable-asset, and window adapters.
//!
//! Everything persistent lives under one application-data root, resolved by
//! [`app_data_dir`]: `controls.json` for settings, and a `replays` directory
//! holding one `<uuid>.json` file per completed Replay plus the disposable
//! `catalog.json` cache. A root that cannot be resolved is not a failure to
//! construct: the reason is kept as [`NativePath::Unavailable`] and returned
//! per operation as `SettingsStorageError::Unavailable` or
//! `ReplayError::StorageUnavailable`, so a missing home directory degrades to
//! defaults and an empty Library rather than a startup crash.
//!
//! Two invariants run through the module. Every path is inspected with
//! `symlink_metadata` before it is read or listed, and before an existing
//! Replay is rewritten; a symlink or other non-regular file is refused, so a
//! link planted in the data directory cannot make the game read or replace a
//! file elsewhere. And every write goes through [`atomic_write`], so a crash
//! mid-save leaves the previous file rather than a truncated one.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};

use super::{
    ReplayMutation, ReplayStorage, ReplayStorageUpdate, ReplayStorageWarning, SettingsStorage,
    SettingsStorageError, WindowPlatformConfig,
};
use crate::replay::{MAX_REPLAY_JSON_BYTES, Replay, ReplayError, canonical_replay_id};
use crate::replay_library::ReplayCatalogEntry;
use crate::settings::MAX_SETTINGS_JSON_BYTES;

/// The settings file name, alongside the Replay names below because this
/// module owns the directory all three sit in. It is a filesystem detail, not
/// domain policy, so `settings` does not know it; the byte cap that module
/// does own travels the other way, as `MAX_SETTINGS_JSON_BYTES` above. No
/// such file exists in a `wasm32` build, where settings live in browser
/// storage instead - which is why this module is compiled out there.
pub(crate) const CONFIG_FILE_NAME: &str = "controls.json";
const REPLAY_DIRECTORY_NAME: &str = "replays";
/// A disposable, rebuildable cache alongside the per-Replay files - never the
/// source of truth. See [`NativeReplayStorage::load_catalog`].
const CATALOG_CACHE_FILE_NAME: &str = "catalog.json";
/// Bump when a cached [`ReplayCatalogEntry`] would still deserialize but mean
/// something different - a field keeping its serde name and type while its
/// definition changes (what `round_count` counts, say). A renamed or newly
/// required field already fails to parse and rebuilds the cache by itself; a
/// purely semantic change does not, because a matching (length, modification
/// time) fingerprint trusts the cached entry without reading the Replay file.
/// Any other value is treated exactly like no cache at all.
const CATALOG_CACHE_VERSION: u32 = 1;
/// Generous upper bound on the cache file itself; it holds only metadata, so
/// even thousands of Replays stay far under this.
const MAX_CATALOG_CACHE_BYTES: usize = 8 * 1024 * 1024;

/// One catalog entry plus the source file fingerprint it was built from, so a
/// file modified, replaced, or removed outside the application is detected by
/// a cheap `stat` rather than trusted as still-valid cached metadata.
#[derive(Clone, Serialize, Deserialize)]
struct CatalogCacheEntry {
    entry: ReplayCatalogEntry,
    file_len: u64,
    file_modified_ms: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct CatalogCacheFile {
    version: u32,
    entries: Vec<CatalogCacheEntry>,
}

fn file_fingerprint(metadata: &fs::Metadata) -> Option<(u64, i64)> {
    let modified_ms = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()?;
    Some((metadata.len(), modified_ms))
}

#[derive(Clone, Debug)]
enum NativePath {
    Available(PathBuf),
    Unavailable(String),
}

impl NativePath {
    fn get(&self) -> Result<&Path, String> {
        match self {
            Self::Available(path) => Ok(path),
            Self::Unavailable(error) => Err(error.clone()),
        }
    }
}

/// Atomic native settings file adapter.
#[derive(Clone, Debug)]
pub(crate) struct NativeSettingsStorage {
    path: NativePath,
}

impl NativeSettingsStorage {
    pub(crate) fn user() -> Self {
        match app_data_dir() {
            Ok(root) => Self::new(root.join(CONFIG_FILE_NAME)),
            Err(error) => Self {
                path: NativePath::Unavailable(error),
            },
        }
    }

    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path: NativePath::Available(path),
        }
    }
}

impl SettingsStorage for NativeSettingsStorage {
    fn load(&self) -> Result<Option<String>, SettingsStorageError> {
        let path = self.path.get().map_err(SettingsStorageError::Unavailable)?;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(SettingsStorageError::Failed(format!(
                    "Could not inspect settings at {}: {error}",
                    path.display()
                )));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SettingsStorageError::Failed(format!(
                "Settings path {} is not a regular file",
                path.display()
            )));
        }
        if metadata.len() > MAX_SETTINGS_JSON_BYTES as u64 {
            return Err(SettingsStorageError::Failed(format!(
                "Settings at {} exceed the {MAX_SETTINGS_JSON_BYTES}-byte limit",
                path.display()
            )));
        }
        fs::read_to_string(path).map(Some).map_err(|error| {
            SettingsStorageError::Failed(format!(
                "Could not read settings from {}: {error}",
                path.display()
            ))
        })
    }

    fn save(&self, json: &str) -> Result<(), SettingsStorageError> {
        if json.len() > MAX_SETTINGS_JSON_BYTES {
            return Err(SettingsStorageError::Failed(format!(
                "Settings exceed the {MAX_SETTINGS_JSON_BYTES}-byte limit"
            )));
        }
        let path = self.path.get().map_err(SettingsStorageError::Unavailable)?;
        let parent = path.parent().ok_or_else(|| {
            SettingsStorageError::Failed(format!("Invalid settings path {}", path.display()))
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            SettingsStorageError::Failed(format!(
                "Could not create settings directory {}: {error}",
                parent.display()
            ))
        })?;
        atomic_write(path, json).map_err(|error| {
            SettingsStorageError::Failed(format!(
                "Could not save settings to {}: {error}",
                path.display()
            ))
        })
    }
}

/// UUID-file Replay adapter used on macOS, Windows, and Linux.
#[derive(Clone, Debug)]
pub(crate) struct NativeReplayStorage {
    root: NativePath,
}

impl NativeReplayStorage {
    pub(crate) fn user() -> Self {
        match app_data_dir() {
            Ok(root) => Self::new(root.join(REPLAY_DIRECTORY_NAME)),
            Err(error) => Self {
                root: NativePath::Unavailable(error),
            },
        }
    }

    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root: NativePath::Available(root),
        }
    }

    /// Writes one Replay file. The Replay is trusted to be valid already -
    /// see [`crate::replay::Replay::validate`] for which layer owns that.
    pub(crate) fn save(&self, replay: &Replay) -> Result<(), ReplayError> {
        let root = self.replay_root()?;
        ensure_replay_directory(root)?;
        let path = self.path_for_id(&replay.id)?;
        let json = replay.to_storage_json()?;
        if json.len() > MAX_REPLAY_JSON_BYTES {
            return Err(ReplayError::DataTooLarge {
                item: "Replay",
                max_bytes: MAX_REPLAY_JSON_BYTES,
            });
        }
        atomic_write(&path, &json).map_err(ReplayError::Storage)
    }

    /// Lists every completed Replay's catalog entry without loading any
    /// complete event timeline, using [`CATALOG_CACHE_FILE_NAME`] as a
    /// disposable cache.
    ///
    /// A cached entry is trusted only while its source file's length and
    /// modification time still match what was cached - so a file changed,
    /// added, or removed outside the application is detected by a cheap
    /// `stat` per file, and only a changed or brand-new file pays the cost of
    /// a full read, parse, and validation. A missing, corrupt, or
    /// incompatible-version cache is treated exactly like an empty cache
    /// (every file is fully loaded once), never as a reason to skip a
    /// Replay - the cache is a performance optimization, never the source of
    /// truth for whether a Replay exists or is valid.
    pub(crate) fn load_catalog(&self) -> (Vec<ReplayCatalogEntry>, Vec<ReplayStorageWarning>) {
        let root = match self.replay_root() {
            Ok(root) => root,
            Err(error) => {
                return (
                    Vec::new(),
                    vec![ReplayStorageWarning::Storage(error.to_string())],
                );
            }
        };
        let dir_entries = match checked_replay_directory(root) {
            Ok(Some(entries)) => entries,
            Ok(None) => return (Vec::new(), Vec::new()),
            Err(error) => {
                return (
                    Vec::new(),
                    vec![ReplayStorageWarning::Storage(error.to_string())],
                );
            }
        };

        let mut cached = self.read_catalog_cache(root);
        let mut warnings = Vec::new();
        // Cacheable entries carry the fingerprint they were verified or built
        // against; entries pushed without one (fingerprint unavailable) are
        // simply left out of the rewritten cache, so they are re-verified
        // next time rather than trusted incorrectly.
        let mut cacheable = Vec::new();
        let mut uncacheable = Vec::new();
        let mut changed = cached.is_none();

        for entry in dir_entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                        "Could not read a Replay directory entry: {error}"
                    )));
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json")
                || path.file_name().and_then(|name| name.to_str()) == Some(CATALOG_CACHE_FILE_NAME)
            {
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                        "Could not inspect Replay {}: {error}",
                        replay_file_name(&path)
                    )));
                    continue;
                }
            };
            if !file_type.is_file() || file_type.is_symlink() {
                warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                    "Skipped non-regular Replay {}",
                    replay_file_name(&path)
                )));
                continue;
            }

            let id = path.file_stem().and_then(|stem| stem.to_str());
            let fingerprint = fs::symlink_metadata(&path)
                .ok()
                .as_ref()
                .and_then(file_fingerprint);
            let cache_hit = id.zip(fingerprint).and_then(|(id, fingerprint)| {
                let cached_entry = cached.as_mut()?.remove(id)?;
                ((cached_entry.file_len, cached_entry.file_modified_ms) == fingerprint)
                    .then_some(cached_entry)
            });

            if let Some(cache_entry) = cache_hit {
                cacheable.push(cache_entry);
                continue;
            }
            match self.load_path(&path) {
                Ok(replay) => {
                    changed = true;
                    let entry = ReplayCatalogEntry::from_replay(&replay);
                    match fingerprint {
                        Some((file_len, file_modified_ms)) => cacheable.push(CatalogCacheEntry {
                            entry,
                            file_len,
                            file_modified_ms,
                        }),
                        // No usable fingerprint (e.g. the platform clock is
                        // unavailable) - return the entry but do not cache it.
                        None => uncacheable.push(entry),
                    }
                }
                Err(error) => warnings.push(ReplayStorageWarning::SkippedReplay(format!(
                    "Skipped corrupt Replay {}: {error}",
                    replay_file_name(&path)
                ))),
            }
        }

        // Any cache entries not removed above belonged to a file that is no
        // longer present - deleted outside the application.
        if cached.is_some_and(|cached| !cached.is_empty()) {
            changed = true;
        }
        if changed {
            if cacheable.is_empty() {
                // Nothing left to cache - remove a stale cache file rather
                // than writing an empty stub into an otherwise untouched or
                // now-empty Replay directory.
                let _ = fs::remove_file(root.join(CATALOG_CACHE_FILE_NAME));
            } else {
                self.write_catalog_cache(root, &cacheable);
            }
        }
        let entries = cacheable
            .into_iter()
            .map(|cache_entry| cache_entry.entry)
            .chain(uncacheable)
            .collect();
        (entries, warnings)
    }

    /// Reads and validates the on-disk catalog cache, if any. Returns `None`
    /// for a missing, corrupt, or version-incompatible cache - callers treat
    /// that exactly like "nothing cached yet," never as an error.
    fn read_catalog_cache(&self, root: &Path) -> Option<HashMap<String, CatalogCacheEntry>> {
        let path = root.join(CATALOG_CACHE_FILE_NAME);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return None;
        }
        if metadata.len() > MAX_CATALOG_CACHE_BYTES as u64 {
            return None;
        }
        let json = fs::read_to_string(&path).ok()?;
        let file: CatalogCacheFile = serde_json::from_str(&json).ok()?;
        if file.version != CATALOG_CACHE_VERSION {
            return None;
        }
        Some(
            file.entries
                .into_iter()
                .map(|cache_entry| (cache_entry.entry.id.clone(), cache_entry))
                .collect(),
        )
    }

    fn write_catalog_cache(&self, root: &Path, entries: &[CatalogCacheEntry]) {
        let file = CatalogCacheFile {
            version: CATALOG_CACHE_VERSION,
            entries: entries.to_vec(),
        };
        if let Ok(json) = serde_json::to_string(&file)
            && json.len() <= MAX_CATALOG_CACHE_BYTES
        {
            let _ = atomic_write(&root.join(CATALOG_CACHE_FILE_NAME), &json);
        }
    }

    pub(crate) fn load_full(&self, id: &str) -> Result<Replay, ReplayError> {
        let path = self.path_for_id(id)?;
        self.load_path(&path)
    }

    #[cfg(test)]
    pub(crate) fn rename(&self, replay: &Replay, new_label: String) -> Result<Replay, ReplayError> {
        let mut renamed = replay.clone();
        renamed
            .rename(new_label)
            .map_err(|error| ReplayError::InvalidData(error.to_string()))?;
        self.save(&renamed)?;
        Ok(renamed)
    }

    pub(crate) fn delete(&self, id: &str) -> Result<(), ReplayError> {
        let path = self.path_for_id(id)?;
        fs::remove_file(&path).map_err(|error| {
            ReplayError::Storage(format!("Could not delete {}: {error}", path.display()))
        })
    }

    fn replay_root(&self) -> Result<&Path, ReplayError> {
        self.root.get().map_err(ReplayError::StorageUnavailable)
    }

    fn load_path(&self, path: &Path) -> Result<Replay, ReplayError> {
        let metadata =
            fs::symlink_metadata(path).map_err(|error| ReplayError::Storage(error.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ReplayError::InvalidData(
                "Replay path is not a regular file".into(),
            ));
        }
        if metadata.len() > MAX_REPLAY_JSON_BYTES as u64 {
            return Err(ReplayError::DataTooLarge {
                item: "Replay",
                max_bytes: MAX_REPLAY_JSON_BYTES,
            });
        }
        let json =
            fs::read_to_string(path).map_err(|error| ReplayError::Storage(error.to_string()))?;
        let replay = Replay::from_json(&json)?;
        if path.file_stem().and_then(|stem| stem.to_str()) != Some(replay.id.as_str()) {
            return Err(ReplayError::InvalidData(
                "file name does not match Replay ID".into(),
            ));
        }
        Ok(replay)
    }

    fn path_for_id(&self, id: &str) -> Result<PathBuf, ReplayError> {
        let parsed = canonical_replay_id(id)?;
        Ok(self
            .replay_root()?
            .join(format!("{}.json", parsed.hyphenated())))
    }
}

impl ReplayStorage for NativeReplayStorage {
    fn load_catalog(&self) -> (Vec<ReplayCatalogEntry>, Vec<ReplayStorageWarning>) {
        Self::load_catalog(self)
    }

    fn load_full(&self, id: &str) -> Result<Replay, ReplayError> {
        Self::load_full(self, id)
    }

    fn apply(&self, mutation: ReplayMutation<'_>) -> Result<ReplayStorageUpdate, ReplayError> {
        match mutation {
            ReplayMutation::Reconcile => {}
            ReplayMutation::Insert(replay) => self.save(replay)?,
            ReplayMutation::Update(replay) => {
                let path = self.path_for_id(&replay.id)?;
                let metadata = fs::symlink_metadata(&path).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        ReplayError::Storage("Replay no longer exists.".into())
                    } else {
                        ReplayError::Storage(error.to_string())
                    }
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(ReplayError::Storage(
                        "Replay path is not a regular file".into(),
                    ));
                }
                self.save(replay)?;
            }
            ReplayMutation::Delete(id) => self.delete(id)?,
        }
        Ok(ReplayStorageUpdate::default())
    }

    fn retention_limit(&self) -> Option<usize> {
        None
    }
}

pub(crate) fn settings_storage() -> Box<dyn SettingsStorage> {
    Box::new(NativeSettingsStorage::user())
}

pub(crate) fn replay_storage() -> Box<dyn ReplayStorage> {
    Box::new(NativeReplayStorage::user())
}

pub(crate) fn now_millis() -> i64 {
    Local::now().timestamp_millis()
}

pub(crate) fn format_local_timestamp(timestamp_ms: i64) -> Option<String> {
    let timestamp: DateTime<Local> = Local.timestamp_millis_opt(timestamp_ms).single()?;
    Some(timestamp.format("%Y-%m-%d %H:%M").to_string())
}

pub(crate) fn settings_storage_note() -> &'static str {
    "Settings are stored in your platform application-data folder."
}

/// Whether the host asks for reduced motion. Nothing in the native
/// dependency tree reads the operating system's accessibility setting -
/// Bevy's windowing layer exposes none - so the answer here is always no,
/// and a native player who wants less motion says so with the in-game
/// preference. See `app::motion`.
pub(crate) fn prefers_reduced_motion() -> bool {
    false
}

pub(crate) fn window_platform_config() -> WindowPlatformConfig {
    WindowPlatformConfig {
        canvas: None,
        fit_canvas_to_parent: false,
        prevent_default_event_handling: false,
        resizable: false,
    }
}

/// Resolves the directory assets load from, plus the technical detail of a
/// failure to locate the running executable if there was one.
///
/// Locating the running executable ([`env::current_exe`]) can fail in
/// unusual sandboxes or OS states; this used to `panic!` on that failure in
/// a release build, crashing the whole app before a window ever opened. It
/// now falls back to [`resolve_asset_root`]'s
/// executable-relative-path-unavailable behavior (a plain `assets` directory
/// relative to the current working directory) instead - the same fallback a
/// debug build already used. Asset loading may still fail later if that
/// relative path does not exist, but that is an ordinary, already-handled
/// asset-load failure rather than an unhandled startup crash.
///
/// The failure is returned rather than logged here because of *when* this
/// runs: `AppPlugin::build` evaluates it while assembling `DefaultPlugins`,
/// before `LogPlugin` has installed a `tracing` subscriber, so a `warn!` at
/// this point would be discarded. Its caller hands the string to
/// [`crate::app::startup_log::DeferredStartupWarnings`] instead, which logs
/// it once a subscriber exists. Both this and
/// [`crate::settings::load_user_controls`] return `(value, Option<String>)`
/// for that same reason.
pub(crate) fn asset_root() -> (String, Option<String>) {
    let executable = env::current_exe();
    let warning = executable.as_ref().err().map(|error| {
        format!("Could not locate the executable for bundled asset resolution: {error}")
    });
    let root = resolve_asset_root(
        executable.ok().as_deref(),
        Path::new("assets"),
        cfg!(debug_assertions),
    )
    .to_string_lossy()
    .into_owned();
    (root, warning)
}

fn resolve_asset_root(
    executable: Option<&Path>,
    working_directory_assets: &Path,
    allow_development_fallback: bool,
) -> PathBuf {
    let adjacent = executable
        .and_then(Path::parent)
        .map(|parent| parent.join("assets"));
    if let Some(path) = &adjacent
        && path.is_dir()
    {
        return canonical_or_owned(path);
    }
    if allow_development_fallback && working_directory_assets.is_dir() {
        return canonical_or_owned(working_directory_assets);
    }
    adjacent.unwrap_or_else(|| working_directory_assets.to_path_buf())
}

fn canonical_or_owned(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The application-data root, per OS: macOS `$HOME/Library/Application
/// Support/Go Together`, Windows `%APPDATA%\Go Together`, everything else
/// `$XDG_CONFIG_HOME/go-together` or, failing that,
/// `$HOME/.config/go-together`. The Unix branch keeps Replays in the XDG
/// *config* home alongside the settings, where the spec would put data under
/// `XDG_DATA_HOME`; existing libraries live there, so moving it is a
/// migration rather than a rename - see `CONTRIBUTING.md`'s "Changing
/// persisted data". `Err` names the variable that is unset and becomes a
/// [`NativePath::Unavailable`], never a panic.
fn app_data_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| {
                home.join("Library")
                    .join("Application Support")
                    .join("Go Together")
            })
            .ok_or_else(|| "HOME is not set".to_string())
    }

    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Go Together"))
            .ok_or_else(|| "APPDATA is not set".to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(root).join("go-together"));
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".config").join("go-together"))
            .ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".to_string())
    }
}

/// Durable, crash-safe replacement, as `atomicwrites` performs it: write a
/// temporary file in a scratch subdirectory beside the target, `sync_all` it,
/// rename it into place, then `sync_all` the scratch directory and the
/// target's own directory, which is what makes the rename itself durable. The
/// syncs are what make the write durable rather than merely buffered - and on
/// macOS they issue `F_FULLFSYNC`, flushing the drive cache - so they
/// dominate the cost of a small write. See
/// [Replay-save timing](../../docs/ARCHITECTURE.md#replay-save-timing).
pub(crate) fn atomic_write(path: &Path, json: &str) -> Result<(), String> {
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| file.write_all(json.as_bytes()))
        .map_err(|error| error.to_string())
}

fn ensure_replay_directory(root: &Path) -> Result<(), ReplayError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            ReplayError::Storage("Replay storage root is not a regular directory".into()),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(root).map_err(|error| ReplayError::Storage(error.to_string()))
        }
        Err(error) => Err(ReplayError::Storage(error.to_string())),
    }
}

fn checked_replay_directory(root: &Path) -> Result<Option<fs::ReadDir>, ReplayError> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            ReplayError::Storage("Replay storage root is not a regular directory".into()),
        ),
        Ok(_) => fs::read_dir(root)
            .map(Some)
            .map_err(|error| ReplayError::Storage(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ReplayError::Storage(error.to_string())),
    }
}

fn replay_file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>")
}

#[cfg(test)]
mod tests {
    use super::*;
    // The per-schema-version fixtures are `replay`'s own, not copies:
    // `replay::test_support` is headless, so a platform adapter's tests can
    // use the same builders every other schema test does. See
    // `docs/TESTING.md`'s "Test organization".
    use crate::replay::test_support::{assert_storage_schema, literal_replay_value};
    use crate::replay::{REPLAY_SCHEMA_VERSION, ReplayEvent, ReplayEventKind, ReplayFrame};
    use crate::rules::{Board, Score};

    fn unique_root(name: &str) -> PathBuf {
        env::temp_dir().join(format!("{name}-{}", uuid::Uuid::new_v4()))
    }

    fn replay() -> Replay {
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
            id: "00000000-0000-4000-8000-000000000001".into(),
            label: "Native Match".into(),
            started_at_ms: 1,
            completed_at_ms: 2,
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
    fn release_asset_resolution_prefers_executable_adjacent_assets() {
        let root = unique_root("go-together-assets");
        let executable_dir = root.join("bundle");
        let adjacent_assets = executable_dir.join("assets");
        let development_assets = root.join("checkout-assets");
        fs::create_dir_all(&adjacent_assets).unwrap();
        fs::create_dir_all(&development_assets).unwrap();
        let executable = executable_dir.join("go-together");

        assert_eq!(
            resolve_asset_root(Some(&executable), &development_assets, true),
            fs::canonicalize(&adjacent_assets).unwrap()
        );

        fs::remove_dir_all(&adjacent_assets).unwrap();
        assert_eq!(
            resolve_asset_root(Some(&executable), &development_assets, false),
            adjacent_assets
        );
        assert_eq!(
            resolve_asset_root(Some(&executable), &development_assets, true),
            fs::canonicalize(&development_assets).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// `asset_root` degrades to this - `resolve_asset_root(None, ..)` - when
    /// `env::current_exe()` fails, in both debug and release builds; it must
    /// never panic, since asset resolution failing is recoverable (assets
    /// simply fail to load later) while a startup panic is not.
    #[test]
    fn unresolvable_executable_falls_back_to_a_relative_assets_path_without_panicking() {
        let root = unique_root("go-together-no-executable");
        let working_directory_assets = root.join("assets");

        assert_eq!(
            resolve_asset_root(None, &working_directory_assets, false),
            working_directory_assets
        );
        assert_eq!(
            resolve_asset_root(None, &working_directory_assets, true),
            working_directory_assets
        );

        fs::create_dir_all(&working_directory_assets).unwrap();
        assert_eq!(
            resolve_asset_root(None, &working_directory_assets, true),
            fs::canonicalize(&working_directory_assets).unwrap(),
            "a development fallback that does exist is still preferred over a bare relative path"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn native_replay_loading_does_not_follow_symlinks_or_directories() {
        use std::os::unix::fs::symlink;

        let root = unique_root("go-together-replay-types");
        let store = NativeReplayStorage::new(root.clone());
        let replay = replay();
        store.save(&replay).unwrap();
        let canonical = root.join(format!("{}.json", replay.id));
        symlink(&canonical, root.join("alias.json")).unwrap();
        fs::create_dir(root.join("directory.json")).unwrap();

        let (loaded, warnings) = store.load_catalog();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, replay.id);
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().all(|warning| {
            warning.is_skipped_replay() && warning.to_string().contains("non-regular Replay")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalog_cache_is_rebuilt_from_scratch_when_missing() {
        let root = unique_root("go-together-catalog-missing");
        let store = NativeReplayStorage::new(root.clone());
        store.save(&replay()).unwrap();
        assert!(!root.join(CATALOG_CACHE_FILE_NAME).exists());

        let (entries, warnings) = store.load_catalog();

        assert!(warnings.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, replay().id);
        assert!(
            root.join(CATALOG_CACHE_FILE_NAME).exists(),
            "a missing cache is rebuilt and persisted for next time"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalog_rebuilds_when_the_cache_is_corrupt_or_an_incompatible_version() {
        let root = unique_root("go-together-catalog-corrupt");
        let store = NativeReplayStorage::new(root.clone());
        store.save(&replay()).unwrap();

        fs::write(root.join(CATALOG_CACHE_FILE_NAME), "{ not json").unwrap();
        let (entries, warnings) = store.load_catalog();
        assert!(warnings.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, replay().id);

        fs::write(
            root.join(CATALOG_CACHE_FILE_NAME),
            serde_json::json!({"version": 999_999, "entries": []}).to_string(),
        )
        .unwrap();
        let (entries, warnings) = store.load_catalog();
        assert!(warnings.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, replay().id);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalog_detects_replay_files_changed_or_removed_outside_the_application() {
        let root = unique_root("go-together-catalog-external-changes");
        let store = NativeReplayStorage::new(root.clone());
        let mut second = replay();
        second.id = "00000000-0000-4000-8000-000000000002".into();
        second.label = "Second Match".into();
        store.save(&replay()).unwrap();
        store.save(&second).unwrap();

        let (entries, _) = store.load_catalog();
        assert_eq!(entries.len(), 2);

        // Rewrite one file's content directly, bypassing the application -
        // the cached entry must not be trusted once the fingerprint changes.
        let mut relabeled = replay();
        relabeled.label = "Changed Outside The App".into();
        fs::write(
            root.join(format!("{}.json", relabeled.id)),
            relabeled.to_storage_json().unwrap(),
        )
        .unwrap();
        // Delete the other file entirely.
        fs::remove_file(root.join(format!("{}.json", second.id))).unwrap();

        let (entries, warnings) = store.load_catalog();
        assert!(warnings.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, relabeled.id);
        assert_eq!(entries[0].label, "Changed Outside The App");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalog_isolates_a_corrupt_replay_file_without_hiding_valid_ones() {
        let root = unique_root("go-together-catalog-isolation");
        fs::create_dir_all(&root).unwrap();
        let store = NativeReplayStorage::new(root.clone());
        store.save(&replay()).unwrap();
        fs::write(
            root.join("00000000-0000-4000-8000-000000000099.json"),
            "{ this is not valid replay json",
        )
        .unwrap();

        let (entries, warnings) = store.load_catalog();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, replay().id);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].is_skipped_replay());
        assert!(warnings[0].to_string().contains("corrupt"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_replay_directory_never_gains_a_catalog_cache_file() {
        let root = unique_root("go-together-catalog-empty");
        fs::create_dir_all(&root).unwrap();
        let store = NativeReplayStorage::new(root.clone());

        let (entries, warnings) = store.load_catalog();

        assert!(entries.is_empty());
        assert!(warnings.is_empty());
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            0,
            "an empty Replay directory must stay empty, not gain a cache stub"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_rename_preserves_each_replay_schema_on_disk() {
        let root = unique_root("go-together-versioned-replays");
        fs::create_dir_all(&root).unwrap();
        let store = NativeReplayStorage::new(root.clone());

        for version in 1..=REPLAY_SCHEMA_VERSION {
            let id = format!("00000000-0000-4000-8000-{version:012}");
            // `sample_replay` mints a fresh UUID per call and this test needs
            // one file per version, so the id is overridden in the JSON
            // rather than in a fixture of its own.
            let mut literal = literal_replay_value(version);
            literal["id"] = serde_json::json!(id);
            let path = root.join(format!("{id}.json"));
            fs::write(&path, serde_json::to_string_pretty(&literal).unwrap()).unwrap();

            let (loaded, warnings) = store.load_catalog();
            assert!(warnings.is_empty());
            assert!(loaded.iter().any(|entry| entry.id == id));
            let replay = store.load_full(&id).unwrap();
            let label = format!("Renamed Schema {version}");
            store.rename(&replay, label.clone()).unwrap();

            let stored: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_storage_schema(&stored, version);
            let mut expected = literal;
            expected["label"] = serde_json::json!(label);
            assert_eq!(stored, expected);

            let (reloaded, warnings) = store.load_catalog();
            assert!(warnings.is_empty());
            let reloaded = reloaded.iter().find(|entry| entry.id == id).unwrap();
            assert_eq!(reloaded.version, version);
            assert_eq!(reloaded.label, format!("Renamed Schema {version}"));
        }

        fs::remove_dir_all(root).unwrap();
    }
}
