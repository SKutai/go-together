//! Integration test proving `go_together`'s Replay layer works for an
//! external, headless library consumer: this file reaches only through the
//! published API (`go_together::replay`, `go_together::cue`, and the value
//! types they expose), never touches Bevy, and runs under
//! `cargo test --no-default-features` - so if anything it needs is not
//! headlessly `pub`, it fails to compile rather than silently proving less.
//!
//! The inputs are the literal on-disk files under `tests/fixtures/replays/`,
//! one per supported schema version plus seven richer shapes; see the README
//! there for how they are produced and pinned. Together the tests below
//! cover the four verbs `src/lib.rs` promises a headless consumer: load,
//! validate, inspect, serialize - plus playing a Replay back.

use go_together::authority::player_index;
use go_together::cue::GameCue;
use go_together::match_settings::{MatchSettingField, MatchSettings};
use go_together::opening::OpeningCountdownStage;
use go_together::replay::{
    COMPACT_BOARD_REPLAY_SCHEMA_VERSION, EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION, EndReason,
    LEGACY_REPLAY_SCHEMA_VERSION, LabelError, MATCH_RULES_REPLAY_SCHEMA_VERSION,
    MAX_REPLAY_JSON_BYTES, OPENING_REPLAY_SCHEMA_VERSION, PLAYBACK_SPEEDS, PlaybackController,
    REPLAY_SCHEMA_VERSION, Replay, ReplayError, ReplayEventKind, ReplayPhase, ReplayWinner,
    TimeoutCause, validate_label,
};
use go_together::rules::{Cell, Player, Point, Score};

const FIXTURE_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/replays/");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURE_DIRECTORY}{name}"))
        .unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}

fn load(name: &str) -> Replay {
    Replay::from_json(&fixture(name)).unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}

