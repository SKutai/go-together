//! Shared Replay fixtures used across the `replay` submodule tests: sample
//! frames and replays, and JSON-manipulation helpers for exercising older
//! schema versions.
//!
//! Every submodule's own `#[cfg(test)] mod tests` imports its fixtures from
//! here - most with `use crate::replay::test_support::*;`, a few by name -
//! instead of redefining them.
//!
//! Everything here is headless, like the code it tests. The mock
//! `platform::ReplayStorage` backends that used to live beside it are in
//! `replay_library::test_support`, because implementing that trait needs the
//! `client` feature and would pull every test that imports this module
//! behind it too. (Those paths are plain backticks, not intra-doc links:
//! both are client-only, and a link to either would be unresolved in the
//! headless documentation build.)

use crate::cue::GameCue;
use crate::match_settings::MatchSettings;
use crate::opening::{OpeningCountdownStage, OpeningSelectionOutcome, reveal_opening};
use crate::rules::{Board, Cell, Player, Point, ResolutionKind, Score};

use super::compat::compact_event_boards;
use super::schema::{
    COMPACT_BOARD_REPLAY_SCHEMA_VERSION, MATCH_RULES_REPLAY_SCHEMA_VERSION,
    OPENING_REPLAY_SCHEMA_VERSION, ROUND_TRACKING_REPLAY_SCHEMA_VERSION,
};
use super::{
    EndReason, MatchRecorder, Replay, ReplayEventKind, ReplayFrame, ReplayPhase, TimeoutCause,
};

pub(crate) fn point(x: usize, y: usize) -> Point {
    Point::new(x, y)
}

pub(crate) fn strip_v5_fields(value: &mut serde_json::Value) {
    value.as_object_mut().unwrap().remove("match_settings");
    value.as_object_mut().unwrap().remove("end_reasons");
    for event in value["events"].as_array_mut().unwrap() {
        let frame = event["frame"].as_object_mut().unwrap();
        frame.remove("capture_totals");
        frame.remove("no_stone_streak");
        frame.remove("round_elapsed_seconds");
        let kind = event["kind"].as_object_mut().unwrap();
        kind.remove("captures");
        kind.remove("submitted");
        kind.remove("cause");
    }
}

pub(crate) fn strip_v4_frame_fields(value: &mut serde_json::Value) {
    value.as_object_mut().unwrap().remove("opening_completed");
    for event in value["events"].as_array_mut().unwrap() {
        let frame = event["frame"].as_object_mut().unwrap();
        frame.remove("opening_selections");
        frame.remove("phase");
        frame.remove("opening_countdown");
    }
}

pub(crate) fn literal_replay_value(version: u32) -> serde_json::Value {
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
    let mut value = replay.to_in_memory_value().unwrap();
    if version < MATCH_RULES_REPLAY_SCHEMA_VERSION {
        strip_v5_fields(&mut value);
    }
    if version < OPENING_REPLAY_SCHEMA_VERSION {
        strip_v4_frame_fields(&mut value);
    }
    if version < ROUND_TRACKING_REPLAY_SCHEMA_VERSION {
        value.as_object_mut().unwrap().remove("round_count");
        for event in value["events"].as_array_mut().unwrap() {
            event["frame"]
                .as_object_mut()
                .unwrap()
                .remove("current_round");
        }
    }
    if version >= COMPACT_BOARD_REPLAY_SCHEMA_VERSION {
        compact_event_boards(value["events"].as_array_mut().unwrap());
    }
    value
}

