//! Backward-compatible loading and on-disk serialization of [`Replay`].
//!
//! `Replay` itself implements neither [`Deserialize`] nor [`serde::Serialize`],
//! so both directions run through a private shadow struct here, and the two
//! are a matched pair:
//!
//! - [`ReplayFile`] is the permissive shadow used only to deserialize
//!   whatever schema version is on disk; the real compatibility logic - which
//!   fields are required or defaulted per version - lives in
//!   [`deserialize_replay`] below. It is a function rather than a
//!   `Deserialize` impl on `Replay` on purpose: the only ways to reach it are
//!   [`Replay::from_json`] and [`Replay::from_value`], which validate what it
//!   produces before handing it out, and the crate-internal
//!   [`Replay::parse_value`], for the one adapter that checks a record's key
//!   against the parsed ID before validating. A `Replay` therefore never
//!   exists outside this crate without having passed [`Replay::validate`].
//! - [`ReplayInMemory`] is the borrowing shadow that carries the `Serialize`
//!   derive. Its output is the complete in-memory shape, which is *not* what
//!   a Replay file holds, so it is not a public impl on `Replay`: outside
//!   this crate the only way to serialize a Replay is
//!   [`Replay::to_storage_value`] (or [`Replay::to_storage_json`]), the
//!   inverse of loading, which starts from that shape and strips the fields a
//!   given version never had, so re-saving an old Replay preserves its exact
//!   historical shape.

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use super::schema::{
    COMPACT_BOARD_REPLAY_SCHEMA_VERSION, EndReason, MATCH_RULES_REPLAY_SCHEMA_VERSION,
    MAX_REPLAY_JSON_BYTES, OPENING_REPLAY_SCHEMA_VERSION, ROUND_TRACKING_REPLAY_SCHEMA_VERSION,
    Replay, ReplayError, ReplayEvent,
};
use super::validation::validate_end_reasons;
use crate::match_settings::MatchSettings;
use crate::rules::{Board, Score};

/// Replaces an event's `frame.board` with `null` whenever it is identical to
/// the previous event's board. The inverse of [`resolve_compact_event_boards`];
/// see [`COMPACT_BOARD_REPLAY_SCHEMA_VERSION`] for why.
///
/// Only ever called for [`COMPACT_BOARD_REPLAY_SCHEMA_VERSION`] and later;
/// silently leaves an event's board alone if `frame` or `board` is missing or
/// malformed; the compat/validation loaders that run before or after this
/// (depending on direction) are what reject malformed data.
pub(crate) fn compact_event_boards(events: &mut [serde_json::Value]) {
    let mut last_board: Option<serde_json::Value> = None;
    for event in events.iter_mut() {
        let Some(frame) = event
            .get_mut("frame")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        let Some(board) = frame.get("board").cloned() else {
            continue;
        };
        if last_board.as_ref() == Some(&board) {
            frame.insert("board".to_string(), serde_json::Value::Null);
        } else {
            last_board = Some(board);
        }
    }
}

