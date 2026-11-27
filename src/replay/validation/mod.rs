//! Structural and rules validation for [`Replay`] data - the single gate a
//! Replay must pass before it reaches the UI, whether just loaded from disk,
//! about to be written, or freshly recorded.
//!
//! [`Replay::validate`] lives here and is what a caller outside this
//! directory reaches for. It checks the schema version, the top-level
//! metadata and bounds, and the first and last event; then walks the
//! timeline once, handing each event to whichever submodule owns that kind
//! of check; then makes the few checks that only make sense once the walk
//! has finished. Two narrower items are also called from outside:
//! [`validate_end_reasons`], which `compat` applies while a file is still
//! being deserialized (see below), and [`canonical_replay_id`], which the
//! client's storage adapters apply to a file stem or record key before there
//! is a Replay to open.
//!
//! # Submodules
//!
//! The walk asks four different questions of an event, and each has a
//! different amount of context to answer with. That is where the boundaries
//! fall:
//!
//! - `frame` - whether one [`ReplayFrame`](crate::replay::ReplayFrame) or one
//!   [`ReplayEventKind`] is coherent on its own, judged with no reference to
//!   its neighbours.
//! - `transitions` - whether an event follows from the frame before it. This
//!   is where a v5+ Replay's ordinary rounds are re-derived through
//!   [`crate::rules::resolve_round`], so a forged or corrupted file cannot
//!   claim an impossible board transition, capture count, or timeout.
//! - `opening` - the Round 0 timeline, whose events are ordered against each
//!   other rather than against the board, and so are checked by a small
//!   state machine carried along the walk.
//! - `end_reasons` - how the match ended: the [`EndReason`] vocabulary and
//!   ordering, and the settings limits whose crossing makes one particular
//!   ending the only possible one.
//!
//! `frame` and `end_reasons` are leaves: they answer from what they are
//! handed and keep nothing between events. `transitions` and `opening` are
//! the two that need the walk's running state - the repetition history and
//! the Round 0 state machine - which this module owns and carries from event
//! to event. Everything a submodule exposes is `pub(super)` - visible here
//! and no further - so the split is invisible from outside. The one
//! exception is [`validate_end_reasons`], re-exported below because `compat`
//! checks a file's end reasons while deserializing it, before there is a
//! `Replay` to validate.
//!
//! Neither this split nor the earlier one that lifted this code out of
//! `schema` changed what is validated: both moved where the checks are
//! written down and nothing else.
//!
//! What is validated has moved once since, and this is the paragraph to
//! keep current when it does. The Round 0 reveal's expected board became
//! the *swept* one `MatchState::reveal_round_zero` records rather than the
//! raw six-slot combination, and from
//! `MATCH_RULES_REPLAY_SCHEMA_VERSION` the reveal frame's capture totals
//! must carry exactly that sweep's credits - see [`opening`]. Production
//! never wrote the raw board, so no file that used to load stopped
//! loading; what changed is that a sweeping reveal can now be represented
//! at all.

mod end_reasons;
mod frame;
mod opening;
mod transitions;

use std::collections::HashSet;

use uuid::Uuid;

use super::schema::{
    COMPACT_BOARD_REPLAY_SCHEMA_VERSION, EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION, EndReason,
    LEGACY_REPLAY_SCHEMA_VERSION, MATCH_RULES_REPLAY_SCHEMA_VERSION, MAX_REPLAY_DURATION_SECONDS,
    MAX_REPLAY_EVENTS, MAX_TIMESTAMP_MS, OPENING_REPLAY_SCHEMA_VERSION,
    ROUND_TRACKING_REPLAY_SCHEMA_VERSION, Replay, ReplayError, ReplayEvent, ReplayEventKind,
    ReplayPhase, TIME_EPSILON, validate_label,
};
use crate::opening::OPENING_GO_DISPLAY;
use crate::rules::ResolutionKind;

