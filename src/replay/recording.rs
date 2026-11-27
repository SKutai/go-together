//! Turning a live Match into a [`Replay`]: [`MatchRecorder`] appends a
//! [`ReplayEvent`] (semantic kind plus post-event [`ReplayFrame`] snapshot)
//! per change, then produces the finished [`Replay`] once.
//!
//! This is pure data assembly, though the module itself is compiled only
//! with the `client` feature or under `cfg(test)`, for the reason below.
//! The one host-specific thing a new Replay needs - the local wall-clock
//! text in its default label - is not computed here: the caller passes it to
//! [`MatchRecorder::new_with_settings`] as already-formatted text, so this
//! module needs no clock, locale, or platform adapter. The Bevy client gets
//! that text from `replay_library::format_local_timestamp`; a headless
//! caller can pass whatever it likes.
//!
//! [`MatchRecorder`] stays `pub(crate)` rather than joining `replay`'s
//! public API. That is a deliberate limit, not an oversight: the crate
//! guarantees that every [`Replay`] a consumer can hold has passed
//! [`Replay::validate`], and [`MatchRecorder::complete_with_reasons`] does
//! not validate - `ReplayLibrary::save_completed` does, at the top of the
//! write path. Exposing the recorder would mean either giving up that
//! guarantee or changing completion into a validating, fallible operation,
//! which is a larger design question than where the code compiles. The
//! module declaration in `replay/mod.rs` is therefore
//! `#[cfg(any(feature = "client", test))]`: the client builds it because it
//! records Matches, and every configuration builds it under `cfg(test)`
//! because the Replay fixtures do, but a headless non-test build has no
//! caller for it and would report it as dead code.

use uuid::Uuid;

use super::schema::{
    EndReason, REPLAY_SCHEMA_VERSION, Replay, ReplayEvent, ReplayEventKind, ReplayFrame,
    ReplayPhase, ReplayWinner, TIME_EPSILON,
};
use crate::match_settings::MatchSettings;
use crate::rules::Score;

/// The start-time text every test fixture is recorded with. Production
/// passes the local wall-clock rendering of the same instant; pinning it
/// here keeps a fixture's default label - and so the bytes a fixture writes
/// - identical on every machine and in every time zone.
#[cfg(test)]
const TEST_STARTED_AT_TEXT: &str = "1970-01-01 00:00";

/// The separator between the three parts of a default Replay label.
pub(crate) const REPLAY_METADATA_SEPARATOR: &str = " - ";

/// Builds the label a newly recorded Replay is named by, from its final
/// score and the text form of its start time.
///
/// `started_at_text` is already formatted, which is what keeps this - and
/// [`MatchRecorder`] with it - free of any clock or locale dependency; see
/// this module's docs.
pub(crate) fn default_replay_label(score: Score, started_at_text: &str) -> String {
    format!(
        "{}{}{}-{}{}{}",
        ReplayWinner::from_score(score).label(),
        REPLAY_METADATA_SEPARATOR,
        score.black,
        score.white,
        REPLAY_METADATA_SEPARATOR,
        started_at_text
    )
}

/// Builds a Replay from live semantic events and post-event frame snapshots.
///
/// Completion is one-shot so a Match cannot produce duplicate Replay files.
#[derive(Clone, Debug)]
pub(crate) struct MatchRecorder {
    id: String,
    started_at_ms: i64,
    started_at_text: String,
    match_settings: MatchSettings,
    events: Vec<ReplayEvent>,
    completed: bool,
}

impl MatchRecorder {
    /// A recorder with default Match settings, for tests that do not care
    /// about them.
    #[cfg(test)]
    pub(crate) fn new(started_at_ms: i64, initial_frame: ReplayFrame) -> Self {
        Self::for_test(started_at_ms, initial_frame, MatchSettings::default())
    }

    /// [`Self::new_with_settings`] with [`TEST_STARTED_AT_TEXT`] as the
    /// start-time text, so no fixture has to name it and none of them
    /// depend on the machine's time zone.
    #[cfg(test)]
    pub(crate) fn for_test(
        started_at_ms: i64,
        initial_frame: ReplayFrame,
        match_settings: MatchSettings,
    ) -> Self {
        Self::new_with_settings(
            started_at_ms,
            TEST_STARTED_AT_TEXT.into(),
            initial_frame,
            match_settings,
        )
    }