/// Every schema version loads, reports itself truthfully through the
/// accessors, and re-serializes to exactly the bytes it was loaded from - so
/// a Replay recorded by any build this crate has shipped survives a round
/// trip through this one unchanged.
#[test]
fn every_schema_version_loads_validates_and_round_trips_byte_for_byte() {
    for version in LEGACY_REPLAY_SCHEMA_VERSION..=REPLAY_SCHEMA_VERSION {
        let name = format!("v{version}.json");
        let json = fixture(&name);
        let replay = Replay::from_json(&json).unwrap_or_else(|error| panic!("{name}: {error}"));
        replay.validate().unwrap();

        assert_eq!(replay.version(), version, "{name}");
        assert_eq!(
            replay.id(),
            format!("00000000-0000-4000-8000-{:012}", version),
            "{name}"
        );
        assert_eq!(replay.label(), format!("Schema {version}"), "{name}");
        assert_eq!(replay.started_at_ms(), 1, "{name}");
        assert_eq!(replay.completed_at_ms(), 2, "{name}");
        assert_eq!(replay.duration_seconds(), 3.0, "{name}");
        assert_eq!(replay.final_score(), Score { black: 1, white: 0 }, "{name}");
        assert_eq!(replay.winner(), ReplayWinner::Black, "{name}");
        assert_eq!(replay.winner().to_string(), "Black", "{name}");
        assert_eq!(
            replay.final_board().get(Point::new(4, 4)),
            Cell::Black,
            "{name}"
        );
        assert_eq!(replay.completed_round_count(), 0, "{name}");
        assert!(!replay.opening_completed(), "{name}");
        assert_eq!(*replay.match_settings(), MatchSettings::default(), "{name}");
        // Below v5 the file never said why play ended, and the accessor says
        // so rather than inventing a reason.
        let expected_reasons = if version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
            [EndReason::Manual]
        } else {
            [EndReason::Legacy]
        };
        assert_eq!(replay.end_reasons(), expected_reasons, "{name}");

        // The complete record: three events, complete frames in memory even
        // where v6 omitted an unchanged board on disk.
        let events = replay.events();
        assert_eq!(events.len(), 3, "{name}");
        assert!(matches!(events[0].kind(), ReplayEventKind::MatchStart));
        assert_eq!(events[0].at_seconds(), 0.0);
        assert!(matches!(
            events[1].kind(),
            ReplayEventKind::CursorMoved { player: Player::One, from, to, .. }
                if *from == Point::new(7, 4) && *to == Point::new(6, 4)
        ));
        assert_eq!(events[1].at_seconds(), 1.0);
        assert_eq!(
            events[1].frame().board(),
            events[0].frame().board(),
            "{name}"
        );
        assert!(matches!(events[2].kind(), ReplayEventKind::MatchEnd));
        assert_eq!(events[2].at_seconds(), replay.duration_seconds());
        assert_eq!(events[2].frame().board(), replay.final_board(), "{name}");
        assert_eq!(
            events[2].frame().current_score(replay.match_settings()),
            replay.final_score(),
            "{name}"
        );
        // Frame fields report what their version recorded, not more.
        assert_eq!(
            events[1].frame().phase(),
            ReplayPhase::Playing,
            "{name}: every version is Playing mid-Match"
        );
        assert_eq!(
            events[2].frame().phase(),
            if version >= OPENING_REPLAY_SCHEMA_VERSION {
                ReplayPhase::Ended
            } else {
                ReplayPhase::Playing
            },
            "{name}: the final frame's phase exists from v4"
        );
        assert_eq!(
            events[1].frame().round_elapsed_seconds(),
            if version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
                1.0
            } else {
                0.0
            },
            "{name}: the round clock exists from v5"
        );

        // Serialize: the on-disk shape for this version, byte-for-byte.
        assert_eq!(replay.to_storage_json().unwrap(), json, "{name}");
        let value = replay.to_storage_value().unwrap();
        assert_eq!(value["version"], serde_json::json!(version), "{name}");
        assert_eq!(
            value.get("match_settings").is_some(),
            version >= MATCH_RULES_REPLAY_SCHEMA_VERSION,
            "{name}: v5 introduced match_settings"
        );
        let reloaded = Replay::from_value(value.clone()).unwrap();
        assert_eq!(reloaded, replay, "{name}");

        // Validate, in the negative direction, for this version's own shape:
        // the same forgeries the game rejects are rejected here.
        let mut forged_board = value.clone();
        forged_board["final_board"]["cells"][0] = serde_json::json!("white");
        assert!(
            matches!(
                Replay::from_value(forged_board),
                Err(ReplayError::InvalidData(message))
                    if message == "final event does not match replay result"
            ),
            "{name}"
        );
        let mut forged_id = value;
        forged_id["id"] = serde_json::json!("not-a-uuid");
        assert!(
            matches!(Replay::from_value(forged_id), Err(ReplayError::InvalidId)),
            "{name}"
        );
        // Those two methods are the whole serialization surface - `Replay`
        // implements no `Serialize`, so a consumer cannot reach the complete
        // in-memory shape and write it out by mistake - and what they emit is
        // narrower than that shape in two directions. Fields the version
        // never had are stripped, as `match_settings` shows above; and from
        // v6 an unchanged board is omitted, even though the frame it came
        // from carries a complete one in memory (asserted above).
        let storage_shape = replay.to_storage_value().unwrap();
        assert_eq!(
            storage_shape["events"][1]["frame"]["board"].is_null(),
            version >= COMPACT_BOARD_REPLAY_SCHEMA_VERSION,
            "{name}: v6 omits the cursor move's unchanged board"
        );
        assert!(
            !storage_shape["events"][0]["frame"]["board"].is_null(),
            "{name}: the first board is never omitted"
        );
    }
}

