//! The Round 0 timeline: opening selections, the countdown, and the reveal.
//!
//! Round 0 is the one part of a Replay whose events are ordered against each
//! other rather than against the board, so it is checked by a small state
//! machine that the walk carries from event to event. A selection may only
//! change while Round 0 is still running and before the reveal, the
//! countdown must step Three-Two-One-Go with the reveal falling exactly
//! between One and Go, and the revealed board must be
//! what [`reveal_opening`] produces from the selections on the previous
//! frame *after* [`crate::rules::Board::resolve_opening_captures`] sweeps
//! it - the cleaned board `MatchState::reveal_round_zero` records, with
//! (from schema v5) the frame's capture totals carrying exactly the sweep's
//! credits - so the opening cannot be forged any more than an ordinary
//! round can.
//!
//! The same pass enforces the negative: a Replay whose metadata says it has
//! no opening must contain no Round 0 event and no Round 0 frame data, which
//! is what keeps a fabricated opening out of a file claiming an older schema
//! version.
//!
//! One exception is deliberate. A Match whose match-time limit expires
//! during Round 0 reveals a partial opening, so a reveal at exactly that
//! deadline is accepted without a full set of selections and without
//! reaching stage One. The caller identifies that case - it needs the whole
//! timeline to do so - and passes it in.

use crate::opening::{
    OpeningCountdownStage, OpeningSelectionOutcome, OpeningSelections, reveal_opening,
};
use crate::replay::schema::{
    MATCH_RULES_REPLAY_SCHEMA_VERSION, ReplayError, ReplayEvent, ReplayEventKind, ReplayFrame,
    ReplayPhase, TIME_EPSILON,
};
use crate::rules::{Player, Score};

#[derive(Default)]
pub(super) struct OpeningValidationState {
    started: bool,
    countdown_stage: Option<OpeningCountdownStage>,
    revealed: bool,
    pub(super) revealed_at_seconds: Option<f64>,
    pub(super) finished: bool,
}