pub(crate) fn assert_storage_schema(value: &serde_json::Value, version: u32) {
    let replay = value.as_object().unwrap();
    assert_eq!(replay["version"], serde_json::json!(version));
    assert_eq!(
        replay.contains_key("round_count"),
        version >= ROUND_TRACKING_REPLAY_SCHEMA_VERSION
    );
    assert_eq!(
        replay.contains_key("opening_completed"),
        version >= OPENING_REPLAY_SCHEMA_VERSION
    );
    assert_eq!(
        replay.contains_key("match_settings"),
        version >= MATCH_RULES_REPLAY_SCHEMA_VERSION
    );
    assert_eq!(
        replay.contains_key("end_reasons"),
        version >= MATCH_RULES_REPLAY_SCHEMA_VERSION
    );
    for event in replay["events"].as_array().unwrap() {
        let frame = event["frame"].as_object().unwrap();
        assert_eq!(
            frame.contains_key("current_round"),
            version >= ROUND_TRACKING_REPLAY_SCHEMA_VERSION
        );
        for field in ["opening_selections", "phase"] {
            assert_eq!(
                frame.contains_key(field),
                version >= OPENING_REPLAY_SCHEMA_VERSION
            );
        }
        if version < OPENING_REPLAY_SCHEMA_VERSION {
            assert!(!frame.contains_key("opening_countdown"));
        }
        for field in ["capture_totals", "no_stone_streak", "round_elapsed_seconds"] {
            assert_eq!(
                frame.contains_key(field),
                version >= MATCH_RULES_REPLAY_SCHEMA_VERSION
            );
        }
        if version < MATCH_RULES_REPLAY_SCHEMA_VERSION {
            let kind = event["kind"].as_object().unwrap();
            for field in ["captures", "submitted", "cause"] {
                assert!(!kind.contains_key(field));
            }
        }
    }
    if version >= COMPACT_BOARD_REPLAY_SCHEMA_VERSION {
        let events = replay["events"].as_array().unwrap();
        assert!(!events[0]["frame"]["board"].is_null());
        assert!(
            events[1]["frame"]["board"].is_null(),
            "sample_replay's second event (a cursor move) has the same board as its first, so \
             it must be compacted away in this schema version"
        );
        assert!(!events[2]["frame"]["board"].is_null());
    }
}

pub(crate) fn frame() -> ReplayFrame {
    ReplayFrame {
        board: Board::default(),
        cursors: [point(7, 4), point(1, 4)],
        teleport_markers: [None, None],
        selected_moves: [None, None],
        opening_selections: Default::default(),
        phase: ReplayPhase::Playing,
        opening_countdown: None,
        score: Score::default(),
        current_round: 1,
        capture_totals: Score::default(),
        no_stone_streak: 0,
        round_elapsed_seconds: 0.0,
    }
}

pub(crate) fn round_timeout_replay() -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_value(crate::match_settings::MatchSettingField::RoundTime, 5);
    settings.set_enabled(crate::match_settings::MatchSettingField::RoundTime, true);
    let mut recorder = MatchRecorder::for_test(1, frame(), settings);
    let checkpoint = ReplayFrame {
        cursors: [point(6, 4), point(1, 4)],
        round_elapsed_seconds: 2.0,
        ..frame()
    };
    recorder.record(
        2.0,
        ReplayEventKind::CursorMoved {
            player: Player::One,
            from: point(7, 4),
            to: point(6, 4),
        },
        checkpoint.clone(),
    );
    let timed_out = ReplayFrame {
        current_round: 2,
        no_stone_streak: 1,
        round_elapsed_seconds: 0.0,
        ..checkpoint
    };
    recorder.record(
        5.0,
        ReplayEventKind::RoundTimedOut {
            resolution: ResolutionKind::NoPlacement,
            submitted: [false, false],
            captures: [0, 0],
            cause: TimeoutCause::RoundTime,
        },
        timed_out.clone(),
    );
    recorder
        .complete(
            8,
            7.0,
            ReplayFrame {
                round_elapsed_seconds: 2.0,
                ..timed_out
            },
        )
        .unwrap()
}

pub(crate) fn match_timeout_replay() -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_value(crate::match_settings::MatchSettingField::MatchTime, 5);
    settings.set_enabled(crate::match_settings::MatchSettingField::MatchTime, true);
    let mut recorder = MatchRecorder::for_test(1, frame(), settings);
    let timed_out = ReplayFrame {
        current_round: 2,
        no_stone_streak: 1,
        ..frame()
    };
    recorder.record(
        5.0,
        ReplayEventKind::RoundTimedOut {
            resolution: ResolutionKind::NoPlacement,
            submitted: [false, false],
            captures: [0, 0],
            cause: TimeoutCause::MatchTime,
        },
        timed_out.clone(),
    );
    recorder
        .complete_with_reasons(6, 5.0, timed_out, vec![EndReason::MatchTimeLimit])
        .unwrap()
}