/// Every checked-in fixture still loads when pretty-printed, and yields the
/// identical `Replay`.
///
/// The fixtures are compact because that is what the writer emits, but
/// builds through 2026-09-11 pretty-printed their files, so the Replays
/// already on players' disks are indented. Nothing else in this suite reads
/// an indented file any more - the fixtures used to *be* that coverage - so
/// without this test the whitespace change that shrank a native file could
/// silently stop reading every Replay recorded before it.
///
/// It re-indents each fixture rather than checking in a second copy so that
/// the two forms cannot drift: the pretty text is the same value by
/// construction, which is the property under test.
#[test]
fn a_pretty_printed_replay_file_still_loads_identically() {
    let mut checked = 0;
    for entry in std::fs::read_dir(FIXTURE_DIRECTORY).expect("fixture directory") {
        let path = entry.expect("fixture entry").path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let name = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap();
        let compact = fixture(name);
        let value: serde_json::Value = serde_json::from_str(&compact).expect(name);
        let pretty = serde_json::to_string_pretty(&value).expect(name);

        assert!(
            pretty.len() > compact.len(),
            "{name}: the re-indented form must actually differ from the checked-in one, or \
             this test proves nothing"
        );
        let from_pretty = Replay::from_json(&pretty)
            .unwrap_or_else(|error| panic!("{name}: an indented file no longer loads: {error}"));
        from_pretty
            .validate()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            from_pretty,
            Replay::from_json(&compact).unwrap_or_else(|error| panic!("{name}: {error}")),
            "{name}: indentation changed the Replay it decodes to"
        );
        // And it re-serializes to the compact form, so opening and saving an
        // older file migrates it to the smaller encoding rather than
        // preserving the bytes it came from.
        assert_eq!(
            from_pretty.to_storage_json().unwrap(),
            compact,
            "{name}: rewriting an indented file must produce the current encoding"
        );
        checked += 1;
    }
    assert_eq!(
        checked, 13,
        "every fixture must be covered; add the new one's count here"
    );
}

/// Playback answers version differences itself: a v1 frame never recorded
/// its round number and a v1/v2 file never recorded sounds, yet the seek and
/// audio APIs report the reconstructed and derived values for them.
#[test]
fn playback_reconstructs_what_older_schema_versions_did_not_record() {
    for version in LEGACY_REPLAY_SCHEMA_VERSION..=REPLAY_SCHEMA_VERSION {
        let name = format!("v{version}.json");
        let replay = load(&name);

        let raw_round = replay.events()[0].frame().current_round();
        assert_eq!(
            raw_round,
            if version == LEGACY_REPLAY_SCHEMA_VERSION {
                0
            } else {
                1
            },
            "{name}: v1 frames persist no round number"
        );
        assert_eq!(replay.playback_frame_at(0.0).current_round(), 1, "{name}");
        assert_eq!(
            replay.playback_frame_at(0.5).cursors()[player_index(Player::One)],
            Point::new(7, 4),
            "{name}: before the move"
        );
        assert_eq!(
            replay.playback_frame_at(1.0).cursors()[player_index(Player::One)],
            Point::new(6, 4),
            "{name}: at the move"
        );
        assert_eq!(
            replay.playback_frame_at(99.0).phase(),
            replay.events()[2].frame().phase()
        );
        assert_eq!(replay.playback_boundary_key(0.99), Some(0), "{name}");
        assert_eq!(replay.playback_boundary_key(1.0), Some(1), "{name}");
        assert_eq!(replay.significant_times(), [0.0, 3.0], "{name}");

        let cues = replay.audio_cues_between(0.0, replay.duration_seconds());
        if version >= EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION {
            assert!(cues.is_empty(), "{name}: no sound events were recorded");
        } else {
            assert_eq!(cues, [GameCue::MatchEnd], "{name}: derived from the events");
        }
    }
}

