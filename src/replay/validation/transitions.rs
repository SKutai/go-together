//! Whether one event follows from the frame before it.
//!
//! This is the expensive half of validation and the half that makes forgery
//! hard. Where the persisted selections allow it, an ordinary round is not
//! merely checked for plausibility but re-derived: the two locked-in moves
//! go back through [`resolve_round`] with the same board and repetition
//! history the live Match had, and the resulting board, resolution kind, and
//! capture credits must equal the ones on disk. A file cannot claim a
//! capture that the rules would not produce.
//!
//! Two things keep that from being the only path. Replay v5 has always
//! allowed a `RoundResolved` timeline that omits one or both
//! `SelectionChanged` checkpoints, so when a required point is missing there
//! is nothing to re-derive from; those rounds fall back to the older
//! board-delta bound, which is looser but still rejects a payload the board
//! difference cannot account for. And a timeout carries a further check of
//! its own on top of the re-derivation: the recorded cause must name the
//! timer that could actually have fired at that instant, and a `MatchTime`
//! timeout must be the last ordinary completion in the file.
//!
//! The round clock is validated here too, rather than in `frame`, because a
//! frame's elapsed time only means anything against the authoritative start
//! of the round it belongs to - which is a property of the timeline, not of
//! the frame.

use std::collections::HashSet;

use crate::replay::schema::{
    Replay, ReplayError, ReplayEvent, ReplayEventKind, ReplayFrame, ReplayPhase, TIME_EPSILON,
    TimeoutCause,
};
use crate::rules::{
    BOARD_SIZE, Board, Cell, LockedMove, Player, Point, ResolutionKind, Score, resolve_round,
};