pub(crate) fn sequential_new_stone_capture_replay() -> Replay {
    let mut initial = frame();
    for point in [point(0, 1), point(1, 0), point(2, 1)] {
        initial.board.set(point, Cell::White);
    }
    let mut recorder = MatchRecorder::new(1, initial.clone());
    let early_point = point(1, 1);
    let late_point = point(1, 2);
    let early = ReplayFrame {
        selected_moves: [Some(early_point), None],
        round_elapsed_seconds: 0.25,
        ..initial.clone()
    };
    recorder.record(
        0.25,
        ReplayEventKind::SelectionChanged {
            player: Player::One,
            point: early_point,
        },
        early.clone(),
    );
    let both = ReplayFrame {
        selected_moves: [Some(early_point), Some(late_point)],
        round_elapsed_seconds: 0.5,
        ..early
    };
    recorder.record(
        0.5,
        ReplayEventKind::SelectionChanged {
            player: Player::Two,
            point: late_point,
        },
        both.clone(),
    );
    let early_outcome = initial.board.apply_move(Player::One, early_point).unwrap();
    assert!(early_outcome.opponent_stones_captured.is_empty());
    let late_outcome = early_outcome
        .board
        .apply_move(Player::Two, late_point)
        .unwrap();
    assert_eq!(late_outcome.opponent_stones_captured, vec![early_point]);
    let board = late_outcome.board;
    let resolved = ReplayFrame {
        board,
        selected_moves: [None, None],
        current_round: 2,
        capture_totals: Score { black: 0, white: 1 },
        round_elapsed_seconds: 0.0,
        ..both
    };
    recorder.record(
        0.5,
        ReplayEventKind::RoundResolved {
            resolution: ResolutionKind::Sequential,
            captures: [0, 1],
        },
        resolved.clone(),
    );
    let mut ended = resolved;
    ended.board.fill_territory();
    ended.score = ended.current_score(&MatchSettings::default());
    recorder.complete(2, 1.0, ended).unwrap()
}

pub(crate) fn suicide_timeout_replay() -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_capture_points(true);
    settings.set_value(crate::match_settings::MatchSettingField::RoundTime, 1);
    settings.set_enabled(crate::match_settings::MatchSettingField::RoundTime, true);
    let mut initial = frame();
    initial.board.set(point(0, 0), Cell::Black);
    for blocker in [point(0, 1), point(1, 1), point(2, 0)] {
        initial.board.set(blocker, Cell::White);
    }
    let mut recorder = MatchRecorder::for_test(1, initial.clone(), settings.clone());
    let selected_point = point(1, 0);
    let selected = ReplayFrame {
        selected_moves: [Some(selected_point), None],
        round_elapsed_seconds: 0.5,
        ..initial.clone()
    };
    recorder.record(
        0.5,
        ReplayEventKind::SelectionChanged {
            player: Player::One,
            point: selected_point,
        },
        selected.clone(),
    );
    let outcome = initial
        .board
        .apply_move(Player::One, selected_point)
        .unwrap();
    assert_eq!(
        outcome.acting_stones_self_removed,
        vec![point(0, 0), selected_point]
    );
    let resolved = ReplayFrame {
        board: outcome.board,
        selected_moves: [None, None],
        current_round: 2,
        capture_totals: Score { black: 0, white: 2 },
        round_elapsed_seconds: 0.0,
        ..selected
    };
    recorder.record(
        1.0,
        ReplayEventKind::RoundTimedOut {
            resolution: ResolutionKind::Single,
            submitted: [true, false],
            captures: [0, 2],
            cause: TimeoutCause::RoundTime,
        },
        resolved.clone(),
    );
    recorder.record(
        1.0,
        ReplayEventKind::MatchSound {
            cue: GameCue::WhiteCapture,
        },
        resolved.clone(),
    );
    let mut ended = resolved;
    ended.board.fill_territory();
    ended.score = ended.current_score(&settings);
    recorder.complete(2, 1.5, ended).unwrap()
}

