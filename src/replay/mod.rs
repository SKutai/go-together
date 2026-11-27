//! The Replay format: recording a live Match and reconstructing it for
//! playback - and the headless public API for loading, validating,
//! inspecting, serializing, and playing back a Replay.
//!
//! This module is wholly headless core. Persisting a Replay and browsing
//! the saved collection are the client's `replay_library` and `platform`;
//! nothing here reads or writes storage.
//!
//! # What a headless consumer can do
//!
//! With `default-features = false` this module offers, with no Bevy, window,
//! audio device, or storage backend involved:
//!
//! - **Load** a Replay of any supported schema version with
//!   [`Replay::from_json`] (from text) or [`Replay::from_value`] (from a
//!   [`serde_json::Value`] embedded in a larger document). Both validate
//!   before returning, and they are the only ways to obtain a [`Replay`]
//!   outside this crate: its fields are private and it deliberately has no
//!   [`serde::Deserialize`] impl, so **every `Replay` a consumer can hold has
//!   passed [`Replay::validate`]**. The `MAX_*` constants here are the
//!   limits a consumer can hit: the size `from_json` parses, and the event
//!   count and duration `validate` accepts (its timestamp, capture-total,
//!   and end-reason-count bounds are internal).
//! - **Inspect** it through accessors: the Match-level result
//!   ([`Replay::final_score`], [`Replay::winner`], [`Replay::end_reasons`],
//!   [`Replay::match_settings`], [`Replay::final_board`],
//!   [`Replay::completed_round_count`], [`Replay::opening_completed`]) and
//!   the complete event record ([`Replay::events`], each a [`ReplayEvent`]
//!   with its [`ReplayEventKind`] and the [`ReplayFrame`] snapshot taken
//!   after it). [`ReplayFrame::current_score`] is the running score at any
//!   frame; see that type's docs for why the persisted per-frame score is
//!   not exposed.
//! - **Serialize** it back to the on-disk shape for its own schema version
//!   with [`Replay::to_storage_json`] or [`Replay::to_storage_value`] - the
//!   exact bytes the game writes, so a loaded v1 file re-saves as a v1 file.
//!   Those two are the only ways to serialize a `Replay`: it implements no
//!   [`serde::Serialize`] any more than it implements `Deserialize`, so
//!   **there is no second, in-memory shape to write out by mistake**. The
//!   crate keeps that shape to itself, in `compat`. [`Replay::rename`] is the
//!   one mutation, and it enforces the same label rule validation does.
//! - **Play it back**: [`Replay::playback_frame_at`] is the state at any
//!   time on the timeline, and [`PlaybackController`] is the transport
//!   (play/pause, speed, rewind, step between [`Replay::significant_times`],
//!   fast-forward) that a caller drives from its own clock through
//!   [`PlaybackController::advance_with_cues`], which also reports the
//!   [`crate::cue::GameCue`]s crossed - the same controller the Replay
//!   Viewer uses.
//!
//! `tests/headless_replay.rs` exercises all of this through the public API
//! against the checked-in fixtures under `tests/fixtures/replays/`, one per
//! schema version, and runs under `cargo test --no-default-features`.
//!
//! What a headless consumer cannot do is *record* a Replay or *browse* a
//! library of them. Those are two different limits, and only one of them is
//! about this module's layer. Browsing needs a storage adapter and a Bevy
//! resource, and lives in `crate::replay_library` with the rest of the
//! client. Recording is pure data assembly and lives here in `recording`,
//! but `MatchRecorder` stays crate-private, because the guarantee above -
//! that every `Replay` a consumer can hold has passed [`Replay::validate`] -
//! depends on there being no other way to build one; see `recording`'s own
//! module docs.
//!
//! # Exhaustiveness
//!
//! The vocabularies that have grown with the schema and would grow again
//! with a new version - [`ReplayEventKind`] (and each of its struct
//! variants, which have gained fields before), [`EndReason`], and
//! [`crate::cue::GameCue`] - are `#[non_exhaustive]`, as are the two error
//! enums, so a future schema version is not also a downstream compile break.
//! [`ReplayPhase`], [`TimeoutCause`], and [`ReplayWinner`] are small, closed
//! sets and stay exhaustive on purpose; the inconsistency within one module
//! is deliberate, not an oversight.
//!
//! # Submodules
//!
//! - `schema` - the persisted wire format: every `struct`/`enum` and field
//!   name that appears on disk, plus the safety-limit and schema-version
//!   constants, and the accessors over them. Read this first to see *what* a
//!   Replay is. [`EndReason`] and [`TimeoutCause`] are defined here even
//!   though [`crate::authority`] is what produces them, because they are
//!   persisted: every `#[serde]` name in `schema` is the on-disk contract,
//!   and keeping the persisted enums together keeps that contract visible.
//! - `compat` - the hand-written per-version loader behind
//!   [`Replay::from_json`]/[`Replay::from_value`] and its inverse,
//!   [`Replay::to_storage_value`]. This is where an older schema version
//!   gets accepted (or a newer one written back down to an older shape); the
//!   version-by-version differences live here, not scattered through
//!   `schema`. Because `Replay` implements neither serde trait, both
//!   directions run through a private shadow struct here - one per
//!   direction, and the reason neither shape leaks out as a derive on the
//!   public type.
//! - `validation` - [`Replay::validate`], the single gate every Replay passes
//!   through before reaching the UI or a consumer. It re-derives ordinary
//!   rounds through [`crate::rules::resolve_round`] wherever the persisted
//!   data allows, so a forged or corrupted file cannot claim an impossible
//!   transition. It is a directory, split by how much context a check needs -
//!   one frame alone, one frame against its predecessor, the Round 0
//!   timeline, or how the Match ended - and its own module docs say which
//!   submodule owns which.
//! - `recording` - `MatchRecorder` turns live gameplay events into a
//!   `Replay`. Pure data assembly: the local wall-clock text a default
//!   label carries is passed in already formatted rather than produced
//!   here. Its module declaration is still gated
//!   `#[cfg(any(feature = "client", test))]`, because it is crate-private
//!   and a headless non-test build has no caller for it - see its own docs.
//! - `playback` - [`PlaybackController`] (the transport clock) and `Replay`'s
//!   frame-seeking and audio-cue methods.
//!
//! Every submodule here is headless, and so is every one of their
//! `#[cfg(test)] mod tests` blocks: `cargo test --no-default-features` runs
//! them. Everything a Replay needs a storage backend, a wall clock, or Bevy
//! for - `ReplayLibrary`, `ReplayCatalogEntry`, search and sort, local
//! timestamp formatting - is in `replay_library`, behind the `client`
//! feature, and reaches this module's types the same way any other consumer
//! does. Shared test fixtures live in `test_support`; the mock storage
//! backends that used to sit beside them are in
//! `replay_library::test_support`, since implementing
//! `platform::ReplayStorage` needs `client`.
//!
//! # Frames, not simulation
//!
//! Every recorded [`ReplayEvent`] carries a full [`ReplayFrame`] snapshot
//! taken *after* the change. Playback therefore never re-runs
//! [`crate::rules`]: it seeks to a time and draws the last frame at or before
//! it. That makes seeking exact and immune to rule changes, at the cost of a
//! larger file.
//!
//! # Audio during playback
//!
//! Sound is recorded as explicit [`ReplayEventKind::MatchSound`] events, and
//! [`PlaybackController::advance_with_cues`] only reports cues for the
//! interval actually played through. Seeking, jumping and rewinding move the
//! position without collecting cues, so scrubbing is silent instead of
//! replaying a burst of stale sounds.
//!
//! # Persistence and compatibility
//!
//! Native files are written atomically (temp file plus rename). Browser
//! records live in one versioned catalog replaced by a single
//! `localStorage.setItem`; the browser keeps only the newest three completed
//! Replays. Loading is deliberately tolerant: an unreadable or invalid
//! record is skipped and reported rather than hiding every other Replay.
//! Size, event-count, timestamp, UUID, and domain validation run before a
//! Replay reaches the UI - including a Replay loaded lazily through
//! `replay_library::ReplayLibrary::load_full` when a player opens it,
//! exactly the same validation a startup catalog rebuild applies. (That
//! persistence layer is the client's; this module defines the format and
//! the validation it must satisfy, and knows nothing about where the bytes
//! come from.)
//!
//! The schema-version constants record what each version introduced:
//! [`LEGACY_REPLAY_SCHEMA_VERSION`] (v1) is the oldest that loads;
//! [`ROUND_TRACKING_REPLAY_SCHEMA_VERSION`] (v2) persisted round numbers;
//! [`EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION`] (v3) recorded sounds as events;
//! [`OPENING_REPLAY_SCHEMA_VERSION`] (v4) added the explicit Round 0 phases,
//! bounded opening windows, countdown stages, and reveal events;
//! [`MATCH_RULES_REPLAY_SCHEMA_VERSION`] (v5) added the immutable Match
//! settings, explicit end reasons, timeout causes, capture deltas, no-stone
//! streaks, and the ordinary-round clock; [`COMPACT_BOARD_REPLAY_SCHEMA_VERSION`]
//! (v6, the current [`REPLAY_SCHEMA_VERSION`]) omits an unchanged board from
//! disk. Schemas v1-v3 are not retroactively given an opening; schemas v1-v4
//! use disabled rule defaults and [`EndReason::Legacy`] as their end reason
//! because the old files did not persist enough information to prove why
//! play ended. A loaded Replay keeps its own version, and the accessors say
//! what they report below the version that introduced them.
//!
//! [`REPLAY_SCHEMA_VERSION`] and every `#[serde]` field name under `schema`
//! are part of the on-disk format. Renaming a field, reordering an enum's
//! variants, or changing the version silently invalidates existing replays -
//! and, now that the same names are a Rust API, breaks downstream code too.