use end_reasons::{
    deterministic_completion_end_reasons, opening_match_time_deadline_reveal_index,
    validate_v5_end_metadata,
};
use opening::{OpeningValidationState, validate_opening_event};
use transitions::{validate_v5_frame_transition, validate_v5_round_clock};

pub(super) use end_reasons::validate_end_reasons;

impl Replay {
    /// Full structural and rules validation. Expensive - for a v5+ Replay it
    /// re-derives through [`crate::rules::resolve_round`] every ordinary
    /// round whose selections were recorded - so exactly one layer owns each
    /// call.
    ///
    /// # Who validates, and where
    ///
    /// * **Untrusted data entering the process** is validated once, at the
    ///   boundary where it enters: [`Replay::from_json`] for native files and
    ///   [`Replay::from_value`] for values embedded in a larger document,
    ///   both of which call this before returning; and the browser catalog's
    ///   `valid_replay` / `valid_replays` for `localStorage` records, which
    ///   parse, check the record key, then call this.
    /// * **Data this process just constructed** is validated once, at the top
    ///   of the write path: the Replay Library's `save_completed` and
    ///   `rename` (behind the `client` feature).
    /// * **A consumer of the headless library** never calls this itself: the
    ///   only ways to obtain a `Replay` outside this crate are the two
    ///   loading functions above, which already did, and [`Replay::rename`]
    ///   applies the same label rule this does. It is public so that a
    ///   consumer can see exactly what was checked, and so that a test can
    ///   prove a fixture still passes - not because anything a consumer can
    ///   hold is unvalidated.
    ///
    /// Everything below those layers - the platform storage adapters and
    /// [`Replay::to_storage_value`] - trusts what it is handed and does not
    /// re-validate. Adding a call anywhere else duplicates work that has
    /// already happened rather than closing a hole.
    ///
    /// # What is checked
    ///
    /// The schema version must be one this build knows
    /// ([`ReplayError::UnsupportedVersion`] otherwise - a Replay from a
    /// *newer* build is rejected, never partially read); the ID must be a
    /// canonical UUID; the label, timestamps, duration, and event count must
    /// be within their documented bounds; the timeline must begin with
    /// `MatchStart` at zero and end with `MatchEnd` at the duration, with the
    /// final frame matching the final board and score; and every event's
    /// frame must be consistent with its predecessor - for a v5+ Replay, by
    /// re-deriving each ordinary round through the rules wherever its
    /// selections were recorded, and by a looser board-delta bound where a
    /// selection checkpoint is missing, so a forged capture count, board
    /// transition, timeout, or end reason is rejected rather than displayed.
    pub fn validate(&self) -> Result<(), ReplayError> {
        self.validate_metadata()?;
        let mut previous = -1.0;
        let mut expected_round = usize::from(!self.opening_completed);
        let mut previous_event: Option<&ReplayEvent> = None;
        let mut opening_validation = OpeningValidationState::default();
        let opening_match_time_reveal = if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
            opening_match_time_deadline_reveal_index(self)
        } else {
            None
        };
        let final_ordinary_completion = self.events.iter().rposition(|event| {
            matches!(
                event.kind,
                ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
            )
        });
        let mut completed_rounds = 0;
        let mut automatic_end_validated = false;
        let mut round_started_at = (self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION
            && !self.opening_completed)
            .then_some(0.0);
        let mut completed_history = HashSet::new();
        if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION && !self.opening_completed {
            completed_history.insert(
                self.events
                    .first()
                    .expect("a validated Replay has at least one event")
                    .frame
                    .board
                    .clone(),
            );
        }
        for (event_index, event) in self.events.iter().enumerate() {
            if !event.at_seconds.is_finite()
                || event.at_seconds < previous
                || event.at_seconds < 0.0
                || event.at_seconds > self.duration_seconds + TIME_EPSILON
            {
                return Err(ReplayError::InvalidData(
                    "replay event timeline is invalid".into(),
                ));
            }
            event
                .frame
                .validate(self.version, self.duration_seconds, &self.match_settings)?;
            event.kind.validate()?;
            if self.version < MATCH_RULES_REPLAY_SCHEMA_VERSION
                && matches!(
                    event.kind,
                    ReplayEventKind::RoundTimedOut { .. }
                        | ReplayEventKind::RoundResolved {
                            resolution: ResolutionKind::NoPlacement | ResolutionKind::Single,
                            ..
                        }
                )
            {
                return Err(ReplayError::InvalidData(
                    "legacy replay contains a v5 timeout event".into(),
                ));
            }
            if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION
                && matches!(
                    event.kind,
                    ReplayEventKind::RoundResolved {
                        resolution: ResolutionKind::NoPlacement | ResolutionKind::Single,
                        ..
                    }
                )
            {
                return Err(ReplayError::InvalidData(
                    "normal round event contains timeout-only resolution".into(),
                ));
            }
            if self.version >= ROUND_TRACKING_REPLAY_SCHEMA_VERSION {
                if matches!(event.kind, ReplayEventKind::OpeningReveal) {
                    expected_round = 1;
                }
                if matches!(
                    event.kind,
                    ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
                ) {
                    expected_round += 1;
                }
                if event.frame.current_round != expected_round {
                    return Err(ReplayError::InvalidData(
                        "replay frame round does not match its timeline".into(),
                    ));
                }
            }
            if self.version >= OPENING_REPLAY_SCHEMA_VERSION {
                validate_opening_event(
                    event,
                    previous_event.map(|previous| &previous.frame),
                    self.opening_completed,
                    self.version,
                    opening_match_time_reveal == Some(event_index),
                    &mut opening_validation,
                )?;
            }
            if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
                validate_v5_round_clock(event, round_started_at)?;
                validate_v5_frame_transition(
                    self,
                    event_index,
                    previous_event,
                    round_started_at,
                    &completed_history,
                )?;
                let ordinary_completion = matches!(
                    event.kind,
                    ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
                );
                if ordinary_completion {
                    completed_rounds += 1;
                    let reasons =
                        deterministic_completion_end_reasons(self, event, completed_rounds);
                    if !reasons.is_empty() {
                        if final_ordinary_completion != Some(event_index) {
                            return Err(ReplayError::InvalidData(
                                "automatic match limit was crossed before the final ordinary completion"
                                    .into(),
                            ));
                        }
                        if reasons != self.end_reasons {
                            return Err(ReplayError::InvalidData(
                                "final automatic completion does not match replay end reasons"
                                    .into(),
                            ));
                        }
                        automatic_end_validated = true;
                    }
                }
                if matches!(event.kind, ReplayEventKind::OpeningReveal) || ordinary_completion {
                    round_started_at = Some(event.at_seconds);
                }
                if matches!(event.kind, ReplayEventKind::OpeningReveal) {
                    completed_history.clear();
                    completed_history.insert(event.frame.board.clone());
                } else if ordinary_completion {
                    completed_history.insert(event.frame.board.clone());
                }
            } else if event.kind.capture_delta() != [0, 0] {
                return Err(ReplayError::InvalidData(
                    "legacy replay contains v5 capture metadata".into(),
                ));
            }
            previous = event.at_seconds;
            previous_event = Some(event);
        }
        self.validate_ending(
            &opening_validation,
            automatic_end_validated,
            opening_match_time_reveal,
        )
    }

    /// The checks a Replay's own header and its first and last event
    /// answer between them: schema version, identity, label, timestamps,
    /// duration, event count, and the summary counters that must agree with
    /// the events they claim to summarise.
    ///
    /// These run before the walk because a Replay that fails one of them
    /// has no timeline worth walking.
    fn validate_metadata(&self) -> Result<(), ReplayError> {
        if !matches!(
            self.version,
            LEGACY_REPLAY_SCHEMA_VERSION
                | ROUND_TRACKING_REPLAY_SCHEMA_VERSION
                | EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION
                | OPENING_REPLAY_SCHEMA_VERSION
                | MATCH_RULES_REPLAY_SCHEMA_VERSION
                | COMPACT_BOARD_REPLAY_SCHEMA_VERSION
        ) {
            return Err(ReplayError::UnsupportedVersion(self.version));
        }
        validate_end_reasons(self.version, &self.end_reasons)?;
        canonical_replay_id(&self.id)?;
        validate_label(&self.label)
            .map_err(|_| ReplayError::InvalidData("invalid replay label".into()))?;
        if self.started_at_ms < 0
            || self.started_at_ms > MAX_TIMESTAMP_MS
            || self.completed_at_ms < self.started_at_ms
            || self.completed_at_ms > MAX_TIMESTAMP_MS
        {
            return Err(ReplayError::InvalidData("invalid replay timestamps".into()));
        }
        if !self.duration_seconds.is_finite()
            || self.duration_seconds < 0.0
            || self.duration_seconds > MAX_REPLAY_DURATION_SECONDS
        {
            return Err(ReplayError::InvalidData("invalid replay duration".into()));
        }
        if self.events.len() > MAX_REPLAY_EVENTS {
            return Err(ReplayError::InvalidData(format!(
                "Replay has more than {MAX_REPLAY_EVENTS} events"
            )));
        }
        let Some(first) = self.events.first() else {
            return Err(ReplayError::InvalidData("replay has no events".into()));
        };
        if first.kind != ReplayEventKind::MatchStart || f64::abs(first.at_seconds) > TIME_EPSILON {
            return Err(ReplayError::InvalidData(
                "replay must begin with match_start at 0".into(),
            ));
        }
        let Some(last) = self.events.last() else {
            unreachable!("events.first() above already proved events is non-empty");
        };
        if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
            validate_v5_end_metadata(self)?;
        }
        if last.kind != ReplayEventKind::MatchEnd
            || f64::abs(last.at_seconds - self.duration_seconds) > TIME_EPSILON
        {
            return Err(ReplayError::InvalidData(
                "replay must end at its completed duration".into(),
            ));
        }
        if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION
            && last.frame.phase != ReplayPhase::Ended
        {
            return Err(ReplayError::InvalidData(
                "Replay v5 final frame is not ended".into(),
            ));
        }
        if last.frame.board != self.final_board || last.frame.score != self.score {
            return Err(ReplayError::InvalidData(
                "final event does not match replay result".into(),
            ));
        }
        let final_captures = last.frame.capture_totals;
        if self
            .final_board
            .score_with_captures(final_captures, self.match_settings.capture_points())
            != self.score
        {
            return Err(ReplayError::InvalidData(
                "final score does not match the final board".into(),
            ));
        }
        let resolved_rounds = self
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
                )
            })
            .count();
        if self.version >= ROUND_TRACKING_REPLAY_SCHEMA_VERSION
            && self.round_count != resolved_rounds
        {
            return Err(ReplayError::InvalidData(
                "replay round count does not match its resolution events".into(),
            ));
        }
        let opening_reveals = self
            .events
            .iter()
            .filter(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))
            .count();
        let opening_starts = self
            .events
            .iter()
            .filter(|event| matches!(event.kind, ReplayEventKind::OpeningStarted))
            .count();
        if self.version >= OPENING_REPLAY_SCHEMA_VERSION
            && (self.opening_completed != (opening_reveals == 1)
                || self.opening_completed != (opening_starts == 1))
        {
            return Err(ReplayError::InvalidData(
                "replay opening metadata does not match its lifecycle events".into(),
            ));
        }
        if self.version < OPENING_REPLAY_SCHEMA_VERSION
            && (self.opening_completed || opening_reveals != 0 || opening_starts != 0)
        {
            return Err(ReplayError::InvalidData(
                "legacy replay contains Round 0 data".into(),
            ));
        }
        Ok(())
    }

    /// The checks that can only be made once the walk is over: that a
    /// Round 0 countdown reached its end, and that a Replay whose ending was
    /// not already pinned to a deterministic limit crossing carries the end
    /// reason its own chronology implies.
    fn validate_ending(
        &self,
        opening_validation: &OpeningValidationState,
        automatic_end_validated: bool,
        opening_match_time_reveal: Option<usize>,
    ) -> Result<(), ReplayError> {
        if self.opening_completed && !opening_validation.finished {
            let ended_before_go_cleared = self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION
                && opening_validation
                    .revealed_at_seconds
                    .is_some_and(|revealed_at| {
                        self.duration_seconds
                            < revealed_at + OPENING_GO_DISPLAY.as_secs_f64() - TIME_EPSILON
                    });
            if !ended_before_go_cleared {
                return Err(ReplayError::InvalidData(
                    "replay Round 0 countdown did not finish".into(),
                ));
            }
        }
        if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION && !automatic_end_validated {
            let expected_reasons = if opening_match_time_reveal.is_some() {
                [EndReason::MatchTimeLimit].as_slice()
            } else {
                [EndReason::Manual].as_slice()
            };
            if self.end_reasons != expected_reasons {
                return Err(ReplayError::InvalidData(
                    "replay end reasons do not match its chronological limits".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Parses `id` as a UUID and requires the canonical lower-case hyphenated
/// spelling, so one Replay cannot appear under two forms of its ID.
///
/// [`Replay::validate`] applies it to the ID inside a file; the client's
/// storage adapters also call it, through `crate::replay`'s `client`-gated
/// re-export, on a file stem or record key - the one check that runs before
/// there is a `Replay` to validate.
pub(crate) fn canonical_replay_id(id: &str) -> Result<Uuid, ReplayError> {
    let parsed = Uuid::parse_str(id).map_err(|_| ReplayError::InvalidId)?;
    if parsed.hyphenated().to_string() != id {
        return Err(ReplayError::InvalidId);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue::GameCue;
    use crate::match_settings::MatchSettings;
    use crate::opening::OpeningSelections;
    use crate::replay::test_support::*;
    use crate::replay::{
        MAX_REPLAY_JSON_BYTES, MatchRecorder, REPLAY_SCHEMA_VERSION, ReplayFrame, TimeoutCause,
    };
    use crate::rules::{Cell, Player, Score};

    #[test]
    fn persisted_labels_reject_non_ascii_and_control_characters() {
        for label in ["Café", "Line\nBreak", "Tabbed\tLabel", "Delete\u{7f}"] {
            let replay = sample_replay(1, 2, label);
            assert!(matches!(
                replay.validate(),
                Err(ReplayError::InvalidData(ref message)) if message == "invalid replay label"
            ));
        }
    }

    #[test]
    fn existing_version_one_replay_data_still_loads_with_start_timestamp() {
        let replay = sample_replay(1, 2, "Round Trip");
        let mut value = replay.to_in_memory_value().unwrap();
        value["version"] = serde_json::json!(LEGACY_REPLAY_SCHEMA_VERSION);
        strip_v5_fields(&mut value);
        strip_v4_frame_fields(&mut value);
        value.as_object_mut().unwrap().remove("round_count");
        for event in value["events"].as_array_mut().unwrap() {
            event["frame"]
                .as_object_mut()
                .unwrap()
                .remove("current_round");
        }

        let json = serde_json::to_string(&value).unwrap();
        let decoded = Replay::from_json(&json).unwrap();
        assert_eq!(decoded.version, LEGACY_REPLAY_SCHEMA_VERSION);
        assert_eq!(decoded.started_at_ms, 1);
        assert_eq!(decoded.completed_at_ms, 2);
        assert_eq!(decoded.completed_round_count(), 0);
        assert_eq!(decoded.frame_at(0.0).current_round, 1);

        let mut unsupported = decoded;
        unsupported.version = REPLAY_SCHEMA_VERSION + 1;
        assert!(matches!(
            unsupported.validate(),
            Err(ReplayError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn replay_limits_reject_oversized_json_event_counts_duration_and_timestamps() {
        let oversized = " ".repeat(MAX_REPLAY_JSON_BYTES + 1);
        assert!(matches!(
            Replay::from_json(&oversized),
            Err(ReplayError::DataTooLarge { .. })
        ));

        let mut too_many_events = sample_replay(1, 2, "Too Many Events");
        too_many_events.events = vec![too_many_events.events[0].clone(); MAX_REPLAY_EVENTS + 1];
        assert!(matches!(
            too_many_events.validate(),
            Err(ReplayError::InvalidData(ref error))
                if error.contains("more than 10000 events")
        ));

        let mut too_long = sample_replay(1, 2, "Too Long");
        too_long.duration_seconds = MAX_REPLAY_DURATION_SECONDS + 1.0;
        assert!(matches!(
            too_long.validate(),
            Err(ReplayError::InvalidData(ref error)) if error == "invalid replay duration"
        ));

        let mut impossible_time = sample_replay(1, 2, "Impossible Time");
        impossible_time.completed_at_ms = MAX_TIMESTAMP_MS + 1;
        assert!(matches!(
            impossible_time.validate(),
            Err(ReplayError::InvalidData(ref error)) if error == "invalid replay timestamps"
        ));
    }

    #[test]
    fn legacy_round_events_infer_current_and_completed_rounds_through_seeking() {
        let mut recorder = MatchRecorder::new(1, frame());
        let mut resolved = frame();
        resolved.current_round = 2;
        recorder.record(
            1.0,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::Sequential,
                captures: [0, 0],
            },
            resolved.clone(),
        );
        let replay = recorder.complete(2, 2.0, resolved).unwrap();
        let mut value = replay.to_in_memory_value().unwrap();
        value["version"] = serde_json::json!(LEGACY_REPLAY_SCHEMA_VERSION);
        strip_v5_fields(&mut value);
        strip_v4_frame_fields(&mut value);
        value.as_object_mut().unwrap().remove("round_count");
        for event in value["events"].as_array_mut().unwrap() {
            event["frame"]
                .as_object_mut()
                .unwrap()
                .remove("current_round");
        }

        let legacy = Replay::from_value(value).unwrap();
        assert_eq!(legacy.completed_round_count(), 1);
        assert_eq!(legacy.frame_at(0.0).current_round, 1);
        assert_eq!(legacy.frame_at(0.99).current_round, 1);
        assert_eq!(legacy.frame_at(1.0).current_round, 2);
        assert_eq!(legacy.frame_at(2.0).current_round, 2);
    }

    #[test]
    fn schema_v4_non_opening_fixture_persists_and_validates_round_timeline() {
        let mut recorder = MatchRecorder::new(1, frame());
        let mut resolved = frame();
        resolved.current_round = 2;
        recorder.record(
            1.0,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::SamePointGray,
                captures: [0, 0],
            },
            resolved.clone(),
        );
        let replay = recorder.complete(2, 2.0, resolved).unwrap();
        let mut value = replay.to_in_memory_value().unwrap();
        value["version"] = serde_json::json!(OPENING_REPLAY_SCHEMA_VERSION);
        strip_v5_fields(&mut value);
        let json = serde_json::to_string(&value).unwrap();
        let decoded = Replay::from_json(&json).unwrap();

        assert_eq!(decoded.version, OPENING_REPLAY_SCHEMA_VERSION);
        assert_eq!(decoded.end_reasons, vec![EndReason::Legacy]);
        assert_eq!(decoded.round_count, 1);
        assert_eq!(decoded.frame_at(0.0).current_round, 1);
        assert_eq!(decoded.frame_at(1.0).current_round, 2);
        assert!(json.contains("\"round_count\":1"));
        assert!(json.contains("\"current_round\":2"));
        assert!(!json.contains("\"captures\""));
    }

    #[test]
    fn version_two_replays_derive_best_effort_teleport_gray_and_end_audio() {
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
        replay.validate().unwrap();
    }

    #[test]
    fn schemas_v1_through_v3_load_without_fabricated_round_zero() {
        for version in 1..=3 {
            let replay = sample_replay(1, 2, "Legacy Replay");
            let mut value = replay.to_in_memory_value().unwrap();
            value["version"] = serde_json::json!(version);
            value.as_object_mut().unwrap().remove("opening_completed");
            value.as_object_mut().unwrap().remove("match_settings");
            value.as_object_mut().unwrap().remove("end_reasons");
            for event in value["events"].as_array_mut().unwrap() {
                let frame = event["frame"].as_object_mut().unwrap();
                frame.remove("opening_selections");
                frame.remove("phase");
                frame.remove("opening_countdown");
                frame.remove("capture_totals");
                frame.remove("no_stone_streak");
                frame.remove("round_elapsed_seconds");
                if version == 1 {
                    frame.remove("current_round");
                }
            }
            if version == 1 {
                value.as_object_mut().unwrap().remove("round_count");
            }

            let decoded = Replay::from_value(value).unwrap();
            assert!(!decoded.opening_completed);
            assert_eq!(decoded.match_settings, MatchSettings::default());
            assert_eq!(decoded.end_reasons, vec![EndReason::Legacy]);
            assert_eq!(decoded.frame_at(0.0).capture_totals, Score::default());
            assert_eq!(decoded.frame_at(0.0).no_stone_streak, 0);
            assert_eq!(decoded.frame_at(0.0).round_elapsed_seconds, 0.0);
            assert_eq!(decoded.frame_at(0.0).phase, ReplayPhase::Playing);
            assert_eq!(decoded.frame_at(0.0).current_round, 1);
            assert!(
                decoded
                    .frame_at(0.0)
                    .opening_selections
                    .iter()
                    .all(OpeningSelections::is_empty)
            );
        }
    }

    #[test]
    fn literal_schema_v4_defaults_v5_match_metadata_without_fabricating_opening() {
        let replay = sample_replay(1, 2, "Schema Four");
        let mut value = replay.to_in_memory_value().unwrap();
        value["version"] = serde_json::json!(OPENING_REPLAY_SCHEMA_VERSION);
        value.as_object_mut().unwrap().remove("match_settings");
        value.as_object_mut().unwrap().remove("end_reasons");
        for event in value["events"].as_array_mut().unwrap() {
            let frame = event["frame"].as_object_mut().unwrap();
            frame.remove("capture_totals");
            frame.remove("no_stone_streak");
            frame.remove("round_elapsed_seconds");
        }

        let decoded = Replay::from_value(value).unwrap();
        assert_eq!(decoded.version, 4);
        assert_eq!(decoded.match_settings, MatchSettings::default());
        assert_eq!(decoded.end_reasons, vec![EndReason::Legacy]);
        assert!(!decoded.opening_completed);
        assert_eq!(decoded.frame_at(0.5).capture_totals, Score::default());
        let rewritten = decoded.to_storage_json().unwrap();
        let reloaded = Replay::from_json(&rewritten).unwrap();
        assert_eq!(reloaded.version, 4);
        assert_eq!(reloaded.end_reasons, vec![EndReason::Legacy]);
    }

    fn settings_with_capture_points() -> MatchSettings {
        let mut settings = MatchSettings::default();
        settings.set_capture_points(true);
        settings
    }

    #[test]
    fn schema_v5_round_trip_keeps_settings_reasons_and_progressive_capture_score() {
        let mut settings = MatchSettings::default();
        settings.set_capture_points(true);
        settings.set_value(crate::match_settings::MatchSettingField::RoundTime, 30);
        settings.set_enabled(crate::match_settings::MatchSettingField::RoundTime, true);
        let mut initial = frame();
        initial.board.set(point(0, 0), Cell::White);
        initial.board.set(point(1, 0), Cell::Black);
        initial.board.set(point(8, 8), Cell::Black);
        initial.board.set(point(7, 8), Cell::White);
        let mut recorder = MatchRecorder::for_test(10, initial.clone(), settings.clone());
        settings.set_capture_points(false);

        let mut resolved = initial;
        resolved.board.set(point(0, 0), Cell::Empty);
        resolved.board.set(point(8, 8), Cell::Empty);
        resolved.board.set(point(0, 1), Cell::Black);
        resolved.board.set(point(8, 7), Cell::White);
        resolved.capture_totals = Score { black: 1, white: 1 };
        resolved.current_round = 2;
        recorder.record(
            1.0,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::Simultaneous,
                captures: [1, 1],
            },
            resolved.clone(),
        );
        let mut ended = resolved.clone();
        ended.phase = ReplayPhase::Ended;
        ended.score = ended.current_score(&settings_with_capture_points());
        let replay = recorder
            .complete_with_reasons(12, 2.0, ended, vec![EndReason::Manual])
            .unwrap();

        replay.validate().unwrap();
        let json = replay.to_storage_json().unwrap();
        let decoded = Replay::from_json(&json).unwrap();
        assert!(decoded.match_settings.capture_points());
        assert_eq!(decoded.end_reasons, vec![EndReason::Manual]);
        assert_eq!(decoded.frame_at(1.0).capture_totals.black, 1);
        assert_eq!(
            decoded.frame_at(1.0).current_score(&decoded.match_settings),
            Score { black: 3, white: 3 }
        );
        assert_eq!(decoded.score, Score { black: 3, white: 3 });
        assert!(decoded.label.contains("3-3"));
    }

    #[test]
    fn timeout_events_are_significant_and_count_as_completed_rounds() {
        let mut settings = MatchSettings::default();
        settings.set_value(crate::match_settings::MatchSettingField::RoundTime, 1);
        settings.set_enabled(crate::match_settings::MatchSettingField::RoundTime, true);
        let mut recorder = MatchRecorder::for_test(1, frame(), settings);
        let mut timed_out = frame();
        timed_out.current_round = 2;
        timed_out.no_stone_streak = 1;
        recorder.record(
            1.0,
            ReplayEventKind::RoundTimedOut {
                resolution: ResolutionKind::NoPlacement,
                submitted: [false, false],
                captures: [0, 0],
                cause: TimeoutCause::RoundTime,
            },
            timed_out.clone(),
        );
        timed_out.phase = ReplayPhase::Ended;
        let replay = recorder.complete(2, 2.0, timed_out).unwrap();

        replay.validate().unwrap();
        assert_eq!(replay.round_count, 1);
        assert_eq!(replay.completed_round_count(), 1);
        assert_eq!(replay.significant_times(), vec![0.0, 1.0, 2.0]);
    }

    #[test]
    fn schema_v3_keeps_using_its_explicit_audio_track() {
        let mut replay = replay_with_audio_track();
        replay.version = EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION;
        replay.end_reasons = vec![EndReason::Legacy];
        for event in &mut replay.events {
            event.frame.phase = ReplayPhase::Playing;
        }

        replay.validate().unwrap();
        assert_eq!(
            replay.audio_cues_between(0.0, replay.duration_seconds),
            vec![
                GameCue::BlackTeleport,
                GameCue::BothCapture,
                GameCue::GrayCapture,
                GameCue::MatchEnd
            ]
        );
    }
}