pub(crate) fn missed_older_repetition_replay() -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_value(crate::match_settings::MatchSettingField::RoundTime, 1);
    settings.set_enabled(crate::match_settings::MatchSettingField::RoundTime, true);
    let mut initial = frame();
    let white_point = point(1, 1);
    let ko_point = point(1, 2);
    initial.board.set(white_point, Cell::White);
    for black in [point(0, 1), point(1, 0), point(2, 1)] {
        initial.board.set(black, Cell::Black);
    }
    for gray in [point(0, 2), point(2, 2), point(1, 3)] {
        initial.board.set(gray, Cell::Gray);
    }
    let mut recorder = MatchRecorder::for_test(1, initial.clone(), settings.clone());

    let black_selected = ReplayFrame {
        selected_moves: [Some(ko_point), None],
        round_elapsed_seconds: 0.5,
        ..initial.clone()
    };
    recorder.record(
        0.5,
        ReplayEventKind::SelectionChanged {
            player: Player::One,
            point: ko_point,
        },
        black_selected.clone(),
    );
    let black_outcome = initial.board.apply_move(Player::One, ko_point).unwrap();
    assert_eq!(black_outcome.opponent_stones_captured, vec![white_point]);
    let black_resolved = ReplayFrame {
        board: black_outcome.board,
        selected_moves: [None, None],
        current_round: 2,
        capture_totals: Score { black: 1, white: 0 },
        round_elapsed_seconds: 0.0,
        ..black_selected
    };
    recorder.record(
        1.0,
        ReplayEventKind::RoundTimedOut {
            resolution: ResolutionKind::Single,
            submitted: [true, false],
            captures: [1, 0],
            cause: TimeoutCause::RoundTime,
        },
        black_resolved.clone(),
    );

    let white_selected = ReplayFrame {
        selected_moves: [None, Some(white_point)],
        round_elapsed_seconds: 0.5,
        ..black_resolved
    };
    recorder.record(
        1.5,
        ReplayEventKind::SelectionChanged {
            player: Player::Two,
            point: white_point,
        },
        white_selected.clone(),
    );
    let white_outcome = white_selected
        .board
        .apply_move(Player::Two, white_point)
        .unwrap();
    assert_eq!(white_outcome.opponent_stones_captured, vec![ko_point]);
    assert_eq!(white_outcome.board, initial.board);
    let white_resolved = ReplayFrame {
        board: white_outcome.board,
        selected_moves: [None, None],
        current_round: 3,
        capture_totals: Score { black: 1, white: 1 },
        round_elapsed_seconds: 0.0,
        ..white_selected
    };
    recorder.record(
        2.0,
        ReplayEventKind::RoundTimedOut {
            resolution: ResolutionKind::Single,
            submitted: [false, true],
            captures: [0, 1],
            cause: TimeoutCause::RoundTime,
        },
        white_resolved.clone(),
    );
    let mut ended = white_resolved;
    ended.board.fill_territory();
    ended.score = ended.current_score(&settings);
    recorder.complete(2, 2.0, ended).unwrap()
}

pub(crate) fn sample_replay(started_at_ms: i64, completed_at_ms: i64, label: &str) -> Replay {
    let mut recorder = MatchRecorder::new(started_at_ms, frame());
    recorder.record(
        1.0,
        ReplayEventKind::CursorMoved {
            player: Player::One,
            from: point(7, 4),
            to: point(6, 4),
        },
        ReplayFrame {
            cursors: [point(6, 4), point(1, 4)],
            round_elapsed_seconds: 1.0,
            ..frame()
        },
    );
    let mut final_frame = frame();
    final_frame.board.set(point(4, 4), Cell::Black);
    final_frame.score = Score { black: 1, white: 0 };
    let mut replay = recorder
        .complete(completed_at_ms, 3.0, final_frame)
        .unwrap();
    replay.label = label.into();
    replay
}