/// Restores every `null` `frame.board` (see [`compact_event_boards`]) to the
/// last non-null board seen, in event order.
///
/// Returns a plain error message (wrapped by the caller into whichever error
/// type its context needs) rather than panicking, since the input is
/// untrusted on-disk or browser-storage data: a `null` board with nothing
/// preceding it - most importantly on the very first event - is exactly the
/// kind of malformed input this must reject cleanly instead of unwrapping.
pub(crate) fn resolve_compact_event_boards(events: &mut [serde_json::Value]) -> Result<(), String> {
    let mut last_board: Option<serde_json::Value> = None;
    for event in events.iter_mut() {
        let frame = event
            .get_mut("frame")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("Replay event is missing its frame")?;
        match frame.get("board") {
            None | Some(serde_json::Value::Null) => {
                let board = last_board
                    .clone()
                    .ok_or("Replay's first event must include a complete board")?;
                frame.insert("board".to_string(), board);
            }
            Some(board) => {
                last_board = Some(board.clone());
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayFile {
    version: u32,
    id: String,
    label: String,
    started_at_ms: i64,
    completed_at_ms: i64,
    duration_seconds: f64,
    score: crate::rules::Score,
    #[serde(default)]
    round_count: usize,
    #[serde(default)]
    opening_completed: bool,
    #[serde(default)]
    match_settings: Option<serde_json::Value>,
    #[serde(default)]
    end_reasons: Option<Vec<EndReason>>,
    final_board: crate::rules::Board,
    events: Vec<serde_json::Value>,
}

/// The serializing shadow of [`Replay`]: the complete in-memory shape, and
/// the counterpart of [`ReplayFile`] above.
///
/// It exists so that `Replay` need not implement [`Serialize`] itself. The
/// derived shape is every field of every frame, with no board compaction and
/// with fields a v1-v4 file never had - the input
/// [`Replay::to_storage_value`] edits down to a version's real on-disk shape,
/// and wrong to write anywhere as it stands. Keeping it here keeps it
/// crate-private, the way the loading half already is.
///
/// It borrows rather than clones because the event list is the bulk of a
/// Replay and serializing must not double it.
///
/// The field names below are the on-disk contract, so this must not drift
/// from `Replay`. Two things stop it: [`ReplayInMemory::from`] destructures
/// `Replay` exhaustively, so a new or renamed field there fails to compile
/// until it is mirrored here, and
/// `tests::the_in_memory_shadow_matches_replay_field_for_field` pins the
/// emitted key set and round-trips the result back through [`ReplayFile`],
/// whose `deny_unknown_fields` rejects a name only one of the two knows.
#[derive(Serialize)]
struct ReplayInMemory<'a> {
    version: u32,
    id: &'a str,
    label: &'a str,
    started_at_ms: i64,
    completed_at_ms: i64,
    duration_seconds: f64,
    score: Score,
    round_count: usize,
    opening_completed: bool,
    match_settings: &'a MatchSettings,
    end_reasons: &'a [EndReason],
    final_board: &'a Board,
    events: &'a [ReplayEvent],
}

impl<'a> From<&'a Replay> for ReplayInMemory<'a> {
    fn from(replay: &'a Replay) -> Self {
        // Destructured rather than field-accessed on purpose: this is the
        // compile-time half of the guard described above, and it only works
        // as long as the pattern stays exhaustive - do not add `..` to it.
        let Replay {
            version,
            id,
            label,
            started_at_ms,
            completed_at_ms,
            duration_seconds,
            score,
            round_count,
            opening_completed,
            match_settings,
            end_reasons,
            final_board,
            events,
        } = replay;
        Self {
            version: *version,
            id,
            label,
            started_at_ms: *started_at_ms,
            completed_at_ms: *completed_at_ms,
            duration_seconds: *duration_seconds,
            score: *score,
            round_count: *round_count,
            opening_completed: *opening_completed,
            match_settings,
            end_reasons,
            final_board,
            events,
        }
    }
}

/// Decodes any supported schema version into a [`Replay`] without validating
/// it - the body of what used to be `Replay`'s `Deserialize` impl, kept as a
/// private function so the unvalidated value cannot escape this module
/// except through the callers named in the module docs.
fn deserialize_replay<'de, D>(deserializer: D) -> Result<Replay, D::Error>
where
    D: Deserializer<'de>,
{
    {
        let mut file = ReplayFile::deserialize(deserializer)?;
        if file.version >= COMPACT_BOARD_REPLAY_SCHEMA_VERSION {
            resolve_compact_event_boards(&mut file.events).map_err(D::Error::custom)?;
        }
        let (match_settings, end_reasons) = if file.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
            let settings = file
                .match_settings
                .ok_or_else(|| D::Error::custom("Replay v5 is missing match_settings"))
                .and_then(|value| {
                    MatchSettings::from_replay_value_strict(value).map_err(D::Error::custom)
                })?;
            let reasons = file
                .end_reasons
                .ok_or_else(|| D::Error::custom("Replay v5 is missing end_reasons"))?;
            validate_end_reasons(file.version, &reasons).map_err(D::Error::custom)?;
            (settings, reasons)
        } else {
            if let Some(value) = file.match_settings {
                let settings =
                    MatchSettings::from_replay_value_strict(value).map_err(D::Error::custom)?;
                if settings != MatchSettings::default() {
                    return Err(D::Error::custom(
                        "legacy Replay contains enabled v5 match settings",
                    ));
                }
            }
            if file
                .end_reasons
                .is_some_and(|reasons| reasons != [EndReason::Legacy])
            {
                return Err(D::Error::custom(
                    "legacy Replay contains non-legacy end metadata",
                ));
            }
            (MatchSettings::default(), vec![EndReason::Legacy])
        };
        let events = file
            .events
            .into_iter()
            .map(|value| {
                if file.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
                    let frame = value
                        .get("frame")
                        .and_then(serde_json::Value::as_object)
                        .ok_or_else(|| D::Error::custom("Replay v5 event is missing its frame"))?;
                    for field in ["capture_totals", "no_stone_streak", "round_elapsed_seconds"] {
                        if !frame.contains_key(field) {
                            return Err(D::Error::custom(format!(
                                "Replay v5 frame is missing {field}"
                            )));
                        }
                    }
                }
                serde_json::from_value(value).map_err(D::Error::custom)
            })
            .collect::<Result<Vec<ReplayEvent>, D::Error>>()?;
        Ok(Replay {
            version: file.version,
            id: file.id,
            label: file.label,
            started_at_ms: file.started_at_ms,
            completed_at_ms: file.completed_at_ms,
            duration_seconds: file.duration_seconds,
            score: file.score,
            round_count: file.round_count,
            opening_completed: file.opening_completed,
            match_settings,
            end_reasons,
            final_board: file.final_board,
            events,
        })
    }
}