mod compat;
mod playback;
mod schema;
mod validation;

// Pure data assembly, but a headless build has no caller for it - see
// `recording`'s own module docs for why `MatchRecorder` stays crate-private
// and what that costs.
#[cfg(any(feature = "client", test))]
mod recording;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod measurement;

// The headless public API. Everything a consumer needs to load, validate,
// inspect, serialize, and play back a Replay is reachable from here; see
// the module docs above for the tour and `schema` for why fields stay
// private behind these types.
pub use playback::{PLAYBACK_SPEEDS, PlaybackController};
pub use schema::{
    COMPACT_BOARD_REPLAY_SCHEMA_VERSION, EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION, EndReason,
    LEGACY_REPLAY_SCHEMA_VERSION, LabelError, MATCH_RULES_REPLAY_SCHEMA_VERSION,
    MAX_REPLAY_DURATION_SECONDS, MAX_REPLAY_EVENTS, MAX_REPLAY_JSON_BYTES,
    OPENING_REPLAY_SCHEMA_VERSION, REPLAY_LABEL_RULE, REPLAY_SCHEMA_VERSION,
    ROUND_TRACKING_REPLAY_SCHEMA_VERSION, Replay, ReplayError, ReplayEvent, ReplayEventKind,
    ReplayFrame, ReplayPhase, ReplayWinner, TimeoutCause, validate_label,
};

#[cfg(any(feature = "client", test))]
pub(crate) use recording::MatchRecorder;
// Nothing in production builds a default label outside `recording` itself;
// this path exists for `replay_library`'s test that the real local-timestamp
// text keeps the longest possible label inside the label rule.
#[cfg(all(feature = "client", test))]
pub(crate) use recording::default_replay_label;

// Headless helpers whose only consumers are the client's UI and storage
// adapters. The items compile in every configuration; only this path is
// gated, so a headless build - where nothing imports them through it -
// reports no unused re-export.
#[cfg(feature = "client")]
pub(crate) use {schema::is_printable_ascii, validation::canonical_replay_id};