pub(crate) fn round_zero_replay() -> Replay {
    let mut opening = ReplayFrame {
        cursors: [point(7, 4), point(1, 4)],
        phase: ReplayPhase::Opening,
        current_round: 0,
        ..Default::default()
    };
    let mut recorder = MatchRecorder::new(1, opening.clone());
    recorder.record(0.0, ReplayEventKind::OpeningStarted, opening.clone());
    for (player, points) in [
        (Player::One, [point(0, 0), point(1, 0), point(2, 0)]),
        (Player::Two, [point(1, 0), point(7, 8), point(8, 8)]),
    ] {
        for point in points {
            let outcome = opening.opening_selections[player.index()].add(point);
            let OpeningSelectionOutcome::Added { evicted } = outcome else {
                panic!("fixture points are unique and on-board");
            };
            recorder.record(
                0.25,
                ReplayEventKind::OpeningSelectionChanged {
                    player,
                    point,
                    evicted,
                },
                opening.clone(),
            );
        }
    }
    opening.opening_countdown = Some(OpeningCountdownStage::Three);
    recorder.record(
        0.25,
        ReplayEventKind::OpeningCountdown {
            stage: OpeningCountdownStage::Three,
        },
        opening.clone(),
    );
    recorder.record(
        0.25,
        ReplayEventKind::MatchSound {
            cue: GameCue::Countdown3,
        },
        opening.clone(),
    );
    let replacement = point(3, 0);
    let OpeningSelectionOutcome::Added { evicted } =
        opening.opening_selections[Player::One.index()].add(replacement)
    else {
        panic!("fixture replacement is distinct and on-board");
    };
    recorder.record(
        0.75,
        ReplayEventKind::OpeningSelectionChanged {
            player: Player::One,
            point: replacement,
            evicted,
        },
        opening.clone(),
    );
    for (at_seconds, stage, cue) in [
        (1.25, OpeningCountdownStage::Two, GameCue::Countdown2),
        (2.25, OpeningCountdownStage::One, GameCue::Countdown1),
    ] {
        opening.opening_countdown = Some(stage);
        recorder.record(
            at_seconds,
            ReplayEventKind::OpeningCountdown { stage },
            opening.clone(),
        );
        recorder.record(
            at_seconds,
            ReplayEventKind::MatchSound { cue },
            opening.clone(),
        );
    }
    let mut playing = ReplayFrame {
        board: reveal_opening(&opening.opening_selections),
        cursors: opening.cursors,
        teleport_markers: opening.teleport_markers,
        selected_moves: [None, None],
        opening_selections: Default::default(),
        phase: ReplayPhase::Playing,
        opening_countdown: Some(OpeningCountdownStage::Go),
        score: Score::default(),
        current_round: 1,
        capture_totals: Score::default(),
        no_stone_streak: 0,
        round_elapsed_seconds: 0.0,
    };
    recorder.record(3.25, ReplayEventKind::OpeningReveal, playing.clone());
    recorder.record(
        3.25,
        ReplayEventKind::OpeningCountdown {
            stage: OpeningCountdownStage::Go,
        },
        playing.clone(),
    );
    recorder.record(
        3.25,
        ReplayEventKind::MatchSound { cue: GameCue::Go },
        playing.clone(),
    );
    playing.opening_countdown = None;
    playing.round_elapsed_seconds = 0.75;
    recorder.record(
        4.0,
        ReplayEventKind::OpeningCountdownFinished,
        playing.clone(),
    );
    playing.phase = ReplayPhase::Ended;
    playing.score = playing.board.score();
    recorder.complete(5, 4.5, playing).unwrap()
}

