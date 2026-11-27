//! The persisted settings document: preferences plus keyboard and controller
//! bindings, and the schema migrations that keep older files loadable.
//!
//! # Responsibility
//!
//! This module owns settings serialization and delegates raw document storage
//! to [`crate::platform::SettingsStorage`].
//! [`ControlsConfig`] is the whole document - the [`KeyboardBindings`] and
//! [`ControllerBindings`] owned by [`crate::input`] plus this module's
//! [`GamePreferences`] and [`crate::match_settings::MatchSettings`] - and
//! [`schema`]'s `#[serde]` records are its on-disk shape.
//!
//! [`KeyboardBindings`]: crate::input::keyboard::KeyboardBindings
//! [`ControllerBindings`]: crate::input::controller::ControllerBindings
//!
//! # Schema boundary
//!
//! Everything above this module speaks in live resources; everything below it
//! speaks in versioned JSON. Schema v1 was keyboard-only, v2 added controller
//! profiles, v3 added display preferences, v4 split game and UI volume, v5
//! added match rules, and v6 added the Reduce Motion preference.
//! [`ControlsConfig::from_json`] migrates all six onto the current shape, so
//! an upgrade never discards a player's bindings.
//!
//! # Persistence and compatibility
//!
//! [`SettingsStore`] selects atomic `controls.json` files on native platforms
//! and a namespaced, versioned `localStorage` key in browsers.
//! [`load_user_controls`] never fails: a missing document yields defaults
//! silently, unreadable or oversized data or a version this build does not
//! know falls back to defaults, and a section that cannot be read falls
//! back on its own, each
//! with a warning for the Settings screen. Bindings are stored as
//! `{action, key}` records rather than a map so an unknown entry can be
//! reported precisely. All `#[serde]` names in [`schema`] and [`config`] are
//! part of the saved configuration format.
//!
//! # Submodules
//!
//! The cut follows `crate::replay`'s, for the same reason: six schema
//! versions and a document that must keep loading older forms make
//! "where does a compatibility change go?" the question the layout should
//! answer.
//!
//! - [`schema`] - the persisted wire format. Version constants, the size
//!   limit, the `#[serde]` records, the two file envelopes, and
//!   [`schema::BindingConfigError`]. Read this first to see *what* a settings
//!   file is. Nothing here interprets a document.
//! - [`compat`] - loading every supported version and writing the current one:
//!   [`ControlsConfig::from_json`], [`ControlsConfig::to_json`], and the
//!   per-section loaders behind them. A compatibility-sensitive change belongs
//!   here or in [`schema`], and nowhere else.
//! - [`config`] - the live resources the rest of the client reads:
//!   [`ControlsConfig`], [`GamePreferences`], [`VolumeLevel`]. No JSON.
//! - [`store`] - the storage backend and the two calls over it,
//!   [`load_user_controls`] and [`save_user_controls`].
//!
//! Where this differs from `replay` is [`compat`]'s interior. `replay::compat`
//! is a version ladder; settings' loaders are per *section*, because a v5 file
//! with a corrupt controller section must still yield the player's keyboard
//! bindings, and one with a corrupt keyboard section their controller
//! profiles and preferences. That file's own docs explain the split between
//! fatal and recoverable failure that this depends on.

mod compat;
mod config;
mod schema;
mod store;

pub(crate) use self::config::{ControlsConfig, GamePreferences, VolumeLevel};
pub(crate) use self::schema::MAX_SETTINGS_JSON_BYTES;
pub(crate) use self::store::{SettingsStore, load_user_controls, save_user_controls};