/// A v2 Replay's derived audio track, driven through the transport the way
/// a headless harness would: one tick at a time, at more than one speed.
#[test]
fn the_transport_plays_a_legacy_audio_track_at_every_speed() {
    let replay = load("v2-legacy-audio.json");
    assert_eq!(replay.version(), 2);
    assert_eq!(replay.completed_round_count(), 1);
    assert_eq!(replay.significant_times(), [0.0, 0.5, 1.0, 2.0]);
    assert_eq!(replay.playback_frame_at(0.9).current_round(), 1);
    assert_eq!(replay.playback_frame_at(1.0).current_round(), 2);

    let expected = [
        GameCue::BlackTeleport,
        GameCue::SamePointGray,
        GameCue::MatchEnd,
    ];
    for speed in PLAYBACK_SPEEDS {
        let mut controller = PlaybackController::default();
        assert!(!controller.is_playing());
        assert!(controller.at_start());
        controller.set_speed(speed);
        assert_eq!(controller.speed(), speed);
        controller.toggle(&replay);
        assert!(controller.is_playing());

        let mut heard = Vec::new();
        let mut ticks = 0;
        while controller.is_playing() {
            heard.extend(controller.advance_with_cues(0.1, &replay));
            ticks += 1;
            assert!(
                ticks < 1_000,
                "speed {speed}: playback never reached the end"
            );
        }
        assert_eq!(heard, expected, "speed {speed}");
        assert!(controller.at_end(&replay));
        assert_eq!(controller.position_seconds(), replay.duration_seconds());
        // At the end there is nothing to play and nothing to hear.
        controller.toggle(&replay);
        assert!(!controller.is_playing());
        assert!(controller.advance_with_cues(1.0, &replay).is_empty());
    }
}

/// Seeking is silent and the seek methods land on the significant times;
/// `pause` stops without moving; an unsupported speed is ignored.
#[test]
fn seeking_is_silent_and_lands_on_significant_times() {
    let replay = load("v2-legacy-audio.json");
    let mut controller = PlaybackController::default();

    controller.jump_forward(&replay);
    assert_eq!(controller.position_seconds(), 0.5);
    controller.jump_forward(&replay);
    assert_eq!(controller.position_seconds(), 1.0);
    controller.jump_back(&replay);
    assert_eq!(controller.position_seconds(), 0.5);
    controller.fast_forward(&replay);
    assert!(controller.at_end(&replay));
    controller.rewind();
    assert!(controller.at_start());
    // None of that produced a cue, and the next real tick starts from zero.
    assert!(controller.advance_with_cues(1.0, &replay).is_empty());

    controller.toggle(&replay);
    controller.set_speed(4.0);
    assert_eq!(controller.speed(), 1.0, "only PLAYBACK_SPEEDS are accepted");
    controller.advance_with_cues(0.25, &replay);
    controller.pause();
    assert!(!controller.is_playing());
    assert_eq!(controller.position_seconds(), 0.25);
    controller.pause();
    assert_eq!(controller.position_seconds(), 0.25);
}