/// A completed Round 0 whose reveal sweeps a stone: Black's corner pick at
/// (0,0) meets White's picks on both adjacent points, so
/// `resolve_opening_captures` removes the zero-liberty Black stone and
/// credits White one capture. The `OpeningReveal` frame therefore carries
/// the cleaned board and the sweep's credits - the shape
/// `MatchState::reveal_round_zero` records in production, which no other
/// fixture reaches because every other opening here is capture-free.
pub(crate) fn sweeping_reveal_replay() -> Replay {
    let mut opening = ReplayFrame {
        cursors: [point(7, 4), point(1, 4)],
        phase: ReplayPhase::Opening,
        current_round: 0,
        ..Default::default()
    };
    let mut recorder = MatchRecorder::new(1, opening.clone());
    recorder.record(0.0, ReplayEventKind::OpeningStarted, opening.clone());
    for (player, points) in [
        (Player::One, [point(0, 0), point(4, 4), point(6, 6)]),
        (Player::Two, [point(1, 0), point(0, 1), point(8, 8)]),
    ] {
        for point in points {
            let outcome = opening.opening_selections[player.index()].add(point);
            let OpeningSelectionOutcome::Added { evicted } = outcome else {
                panic!("fixture points are unique and on-board");
            };
            recorder.record(
                0.25,
                ReplayEventKind::OpeningSelectionChanged {
                    player,
                    point,
                    evicted,
                },
                opening.clone(),
            );
        }
    }
    for (at_seconds, stage, cue) in [
        (0.25, OpeningCountdownStage::Three, GameCue::Countdown3),
        (1.25, OpeningCountdownStage::Two, GameCue::Countdown2),
        (2.25, OpeningCountdownStage::One, GameCue::Countdown1),
    ] {
        opening.opening_countdown = Some(stage);
        recorder.record(
            at_seconds,
            ReplayEventKind::OpeningCountdown { stage },
            opening.clone(),
        );
        recorder.record(
            at_seconds,
            ReplayEventKind::MatchSound { cue },
            opening.clone(),
        );
    }
    let sweep = reveal_opening(&opening.opening_selections).resolve_opening_captures();
    let (cleaned_board, capture_credits) = (sweep.board, sweep.capture_credits);
    assert_eq!(
        cleaned_board.get(point(0, 0)),
        Cell::Empty,
        "the sweep fired"
    );
    let mut playing = ReplayFrame {
        board: cleaned_board,
        cursors: opening.cursors,
        teleport_markers: opening.teleport_markers,
        selected_moves: [None, None],
        opening_selections: Default::default(),
        phase: ReplayPhase::Playing,
        opening_countdown: Some(OpeningCountdownStage::Go),
        score: Score::default(),
        current_round: 1,
        capture_totals: Score {
            black: capture_credits[Player::One.index()],
            white: capture_credits[Player::Two.index()],
        },
        no_stone_streak: 0,
        round_elapsed_seconds: 0.0,
    };
    recorder.record(3.25, ReplayEventKind::OpeningReveal, playing.clone());
    recorder.record(
        3.25,
        ReplayEventKind::OpeningCountdown {
            stage: OpeningCountdownStage::Go,
        },
        playing.clone(),
    );
    recorder.record(
        3.25,
        ReplayEventKind::MatchSound { cue: GameCue::Go },
        playing.clone(),
    );
    playing.opening_countdown = None;
    playing.round_elapsed_seconds = 0.75;
    recorder.record(
        4.0,
        ReplayEventKind::OpeningCountdownFinished,
        playing.clone(),
    );
    playing.phase = ReplayPhase::Ended;
    playing.score = playing.current_score(&MatchSettings::default());
    recorder.complete(5, 4.5, playing).unwrap()
}

pub(crate) fn partial_opening_match_timeout_replay(
    reveal_at_seconds: f64,
    with_selection: bool,
) -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_value(crate::match_settings::MatchSettingField::MatchTime, 5);
    settings.set_enabled(crate::match_settings::MatchSettingField::MatchTime, true);
    let mut opening = ReplayFrame {
        cursors: [point(7, 4), point(1, 4)],
        phase: ReplayPhase::Opening,
        current_round: 0,
        ..Default::default()
    };
    let mut recorder = MatchRecorder::for_test(1, opening.clone(), settings.clone());
    recorder.record(0.0, ReplayEventKind::OpeningStarted, opening.clone());
    if with_selection {
        let selected = point(0, 0);
        assert!(matches!(
            opening.opening_selections[Player::One.index()].add(selected),
            OpeningSelectionOutcome::Added { evicted: None }
        ));
        recorder.record(
            1.0,
            ReplayEventKind::OpeningSelectionChanged {
                player: Player::One,
                point: selected,
                evicted: None,
            },
            opening.clone(),
        );
    }
    let mut playing = ReplayFrame {
        board: reveal_opening(&opening.opening_selections),
        cursors: opening.cursors,
        teleport_markers: opening.teleport_markers,
        phase: ReplayPhase::Playing,
        current_round: 1,
        ..Default::default()
    };
    recorder.record(
        reveal_at_seconds,
        ReplayEventKind::OpeningReveal,
        playing.clone(),
    );
    playing.phase = ReplayPhase::Ended;
    playing.board.fill_territory();
    playing.score = playing.current_score(&settings);
    recorder.record(
        5.0,
        ReplayEventKind::MatchSound {
            cue: GameCue::MatchEnd,
        },
        playing.clone(),
    );
    recorder
        .complete_with_reasons(6, 5.0, playing, vec![EndReason::MatchTimeLimit])
        .unwrap()
}

