//! Platform adapters for storage, time, assets, and the primary window.
//!
//! Domain code works with [`SettingsStorage`] and [`ReplayStorage`] rather than
//! filesystem or browser APIs. Native builds select atomic files beneath the
//! platform application-data directory; WASM builds select versioned,
//! namespaced browser storage. The same split owns the few other host-specific
//! concerns - the wall clock, the asset root, the window configuration, the
//! settings-location sentence the Settings screen shows, and whether the host
//! asks for reduced motion - so `settings`, `replay_library`, `gameplay`,
//! `ui`, and `app` stay platform-neutral, reaching the host only through
//! this module. [`crate::replay`] is more than
//! platform-neutral: it is headless core and never sees this module at all -
//! it defines the format and the validation these adapters must satisfy, and
//! they depend on it.

#[cfg(any(target_arch = "wasm32", test))]
mod browser_storage;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::fmt;

use crate::replay::{Replay, ReplayError};
use crate::replay_library::ReplayCatalogEntry;

#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use native::{
    CONFIG_FILE_NAME, NativeReplayStorage, NativeSettingsStorage, atomic_write,
};

/// Maximum number of completed Replays retained by the browser adapter.
///
/// Three is derived, not chosen by taste. `browser_storage` caps a single
/// record at `MAX_BROWSER_REPLAY_BYTES` (800 KiB, 819,200 bytes) and the
/// whole serialized catalog at `MAX_BROWSER_CATALOG_BYTES` (2,500,000
/// bytes), and 3 is the largest count for which the first cannot overflow
/// the second:
///
/// ```text
/// 3 x 819,200 = 2,457,600  <  2,500,000   (42,400 bytes spare, ample for
///                                          the catalog's own JSON envelope)
/// 4 x 819,200 = 3,276,800  >  2,500,000
/// ```
///
/// So retention is provably safe rather than usually-fine: no combination of
/// individually accepted Replays can produce a catalog that `to_json` then
/// refuses to write.
///
/// **The tension this creates, since it invites a "fix":** the worst case is
/// not what players produce. A default-length Match measures 320,039 bytes
/// post-compaction - the "~320 KB" row of the "Replay size" table in
/// [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md#replay-size) - so
/// three of them use about 38% of the catalog budget, and the other 62% is
/// reserved against a worst case that is rare in practice. This was reviewed
/// and deliberately kept at 3. Raising the count alone would discard the
/// safety property outright, not weaken it by a margin: at 4 the arithmetic
/// no longer holds, and a player with four large Replays would find every
/// save failing. If more retained Replays are ever wanted, the shape is
/// byte-aware eviction - evict until both `count <= N` and the serialized
/// total fits `MAX_BROWSER_CATALOG_BYTES` - which keeps a provable bound
/// while letting typical Replays use the space they actually need. A larger
/// bare count does not.
///
/// Raising any of these three caps should also be preceded by measuring the
/// synchronous `localStorage.setItem` cost, which
/// [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md#replay-save-timing)
/// records as unmeasured: a bigger catalog means a bigger synchronous write
/// on the single thread WASM has, and nothing here yet says what that costs.
#[cfg(target_arch = "wasm32")]
pub(crate) const BROWSER_REPLAY_LIMIT: usize = 3;

/// The host differences in Bevy's primary window: how it is made, and what
/// the host leaves to the game once it exists. Everything the two hosts
/// share - the title, that the window can be resized, and its smallest
/// size - is the game's, in `crate::app::window`.
pub(crate) struct WindowPlatformConfig {
    /// The page's canvas a browser build draws into, by CSS selector;
    /// `None` for a desktop window.
    pub(crate) canvas: Option<String>,
    pub(crate) fit_canvas_to_parent: bool,
    pub(crate) prevent_default_event_handling: bool,
    /// The window's size as it opens, in logical pixels, where the host
    /// decides it: in a browser, the size the page has already given the
    /// canvas. `None` opens it at the game's own opening size.
    pub(crate) opening_size: Option<(u32, u32)>,
    /// Whether the game places the window on its monitor as it opens -
    /// centred, or maximised where the monitor has no room for it - which
    /// is why it opens hidden until it is placed. A desktop window is
    /// placed; a browser's canvas stands where the page puts it.
    pub(crate) placed_on_a_monitor: bool,
    /// Whether F11 is the game's, to switch the window to fullscreen and
    /// back. It is on a desktop; a browser keeps F11 for its own
    /// fullscreen, and the page passes it to the browser before the game
    /// sees it.
    pub(crate) fullscreen_key: bool,
}