/// A complete Round 0 as a v6 file: the opening phase, both players'
/// selections, the countdown with its recorded cues, and the reveal are all
/// visible to a consumer, and the recorded sound track plays through the
/// transport.
#[test]
fn a_round_zero_replay_exposes_the_opening_and_its_recorded_cues() {
    let replay = load("v6-round-zero.json");
    assert_eq!(replay.version(), REPLAY_SCHEMA_VERSION);
    assert!(replay.opening_completed());
    assert_eq!(replay.completed_round_count(), 0);
    assert_eq!(replay.final_score(), Score { black: 2, white: 2 });
    assert_eq!(replay.winner(), ReplayWinner::Draw);
    assert_eq!(replay.end_reasons(), [EndReason::Manual]);
    assert_eq!(
        replay.significant_times(),
        [0.0, 0.25, 0.75, 1.25, 2.25, 3.25, 4.5]
    );

    let opening = replay.playback_frame_at(0.1);
    assert_eq!(opening.phase(), ReplayPhase::Opening);
    assert_eq!(opening.current_round(), 0);
    assert_eq!(opening.opening_countdown(), None);

    // Black's fourth pick evicted the oldest of the three (FIFO), which is
    // exactly what the recorded frame shows.
    let selecting = replay.playback_frame_at(0.75);
    assert_eq!(
        selecting.opening_selections()[player_index(Player::One)].as_slice(),
        [Point::new(1, 0), Point::new(2, 0), Point::new(3, 0)]
    );
    assert_eq!(
        selecting.opening_selections()[player_index(Player::Two)].len(),
        3
    );
    assert_eq!(
        selecting.opening_countdown(),
        Some(OpeningCountdownStage::Three)
    );

    let revealed = replay.playback_frame_at(3.25);
    assert_eq!(revealed.phase(), ReplayPhase::Playing);
    assert_eq!(revealed.current_round(), 1);
    assert_eq!(
        revealed.opening_countdown(),
        Some(OpeningCountdownStage::Go)
    );
    assert!(
        revealed
            .opening_selections()
            .iter()
            .all(|selections| selections.is_empty())
    );
    assert_eq!(
        revealed.board().get(Point::new(1, 0)),
        Cell::Gray,
        "picked by both"
    );
    assert_eq!(revealed.board().get(Point::new(2, 0)), Cell::Black);
    assert_eq!(revealed.board().get(Point::new(7, 8)), Cell::White);
    assert_eq!(replay.playback_frame_at(4.0).opening_countdown(), None);
    assert_eq!(replay.playback_frame_at(4.5).phase(), ReplayPhase::Ended);

    let recorded: Vec<GameCue> = replay
        .events()
        .iter()
        .filter_map(|event| match event.kind() {
            ReplayEventKind::MatchSound { cue, .. } => Some(*cue),
            _ => None,
        })
        .collect();
    assert_eq!(
        recorded,
        [
            GameCue::OpeningDuplicate,
            GameCue::Countdown3,
            GameCue::Countdown2,
            GameCue::Countdown1,
            GameCue::Go
        ]
    );
    assert_eq!(replay.events()[2].at_seconds(), 0.0);
    assert_eq!(
        replay.audio_cues_between(0.0, 0.1),
        [GameCue::OpeningDuplicate],
        "an interval starting at zero includes the cue recorded at zero"
    );
    assert!(
        replay.audio_cues_between(0.1, 0.2).is_empty(),
        "and no later interval repeats it"
    );
    let mut controller = PlaybackController::default();
    controller.toggle(&replay);
    assert_eq!(
        controller.advance_with_cues(10.0, &replay),
        recorded,
        "forward playback from the start hears the recorded track once, time-zero cue included"
    );
}

/// A Round 0 reveal that sweeps a stone: Black's corner pick met White's
/// picks on both adjacent points, so the recorded reveal frame carries the
/// cleaned board - the corner already empty - and White's sweep credit.
/// This is the shape whose Replays validation used to reject at save time,
/// so the fixture existing (it is validated on load, like every fixture
/// here) is itself the regression proof.
#[test]
fn a_sweeping_reveal_replay_exposes_the_cleaned_board_and_credit() {
    let replay = load("v6-sweeping-reveal.json");
    assert_eq!(replay.version(), REPLAY_SCHEMA_VERSION);
    assert!(replay.opening_completed());

    let one = replay.playback_frame_at(2.25);
    assert_eq!(one.phase(), ReplayPhase::Opening);
    assert_eq!(
        one.opening_selections()[player_index(Player::One)].as_slice()[0],
        Point::new(0, 0),
        "Black's corner pick is still on the board pre-reveal"
    );

    let revealed = replay.playback_frame_at(3.25);
    assert_eq!(revealed.phase(), ReplayPhase::Playing);
    assert_eq!(revealed.current_round(), 1);
    assert_eq!(
        revealed.board().get(Point::new(0, 0)),
        Cell::Empty,
        "the zero-liberty corner stone was swept before the frame was recorded"
    );
    assert_eq!(revealed.board().get(Point::new(1, 0)), Cell::White);
    assert_eq!(revealed.board().get(Point::new(0, 1)), Cell::White);
    assert_eq!(revealed.board().get(Point::new(4, 4)), Cell::Black);
    assert_eq!(
        revealed.capture_totals(),
        Score { black: 0, white: 1 },
        "the sweep's credit is on the reveal frame itself"
    );
    assert_eq!(replay.final_score(), Score { black: 2, white: 3 });
}