pub(super) fn validate_v5_round_clock(
    event: &ReplayEvent,
    round_started_at: Option<f64>,
) -> Result<(), ReplayError> {
    let resets_round_clock = matches!(
        event.kind,
        ReplayEventKind::OpeningReveal
            | ReplayEventKind::RoundResolved { .. }
            | ReplayEventKind::RoundTimedOut { .. }
    );
    if resets_round_clock {
        if event.frame.round_elapsed_seconds.abs() > TIME_EPSILON {
            return Err(ReplayError::InvalidData(
                "round-clock reset frame is not zero".into(),
            ));
        }
    } else if event.frame.phase == ReplayPhase::Playing {
        let round_started_at = round_started_at.ok_or_else(|| {
            ReplayError::InvalidData("playing frame has no authoritative round start".into())
        })?;
        let expected = event.at_seconds - round_started_at;
        if expected < -TIME_EPSILON
            || (event.frame.round_elapsed_seconds - expected).abs() > TIME_EPSILON
        {
            return Err(ReplayError::InvalidData(
                "replay round clock does not match its timeline".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_v5_frame_transition(
    replay: &Replay,
    event_index: usize,
    previous_event: Option<&ReplayEvent>,
    round_started_at: Option<f64>,
    completed_history: &HashSet<Board>,
) -> Result<(), ReplayError> {
    let event = &replay.events[event_index];
    let previous = previous_event.map(|previous| &previous.frame);
    let Some(previous) = previous else {
        if event.frame.capture_totals != Score::default()
            || event.frame.no_stone_streak != 0
            || event.frame.round_elapsed_seconds != 0.0
        {
            return Err(ReplayError::InvalidData(
                "initial replay frame has nonzero match counters".into(),
            ));
        }
        return Ok(());
    };

    if event.frame.capture_totals.black < previous.capture_totals.black
        || event.frame.capture_totals.white < previous.capture_totals.white
    {
        return Err(ReplayError::InvalidData(
            "replay capture totals decrease".into(),
        ));
    }
    let ordinary_completion = matches!(
        event.kind,
        ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
    );
    // The Round 0 reveal is the one other event allowed to move the capture
    // totals: its sweep credits are re-derived from the persisted selections
    // and checked exactly in `validation::opening`, which owns everything
    // derived from the reveal.
    let opening_reveal = matches!(event.kind, ReplayEventKind::OpeningReveal);
    if !ordinary_completion
        && !opening_reveal
        && (event.frame.capture_totals != previous.capture_totals
            || event.frame.no_stone_streak != previous.no_stone_streak)
    {
        return Err(ReplayError::InvalidData(
            "replay counters change outside an ordinary completion".into(),
        ));
    }
    if ordinary_completion {
        let black_delta = event.frame.capture_totals.black - previous.capture_totals.black;
        let white_delta = event.frame.capture_totals.white - previous.capture_totals.white;
        let captures = event.kind.capture_delta();
        if captures[0] > crate::rules::CELL_COUNT || captures[1] > crate::rules::CELL_COUNT {
            return Err(ReplayError::InvalidData(
                "one round captures more stones than the board contains".into(),
            ));
        }
        if [black_delta, white_delta] != captures {
            return Err(ReplayError::InvalidData(
                "replay capture totals do not match the round payload".into(),
            ));
        }
        if event.frame.round_elapsed_seconds.abs() > TIME_EPSILON
            || event.frame.selected_moves.iter().any(Option::is_some)
        {
            return Err(ReplayError::InvalidData(
                "completed round frame did not reset transient state".into(),
            ));
        }
        let resolution = match event.kind {
            ReplayEventKind::RoundResolved { resolution, .. }
            | ReplayEventKind::RoundTimedOut { resolution, .. } => resolution,
            _ => unreachable!(),
        };
        let complete_selection_metadata =
            validate_ordinary_selection_transition(previous, &event.kind, resolution)?;
        if matches!(
            resolution,
            ResolutionKind::NoPlacement | ResolutionKind::SamePointGray | ResolutionKind::KoGray
        ) && (black_delta != 0 || white_delta != 0)
        {
            return Err(ReplayError::InvalidData(
                "non-capturing round changes capture totals".into(),
            ));
        }
        let matched_resolution = complete_selection_metadata
            .then(|| {
                ordinary_transition_matches_rules(
                    &previous.board,
                    &event.frame.board,
                    previous.selected_moves,
                    resolution,
                    captures,
                    completed_history,
                )
            })
            .flatten();
        let transition_is_possible = if complete_selection_metadata {
            matched_resolution.is_some()
        } else {
            legacy_sparse_capture_transition_is_possible(
                &previous.board,
                &event.frame.board,
                resolution,
                captures,
            )
        };
        if !transition_is_possible {
            return Err(ReplayError::InvalidData(
                "replay capture payload is impossible for the board transition".into(),
            ));
        }
        // The streak measures rounds that leave the board exactly as it was;
        // a placement, capture, suicide removal, cleanup removal, and
        // repetition Gray conversion are all just ways the board can change,
        // so comparing the two frames' boards directly covers every case.
        let expected_streak = if previous.board == event.frame.board {
            previous.no_stone_streak.saturating_add(1)
        } else {
            0
        };
        if event.frame.no_stone_streak != expected_streak {
            return Err(ReplayError::InvalidData(
                "replay no-stone streak does not match its round result".into(),
            ));
        }
        if resolution == ResolutionKind::NoPlacement && event.frame.board != previous.board {
            return Err(ReplayError::InvalidData(
                "no-placement timeout changes the board".into(),
            ));
        }
    }
    if let ReplayEventKind::RoundTimedOut {
        resolution,
        submitted,
        cause,
        ..
    } = event.kind
    {
        let previous_event = previous_event.expect("a timeout completion has a previous event");
        let expected_submitted = previous.selected_moves.map(|point| point.is_some());
        if submitted != expected_submitted {
            return Err(ReplayError::InvalidData(
                "timeout submissions do not match the preceding frame".into(),
            ));
        }
        let submitted_count = submitted.into_iter().filter(|submitted| *submitted).count();
        if (submitted_count == 0 && resolution != ResolutionKind::NoPlacement)
            || (submitted_count == 1
                && !matches!(resolution, ResolutionKind::Single | ResolutionKind::KoGray))
            || (submitted_count == 2
                && !matches!(
                    resolution,
                    ResolutionKind::SamePointGray
                        | ResolutionKind::Sequential
                        | ResolutionKind::Simultaneous
                        | ResolutionKind::KoGray
                ))
        {
            return Err(ReplayError::InvalidData(
                "timeout event does not match its submitted moves".into(),
            ));
        }
        let move_is_legal = |player: Player, point: Point| {
            crate::rules::apply_selection(&previous.board, player, point).is_ok()
        };
        if submitted_count == 1 {
            let (index, point) = previous
                .selected_moves
                .into_iter()
                .enumerate()
                .find_map(|(index, point)| point.map(|point| (index, point)))
                .expect("submitted presence was checked above");
            if !move_is_legal(Player::ALL[index], point) {
                return Err(ReplayError::InvalidData(
                    "timeout contains a move that could not be normally submitted".into(),
                ));
            }
        }
        if submitted_count == 2 {
            let [Some(first), Some(second)] = previous.selected_moves else {
                unreachable!("submitted presence was checked above");
            };
            if (resolution == ResolutionKind::SamePointGray && first != second)
                || (matches!(
                    resolution,
                    ResolutionKind::Sequential | ResolutionKind::Simultaneous
                ) && first == second)
            {
                return Err(ReplayError::InvalidData(
                    "timeout resolution does not match its submitted points".into(),
                ));
            }
            let can_play_in_order = |early: (Player, Point), late: (Player, Point)| {
                crate::rules::apply_selection(&previous.board, early.0, early.1).is_ok_and(
                    |outcome| {
                        crate::rules::apply_selection(outcome.board(), late.0, late.1).is_ok()
                    },
                )
            };
            let simultaneous_is_legal =
                move_is_legal(Player::One, first) && move_is_legal(Player::Two, second);
            let sequential_is_legal =
                can_play_in_order((Player::One, first), (Player::Two, second))
                    || can_play_in_order((Player::Two, second), (Player::One, first));
            let moves_are_possible = match resolution {
                ResolutionKind::Sequential => sequential_is_legal,
                ResolutionKind::SamePointGray | ResolutionKind::Simultaneous => {
                    simultaneous_is_legal
                }
                ResolutionKind::KoGray => simultaneous_is_legal || sequential_is_legal,
                ResolutionKind::NoPlacement | ResolutionKind::Single => false,
            };
            if !moves_are_possible {
                return Err(ReplayError::InvalidData(
                    "timeout contains moves that could not be normally resolved".into(),
                ));
            }
        }
        validate_timeout_deadline(replay, event_index, previous_event, cause, round_started_at)?;
    }
    Ok(())
}

fn validate_ordinary_selection_transition(
    previous: &ReplayFrame,
    event_kind: &ReplayEventKind,
    resolution: ResolutionKind,
) -> Result<bool, ReplayError> {
    let selected = previous.selected_moves;
    let selected_count = selected.iter().flatten().count();
    // A selection is legal onto an empty point (ordinary placement) or onto
    // the selecting player's own stone (a potential active-cleanup target);
    // anything else - including the opponent's stone - could never have been
    // legally locked in.
    if selected
        .iter()
        .enumerate()
        .filter_map(|(index, point)| point.map(|point| (index, point)))
        .any(|(index, point)| {
            let cell = previous.board.get(point);
            cell != Cell::Empty && cell != Player::ALL[index].cell()
        })
    {
        return Err(ReplayError::InvalidData(
            "ordinary completion contains a selection that was neither empty \
                nor the selecting player's own stone"
                .into(),
        ));
    }

    let expected_count = match event_kind {
        ReplayEventKind::RoundResolved { .. } => 2,
        ReplayEventKind::RoundTimedOut { submitted, .. } => {
            submitted.iter().filter(|submitted| **submitted).count()
        }
        _ => unreachable!("only ordinary completion events reach transition validation"),
    };
    // Replay v5 has always allowed RoundResolved timelines that omit one or
    // both SelectionChanged checkpoints. Preserve those files and use the
    // legacy board-delta validation until every required point is available.
    if selected_count != expected_count {
        return Ok(false);
    }

    let points_match_resolution = match resolution {
        ResolutionKind::NoPlacement => selected_count == 0,
        ResolutionKind::Single => selected_count == 1,
        ResolutionKind::SamePointGray => {
            matches!(selected, [Some(first), Some(second)] if first == second)
        }
        ResolutionKind::Sequential | ResolutionKind::Simultaneous => {
            matches!(selected, [Some(first), Some(second)] if first != second)
        }
        ResolutionKind::KoGray => (1..=2).contains(&selected_count),
    };
    if !points_match_resolution {
        return Err(ReplayError::InvalidData(
            "ordinary completion selections do not match its resolution".into(),
        ));
    }
    Ok(true)
}

/// Re-derives the round through [`resolve_round`] and returns the matching
/// [`crate::rules::RoundResolution`], if any. The caller uses only whether
/// one exists; the no-stone streak beside it is measured by comparing the
/// two frames' boards directly, for the reason given there.
fn ordinary_transition_matches_rules(
    before: &Board,
    after: &Board,
    selected_moves: [Option<Point>; 2],
    resolution: ResolutionKind,
    captures: [usize; 2],
    completed_history: &HashSet<Board>,
) -> Option<crate::rules::RoundResolution> {
    let resolve_with_times = |times: [f64; 2]| {
        let pending = std::array::from_fn(|index| {
            selected_moves[index].map(|point| LockedMove {
                player: Player::ALL[index],
                point,
                at: crate::clock::MatchInstant::from_secs_f64(times[index]),
            })
        });
        resolve_round(before, pending, completed_history).ok()
    };
    let is_match = |candidate: &crate::rules::RoundResolution| {
        candidate.kind == resolution
            && candidate.board == *after
            && candidate.capture_credits == captures
    };

    match resolution {
        ResolutionKind::Sequential => resolve_with_times([0.0, 1.0])
            .filter(is_match)
            .or_else(|| resolve_with_times([1.0, 0.0]).filter(is_match)),
        ResolutionKind::NoPlacement
        | ResolutionKind::Single
        | ResolutionKind::SamePointGray
        | ResolutionKind::Simultaneous => resolve_with_times([0.0, 0.0]).filter(is_match),
        ResolutionKind::KoGray => resolve_with_times([0.0, 0.0]).filter(is_match).or_else(|| {
            if selected_moves.iter().all(Option::is_some) {
                resolve_with_times([0.0, 1.0])
                    .filter(is_match)
                    .or_else(|| resolve_with_times([1.0, 0.0]).filter(is_match))
            } else {
                None
            }
        }),
    }
}

fn legacy_sparse_capture_transition_is_possible(
    before: &Board,
    after: &Board,
    resolution: ResolutionKind,
    captures: [usize; 2],
) -> bool {
    let removed = [
        removed_stones(before, after, Cell::White),
        removed_stones(before, after, Cell::Black),
    ];
    match resolution {
        ResolutionKind::Sequential => captures
            .into_iter()
            .zip(removed)
            .all(|(capture, removed)| (removed..=removed.saturating_add(1)).contains(&capture)),
        ResolutionKind::Single | ResolutionKind::Simultaneous => captures == removed,
        ResolutionKind::NoPlacement | ResolutionKind::SamePointGray | ResolutionKind::KoGray => {
            captures == [0, 0]
        }
    }
}

fn validate_timeout_deadline(
    replay: &Replay,
    event_index: usize,
    previous: &ReplayEvent,
    cause: TimeoutCause,
    round_started_at: Option<f64>,
) -> Result<(), ReplayError> {
    use crate::match_settings::MatchSettingField;

    let event = &replay.events[event_index];
    if previous.frame.phase != ReplayPhase::Playing {
        return Err(ReplayError::InvalidData(
            "timeout occurs outside ordinary play".into(),
        ));
    }
    match cause {
        TimeoutCause::RoundTime => {
            if !replay.match_settings.enabled(MatchSettingField::RoundTime) {
                return Err(ReplayError::InvalidData(
                    "round-time timeout occurs while its timer is disabled".into(),
                ));
            }
            if replay.match_settings.enabled(MatchSettingField::MatchTime)
                && event.at_seconds
                    >= f64::from(replay.match_settings.value(MatchSettingField::MatchTime))
                        - TIME_EPSILON
            {
                return Err(ReplayError::InvalidData(
                    "round-time timeout has the wrong deadline cause".into(),
                ));
            }
            let round_started_at = round_started_at.ok_or_else(|| {
                ReplayError::InvalidData(
                    "round-time timeout has no authoritative round start".into(),
                )
            })?;
            let effective_elapsed = event.at_seconds - round_started_at;
            let limit = f64::from(replay.match_settings.value(MatchSettingField::RoundTime));
            if (effective_elapsed - limit).abs() > TIME_EPSILON {
                return Err(ReplayError::InvalidData(
                    "round-time timeout does not occur at its deadline".into(),
                ));
            }
        }
        TimeoutCause::MatchTime => {
            if !replay.match_settings.enabled(MatchSettingField::MatchTime) {
                return Err(ReplayError::InvalidData(
                    "match-time timeout occurs while its timer is disabled".into(),
                ));
            }
            let limit = f64::from(replay.match_settings.value(MatchSettingField::MatchTime));
            if (event.at_seconds - limit).abs() > TIME_EPSILON {
                return Err(ReplayError::InvalidData(
                    "match-time timeout does not occur at its deadline".into(),
                ));
            }
            if replay.events[event_index + 1..].iter().any(|later| {
                matches!(
                    later.kind,
                    ReplayEventKind::RoundResolved { .. } | ReplayEventKind::RoundTimedOut { .. }
                )
            }) {
                return Err(ReplayError::InvalidData(
                    "match-time timeout is not the final ordinary completion".into(),
                ));
            }
        }
    }
    Ok(())
}

fn removed_stones(before: &Board, after: &Board, color: Cell) -> usize {
    (0..crate::rules::CELL_COUNT)
        .filter(|index| {
            let point = Point::new(index % BOARD_SIZE, index / BOARD_SIZE);
            before.get(point) == color && after.get(point) != color
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue::GameCue;
    use crate::match_settings::MatchSettings;
    use crate::replay::test_support::*;
    use crate::replay::{MatchRecorder, PlaybackController, REPLAY_SCHEMA_VERSION};

    #[test]
    fn sequential_capture_of_new_stone_round_trips_with_exact_delta() {
        let replay = sequential_new_stone_capture_replay();

        replay.validate().unwrap();
        let json = replay.to_storage_json().unwrap();
        let decoded = Replay::from_json(&json).unwrap();
        let round = decoded
            .events
            .iter()
            .find(|event| matches!(event.kind, ReplayEventKind::RoundResolved { .. }))
            .unwrap();
        assert_eq!(
            round.kind,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::Sequential,
                captures: [0, 1],
            }
        );
        assert_eq!(round.frame.capture_totals, Score { black: 0, white: 1 });
        assert_eq!(round.frame.board.get(point(1, 1)), Cell::Empty);
        assert_eq!(round.frame.board.get(point(1, 2)), Cell::White);
    }

    #[test]
    fn sparse_v5_sequential_new_stone_capture_keeps_legacy_validation_tolerance() {
        for retained_player in [None, Some(Player::One)] {
            let mut replay = sequential_new_stone_capture_replay();
            replay.events.retain(|event| match event.kind {
                ReplayEventKind::SelectionChanged { player, .. } => retained_player == Some(player),
                _ => true,
            });

            replay.validate().unwrap();
            Replay::from_json(&replay.to_storage_json().unwrap()).unwrap();
        }
    }

    #[test]
    fn schema_v5_rejects_fabricated_simultaneous_self_removal() {
        let mut recorder = MatchRecorder::new(1, frame());
        let black_point = point(0, 0);
        let white_point = point(8, 8);
        let black_selected = ReplayFrame {
            selected_moves: [Some(black_point), None],
            round_elapsed_seconds: 0.25,
            ..frame()
        };
        recorder.record(
            0.25,
            ReplayEventKind::SelectionChanged {
                player: Player::One,
                point: black_point,
            },
            black_selected.clone(),
        );
        let both_selected = ReplayFrame {
            selected_moves: [Some(black_point), Some(white_point)],
            round_elapsed_seconds: 0.3,
            ..black_selected
        };
        recorder.record(
            0.3,
            ReplayEventKind::SelectionChanged {
                player: Player::Two,
                point: white_point,
            },
            both_selected.clone(),
        );
        let mut resolved = ReplayFrame {
            selected_moves: [None, None],
            current_round: 2,
            capture_totals: Score { black: 1, white: 0 },
            round_elapsed_seconds: 0.0,
            ..both_selected
        };
        resolved.board.set(black_point, Cell::Black);
        recorder.record(
            0.3,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::Simultaneous,
                captures: [1, 0],
            },
            resolved.clone(),
        );
        let mut ended = resolved;
        ended.board.fill_territory();
        ended.score = ended.current_score(&MatchSettings::default());
        let replay = recorder.complete(2, 1.0, ended).unwrap();

        assert!(matches!(
            replay.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "replay capture payload is impossible for the board transition"
        ));
    }

    #[test]
    fn schema_v5_rejects_gray_without_repetition_and_missed_older_repetition() {
        let mut recorder = MatchRecorder::new(1, frame());
        let black_point = point(0, 0);
        let white_point = point(8, 8);
        let black_selected = ReplayFrame {
            selected_moves: [Some(black_point), None],
            round_elapsed_seconds: 0.25,
            ..frame()
        };
        recorder.record(
            0.25,
            ReplayEventKind::SelectionChanged {
                player: Player::One,
                point: black_point,
            },
            black_selected.clone(),
        );
        let both_selected = ReplayFrame {
            selected_moves: [Some(black_point), Some(white_point)],
            round_elapsed_seconds: 0.3,
            ..black_selected
        };
        recorder.record(
            0.3,
            ReplayEventKind::SelectionChanged {
                player: Player::Two,
                point: white_point,
            },
            both_selected.clone(),
        );
        let mut false_gray = ReplayFrame {
            selected_moves: [None, None],
            current_round: 2,
            round_elapsed_seconds: 0.0,
            ..both_selected
        };
        false_gray.board.set(black_point, Cell::Gray);
        false_gray.board.set(white_point, Cell::Gray);
        recorder.record(
            0.3,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::KoGray,
                captures: [0, 0],
            },
            false_gray.clone(),
        );
        let mut ended = false_gray;
        ended.board.fill_territory();
        ended.score = ended.current_score(&MatchSettings::default());
        let false_ko = recorder.complete(2, 1.0, ended).unwrap();

        for replay in [false_ko, missed_older_repetition_replay()] {
            assert!(matches!(
                replay.validate(),
                Err(ReplayError::InvalidData(ref message))
                    if message
                        == "replay capture payload is impossible for the board transition"
            ));
        }
    }

    #[test]
    fn suicide_round_trips_on_v5_with_capture_credit_frame_and_single_audio_cue() {
        let replay = suicide_timeout_replay();

        replay.validate().unwrap();
        let decoded = Replay::from_json(&replay.to_storage_json().unwrap()).unwrap();
        assert_eq!(decoded.version, REPLAY_SCHEMA_VERSION);
        let round = decoded
            .events
            .iter()
            .find(|event| matches!(event.kind, ReplayEventKind::RoundTimedOut { .. }))
            .unwrap();
        assert_eq!(
            round.kind,
            ReplayEventKind::RoundTimedOut {
                resolution: ResolutionKind::Single,
                submitted: [true, false],
                captures: [0, 2],
                cause: TimeoutCause::RoundTime,
            }
        );
        assert_eq!(round.frame.board.get(point(0, 0)), Cell::Empty);
        assert_eq!(round.frame.board.get(point(1, 0)), Cell::Empty);
        assert_eq!(round.frame.capture_totals, Score { black: 0, white: 2 });
        assert_eq!(
            decoded.audio_cues_between(0.0, decoded.duration_seconds),
            vec![GameCue::WhiteCapture]
        );

        let mut playback = PlaybackController::default();
        playback.fast_forward(&decoded);
        assert!(playback.advance_with_cues(1.0, &decoded).is_empty());
        playback.rewind();
        playback.toggle(&decoded);
        assert_eq!(
            playback.advance_with_cues(1.0, &decoded),
            vec![GameCue::WhiteCapture]
        );
        assert!(playback.advance_with_cues(1.0, &decoded).is_empty());
    }

    #[test]
    fn schema_v5_rejects_forged_suicide_capture_credit() {
        let mut forged = suicide_timeout_replay().to_in_memory_value().unwrap();
        let timeout = forged["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_timed_out")
            .unwrap();
        timeout["kind"]["captures"] = serde_json::json!([0, 1]);
        timeout["frame"]["capture_totals"] = serde_json::json!({"black": 0, "white": 1});

        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&forged).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "replay capture payload is impossible for the board transition"
        ));
    }

    #[test]
    fn schema_v5_rejects_forged_or_oversized_capture_payloads() {
        let replay = sequential_new_stone_capture_replay();
        let mut missing_delta = replay.to_in_memory_value().unwrap();
        let round = missing_delta["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_resolved")
            .unwrap();
        round["kind"]["captures"] = serde_json::json!([0, 0]);
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&missing_delta).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "replay capture totals do not match the round payload"
        ));

        let mut oversized = replay.to_in_memory_value().unwrap();
        let round = oversized["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_resolved")
            .unwrap();
        round["kind"]["captures"] = serde_json::json!([0, crate::rules::CELL_COUNT + 1]);
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&oversized).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "one round captures more stones than the board contains"
        ));
    }

    #[test]
    fn v5_timeout_deadlines_and_sparse_round_clock_are_validated() {
        let round_timeout = round_timeout_replay();
        round_timeout.validate().unwrap();
        let match_timeout = match_timeout_replay();
        match_timeout.validate().unwrap();

        let mut timers_off = round_timeout.to_in_memory_value().unwrap();
        timers_off["match_settings"]["round_time"]["enabled"] = serde_json::json!(false);
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&timers_off).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "round-time timeout occurs while its timer is disabled"
        ));

        let mut match_timer_off = round_timeout.to_in_memory_value().unwrap();
        let timeout = match_timer_off["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_timed_out")
            .unwrap();
        timeout["kind"]["cause"] = serde_json::json!("match_time");
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&match_timer_off).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "match-time timeout occurs while its timer is disabled"
        ));

        let mut missing_cause = round_timeout.to_in_memory_value().unwrap();
        let timeout = missing_cause["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_timed_out")
            .unwrap();
        timeout["kind"].as_object_mut().unwrap().remove("cause");
        assert!(Replay::from_json(&serde_json::to_string(&missing_cause).unwrap()).is_err());

        let mut before_deadline = round_timeout.to_in_memory_value().unwrap();
        let timeout = before_deadline["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_timed_out")
            .unwrap();
        timeout["at_seconds"] = serde_json::json!(4.0);
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&before_deadline).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "round-time timeout does not occur at its deadline"
        ));

        let mut wrong_cause = round_timeout.to_in_memory_value().unwrap();
        wrong_cause["match_settings"]["match_time"] =
            serde_json::json!({"enabled": true, "value": 10});
        let timeout = wrong_cause["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_timed_out")
            .unwrap();
        timeout["kind"]["cause"] = serde_json::json!("match_time");
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&wrong_cause).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "match-time timeout does not occur at its deadline"
        ));

        let mut wrong_priority = match_timeout.to_in_memory_value().unwrap();
        wrong_priority["match_settings"]["round_time"] =
            serde_json::json!({"enabled": true, "value": 5});
        let timeout = wrong_priority["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_timed_out")
            .unwrap();
        timeout["kind"]["cause"] = serde_json::json!("round_time");
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&wrong_priority).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "round-time timeout has the wrong deadline cause"
        ));
    }

    #[test]
    fn v5_rejects_forged_mid_round_clocks_and_uses_the_authoritative_round_start() {
        let mut forged_clock = sample_replay(1, 2, "Forged Clock");
        forged_clock.events[1].frame.round_elapsed_seconds = 0.25;
        assert!(matches!(
            forged_clock.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message == "replay round clock does not match its timeline"
        ));

        let mut forged_deadline = round_timeout_replay();
        forged_deadline.events[1].frame.round_elapsed_seconds = 3.0;
        forged_deadline.events[2].at_seconds = 4.0;
        assert!(forged_deadline.validate().is_err());
    }

    #[test]
    fn v5_timeout_rejects_forged_submissions_and_impossible_moves() {
        let replay = round_timeout_replay();
        let mut mismatched = replay.to_in_memory_value().unwrap();
        let timeout = mismatched["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|event| event["kind"]["type"] == "round_timed_out")
            .unwrap();
        timeout["kind"]["submitted"] = serde_json::json!([true, false]);
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&mismatched).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "timeout submissions do not match the preceding frame"
        ));

        let mut impossible = replay.to_in_memory_value().unwrap();
        let black = serde_json::json!("black");
        for event in impossible["events"].as_array_mut().unwrap() {
            event["frame"]["board"]["cells"][0] = black.clone();
        }
        impossible["final_board"]["cells"][0] = black;
        impossible["score"] = serde_json::json!({"black": 1, "white": 0});
        let events = impossible["events"].as_array_mut().unwrap();
        // (0,0) is forced Black above; attributing it to index 1 (White)
        // keeps this neither empty nor that player's own stone, so it stays
        // invalid even though a selecting player's own stone is now a
        // legitimate active-cleanup target.
        events[1]["frame"]["selected_moves"] =
            serde_json::json!([{"x": 8, "y": 8}, {"x": 0, "y": 0}]);
        events[2]["kind"]["submitted"] = serde_json::json!([true, true]);
        events[2]["kind"]["resolution"] = serde_json::json!("simultaneous");
        events[2]["frame"]["no_stone_streak"] = serde_json::json!(0);
        events.last_mut().unwrap()["frame"]["score"] = serde_json::json!({"black": 1, "white": 0});
        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&impossible).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "ordinary completion contains a selection that was neither empty \
                    nor the selecting player's own stone"
        ));
    }

    #[test]
    fn match_time_timeout_must_be_final_ordinary_completion() {
        let mut forged = match_timeout_replay().to_in_memory_value().unwrap();
        let events = forged["events"].as_array_mut().unwrap();
        let mut later = events[1].clone();
        later["kind"] = serde_json::json!({
            "type": "round_resolved",
            "resolution": "same_point_gray"
        });
        later["frame"]["current_round"] = serde_json::json!(3);
        later["frame"]["no_stone_streak"] = serde_json::json!(0);
        events.last_mut().unwrap()["frame"]["current_round"] = serde_json::json!(3);
        events.last_mut().unwrap()["frame"]["no_stone_streak"] = serde_json::json!(0);
        events.insert(events.len() - 1, later);
        forged["round_count"] = serde_json::json!(2);

        assert!(matches!(
            Replay::from_json(&serde_json::to_string(&forged).unwrap()),
            Err(ReplayError::InvalidData(ref message))
                if message == "match-time timeout is not the final ordinary completion"
        ));
    }
}