/// The browser adapter's window: drawn into the page's `#go-together`
/// canvas, opening at `canvas_size`, the size the page has given it, with
/// the browser's default actions kept off the canvas but for F11's, and
/// neither placed on a monitor nor switched to fullscreen by the game.
/// Compiled for native tests as well, so that the browser's window is
/// checked beside the native one.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn browser_window_platform_config(
    canvas_size: Option<(u32, u32)>,
) -> WindowPlatformConfig {
    WindowPlatformConfig {
        canvas: Some("#go-together".into()),
        fit_canvas_to_parent: false,
        prevent_default_event_handling: true,
        opening_size: canvas_size,
        placed_on_a_monitor: false,
        fullscreen_key: false,
    }
}

/// Why a [`SettingsStorage`] operation failed.
///
/// Two variants, not one enormous enum: [`Self::Unavailable`] is an expected
/// environment condition (no app-data directory, no `localStorage`, private
/// browsing) where the backend itself is unusable, while [`Self::Failed`] is
/// a backend that is normally usable but rejected this specific operation
/// (I/O error, oversized document, an invalid stored path). No consumer
/// branches on the difference yet - [`crate::settings::load_user_controls`]
/// falls back to defaults and reports the same warning either way - so the
/// split is kept for the adapters to say which case they hit, ready for a
/// consumer that wants to treat the two differently. Both preserve the full
/// technical message for logs; see [`crate::diagnostics`] for how that
/// becomes a user-facing notice.
#[derive(Clone, Debug)]
pub(crate) enum SettingsStorageError {
    Unavailable(String),
    Failed(String),
}

impl fmt::Display for SettingsStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Failed(message) => write!(formatter, "{message}"),
        }
    }
}

/// Raw settings document persistence. Schema migration stays in `settings`.
pub(crate) trait SettingsStorage: Send + Sync {
    fn load(&self) -> Result<Option<String>, SettingsStorageError>;
    fn save(&self, json: &str) -> Result<(), SettingsStorageError>;
}

/// Replay persistence mutation boundary.
///
/// Browser mutations reconcile against the freshly loaded catalog and return an
/// authoritative snapshot, which keeps multiple tabs coherent. Native storage
/// updates one UUID-named file and leaves the in-memory collection untouched.
#[derive(Clone, Copy)]
pub(crate) enum ReplayMutation<'a> {
    /// No change requested: apply retention and reconciliation only. The
    /// browser adapter still re-reads the stored catalog, evicts past
    /// `BROWSER_REPLAY_LIMIT`, and returns the authoritative snapshot;
    /// native does nothing. `ReplayLibrary` sends this once, right after
    /// `load_catalog`, so a catalog another tab left oversized is trimmed on
    /// load rather than at the next save.
    Reconcile,
    Insert(&'a Replay),
    Update(&'a Replay),
    Delete(&'a str),
}

/// Something a [`ReplayStorage`] operation could not do, without failing.
///
/// A warning is not an error: the operation returned a result, and this says
/// what is missing from it. `load_catalog` returns these beside the entries
/// it did build, and [`ReplayStorageUpdate`] carries the ones a mutation
/// produced. Failures that stop the operation are [`ReplayError`] instead.
///
/// Two variants, not one per I/O failure - the same reasoning as
/// [`SettingsStorageError`], applied to the one distinction these consumers
/// actually make. [`Self::SkippedReplay`] is one stored Replay the adapter
/// refused to hand back (unreadable file, corrupt or oversized record, a
/// record that fails validation, a browser key that is not a Replay ID):
/// a player may notice that Replay missing from the Library, and the
/// Library screen counts these to say how many. [`Self::Storage`] is the
/// backend reporting something that is not one countable Replay - an
/// unusable Replay directory, a browser store that cannot be read at all, a
/// reconcile that failed - where a count would be a lie, since the number of
/// Replays affected is exactly what is unknown.
///
/// Both carry the full technical message, and [`fmt::Display`] is that
/// message: it is written for a log line, not for a player. The Replay
/// Library screen renders a count and a generic sentence and never this
/// text - see `docs/ARCHITECTURE.md`'s "Error handling and diagnostics"
/// section for the broadcast/scoped policy that requires it.
#[derive(Clone, Debug)]
pub(crate) enum ReplayStorageWarning {
    SkippedReplay(String),
    Storage(String),
}

impl ReplayStorageWarning {
    /// Whether this warning means one specific stored Replay is missing
    /// from what the adapter returned.
    pub(crate) const fn is_skipped_replay(&self) -> bool {
        matches!(self, Self::SkippedReplay(_))
    }
}

impl fmt::Display for ReplayStorageWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SkippedReplay(message) | Self::Storage(message) => write!(formatter, "{message}"),
        }
    }
}