pub(crate) fn no_stone_then_placement_replay() -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_value(crate::match_settings::MatchSettingField::RoundTime, 1);
    settings.set_enabled(crate::match_settings::MatchSettingField::RoundTime, true);
    settings.set_value(
        crate::match_settings::MatchSettingField::NoStoneRoundLimit,
        1,
    );
    settings.set_enabled(
        crate::match_settings::MatchSettingField::NoStoneRoundLimit,
        true,
    );
    let mut recorder = MatchRecorder::for_test(1, frame(), settings.clone());
    let first = ReplayFrame {
        current_round: 2,
        no_stone_streak: 1,
        ..frame()
    };
    recorder.record(
        1.0,
        ReplayEventKind::RoundTimedOut {
            resolution: ResolutionKind::NoPlacement,
            submitted: [false, false],
            captures: [0, 0],
            cause: TimeoutCause::RoundTime,
        },
        first.clone(),
    );
    let mut second = ReplayFrame {
        current_round: 3,
        no_stone_streak: 0,
        ..first
    };
    second.board.set(point(0, 0), Cell::Black);
    second.board.set(point(8, 8), Cell::White);
    recorder.record(
        1.5,
        ReplayEventKind::RoundResolved {
            resolution: ResolutionKind::Simultaneous,
            captures: [0, 0],
        },
        second.clone(),
    );
    second.score = second.current_score(&settings);
    recorder
        .complete_with_reasons(3, 1.5, second, vec![EndReason::Manual])
        .unwrap()
}

pub(crate) fn point_target_then_loss_replay() -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_value(crate::match_settings::MatchSettingField::PointTarget, 2);
    settings.set_enabled(crate::match_settings::MatchSettingField::PointTarget, true);
    let mut recorder = MatchRecorder::for_test(1, frame(), settings.clone());
    let mut first = ReplayFrame {
        current_round: 2,
        ..frame()
    };
    first.board.set(point(0, 0), Cell::Black);
    first.board.set(point(1, 0), Cell::Black);
    recorder.record(
        1.0,
        ReplayEventKind::RoundResolved {
            resolution: ResolutionKind::Simultaneous,
            captures: [0, 0],
        },
        first.clone(),
    );
    let mut second = ReplayFrame {
        current_round: 3,
        capture_totals: Score { black: 0, white: 1 },
        ..first
    };
    second.board.set(point(1, 0), Cell::Empty);
    second.board.set(point(8, 8), Cell::White);
    recorder.record(
        2.0,
        ReplayEventKind::RoundResolved {
            resolution: ResolutionKind::Simultaneous,
            captures: [0, 1],
        },
        second.clone(),
    );
    second.score = second.current_score(&settings);
    recorder
        .complete_with_reasons(3, 2.0, second, vec![EndReason::Manual])
        .unwrap()
}

pub(crate) fn extra_round_after_round_limit_replay() -> Replay {
    let mut settings = MatchSettings::default();
    settings.set_value(crate::match_settings::MatchSettingField::RoundLimit, 1);
    settings.set_enabled(crate::match_settings::MatchSettingField::RoundLimit, true);
    let mut recorder = MatchRecorder::for_test(1, frame(), settings.clone());
    let mut first = ReplayFrame {
        current_round: 2,
        ..frame()
    };
    // A genuine SamePointGray round always leaves a new Gray stone, so
    // the board actually changes and the no-stone streak stays at 0.
    first.board.set(point(4, 4), Cell::Gray);
    recorder.record(
        1.0,
        ReplayEventKind::RoundResolved {
            resolution: ResolutionKind::SamePointGray,
            captures: [0, 0],
        },
        first.clone(),
    );
    let mut second = ReplayFrame {
        current_round: 3,
        ..first
    };
    second.board.set(point(5, 4), Cell::Gray);
    recorder.record(
        2.0,
        ReplayEventKind::RoundResolved {
            resolution: ResolutionKind::SamePointGray,
            captures: [0, 0],
        },
        second.clone(),
    );
    recorder
        .complete_with_reasons(3, 2.0, second, vec![EndReason::Manual])
        .unwrap()
}

pub(crate) fn replay_with_audio_track() -> Replay {
    let mut recorder = MatchRecorder::new(1, frame());
    for (at_seconds, cue) in [
        (0.5, GameCue::BlackTeleport),
        (1.0, GameCue::BothCapture),
        (1.5, GameCue::GrayCapture),
        (2.0, GameCue::MatchEnd),
    ] {
        recorder.record(at_seconds, ReplayEventKind::MatchSound { cue }, frame());
    }
    recorder.complete(3, 2.0, frame()).unwrap()
}