impl Replay {
    /// Loads a Replay of any supported schema version from its JSON text and
    /// validates it - the way to accept Replay bytes from a file, a request,
    /// or anywhere else outside this process.
    ///
    /// Rejects text longer than [`MAX_REPLAY_JSON_BYTES`] before parsing it
    /// ([`ReplayError::DataTooLarge`]), text that is not the JSON shape of
    /// any supported version ([`ReplayError::Json`]), and a Replay that
    /// parses but fails [`Replay::validate`] (its error). The Replay keeps
    /// the schema version it was recorded with; see the version constants
    /// in [`crate::replay`].
    ///
    /// Available on every target. The native storage adapter is its only
    /// in-crate caller (the browser adapter holds records as
    /// [`serde_json::Value`]s and goes through the crate-internal
    /// `parse_value` so it can check a record's key before validating), but
    /// a consumer's need to parse a Replay does not depend on its platform.
    ///
    /// # Examples
    ///
    /// ```
    /// use go_together::replay::{REPLAY_SCHEMA_VERSION, Replay};
    ///
    /// let json = std::fs::read_to_string(concat!(
    ///     env!("CARGO_MANIFEST_DIR"),
    ///     "/tests/fixtures/replays/v6.json"
    /// ))?;
    /// let replay = Replay::from_json(&json)?;
    /// assert_eq!(replay.version(), REPLAY_SCHEMA_VERSION);
    /// assert_eq!(replay.to_storage_json()?, json);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_json(json: &str) -> Result<Self, ReplayError> {
        if json.len() > MAX_REPLAY_JSON_BYTES {
            return Err(ReplayError::DataTooLarge {
                item: "Replay",
                max_bytes: MAX_REPLAY_JSON_BYTES,
            });
        }
        // The same steps `serde_json::from_str` takes, spelled out because
        // `Replay` deliberately has no `Deserialize` impl to hand it.
        let mut deserializer = serde_json::Deserializer::from_str(json);
        let replay = deserialize_replay(&mut deserializer).map_err(ReplayError::Json)?;
        deserializer.end().map_err(ReplayError::Json)?;
        replay.validate()?;
        Ok(replay)
    }

    /// Loads a Replay of any supported schema version from an already-parsed
    /// JSON value and validates it - for a Replay embedded in a larger
    /// document, as the browser catalog stores them.
    ///
    /// The value is already in memory, so there is no size check here; a
    /// caller that parsed untrusted bytes into the value is the layer that
    /// bounded them. Otherwise identical to [`Replay::from_json`].
    pub fn from_value(value: serde_json::Value) -> Result<Self, ReplayError> {
        let replay = Self::parse_value(value).map_err(ReplayError::Json)?;
        replay.validate()?;
        Ok(replay)
    }

    /// Decodes a value without validating it. Crate-internal: the browser
    /// adapter checks a record's key against the parsed ID before it
    /// validates, and its warnings name which of the two failed - see
    /// `crate::platform::browser_storage`. Every other caller wants
    /// [`Replay::from_value`].
    pub(crate) fn parse_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        deserialize_replay(value)
    }

    /// Serializes to the on-disk/browser record shape for this Replay's own
    /// stored schema version - as a JSON value, for embedding a Replay in a
    /// larger document the way the browser catalog does. With
    /// [`Self::to_storage_json`], one of the two ways to serialize a
    /// `Replay`: the type has no [`Serialize`] impl of its own, because the
    /// derived one would be the in-memory shape this starts from and edits
    /// down. Serializing does not validate - see [`Self::validate`] for which
    /// layer owns that - because a `Replay` reaching this method has already
    /// been validated, on load or before it is written.
    pub fn to_storage_value(&self) -> Result<serde_json::Value, ReplayError> {
        let mut value = self.to_in_memory_value().map_err(ReplayError::Json)?;
        let replay = value
            .as_object_mut()
            .expect("Replay always serializes as a JSON object");

        if self.version < ROUND_TRACKING_REPLAY_SCHEMA_VERSION {
            replay.remove("round_count");
        }
        if self.version < OPENING_REPLAY_SCHEMA_VERSION {
            replay.remove("opening_completed");
        }
        if self.version < MATCH_RULES_REPLAY_SCHEMA_VERSION {
            replay.remove("match_settings");
            replay.remove("end_reasons");
        }

        let events = replay
            .get_mut("events")
            .and_then(serde_json::Value::as_array_mut)
            .expect("Replay events always serialize as a JSON array");
        for event in events {
            let frame = event
                .get_mut("frame")
                .and_then(serde_json::Value::as_object_mut)
                .expect("Replay frames always serialize as JSON objects");
            if self.version < ROUND_TRACKING_REPLAY_SCHEMA_VERSION {
                frame.remove("current_round");
            }
            if self.version < OPENING_REPLAY_SCHEMA_VERSION {
                frame.remove("opening_selections");
                frame.remove("phase");
                frame.remove("opening_countdown");
            }
            if self.version < MATCH_RULES_REPLAY_SCHEMA_VERSION {
                frame.remove("capture_totals");
                frame.remove("no_stone_streak");
                frame.remove("round_elapsed_seconds");
                if let Some(kind) = event
                    .get_mut("kind")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    kind.remove("captures");
                    kind.remove("submitted");
                    kind.remove("cause");
                }
            }
        }
        if self.version >= COMPACT_BOARD_REPLAY_SCHEMA_VERSION {
            let events = value
                .get_mut("events")
                .and_then(serde_json::Value::as_array_mut)
                .expect("Replay events always serialize as a JSON array");
            compact_event_boards(events);
        }
        Ok(value)
    }

    /// [`Self::to_storage_value`] as the compact text a native Replay file
    /// holds: the exact bytes `from_json` reads back, byte-for-byte stable
    /// for a Replay that has not been renamed.
    ///
    /// Compact rather than pretty-printed because the whitespace was not
    /// free: pretty-printing roughly doubled a file, and
    /// [`MAX_REPLAY_JSON_BYTES`] is enforced on the written text, so the
    /// indentation alone decided whether a long Match could be saved at all -
    /// see [Replay size](../../docs/ARCHITECTURE.md#replay-size). This is the
    /// same encoding the browser adapter already stores a record in, so both
    /// per-replay caps now apply to one encoding.
    ///
    /// Changing it changed the bytes a shipped build writes, but not their
    /// meaning: the JSON *value* is identical, so a pretty-printed file an
    /// older build wrote still parses, and nothing about the schema moved.
    pub fn to_storage_json(&self) -> Result<String, ReplayError> {
        serde_json::to_string(&self.to_storage_value()?).map_err(ReplayError::Json)
    }

    /// The complete in-memory shape as a JSON value, through
    /// [`ReplayInMemory`] - what [`Self::to_storage_value`] edits down, and
    /// deliberately crate-internal, since it is not what a Replay file holds.
    ///
    /// Its other callers are all tests: the fixture helpers in
    /// `replay::test_support` and the schema-version tests across
    /// `replay::validation` and the storage adapters build an older version's
    /// JSON by starting from this shape and removing what that version never
    /// had. That is the in-memory shape's legitimate use, and why the crate
    /// keeps a route to it after removing the public one.
    pub(crate) fn to_in_memory_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(ReplayInMemory::from(self))
    }
}

