//! The persisted settings wire format: every constant, `struct`, and
//! `#[serde]` field name that appears in `controls.json` or in browser
//! storage, plus the error a load reports.
//!
//! Everything in this module is a compatibility boundary. Renaming a field,
//! changing a version constant, or tightening a `deny_unknown_fields` here
//! silently invalidates settings files that already exist on players'
//! machines. `docs/ARCHITECTURE.md`'s "Change a persisted schema" recipe is
//! the standing rule: add a version, never reinterpret one. The one kind
//! of exception ever taken, twice, is a key name withdrawn because the
//! interface reserved the key for itself - `f1`, when F1 became the Help
//! key, and `f11`, when F11 became the fullscreen key. No version could
//! have expressed either: a key no version may bind is not a schema, so a
//! file naming one loses its keyboard section to defaults with a warning. A file naming `escape` has always failed to parse, since that
//! key never had a name; until the keyboard section was made recoverable,
//! that failure cost the whole file.
//!
//! This module describes shapes only. Turning any of those shapes into a live
//! [`super::ControlsConfig`] - which sections a given version has, and what a
//! missing or corrupt one falls back to - is [`super::compat`]'s job.
//!
//! # Why two envelopes
//!
//! [`ControlsLoadFile`] and [`ControlsFile`] are a deliberate pair rather
//! than one type used twice:
//!
//! - `ControlsLoadFile` is the permissive read shape. Every section in it -
//!   the keyboard section under whichever field name its version uses,
//!   `controller`, `preferences`, and `game` - is a [`serde_json::Value`],
//!   not its real type, so that one corrupt section fails when
//!   [`super::compat`] chooses to interpret it rather than taking the whole
//!   document down with it. That is what makes the per-section isolation
//!   `docs/ARCHITECTURE.md` describes possible at all.
//! - `ControlsFile` is the strict write shape, in the current schema version
//!   only, and is also what a test deserializes when it wants a typed handle
//!   on a well-formed document.
//!
//! Both keep `deny_unknown_fields`: a settings file with a field this build
//! does not know is corrupt, not forward-compatible.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::input::GameAction;
use crate::input::controller::{ControllerInput, ProfileAction};
use crate::input::keyboard::KeyboardKey;
use crate::match_settings::MatchSettings;

use super::config::GamePreferences;

// These values are persisted compatibility boundaries, not crate versions.
pub(super) const CONFIG_VERSION: u32 = 1;
pub(super) const CONTROLS_SCHEMA_VERSION: u32 = 2;
pub(super) const LEGACY_SETTINGS_SCHEMA_VERSION: u32 = 3;
pub(super) const SPLIT_AUDIO_SETTINGS_SCHEMA_VERSION: u32 = 4;
pub(super) const MATCH_RULES_SETTINGS_SCHEMA_VERSION: u32 = 5;
pub(super) const SETTINGS_SCHEMA_VERSION: u32 = 6;
/// The size limit every settings backend enforces before parsing, re-exported
/// as [`crate::settings::MAX_SETTINGS_JSON_BYTES`] for [`crate::platform`]'s
/// native and browser adapters as well as [`ControlsConfig::from_json`].
///
/// This is the one constant the storage adapters take from `settings`, and
/// deliberately so: it is domain policy that [`ControlsConfig::from_json`]
/// enforces, and an adapter checking it too is defence in depth. The native
/// file *name* is not domain policy and lives with the adapter that owns the
/// directory, as `platform::native`'s `CONFIG_FILE_NAME`.
///
/// [`ControlsConfig::from_json`]: super::ControlsConfig::from_json
pub(crate) const MAX_SETTINGS_JSON_BYTES: usize = 64 * 1024;

/// Why a settings document could not be turned into a
/// [`super::ControlsConfig`] at all.
///
/// Every variant here ends the load: the caller falls back to complete
/// defaults. A section that is merely corrupt does *not* appear here - it
/// degrades to that section's defaults and returns a warning string instead,
/// which is what keeps a valid section alive through an invalid one, the
/// keyboard section included. See [`super::compat`] for that split.
#[derive(Debug)]
pub(crate) enum BindingConfigError {
    InvalidJson(serde_json::Error),
    DataTooLarge,
    UnsupportedVersion(u32),
}

impl fmt::Display for BindingConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(formatter, "invalid JSON: {error}"),
            Self::DataTooLarge => write!(
                formatter,
                "settings exceed the {MAX_SETTINGS_JSON_BYTES}-byte limit"
            ),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported configuration version {version}")
            }
        }
    }
}

/// One keyboard binding on disk. Stored as a record rather than as a map
/// entry so that an unknown action or key can be reported precisely.
///
/// This shape has never changed across schema v1-v6; only the field it lives
/// under has - `bindings` in v1, `keyboard` from v2 on. The set of accepted
/// key names has changed twice: `f1` was a valid name in every version
/// until F1 became the Help key, and `f11` until F11 became the fullscreen
/// key, and each left [`KeyboardKey`] then. A file that still names either
/// fails to parse here and its keyboard section falls back to defaults with
/// a warning - the outcome a file naming `escape`, which never had a name,
/// now shares, where it once cost the whole document.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BindingRecord {
    pub(super) action: GameAction,
    pub(super) key: KeyboardKey,
}

/// Both controller profiles on disk, added by schema v2.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControllerProfilesRecord {
    pub(super) player_one: Vec<ControllerBindingRecord>,
    pub(super) player_two: Vec<ControllerBindingRecord>,
}

/// One controller binding within a profile.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControllerBindingRecord {
    pub(super) action: ProfileAction,
    pub(super) input: ControllerInput,
}

/// The permissive read envelope: what a settings file of *any* supported
/// schema version deserializes into before [`super::compat`] interprets it.
///
/// Every section is a [`serde_json::Value`] precisely so that none is
/// validated here - deferring them is what lets one bad section fall back
/// alone. The keyboard section lives under `bindings` in v1 and `keyboard`
/// from v2 on, and a version reads only its own field; the other, if a file
/// carries it, is never parsed (see [`super::compat`]).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControlsLoadFile {
    pub(super) version: u32,
    #[serde(default)]
    pub(super) bindings: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) keyboard: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) controller: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) preferences: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) game: Option<serde_json::Value>,
}

/// The strict write envelope, covering the legacy schema v1 shape
/// (`bindings` only), schema v2 (`keyboard` + `controller`), schema v3
/// (controls + display preferences), schema v4 (separate game/UI sound
/// levels), schema v5 (match rules), and schema v6 (the Reduce Motion
/// preference).
///
/// Only schema v6 is ever written; the older fields survive on this type
/// because tests build historical documents through it, and because
/// `skip_serializing_if` is what keeps a v6 write free of the empty `bindings`
/// array v1 used.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControlsFile {
    pub(super) version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) bindings: Vec<BindingRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) keyboard: Vec<BindingRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) controller: Option<ControllerProfilesRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) preferences: Option<GamePreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) game: Option<MatchSettings>,
}