pub(super) fn validate_opening_event(
    event: &ReplayEvent,
    previous_frame: Option<&ReplayFrame>,
    opening_completed: bool,
    version: u32,
    deadline_match_time_reveal: bool,
    state: &mut OpeningValidationState,
) -> Result<(), ReplayError> {
    let opening_event = matches!(
        event.kind,
        ReplayEventKind::OpeningStarted
            | ReplayEventKind::OpeningSelectionChanged { .. }
            | ReplayEventKind::OpeningSelectionDuplicate { .. }
            | ReplayEventKind::OpeningCountdown { .. }
            | ReplayEventKind::OpeningReveal
            | ReplayEventKind::OpeningCountdownFinished
    );
    if opening_event && !opening_completed {
        return Err(ReplayError::InvalidData(
            "replay contains Round 0 events without opening metadata".into(),
        ));
    }
    if !opening_completed
        && (event.frame.phase == ReplayPhase::Opening
            || event.frame.opening_countdown.is_some()
            || event
                .frame
                .opening_selections
                .iter()
                .any(|selections| !selections.is_empty()))
    {
        return Err(ReplayError::InvalidData(
            "replay contains Round 0 frames without opening metadata".into(),
        ));
    }
    if opening_completed
        && !state.started
        && !matches!(
            event.kind,
            ReplayEventKind::MatchStart | ReplayEventKind::OpeningStarted
        )
    {
        return Err(ReplayError::InvalidData(
            "Round 0 event occurs before opening_started".into(),
        ));
    }

    match &event.kind {
        ReplayEventKind::MatchStart if opening_completed => {
            if event.frame.phase != ReplayPhase::Opening || event.frame.current_round != 0 {
                return Err(ReplayError::InvalidData(
                    "Round 0 replay must start in the opening phase".into(),
                ));
            }
        }
        ReplayEventKind::OpeningStarted => {
            if state.started
                || event.at_seconds.abs() > TIME_EPSILON
                || event.frame.phase != ReplayPhase::Opening
                || event.frame.opening_countdown.is_some()
                || event
                    .frame
                    .opening_selections
                    .iter()
                    .any(|selections| !selections.is_empty())
            {
                return Err(ReplayError::InvalidData(
                    "opening_started event is inconsistent".into(),
                ));
            }
            state.started = true;
        }
        ReplayEventKind::OpeningSelectionChanged {
            player,
            point,
            evicted,
        } => {
            let previous = previous_frame.ok_or_else(|| {
                ReplayError::InvalidData("opening selection has no prior frame".into())
            })?;
            if state.revealed || previous.phase != ReplayPhase::Opening {
                return Err(ReplayError::InvalidData(
                    "opening selection occurs outside Round 0".into(),
                ));
            }
            let mut expected = previous.opening_selections.clone();
            let outcome = expected[player.index()].add(*point);
            if outcome != (OpeningSelectionOutcome::Added { evicted: *evicted })
                || expected != event.frame.opening_selections
            {
                return Err(ReplayError::InvalidData(
                    "opening selection event does not match its frame".into(),
                ));
            }
        }
        ReplayEventKind::OpeningSelectionDuplicate { player, point } => {
            let previous = previous_frame.ok_or_else(|| {
                ReplayError::InvalidData("opening duplicate has no prior frame".into())
            })?;
            if state.revealed
                || previous.phase != ReplayPhase::Opening
                || !previous.opening_selections[player.index()].contains(*point)
                || previous.opening_selections != event.frame.opening_selections
            {
                return Err(ReplayError::InvalidData(
                    "opening duplicate event does not match its frame".into(),
                ));
            }
        }
        ReplayEventKind::OpeningCountdown { stage } => {
            let expected = match state.countdown_stage {
                None => OpeningCountdownStage::Three,
                Some(OpeningCountdownStage::Three) => OpeningCountdownStage::Two,
                Some(OpeningCountdownStage::Two) => OpeningCountdownStage::One,
                Some(OpeningCountdownStage::One) => OpeningCountdownStage::Go,
                Some(OpeningCountdownStage::Go) => {
                    return Err(ReplayError::InvalidData(
                        "opening countdown stage repeats after Go".into(),
                    ));
                }
            };
            if *stage != expected || event.frame.opening_countdown != Some(*stage) {
                return Err(ReplayError::InvalidData(
                    "opening countdown stages are out of order".into(),
                ));
            }
            if (*stage == OpeningCountdownStage::Go) != state.revealed {
                return Err(ReplayError::InvalidData(
                    "opening reveal and Go stage are inconsistent".into(),
                ));
            }
            state.countdown_stage = Some(*stage);
        }
        ReplayEventKind::OpeningReveal => {
            let previous = previous_frame.ok_or_else(|| {
                ReplayError::InvalidData("opening reveal has no prior frame".into())
            })?;
            let timed_out_opening =
                version >= MATCH_RULES_REPLAY_SCHEMA_VERSION && deadline_match_time_reveal;
            // The recorded frame is the post-event snapshot of
            // `MatchState::reveal_round_zero`, which sweeps the raw reveal
            // with `resolve_opening_captures` before anything is recorded -
            // so the expected board is the cleaned one, and the expected
            // capture totals are the sweep's credits, both re-derived here
            // from the persisted selections exactly as `transitions`
            // re-derives ordinary rounds through `resolve_round`.
            let sweep = reveal_opening(&previous.opening_selections).resolve_opening_captures();
            let (cleaned_board, capture_credits) = (sweep.board, sweep.capture_credits);
            if (!timed_out_opening && state.countdown_stage != Some(OpeningCountdownStage::One))
                || state.revealed
                || (!timed_out_opening
                    && !previous
                        .opening_selections
                        .iter()
                        .all(OpeningSelections::is_full))
                || event.frame.phase != ReplayPhase::Playing
                || event.frame.current_round != 1
                || event.frame.board != cleaned_board
            {
                return Err(ReplayError::InvalidData(
                    "opening reveal event does not match its selections".into(),
                ));
            }
            if version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
                let expected_totals = Score {
                    black: capture_credits[Player::One.index()],
                    white: capture_credits[Player::Two.index()],
                };
                if event.frame.capture_totals != expected_totals || event.frame.no_stone_streak != 0
                {
                    return Err(ReplayError::InvalidData(
                        "opening reveal capture credits do not match its sweep".into(),
                    ));
                }
            }
            state.revealed = true;
            state.revealed_at_seconds = Some(event.at_seconds);
        }
        ReplayEventKind::OpeningCountdownFinished => {
            if state.countdown_stage != Some(OpeningCountdownStage::Go)
                || !state.revealed
                || state.finished
                || event.frame.phase != ReplayPhase::Playing
                || event.frame.opening_countdown.is_some()
            {
                return Err(ReplayError::InvalidData(
                    "opening countdown finished event is inconsistent".into(),
                ));
            }
            state.finished = true;
        }
        ReplayEventKind::SelectionChanged { .. }
        | ReplayEventKind::RoundResolved { .. }
        | ReplayEventKind::RoundTimedOut { .. }
            if opening_completed && !state.revealed =>
        {
            return Err(ReplayError::InvalidData(
                "ordinary play event occurs before the Round 0 reveal".into(),
            ));
        }
        ReplayEventKind::MatchEnd if opening_completed => {
            if event.frame.phase != ReplayPhase::Ended
                || (!state.finished && version < MATCH_RULES_REPLAY_SCHEMA_VERSION)
            {
                return Err(ReplayError::InvalidData(
                    "completed Round 0 replay ends in an inconsistent phase".into(),
                ));
            }
        }
        ReplayEventKind::MatchStart
        | ReplayEventKind::CursorMoved { .. }
        | ReplayEventKind::SelectionChanged { .. }
        | ReplayEventKind::MarkerChanged { .. }
        | ReplayEventKind::JumpToMarker { .. }
        | ReplayEventKind::RoundResolved { .. }
        | ReplayEventKind::RoundTimedOut { .. }
        | ReplayEventKind::MatchSound { .. }
        | ReplayEventKind::MatchEnd => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_settings::MatchSettings;
    use crate::replay::test_support::*;
    use crate::replay::{EndReason, OPENING_REPLAY_SCHEMA_VERSION, Replay};
    use crate::rules::{Cell, Player};

    #[test]
    fn schema_v4_round_zero_round_trips_with_ordinary_round_count_semantics() {
        let replay = round_zero_replay();
        replay.validate().unwrap();
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
        let json = serde_json::to_string_pretty(&value).unwrap();
        let decoded = Replay::from_json(&json).unwrap();

        assert_eq!(decoded.version, OPENING_REPLAY_SCHEMA_VERSION);
        assert_eq!(decoded.match_settings, MatchSettings::default());
        assert_eq!(decoded.end_reasons, vec![EndReason::Legacy]);
        assert!(decoded.opening_completed);
        assert_eq!(decoded.round_count, 0);
        assert_eq!(decoded.completed_round_count(), 0);
        assert_eq!(decoded.frame_at(0.0).phase, ReplayPhase::Opening);
        assert_eq!(decoded.frame_at(0.0).current_round, 0);
        assert_eq!(
            decoded.frame_at(0.25).opening_selections[Player::One.index()].len(),
            3
        );
        assert_eq!(
            decoded.frame_at(0.75).opening_selections[Player::One.index()].as_slice(),
            &[point(1, 0), point(2, 0), point(3, 0)]
        );
        assert_eq!(
            decoded.frame_at(2.25).opening_countdown,
            Some(OpeningCountdownStage::One)
        );
        assert_eq!(decoded.frame_at(3.25).phase, ReplayPhase::Playing);
        assert_eq!(decoded.frame_at(3.25).current_round, 1);
        assert!(
            decoded
                .frame_at(3.25)
                .opening_selections
                .iter()
                .all(OpeningSelections::is_empty)
        );
        assert_eq!(decoded.frame_at(3.25).board.get(point(0, 0)), Cell::Empty);
        assert_eq!(decoded.frame_at(3.25).board.get(point(1, 0)), Cell::Gray);
        assert_eq!(
            decoded.significant_times(),
            vec![0.0, 0.25, 0.75, 1.25, 2.25, 3.25, 4.5]
        );
    }

    #[test]
    fn schema_v4_rejects_oversized_duplicate_and_out_of_bounds_opening_arrays() {
        let replay = round_zero_replay();
        let mut base = replay.to_in_memory_value().unwrap();
        base["version"] = serde_json::json!(OPENING_REPLAY_SCHEMA_VERSION);
        strip_v5_fields(&mut base);
        let first_opening_frame = 0;

        let mut oversized = base.clone();
        oversized["events"][first_opening_frame]["frame"]["opening_selections"] = serde_json::json!([[
                    {"x": 0, "y": 0},
                    {"x": 1, "y": 0},
                    {"x": 2, "y": 0},
                    {"x": 3, "y": 0}
                ], []]);
        assert!(Replay::parse_value(oversized).is_err());

        let mut duplicate = base.clone();
        duplicate["events"][first_opening_frame]["frame"]["opening_selections"] =
            serde_json::json!([[{"x": 0, "y": 0}, {"x": 0, "y": 0}], []]);
        assert!(Replay::parse_value(duplicate).is_err());

        let mut outside = base;
        outside["events"][first_opening_frame]["frame"]["opening_selections"] =
            serde_json::json!([[{"x": 9, "y": 0}], []]);
        assert!(Replay::parse_value(outside).is_err());
    }

    #[test]
    fn schema_v4_rejects_opening_metadata_and_reveal_inconsistency() {
        let mut missing_metadata = round_zero_replay();
        missing_metadata.version = OPENING_REPLAY_SCHEMA_VERSION;
        missing_metadata.end_reasons = vec![EndReason::Legacy];
        missing_metadata.opening_completed = false;
        assert!(matches!(
            missing_metadata.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "replay opening metadata does not match its lifecycle events"
        ));

        let mut forged_reveal = round_zero_replay();
        forged_reveal.version = OPENING_REPLAY_SCHEMA_VERSION;
        forged_reveal.end_reasons = vec![EndReason::Legacy];
        let reveal = forged_reveal
            .events
            .iter_mut()
            .find(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))
            .unwrap();
        reveal.frame.board.set(point(8, 0), Cell::Black);
        assert!(matches!(
            forged_reveal.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "opening reveal event does not match its selections"
        ));
    }

    /// The reveal frame must carry the swept board `reveal_round_zero`
    /// records, not the raw six-slot combination: a file claiming the
    /// zero-liberty stone survived into Round 1 is forged, because that
    /// state never exists in a Match. The credits check is pinned
    /// separately below.
    #[test]
    fn a_reveal_carrying_the_raw_unswept_board_is_rejected() {
        let replay = sweeping_reveal_replay();
        replay.validate().unwrap();

        let mut forged = sweeping_reveal_replay();
        let reveal = forged
            .events
            .iter_mut()
            .find(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))
            .unwrap();
        // Restore the swept corner stone, undoing the sweep.
        reveal.frame.board.set(point(0, 0), Cell::Black);
        assert!(matches!(
            forged.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "opening reveal event does not match its selections"
        ));
    }

    /// The reveal frame's capture totals must be exactly the sweep's
    /// credits - zeroing them (or inventing extras) does not validate, so a
    /// sweep capture cannot be forged in or out of the score.
    #[test]
    fn a_reveal_with_forged_sweep_credits_is_rejected() {
        let mut forged = sweeping_reveal_replay();
        for event in &mut forged.events {
            event.frame.capture_totals = crate::rules::Score::default();
        }
        let ended = forged.events.last_mut().unwrap();
        ended.frame.score = ended.frame.current_score(&MatchSettings::default());
        assert!(matches!(
            forged.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "opening reveal capture credits do not match its sweep"
        ));
    }

    #[test]
    fn schema_v5_accepts_only_terminal_partial_reveal_at_match_deadline() {
        partial_opening_match_timeout_replay(5.0, false)
            .validate()
            .unwrap();
        partial_opening_match_timeout_replay(5.0, true)
            .validate()
            .unwrap();

        let forged = partial_opening_match_timeout_replay(2.0, true);
        assert!(matches!(
            forged.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "opening reveal event does not match its selections"
        ));
    }

    #[test]
    fn schema_v5_rejects_a_late_manual_end_without_go_finish() {
        let mut replay = round_zero_replay();
        replay
            .events
            .retain(|event| !matches!(event.kind, ReplayEventKind::OpeningCountdownFinished));

        assert!(matches!(
            replay.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "replay Round 0 countdown did not finish"
        ));
    }

    #[test]
    fn schema_v5_allows_missing_go_finish_only_before_its_boundary() {
        let mut before = round_zero_replay();
        before
            .events
            .retain(|event| !matches!(event.kind, ReplayEventKind::OpeningCountdownFinished));
        before.duration_seconds = 3.9;
        let before_end = before.events.last_mut().unwrap();
        before_end.at_seconds = 3.9;
        before_end.frame.round_elapsed_seconds = 0.65;
        before.validate().unwrap();

        let mut at_boundary = before;
        at_boundary.duration_seconds = 4.0;
        let boundary_end = at_boundary.events.last_mut().unwrap();
        boundary_end.at_seconds = 4.0;
        boundary_end.frame.round_elapsed_seconds = 0.75;
        assert!(matches!(
            at_boundary.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "replay Round 0 countdown did not finish"
        ));
    }
}
