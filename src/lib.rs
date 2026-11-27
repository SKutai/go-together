//! Go Together: a local, two-player, simultaneous-move Go variant, playable
//! as a Bevy desktop/browser application and usable as a headless Rust
//! library.
//!
//! # Two layers
//!
//! * **The headless core** is always compiled, has no Bevy dependency, and
//!   needs no window, renderer, audio device, or storage backend to run:
//!   [`rules`] resolves rounds, captures, territory, and scoring;
//!   [`opening`] owns Round 0 selection/reveal; [`match_settings`] validates
//!   optional Match rules; [`clock`] defines [`clock::MatchInstant`] and
//!   [`clock::MatchDuration`], the whole-microsecond time vocabulary every
//!   authoritative timing decision uses instead of `f64` seconds;
//!   [`authority`] is the authoritative, deterministic
//!   [`authority::MatchState`] that drives a Match from those pieces through
//!   explicit command methods and structured outcomes; [`view`] projects that
//!   trusted state into the privacy-scoped [`view::PublicMatchView`] and
//!   [`view::PlayerMatchView`] a future networked client or spectator would
//!   actually be permitted to see; [`replay`] loads a persisted
//!   [`replay::Replay`] of any supported schema version
//!   ([`replay::Replay::from_json`]), validates it, exposes its result and
//!   its complete event-by-event record through accessors, writes it back
//!   in its own on-disk shape, and plays it back through
//!   [`replay::PlaybackController`] - the same validation and playback the
//!   game's Replay Viewer uses; and [`cue`] is the [`cue::GameCue`] sound
//!   vocabulary a Replay records and playback reports. All of [`replay`] is
//!   core in that none of it needs Bevy, recording included - though
//!   `recording`'s module declaration is gated on `client` or `cfg(test)`,
//!   because `MatchRecorder` is crate-private and a headless non-test build
//!   has no caller for it. It stays crate-private only because a public one
//!   could hand out a `Replay` that never passed `validate`. The on-disk
//!   library the UI browses needs a storage adapter and a wall clock, so it
//!   is `replay_library`, behind `client`.
//!   Depend on this crate with `default-features = false`
//!   to get only this layer - `tests/headless_match.rs` and
//!   `tests/headless_replay.rs` show a consumer driving a Match and a Replay
//!   through nothing but this API - and see [`authority`]'s and [`replay`]'s
//!   module docs for the exact boundaries, and `docs/ARCHITECTURE.md` for
//!   the full design and how a future authoritative host could reuse it.
//! * **The Bevy client**, gated behind the default `client` feature, wires
//!   the core into the playable application: `app` owns bootstrap, the
//!   `app::AppScreen` state machine, and per-frame system ordering;
//!   `gameplay` wraps one [`authority::MatchState`] in a Bevy resource and
//!   drives it from input, the match clock, and hold gestures; `input`
//!   translates physical keyboard/controller devices into semantic actions;
//!   `settings` persists those bindings and preferences, and keeps the
//!   six settings schema versions loadable through the same
//!   schema/compat split [`replay`] uses; `replay_library`
//!   owns the saved-Replay collection the Library screen browses;
//!   `platform` adapts native filesystem and browser storage;
//!   `presentation` is the colour vocabulary; `audio` turns queued semantic
//!   cues into sound; `diagnostics` is where a subsystem's typed error
//!   becomes a player-safe notice plus a full log line, and knows no
//!   subsystem itself; `ui` owns every screen. None of this compiles without
//!   `client`, and none of it is part of this crate's public API.
//!
#![cfg_attr(
    feature = "client",
    doc = "Start the application at [`run`] (re-exported from `app::run`); it is \
           the only thing the `go-together` binary's `main` does."
)]
#![cfg_attr(
    not(feature = "client"),
    doc = "The application entry point, `run`, exists only with the `client` \
           feature; this build is the headless core alone."
)]

pub mod authority;
pub mod clock;
pub mod cue;
pub mod match_settings;
pub mod opening;
pub mod replay;
pub mod rules;
pub mod view;

#[cfg(feature = "client")]
mod app;
#[cfg(feature = "client")]
mod audio;
#[cfg(feature = "client")]
mod diagnostics;
#[cfg(feature = "client")]
pub(crate) mod gameplay;
#[cfg(feature = "client")]
mod input;
#[cfg(feature = "client")]
mod platform;
#[cfg(feature = "client")]
mod presentation;
#[cfg(feature = "client")]
mod replay_library;
#[cfg(feature = "client")]
mod settings;
#[cfg(feature = "client")]
mod ui;

/// Test-only guards over the crate's own source: the module dependency
/// graph, the printable-ASCII rule for every string literal in production
/// code, the rule that only `presentation` constructs a colour, and the
/// source reads behind `ui`'s negative assertions and
/// `replay::measurement`'s mirrored-constant check.
#[cfg(test)]
mod architecture;
/// Test-only guard over the colours in `presentation`: each is held to the
/// contrast floor on every background it is drawn on.
#[cfg(all(feature = "client", test))]
mod contrast;
#[cfg(all(feature = "client", test))]
mod test_support;

#[cfg(feature = "client")]
pub use app::run;