    /// `started_at_text` is the local wall-clock rendering of
    /// `started_at_ms` that the default label carries; see this module's
    /// docs for why the recorder takes it rather than formatting it.
    pub(crate) fn new_with_settings(
        started_at_ms: i64,
        started_at_text: String,
        initial_frame: ReplayFrame,
        match_settings: MatchSettings,
    ) -> Self {
        let mut recorder = Self {
            id: Uuid::new_v4().to_string(),
            started_at_ms,
            started_at_text,
            match_settings,
            events: Vec::new(),
            completed: false,
        };
        recorder.record(0.0, ReplayEventKind::MatchStart, initial_frame);
        recorder
    }

    /// Appends one event and the frame the change produced.
    ///
    /// Nothing is recorded once the recorder has completed, or for a
    /// non-finite or negative time; a time earlier than the previous event's
    /// is raised to it, so the non-decreasing timeline [`Replay::validate`]
    /// later requires is kept here rather than trusted to the caller's clock.
    pub(crate) fn record(&mut self, at_seconds: f64, kind: ReplayEventKind, frame: ReplayFrame) {
        if self.completed || !at_seconds.is_finite() || at_seconds < 0.0 {
            return;
        }
        let at_seconds = self
            .events
            .last()
            .map_or(at_seconds, |last| at_seconds.max(last.at_seconds));
        self.events.push(ReplayEvent {
            at_seconds,
            kind,
            frame,
        });
    }

    pub(crate) fn has_opening_countdown_finished_at(&self, at_seconds: f64) -> bool {
        self.events.iter().any(|event| {
            matches!(event.kind, ReplayEventKind::OpeningCountdownFinished)
                && (event.at_seconds - at_seconds).abs() <= TIME_EPSILON
        })
    }

    #[cfg(test)]
    pub(crate) fn complete(
        &mut self,
        completed_at_ms: i64,
        duration_seconds: f64,
        final_frame: ReplayFrame,
    ) -> Option<Replay> {
        self.complete_with_reasons(
            completed_at_ms,
            duration_seconds,
            final_frame,
            vec![EndReason::Manual],
        )
    }