/// A round completed by timeout under an enabled round-time rule: the
/// settings snapshot, the timeout's cause and submissions, the no-stone
/// streak, and the interpolated round clock all read back.
#[test]
fn a_timed_out_round_exposes_its_rule_cause_and_running_clock() {
    let replay = load("v6-round-timeout.json");
    let settings = replay.match_settings();
    assert!(settings.enabled(MatchSettingField::RoundTime));
    assert_eq!(settings.value(MatchSettingField::RoundTime), 5);
    assert_eq!(replay.completed_round_count(), 1);

    let timed_out = &replay.events()[2];
    assert_eq!(timed_out.at_seconds(), 5.0);
    let ReplayEventKind::RoundTimedOut {
        submitted, cause, ..
    } = timed_out.kind()
    else {
        panic!("the third event is the timeout");
    };
    assert_eq!(*submitted, [false, false]);
    assert_eq!(*cause, TimeoutCause::RoundTime);
    assert_eq!(timed_out.frame().current_round(), 2);
    assert_eq!(timed_out.frame().no_stone_streak(), 1);
    assert_eq!(timed_out.frame().capture_totals(), Score::default());
    assert_eq!(timed_out.frame().selected_moves(), [None, None]);
    assert_eq!(timed_out.frame().teleport_markers(), [None, None]);

    // The recorded frames hold the clock at their events; the playback view
    // advances it between them and resets it at the completion.
    assert_eq!(replay.events()[1].frame().round_elapsed_seconds(), 2.0);
    assert_eq!(replay.playback_frame_at(1.0).round_elapsed_seconds(), 1.0);
    assert_eq!(replay.playback_frame_at(4.5).round_elapsed_seconds(), 4.5);
    assert_eq!(replay.playback_frame_at(5.0).round_elapsed_seconds(), 0.0);
    assert_eq!(replay.playback_frame_at(5.5).round_elapsed_seconds(), 0.5);
    assert_eq!(replay.playback_frame_at(7.0).phase(), ReplayPhase::Ended);
    assert_eq!(replay.playback_frame_at(7.0).round_elapsed_seconds(), 2.0);
}