/// What a [`ReplayStorage::apply`] call changed, for `ReplayLibrary` to
/// mirror in memory.
///
/// `authoritative_entries` is the whole catalog as the adapter now holds it.
/// `Some` means "replace the in-memory list with this": the browser adapter
/// answers that way on every mutation, because retention and multi-tab
/// reconciliation happen storage-side, so what it holds afterward may differ
/// from the one change that was asked for. `None` means the adapter changed
/// exactly what it was asked and nothing else, and the Library applies the
/// same single insert, replace, or remove itself - the native adapter always
/// answers this way. `warnings` are whatever the mutation could not read or
/// keep, in the same shape `load_catalog` reports.
#[derive(Default)]
pub(crate) struct ReplayStorageUpdate {
    pub(crate) authoritative_entries: Option<Vec<ReplayCatalogEntry>>,
    pub(crate) warnings: Vec<ReplayStorageWarning>,
}

/// Replay persistence: a lightweight catalog for listing, and complete,
/// validated Replay content loaded only when a player opens one.
///
/// `load_catalog` reports what it could not read as
/// [`ReplayStorageWarning`]s beside the entries it did build, rather than
/// failing: one unreadable Replay must never hide every other one.
///
/// `load_catalog` must never return a `ReplayCatalogEntry` for data it has
/// not itself validated (directly, or through a cache it built from a
/// previously validated load) - see `crate::replay_library::catalog` and
/// `crate::platform::native`'s on-disk catalog cache for why a stale or
/// forged cache entry is never trusted as proof a Replay is safe to open.
///
/// Reads are therefore a validation boundary; writes are not. `apply` trusts
/// the `Replay` it is handed, because `ReplayLibrary` has already validated it
/// at the top of the write path - see [`crate::replay::Replay::validate`] for
/// the full ownership policy, including why no adapter should re-validate.
pub(crate) trait ReplayStorage: Send + Sync {
    fn load_catalog(&self) -> (Vec<ReplayCatalogEntry>, Vec<ReplayStorageWarning>);
    fn load_full(&self, id: &str) -> Result<Replay, ReplayError>;
    fn apply(&self, mutation: ReplayMutation<'_>) -> Result<ReplayStorageUpdate, ReplayError>;
    fn retention_limit(&self) -> Option<usize>;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{
    asset_root, format_local_timestamp, now_millis, prefers_reduced_motion, replay_storage,
    settings_storage, settings_storage_note, window_platform_config,
};
#[cfg(target_arch = "wasm32")]
pub(crate) use web::{
    asset_root, format_local_timestamp, now_millis, prefers_reduced_motion, replay_storage,
    settings_storage, settings_storage_note, window_platform_config,
};