// These tests run in every configuration, including
// `cargo test --no-default-features`: the fixtures they build come from
// `replay::test_support`, which is headless. `checked_in_fixtures_match_their_builders`
// below still needs a filesystem, but not the `client` feature - it reads
// the checked-in files under `tests/fixtures/replays/` that
// `tests/headless_replay.rs` then loads through the public API alone. See
// `docs/ARCHITECTURE.md`'s "Headless core and Bevy client" section.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue::GameCue;
    use crate::replay::test_support::{
        assert_storage_schema, frame, literal_replay_value, match_timeout_replay, point,
        replay_with_audio_track, round_timeout_replay, round_zero_replay, sample_replay,
        sequential_new_stone_capture_replay, suicide_timeout_replay, sweeping_reveal_replay,
    };
    use crate::replay::{
        LEGACY_REPLAY_SCHEMA_VERSION, MATCH_RULES_REPLAY_SCHEMA_VERSION,
        OPENING_REPLAY_SCHEMA_VERSION, REPLAY_SCHEMA_VERSION, ROUND_TRACKING_REPLAY_SCHEMA_VERSION,
        ReplayEventKind, ReplayFrame, ReplayPhase,
    };
    use crate::rules::{Player, ResolutionKind};

    /// `MatchRecorder::new` draws a random UUID, so a fixture's ID is pinned
    /// here or the same builder writes different bytes on every run. (The
    /// default label is deterministic - the recorder is handed
    /// `TEST_STARTED_AT_TEXT` rather than formatting a local time zone - but
    /// these fixtures still name themselves for what they demonstrate.)
    fn with_id_and_label(mut replay: Replay, sequence: u64, label: &str) -> Replay {
        replay.id = format!("00000000-0000-4000-8000-{sequence:012}");
        replay.label = label.to_string();
        replay
    }

    /// A schema v2 Replay whose sounds must be derived from its events -
    /// the same shape `replay::validation`'s legacy-audio test builds - so a
    /// headless consumer can prove the derived track, not just the recorded
    /// one.
    fn legacy_audio_replay_v2() -> Replay {
        let mut replay = replay_with_audio_track();
        replay.version = ROUND_TRACKING_REPLAY_SCHEMA_VERSION;
        replay.end_reasons = vec![EndReason::Legacy];
        for event in &mut replay.events {
            event.frame.phase = ReplayPhase::Playing;
        }
        replay
            .events
            .retain(|event| !matches!(event.kind, ReplayEventKind::MatchSound { .. }));
        replay.events.insert(
            1,
            ReplayEvent {
                at_seconds: 0.5,
                kind: ReplayEventKind::JumpToMarker {
                    player: Player::One,
                    from: point(1, 1),
                    to: point(2, 2),
                },
                frame: frame(),
            },
        );
        replay.events.insert(
            2,
            ReplayEvent {
                at_seconds: 1.0,
                kind: ReplayEventKind::RoundResolved {
                    resolution: ResolutionKind::SamePointGray,
                    captures: [0, 0],
                },
                frame: ReplayFrame {
                    current_round: 2,
                    ..frame()
                },
            },
        );
        replay.round_count = 1;
        for event in replay
            .events
            .iter_mut()
            .filter(|event| event.at_seconds >= 1.0)
        {
            event.frame.current_round = 2;
        }
        assert_eq!(
            replay.audio_cues_between(0.0, replay.duration_seconds),
            vec![
                GameCue::BlackTeleport,
                GameCue::SamePointGray,
                GameCue::MatchEnd
            ]
        );
        replay
    }

    /// `round_zero_replay` with a recorded sound at time zero - the same
    /// insertion `replay::playback`'s Round 0 audio test makes - so the
    /// "an interval starting at zero includes cues at zero" rule of
    /// `Replay::audio_cues_between` is provable headlessly.
    fn round_zero_replay_with_sound_at_zero() -> Replay {
        let mut replay = round_zero_replay();
        replay.events.insert(
            2,
            ReplayEvent {
                at_seconds: 0.0,
                kind: ReplayEventKind::MatchSound {
                    cue: GameCue::OpeningDuplicate,
                },
                frame: replay.events[1].frame.clone(),
            },
        );
        replay
    }

    /// Every checked-in fixture under `tests/fixtures/replays/`, paired with
    /// the builder that produces it.
    fn fixture_replays() -> Vec<(String, Replay)> {
        let mut fixtures = Vec::new();
        for version in LEGACY_REPLAY_SCHEMA_VERSION..=REPLAY_SCHEMA_VERSION {
            let mut value = literal_replay_value(version);
            value["id"] = serde_json::json!(format!(
                "00000000-0000-4000-8000-{:012}",
                u64::from(version)
            ));
            let replay = Replay::from_value(value).unwrap();
            assert_eq!(replay.version, version);
            fixtures.push((format!("v{version}.json"), replay));
        }
        // `replay_with_audio_track` itself is a playback-only fixture (its
        // sound events carry no round clock, which v5+ validation rejects),
        // so it appears here only as the base of the v2 shape above, where
        // no round clock exists to check; `round_zero_replay` carries the
        // recorded cues a v6 fixture needs.
        for (name, sequence, label, replay) in [
            (
                "v2-legacy-audio.json",
                102,
                "Legacy audio",
                legacy_audio_replay_v2(),
            ),
            (
                "v6-round-zero.json",
                106,
                "Round zero",
                round_zero_replay_with_sound_at_zero(),
            ),
            (
                "v6-round-timeout.json",
                116,
                "Round timeout",
                round_timeout_replay(),
            ),
            (
                "v6-match-timeout.json",
                126,
                "Match timeout",
                match_timeout_replay(),
            ),
            (
                "v6-sequential-capture.json",
                136,
                "Sequential capture",
                sequential_new_stone_capture_replay(),
            ),
            (
                "v6-suicide-timeout.json",
                146,
                "Suicide timeout",
                suicide_timeout_replay(),
            ),
            (
                "v6-sweeping-reveal.json",
                156,
                "Sweeping reveal",
                sweeping_reveal_replay(),
            ),
        ] {
            let replay = with_id_and_label(replay, sequence, label);
            replay.validate().unwrap();
            fixtures.push((name.to_string(), replay));
        }
        fixtures
    }

    /// The fixtures under `tests/fixtures/replays/` are what
    /// `tests/headless_replay.rs` loads through the public API under
    /// `cargo test --no-default-features`, one per schema version plus the
    /// richer shapes above. Each is pinned here to the builder that produced
    /// it, so a fixture cannot drift from the crate silently, and a writer
    /// change that alters the bytes of an existing schema version fails here
    /// first - which is the point: those bytes are the on-disk contract.
    ///
    /// `GO_TOGETHER_UPDATE_REPLAY_FIXTURES=1 cargo test checked_in_fixtures`
    /// rewrites them. Do that only when a *builder* was deliberately changed,
    /// never to make a writer change pass; see the README beside them.
    #[test]
    fn checked_in_fixtures_match_their_builders() {
        let update = std::env::var("GO_TOGETHER_UPDATE_REPLAY_FIXTURES").as_deref() == Ok("1");
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replays");
        let mut mismatches = Vec::new();
        for (name, replay) in fixture_replays() {
            let expected = replay.to_storage_json().unwrap();
            let path = directory.join(&name);
            if update && std::fs::read_to_string(&path).ok().as_deref() != Some(&expected) {
                std::fs::write(&path, &expected).unwrap();
                eprintln!("rewrote {}", path.display());
            }
            let actual = std::fs::read_to_string(&path).unwrap_or_default();
            if actual != expected {
                mismatches.push(name);
            }
        }
        assert!(
            mismatches.is_empty(),
            "fixtures differ from their builders: {mismatches:?}. If a builder was \
             deliberately changed, rerun with GO_TOGETHER_UPDATE_REPLAY_FIXTURES=1; if \
             the writer changed, that is an on-disk format change to reconsider."
        );
    }

    /// `ReplayInMemory` mirrors `Replay`'s fields by hand, and its field
    /// names are the on-disk contract every version's bytes are built from,
    /// so the two must not drift apart. `ReplayInMemory::from` destructures
    /// `Replay` exhaustively, which fails to compile if a field is added to
    /// or renamed on `Replay`; this covers the other direction - a field
    /// renamed or dropped on the shadow alone - by pinning the key set it
    /// emits and reading the result back through `ReplayFile`, which requires
    /// every field it does not default and rejects unknown ones.
    #[test]
    fn the_in_memory_shadow_matches_replay_field_for_field() {
        let mut replay = sample_replay(1, 2, "Shadow");
        // `round_count` and `opening_completed` are the only fields
        // `ReplayFile` defaults that loading this version does not then
        // require anyway - `match_settings` and `end_reasons` also default,
        // but a v5-or-later load rejects their absence - so they are the only
        // two the round trip below could lose without noticing; non-default
        // values make it notice. `parse_value` does not validate, so they
        // need not describe a coherent Match.
        replay.round_count = 7;
        replay.opening_completed = true;

        let value = replay.to_in_memory_value().unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("a Replay always serializes as a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "completed_at_ms",
                "duration_seconds",
                "end_reasons",
                "events",
                "final_board",
                "id",
                "label",
                "match_settings",
                "opening_completed",
                "round_count",
                "score",
                "started_at_ms",
                "version",
            ]
        );
        assert_eq!(Replay::parse_value(value).unwrap(), replay);
    }

    /// `sample_replay`'s middle event (a cursor move) leaves the board
    /// exactly as the first event left it, so it is the concrete case
    /// `compact_event_boards` exists for.
    #[test]
    fn an_unchanged_board_is_omitted_on_disk_and_the_first_and_last_are_not() {
        let replay = sample_replay(1, 2, "Compaction");
        assert_eq!(replay.version, REPLAY_SCHEMA_VERSION);
        let value = replay.to_storage_value().unwrap();
        let events = value["events"].as_array().unwrap();
        assert_eq!(events.len(), 3);
        assert!(!events[0]["frame"]["board"].is_null());
        assert!(
            events[1]["frame"]["board"].is_null(),
            "the cursor-move event's board is identical to MatchStart's"
        );
        assert!(!events[2]["frame"]["board"].is_null());
    }

    /// `to_storage_value` writes each version's own field set: nothing a
    /// version never had, and nothing it did have left out. The fixture
    /// guard above pins the exact bytes, but only against files that can be
    /// regenerated; this states the rule those files are supposed to follow
    /// independently of them.
    #[test]
    fn every_version_writes_exactly_its_own_field_set() {
        for version in LEGACY_REPLAY_SCHEMA_VERSION..=REPLAY_SCHEMA_VERSION {
            let mut replay = sample_replay(1, 2, &format!("Schema {version}"));
            replay.version = version;
            if version < MATCH_RULES_REPLAY_SCHEMA_VERSION {
                replay.end_reasons = vec![EndReason::Legacy];
            }
            if version < OPENING_REPLAY_SCHEMA_VERSION {
                for event in &mut replay.events {
                    event.frame.phase = ReplayPhase::Playing;
                }
            }
            assert_storage_schema(&replay.to_storage_value().unwrap(), version);
        }
    }

    /// Compaction is scoped to `COMPACT_BOARD_REPLAY_SCHEMA_VERSION` and
    /// later: a Replay saved under the older v5 wire format must never gain
    /// an omitted board, even though the same repeated board is present.
    #[test]
    fn older_schema_versions_never_omit_a_board() {
        let mut replay = sample_replay(1, 2, "Legacy shape");
        replay.version = MATCH_RULES_REPLAY_SCHEMA_VERSION;
        let value = replay.to_storage_value().unwrap();
        for event in value["events"].as_array().unwrap() {
            assert!(!event["frame"]["board"].is_null());
        }
    }

    #[test]
    fn existing_replay_labels_remain_unchanged() {
        let replay = sample_replay(1_704_067_200_000, 1_704_067_260_000, "My Final Match");
        let json = replay.to_storage_json().unwrap();
        let loaded = Replay::from_json(&json).unwrap();

        assert_eq!(loaded.label, "My Final Match");
    }

    /// Round-tripping through the real on-disk format - compaction, then
    /// resolution - must reproduce the exact original `Replay`, including
    /// every field of every event's frame, not just the board.
    #[test]
    fn compacted_replay_round_trips_through_storage_json_unchanged() {
        let replay = sample_replay(1, 2, "Round trip");
        let json = replay.to_storage_json().unwrap();
        let decoded = Replay::from_json(&json).unwrap();
        assert_eq!(decoded, replay);
    }

    /// The minimum valid Replay is exactly two events (`MatchStart` then
    /// `MatchEnd`; `validate` rejects anything shorter). When a Match ends
    /// with the board it started with, the *first* event must never be
    /// compacted away regardless - there is no earlier event to carry it
    /// forward from - even though the (also-identical) last event can be,
    /// and is.
    #[test]
    fn the_first_event_is_never_omitted_even_in_the_shortest_possible_replay() {
        let mut replay = sample_replay(1, 2, "Minimal");
        replay.events.truncate(2);
        replay.events[1].kind = ReplayEventKind::MatchEnd;
        replay.events[1].frame.board = replay.events[0].frame.board.clone();
        replay.events[1].frame.phase = ReplayPhase::Ended;
        replay.duration_seconds = replay.events[1].at_seconds;
        replay.final_board = replay.events[1].frame.board.clone();
        replay.score = replay.events[1].frame.score;

        let value = replay.to_storage_value().unwrap();
        assert!(!value["events"][0]["frame"]["board"].is_null());
        assert!(value["events"][1]["frame"]["board"].is_null());

        let decoded = Replay::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(decoded, replay);
    }

    /// The direction that matters for safety: loading untrusted data whose
    /// very first event omits its board (nothing to carry forward from) must
    /// fail cleanly, never panic or silently substitute a default board.
    #[test]
    fn a_missing_first_board_is_rejected_cleanly_not_panicked_on() {
        let replay = sample_replay(1, 2, "Corrupt");
        let mut value = replay.to_storage_value().unwrap();
        value["events"][0]["frame"]["board"] = serde_json::Value::Null;
        let json = serde_json::to_string(&value).unwrap();

        let result = Replay::from_json(&json);
        assert!(matches!(result, Err(ReplayError::Json(_))));
    }

    /// Same hazard, but with the board key removed entirely rather than set
    /// to `null` - both must be treated as "omitted" and both must fail
    /// safely on the first event.
    #[test]
    fn a_missing_board_key_on_the_first_event_is_also_rejected_cleanly() {
        let replay = sample_replay(1, 2, "Corrupt key");
        let mut value = replay.to_storage_value().unwrap();
        value["events"][0]["frame"]
            .as_object_mut()
            .unwrap()
            .remove("board");
        let json = serde_json::to_string(&value).unwrap();

        let result = Replay::from_json(&json);
        assert!(matches!(result, Err(ReplayError::Json(_))));
    }

    /// `resolve_compact_event_boards` must recover exactly the boards
    /// `compact_event_boards` omitted, for an arbitrary run of repeats -
    /// not just the two-event case `sample_replay` happens to produce.
    #[test]
    fn resolve_is_the_exact_inverse_of_compact_for_arbitrary_repeat_runs() {
        let boards: Vec<serde_json::Value> = (0..5)
            .map(|value| {
                let cells = vec![value; 81];
                serde_json::json!({ "cells": cells })
            })
            .collect();
        // Repeats: b0, b0, b1, b1, b1, b2, b3, b3, b4.
        let sequence = [0, 0, 1, 1, 1, 2, 3, 3, 4];
        let mut events: Vec<serde_json::Value> = sequence
            .iter()
            .map(|&index| serde_json::json!({"frame": {"board": boards[index].clone()}}))
            .collect();
        let original = events.clone();

        compact_event_boards(&mut events);
        // Every repeat after the first of its run must now be null.
        for (position, &index) in sequence.iter().enumerate() {
            let is_repeat = position > 0 && sequence[position - 1] == index;
            assert_eq!(events[position]["frame"]["board"].is_null(), is_repeat);
        }

        resolve_compact_event_boards(&mut events).unwrap();
        assert_eq!(events, original);
    }
}