/// Validation does real work headlessly: the same forged, oversized, and
/// unsupported inputs the game rejects are rejected here, through both
/// loading paths, with the documented error variants.
#[test]
fn hostile_input_is_rejected_through_both_loading_paths() {
    let json = fixture("v6.json");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    let oversized = " ".repeat(MAX_REPLAY_JSON_BYTES + 1);
    assert!(matches!(
        Replay::from_json(&oversized),
        Err(ReplayError::DataTooLarge { .. })
    ));

    let not_a_replay = Replay::from_json("{").unwrap_err();
    assert!(matches!(not_a_replay, ReplayError::Json(_)));
    assert!(
        std::error::Error::source(&not_a_replay).is_some(),
        "the serde error is exposed as the source"
    );
    // Exactly one JSON document: trailing whitespace is fine, trailing
    // content is not - the same acceptance `serde_json::from_str` has.
    assert!(Replay::from_json(&format!("{json}\n\n")).is_ok());
    assert!(matches!(
        Replay::from_json(&format!("{json} {{}}")),
        Err(ReplayError::Json(_))
    ));

    let mut future = value.clone();
    future["version"] = serde_json::json!(99);
    assert!(matches!(
        Replay::from_value(future.clone()),
        Err(ReplayError::UnsupportedVersion(99))
    ));
    assert!(matches!(
        Replay::from_json(&future.to_string()),
        Err(ReplayError::UnsupportedVersion(99))
    ));

    let mut negative_duration = value.clone();
    negative_duration["duration_seconds"] = serde_json::json!(-1.0);
    assert!(matches!(
        Replay::from_value(negative_duration),
        Err(ReplayError::InvalidData(message)) if message == "invalid replay duration"
    ));

    // A forged final board: the file's `final_board` no longer matches the
    // last frame, which only full validation notices.
    let mut forged = value.clone();
    forged["final_board"]["cells"][0] = serde_json::json!("white");
    assert!(matches!(
        Replay::from_value(forged),
        Err(ReplayError::InvalidData(message)) if message == "final event does not match replay result"
    ));

    // A forged result: the Replay's score and its final frame agree with
    // each other but not with the board they claim to score.
    let mut forged_score = value;
    forged_score["events"][2]["frame"]["score"]["black"] = serde_json::json!(7);
    forged_score["score"]["black"] = serde_json::json!(7);
    assert!(matches!(
        Replay::from_value(forged_score),
        Err(ReplayError::InvalidData(message))
            if message == "final score does not match the final board"
    ));
}

/// A Match ended by its match-time limit: the timeout's `match_time` cause,
/// the `match_time_limit` end reason, and the settings that explain both.
#[test]
fn a_match_time_timeout_reports_its_cause_and_end_reason() {
    let replay = load("v6-match-timeout.json");
    assert!(
        replay
            .match_settings()
            .enabled(MatchSettingField::MatchTime)
    );
    assert_eq!(
        replay.match_settings().value(MatchSettingField::MatchTime),
        5
    );
    assert_eq!(replay.duration_seconds(), 5.0);
    assert_eq!(replay.end_reasons(), [EndReason::MatchTimeLimit]);
    assert_eq!(replay.end_reasons()[0].to_string(), "Match Time Limit");
    assert_eq!(replay.completed_round_count(), 1);

    let ReplayEventKind::RoundTimedOut { cause, .. } = replay.events()[1].kind() else {
        panic!("the second event is the timeout");
    };
    assert_eq!(*cause, TimeoutCause::MatchTime);
    assert_eq!(replay.events()[1].at_seconds(), 5.0);
    assert!(matches!(
        replay.events()[2].kind(),
        ReplayEventKind::MatchEnd
    ));
    assert_eq!(replay.significant_times(), [0.0, 5.0]);
}

/// Two locked selections resolving in order with a capture: the locks are
/// visible in the frames before the round completes, the round's capture
/// payload is nonzero, and the running totals follow it.
#[test]
fn a_sequential_capture_exposes_locks_payload_and_totals() {
    let replay = load("v6-sequential-capture.json");
    let events = replay.events();
    assert_eq!(events.len(), 5);
    assert!(matches!(
        events[1].kind(),
        ReplayEventKind::SelectionChanged { player: Player::One, point, .. }
            if *point == Point::new(1, 1)
    ));
    assert_eq!(
        events[1].frame().selected_moves(),
        [Some(Point::new(1, 1)), None]
    );
    assert_eq!(
        events[2].frame().selected_moves(),
        [Some(Point::new(1, 1)), Some(Point::new(1, 2))],
        "both locks are visible after the fact"
    );
    let ReplayEventKind::RoundResolved {
        resolution,
        captures,
        ..
    } = events[3].kind()
    else {
        panic!("the fourth event resolves the round");
    };
    assert_eq!(*resolution, go_together::rules::ResolutionKind::Sequential);
    assert_eq!(
        *captures,
        [0, 1],
        "White captured the stone Black had just placed"
    );
    assert_eq!(
        events[3].frame().capture_totals(),
        Score { black: 0, white: 1 }
    );
    assert_eq!(events[3].frame().selected_moves(), [None, None]);
    assert_eq!(events[3].frame().current_round(), 2);
    assert_eq!(events[3].frame().board().get(Point::new(1, 1)), Cell::Empty);
    assert_eq!(events[3].frame().board().get(Point::new(1, 2)), Cell::White);
    assert_eq!(replay.significant_times(), [0.0, 0.5, 1.0]);
    assert_eq!(replay.completed_round_count(), 1);
    assert_eq!(
        replay.final_score(),
        events[4].frame().current_score(replay.match_settings())
    );
}