    /// Finishes the recording once and returns the [`Replay`]; a second call
    /// returns `None`. The final frame's phase is forced to
    /// [`ReplayPhase::Ended`], the duration is raised to the last recorded
    /// time if that is later, and a [`ReplayEventKind::MatchEnd`] event is
    /// appended there; `completed_at_ms` is floored at the start time; the
    /// round count and `opening_completed` are derived from the events; and
    /// the label is [`default_replay_label`]'s. Does not validate - see this
    /// module's docs for which layer does.
    pub(crate) fn complete_with_reasons(
        &mut self,
        completed_at_ms: i64,
        duration_seconds: f64,
        mut final_frame: ReplayFrame,
        end_reasons: Vec<EndReason>,
    ) -> Option<Replay> {
        if self.completed {
            return None;
        }
        self.completed = true;
        final_frame.phase = ReplayPhase::Ended;
        let duration_seconds = self.events.last().map_or(duration_seconds, |last| {
            duration_seconds.max(last.at_seconds)
        });
        self.record_completed_event(duration_seconds, final_frame.clone());
        let score = final_frame.score;
        let round_count = self
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
                )
            })
            .count();
        let opening_completed = self
            .events
            .iter()
            .any(|event| matches!(event.kind, ReplayEventKind::OpeningReveal));
        let replay = Replay {
            version: REPLAY_SCHEMA_VERSION,
            id: self.id.clone(),
            label: default_replay_label(score, &self.started_at_text),
            started_at_ms: self.started_at_ms,
            completed_at_ms: completed_at_ms.max(self.started_at_ms),
            duration_seconds,
            score,
            round_count,
            opening_completed,
            match_settings: self.match_settings.clone(),
            end_reasons,
            final_board: final_frame.board,
            events: self.events.clone(),
        };
        Some(replay)
    }

    fn record_completed_event(&mut self, duration_seconds: f64, frame: ReplayFrame) {
        let duration_seconds = self.events.last().map_or(duration_seconds, |last| {
            duration_seconds.max(last.at_seconds)
        });
        self.events.push(ReplayEvent {
            at_seconds: duration_seconds,
            kind: ReplayEventKind::MatchEnd,
            frame,
        });
    }

    #[cfg(test)]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    #[cfg(test)]
    pub(crate) fn events(&self) -> &[ReplayEvent] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::test_support::*;
    use crate::replay::validate_label;
    use crate::rules::{Cell, Player, ResolutionKind};

    #[test]
    fn replay_ids_are_unique_and_rename_is_immutable() {
        let first = MatchRecorder::new(1, frame());
        let second = MatchRecorder::new(1, frame());
        assert_ne!(first.id(), second.id());

        let mut replay = sample_replay(1, 2, "First Match");
        let id = replay.id.clone();
        replay.rename("Renamed Match".into()).unwrap();
        assert_eq!(replay.id, id);
        assert_eq!(replay.label, "Renamed Match");
    }

    #[test]
    fn default_labels_include_winner_black_white_score_and_start_time() {
        let started = "2024-01-01 00:00";
        assert_eq!(
            default_replay_label(
                Score {
                    black: 42,
                    white: 38
                },
                started
            ),
            format!("Black - 42-38 - {started}")
        );
        assert_eq!(
            default_replay_label(
                Score {
                    black: 34,
                    white: 40
                },
                started
            ),
            format!("White - 34-40 - {started}")
        );
        assert_eq!(
            default_replay_label(
                Score {
                    black: 41,
                    white: 41
                },
                started
            ),
            format!("Draw - 41-41 - {started}")
        );
        assert!(REPLAY_METADATA_SEPARATOR.is_ascii());
        let maximum_normal_score = default_replay_label(
            Score {
                black: 81,
                white: 81,
            },
            started,
        );
        assert!(maximum_normal_score.is_ascii());
        assert!(maximum_normal_score.len() <= 60);
        assert!(validate_label(&maximum_normal_score).is_ok());
    }

    /// The one-shot restore in `gameplay::restore_end_gesture_snapshot`
    /// asks whether the countdown it is about to restore was actually
    /// recorded, and matches within [`TIME_EPSILON`] rather than exactly,
    /// because the time it holds is a `f64` round trip of the recorded one.
    #[test]
    fn a_recorded_countdown_finish_is_found_by_its_time_within_epsilon() {
        let mut recorder = MatchRecorder::new(1, frame());
        assert!(!recorder.has_opening_countdown_finished_at(3.0));

        recorder.record(3.0, ReplayEventKind::OpeningCountdownFinished, frame());

        assert!(recorder.has_opening_countdown_finished_at(3.0));
        assert!(recorder.has_opening_countdown_finished_at(3.0 + TIME_EPSILON / 2.0));
        assert!(!recorder.has_opening_countdown_finished_at(3.5));
        // The recorded event, and the `MatchStart` every recorder opens with.
        assert_eq!(recorder.events().len(), 2);
        assert!(matches!(
            recorder.events()[1].kind,
            ReplayEventKind::OpeningCountdownFinished
        ));
    }

    #[test]
    fn significant_events_exclude_cursor_and_selection() {
        assert!(ReplayEventKind::MatchStart.is_significant());
        assert!(
            ReplayEventKind::MarkerChanged {
                player: Player::One,
                point: point(1, 1)
            }
            .is_significant()
        );
        assert!(
            !ReplayEventKind::CursorMoved {
                player: Player::One,
                from: point(1, 1),
                to: point(2, 1)
            }
            .is_significant()
        );
        assert!(
            !ReplayEventKind::SelectionChanged {
                player: Player::Two,
                point: point(2, 2)
            }
            .is_significant()
        );
    }

    #[test]
    fn abandoned_recorders_produce_nothing_and_completion_happens_once() {
        let abandoned = MatchRecorder::new(1, frame());
        assert!(!abandoned.completed);

        let mut completed = MatchRecorder::new(1, frame());
        assert!(completed.complete(2, 1.0, frame()).is_some());
        assert!(completed.complete(3, 2.0, frame()).is_none());
    }

    #[test]
    fn reconstruction_and_significant_seeking_cover_boundaries() {
        let mut recorder = MatchRecorder::new(1, frame());
        let moved = ReplayFrame {
            cursors: [point(6, 4), point(1, 4)],
            ..frame()
        };
        recorder.record(
            1.0,
            ReplayEventKind::CursorMoved {
                player: Player::One,
                from: point(7, 4),
                to: point(6, 4),
            },
            moved.clone(),
        );
        let selected = ReplayFrame {
            selected_moves: [Some(point(6, 4)), None],
            ..moved.clone()
        };
        recorder.record(
            1.5,
            ReplayEventKind::SelectionChanged {
                player: Player::One,
                point: point(6, 4),
            },
            selected,
        );
        let marked = ReplayFrame {
            teleport_markers: [Some(point(6, 4)), None],
            ..moved.clone()
        };
        recorder.record(
            2.0,
            ReplayEventKind::MarkerChanged {
                player: Player::One,
                point: point(6, 4),
            },
            marked.clone(),
        );
        let replay = recorder.complete(4, 3.0, marked).unwrap();

        assert_eq!(replay.frame_at(0.5), frame());
        assert_eq!(replay.frame_at(1.25), moved);
        assert_eq!(replay.previous_significant_time(0.0), 0.0);
        assert_eq!(replay.next_significant_time(0.0), 2.0);
        assert_eq!(replay.previous_significant_time(2.5), 2.0);
        assert_eq!(replay.next_significant_time(2.5), 3.0);
        assert_eq!(replay.next_significant_time(3.0), 3.0);
    }

    #[test]
    fn cursor_selection_marker_jump_and_board_snapshots_reproduce() {
        let mut recorder = MatchRecorder::new(1, frame());
        let cursor = ReplayFrame {
            cursors: [point(6, 4), point(1, 4)],
            ..frame()
        };
        recorder.record(
            0.5,
            ReplayEventKind::CursorMoved {
                player: Player::One,
                from: point(7, 4),
                to: point(6, 4),
            },
            cursor.clone(),
        );
        let selection = ReplayFrame {
            selected_moves: [Some(point(6, 4)), None],
            ..cursor.clone()
        };
        recorder.record(
            1.0,
            ReplayEventKind::SelectionChanged {
                player: Player::One,
                point: point(6, 4),
            },
            selection,
        );
        let marker = ReplayFrame {
            teleport_markers: [Some(point(6, 4)), None],
            ..cursor.clone()
        };
        recorder.record(
            1.5,
            ReplayEventKind::MarkerChanged {
                player: Player::One,
                point: point(6, 4),
            },
            marker.clone(),
        );
        let jumped = ReplayFrame {
            cursors: [point(6, 4), point(1, 4)],
            ..marker.clone()
        };
        recorder.record(
            2.0,
            ReplayEventKind::JumpToMarker {
                player: Player::One,
                from: point(2, 2),
                to: point(6, 4),
            },
            jumped.clone(),
        );
        let mut resolved = jumped;
        resolved.board.set(point(6, 4), Cell::Black);
        resolved.board.set(point(4, 4), Cell::Gray);
        resolved.selected_moves = [None, None];
        resolved.current_round = 2;
        recorder.record(
            2.5,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::SamePointGray,
                captures: [0, 0],
            },
            resolved.clone(),
        );
        let replay = recorder.complete(4, 3.0, resolved.clone()).unwrap();

        assert_eq!(replay.frame_at(0.5).cursors[0], point(6, 4));
        assert_eq!(replay.frame_at(1.0).selected_moves[0], Some(point(6, 4)));
        assert_eq!(replay.frame_at(1.5).teleport_markers[0], Some(point(6, 4)));
        assert_eq!(replay.frame_at(2.0).cursors[0], point(6, 4));
        assert_eq!(replay.frame_at(2.5).board, resolved.board);
        assert_eq!(replay.frame_at(2.5).selected_moves, [None, None]);
        assert_eq!(replay.frame_at(0.0).current_round, 1);
        assert_eq!(replay.frame_at(2.0).current_round, 1);
        assert_eq!(replay.frame_at(2.5).current_round, 2);
        assert_eq!(replay.completed_round_count(), 1);
    }

    #[test]
    fn saved_replays_keep_their_recorded_cursor_positions() {
        let legacy_frame = ReplayFrame {
            cursors: [point(1, 4), point(7, 4)],
            ..frame()
        };
        let mut recorder = MatchRecorder::new(1, legacy_frame.clone());
        let replay = recorder.complete(2, 1.0, legacy_frame.clone()).unwrap();
        let json = replay.to_storage_json().unwrap();
        let loaded = Replay::from_json(&json).unwrap();

        assert_eq!(loaded.frame_at(0.0).cursors, legacy_frame.cursors);
        assert_eq!(loaded.frame_at(1.0).cursors, legacy_frame.cursors);
    }
}