/// Capture points enabled: the same frame scores differently under the
/// Replay's own settings and under the defaults, which is why
/// `current_score` takes the settings and why `final_score` is the result
/// to trust.
#[test]
fn capture_points_make_the_settings_argument_matter() {
    let replay = load("v6-suicide-timeout.json");
    let settings = replay.match_settings();
    assert!(settings.capture_points());
    assert!(settings.enabled(MatchSettingField::RoundTime));

    let ReplayEventKind::RoundTimedOut {
        submitted,
        captures,
        cause,
        ..
    } = replay.events()[2].kind()
    else {
        panic!("the third event is the timeout");
    };
    assert_eq!(*submitted, [true, false]);
    assert_eq!(
        *captures,
        [0, 2],
        "Black's suicide credited two stones to White"
    );
    assert_eq!(*cause, TimeoutCause::RoundTime);

    let final_frame = replay.events().last().unwrap().frame();
    assert_eq!(final_frame.phase(), ReplayPhase::Ended);
    assert_eq!(final_frame.capture_totals(), Score { black: 0, white: 2 });
    let with_captures = final_frame.current_score(settings);
    let without_captures = final_frame.current_score(&MatchSettings::default());
    assert_eq!(with_captures, replay.final_score());
    assert_eq!(with_captures.white, without_captures.white + 2);
    assert_eq!(with_captures.black, without_captures.black);

    let mut controller = PlaybackController::default();
    controller.toggle(&replay);
    assert_eq!(
        controller.advance_with_cues(10.0, &replay),
        [GameCue::WhiteCapture]
    );
}

/// The one mutation keeps the invariant: a rename is checked against the
/// same label rule validation applies, and a renamed Replay re-saves.
#[test]
fn rename_is_validated_and_persists() {
    let mut replay = load("v3.json");
    assert!(matches!(
        replay.rename("1 starts with a digit".into()),
        Err(LabelError::MustStartWithLetter)
    ));
    assert!(matches!(validate_label(""), Err(LabelError::Empty)));
    assert_eq!(
        LabelError::TooLong.to_string(),
        go_together::replay::REPLAY_LABEL_RULE
    );
    assert_eq!(
        replay.label(),
        "Schema 3",
        "a rejected rename changes nothing"
    );

    replay.rename("Renamed headlessly".into()).unwrap();
    assert_eq!(replay.label(), "Renamed headlessly");
    replay.validate().unwrap();
    let reloaded = Replay::from_json(&replay.to_storage_json().unwrap()).unwrap();
    assert_eq!(reloaded.label(), "Renamed headlessly");
    assert_eq!(reloaded.version(), 3, "a rename never migrates the file");
}

/// Both error types are ordinary `std::error::Error`s, so a consumer can use
/// `?` into `Box<dyn Error>` or any error-handling crate.
#[test]
fn the_error_types_implement_std_error() {
    fn assert_error<E: std::error::Error + Send + Sync + 'static>() {}
    assert_error::<ReplayError>();
    assert_error::<LabelError>();

    fn load_with_question_mark(json: &str) -> Result<Replay, Box<dyn std::error::Error>> {
        Ok(Replay::from_json(json)?)
    }
    assert!(load_with_question_mark(&fixture("v5.json")).is_ok());
    assert!(load_with_question_mark("").is_err());
}
