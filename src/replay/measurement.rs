//! Deterministic Replay-size measurement across representative Match
//! scenarios.
//!
//! This is not a correctness test: [`simulate`] does not run
//! [`crate::rules`] at all, and its `ResolutionKind`/capture/board values are
//! plausible rather than rules-derived. What it *does* faithfully reproduce
//! is the real, production [`super::schema`] types and their real `serde`
//! representations, which is what determines a Replay's serialized size - so
//! the numbers this module reports (via [`replay_size_measurements`], run
//! with `cargo test replay_size_measurements -- --nocapture`) are accurate
//! regardless of how the events were produced.
//!
//! Each scenario is reported as one size, because both stores now hold the
//! same encoding: [`Replay::to_storage_json`] writes `to_storage_value`
//! compactly, and `platform::browser_storage` encodes that same value the
//! same way. Two per-replay caps apply to those identical bytes -
//! `MAX_BROWSER_REPLAY_BYTES` (800 KiB) for a browser record and
//! [`MAX_REPLAY_JSON_BYTES`] (8 MiB) for a native file - so the report
//! checks each scenario against both.
//!
//! That was not always so. The writer pretty-printed until 2026-09-11, which
//! made a native file roughly twice its compact size, and a report quoting
//! only the compact figure understated it by about half - which is how the
//! native headroom came to be recorded as comfortable when it was not. The
//! report still measures the two paths separately and asserts they agree,
//! so a writer that drifts apart from the browser encoding again is caught
//! here rather than in a size conclusion drawn against the wrong bytes.
//!
//! See [Replay size](../../docs/ARCHITECTURE.md#replay-size) for the
//! resulting measurements and conclusion.

use crate::cue::GameCue;
use crate::match_settings::{MatchSettingField, MatchSettings};
use crate::rules::{BOARD_SIZE, CELL_COUNT, Cell, Player, Point, ResolutionKind, Score};

use super::MatchRecorder;
use super::schema::{
    MAX_REPLAY_EVENTS, MAX_REPLAY_JSON_BYTES, Replay, ReplayEventKind, ReplayPhase, TimeoutCause,
};
use super::test_support::frame;

/// One representative Match shape to measure.
pub(super) struct ScenarioSpec {
    pub(super) name: &'static str,
    pub(super) rounds: usize,
    pub(super) cursor_moves_per_round: usize,
    pub(super) reselections_per_round: usize,
    pub(super) marker_events_per_round: usize,
    pub(super) captures_per_round: usize,
    pub(super) gray_every_n_rounds: usize,
    pub(super) use_timeouts: bool,
}

impl ScenarioSpec {
    const fn baseline(name: &'static str, rounds: usize) -> Self {
        Self {
            name,
            rounds,
            cursor_moves_per_round: 4,
            reselections_per_round: 1,
            marker_events_per_round: 0,
            captures_per_round: 0,
            gray_every_n_rounds: 0,
            use_timeouts: false,
        }
    }
}

/// The scenarios [`docs/ARCHITECTURE.md`'s "Replay size" section](../../docs/ARCHITECTURE.md#replay-size)
/// reports measurements for.
pub(super) fn scenarios() -> Vec<ScenarioSpec> {
    vec![
        ScenarioSpec::baseline("Short ordinary match (10 rounds)", 10),
        ScenarioSpec::baseline("Default round limit (100 rounds)", 100),
        ScenarioSpec::baseline("Near maximum round limit (1000 rounds)", 1000),
        ScenarioSpec {
            cursor_moves_per_round: 40,
            ..ScenarioSpec::baseline("Cursor-movement-heavy (100 rounds)", 100)
        },
        ScenarioSpec {
            reselections_per_round: 10,
            ..ScenarioSpec::baseline("Frequent reselection (100 rounds)", 100)
        },
        ScenarioSpec {
            marker_events_per_round: 4,
            ..ScenarioSpec::baseline("Frequent Teleport Marker use (100 rounds)", 100)
        },
        ScenarioSpec {
            captures_per_round: 2,
            gray_every_n_rounds: 5,
            ..ScenarioSpec::baseline("Captures, cleanup and repetition-to-Gray (100 rounds)", 100)
        },
        ScenarioSpec {
            use_timeouts: true,
            ..ScenarioSpec::baseline("Round/Match Time timeouts (100 rounds)", 100)
        },
        ScenarioSpec::baseline(
            "Long replay near the event limit (9,998 events)",
            // Each round contributes 1 resolution event plus the baseline's
            // 4 cursor and 1 reselection events; +1 for MatchStart.
            (MAX_REPLAY_EVENTS - 1) / 6,
        ),
    ]
}

/// Builds a synthetic but schema-faithful [`Replay`] for one scenario.
///
/// Every event uses the real [`ReplayEventKind`]/[`super::schema::ReplayFrame`]
/// types, so its serialized size matches what a real Match producing the same
/// shape of events would record - only the *values* (which cell moves,
/// which resolution kind) are simplified, not the representation.
pub(super) fn simulate(spec: &ScenarioSpec) -> Replay {
    let mut settings = MatchSettings::default();
    if spec.use_timeouts {
        settings.set_enabled(MatchSettingField::RoundTime, true);
        settings.set_value(MatchSettingField::RoundTime, 30);
    }

    let capture_points = settings.capture_points();
    let mut current = frame();
    let mut recorder = MatchRecorder::for_test(1, current.clone(), settings);
    let mut at = 0.0_f64;
    let mut totals = Score::default();

    for round in 1..=spec.rounds {
        for i in 0..spec.cursor_moves_per_round {
            at += 0.05;
            let player = if i % 2 == 0 { Player::One } else { Player::Two };
            let from = current.cursors[player.index()];
            let to = Point::new((from.x + 1) % BOARD_SIZE, from.y);
            current.cursors[player.index()] = to;
            recorder.record(
                at,
                ReplayEventKind::CursorMoved { player, from, to },
                current.clone(),
            );
        }

        for i in 0..spec.reselections_per_round {
            at += 0.05;
            let player = if i % 2 == 0 { Player::One } else { Player::Two };
            let point = current.cursors[player.index()];
            current.selected_moves[player.index()] = Some(point);
            recorder.record(
                at,
                ReplayEventKind::SelectionChanged { player, point },
                current.clone(),
            );
        }

        for i in 0..spec.marker_events_per_round {
            at += 0.05;
            let player = if i % 2 == 0 { Player::One } else { Player::Two };
            if i % 2 == 0 {
                let point = current.cursors[player.index()];
                current.teleport_markers[player.index()] = Some(point);
                recorder.record(
                    at,
                    ReplayEventKind::MarkerChanged { player, point },
                    current.clone(),
                );
            } else if let Some(marker) = current.teleport_markers[player.index()] {
                let from = current.cursors[player.index()];
                current.cursors[player.index()] = marker;
                recorder.record(
                    at,
                    ReplayEventKind::JumpToMarker {
                        player,
                        from,
                        to: marker,
                    },
                    current.clone(),
                );
            }
        }

        at += 0.1;
        let cell_index = round % CELL_COUNT;
        let point = Point::new(cell_index % BOARD_SIZE, cell_index / BOARD_SIZE);
        let cell = if spec.gray_every_n_rounds > 0 && round % spec.gray_every_n_rounds == 0 {
            Cell::Gray
        } else if round % 2 == 1 {
            Cell::Black
        } else {
            Cell::White
        };
        current.board.set(point, cell);
        current.current_round = round + 1;
        current.selected_moves = [None, None];

        let captures = if spec.captures_per_round > 0 {
            totals.black += spec.captures_per_round;
            [spec.captures_per_round, 0]
        } else {
            [0, 0]
        };
        current.capture_totals = totals;

        if spec.use_timeouts && round % 3 == 0 {
            recorder.record(
                at,
                ReplayEventKind::RoundTimedOut {
                    resolution: ResolutionKind::Sequential,
                    submitted: [true, false],
                    captures,
                    cause: TimeoutCause::RoundTime,
                },
                current.clone(),
            );
        } else {
            recorder.record(
                at,
                ReplayEventKind::RoundResolved {
                    resolution: ResolutionKind::Sequential,
                    captures,
                },
                current.clone(),
            );
        }

        if captures != [0, 0] && round % 5 == 0 {
            at += 0.01;
            recorder.record(
                at,
                ReplayEventKind::MatchSound {
                    cue: GameCue::BlackCapture,
                },
                current.clone(),
            );
        }
    }

    let mut final_frame = current;
    final_frame.phase = ReplayPhase::Ended;
    final_frame.score = final_frame
        .board
        .score_with_captures(totals, capture_points);
    recorder.complete(2, at + 1.0, final_frame).unwrap()
}

/// One scenario's measured result.
pub(super) struct ScenarioMeasurement {
    pub(super) name: &'static str,
    pub(super) event_count: usize,
    /// What a native Replay file holds, through [`Replay::to_storage_json`],
    /// and what [`MAX_REPLAY_JSON_BYTES`] caps.
    pub(super) file_bytes: usize,
    /// What a browser record holds, encoded the way
    /// `platform::browser_storage` encodes it, and what
    /// `MAX_BROWSER_REPLAY_BYTES` caps. Expected to equal
    /// [`Self::file_bytes`] - the report asserts it rather than assuming it,
    /// because the two come from different calls on different write paths.
    pub(super) browser_record_bytes: usize,
}

pub(super) fn measure(spec: &ScenarioSpec) -> ScenarioMeasurement {
    let replay = simulate(spec);
    let event_count = replay.events.len();
    // Both figures come from the production write path. `to_storage_value`
    // already strips the fields an older schema version never had and omits
    // every board identical to the one before it (see
    // `COMPACT_BOARD_REPLAY_SCHEMA_VERSION`); `to_storage_json` serializes
    // that value compactly, which is exactly what a native file holds, and
    // the `to_vec` below is how `platform::browser_storage` encodes the same
    // value for a record. Neither validates - see `Replay::validate` for
    // which layer owns that - so `simulate`'s schema-shaped but not
    // rules-derived events serialize here exactly as a recorded Replay's do.
    // Serialized size is all this measures.
    let value = replay.to_storage_value().unwrap();
    let browser_record_bytes = serde_json::to_vec(&value).unwrap().len();
    let file_bytes = replay.to_storage_json().unwrap().len();
    ScenarioMeasurement {
        name: spec.name,
        event_count,
        file_bytes,
        browser_record_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `platform::BROWSER_REPLAY_LIMIT`, which is `wasm32`-only and
    /// so cannot be named from a native test build - or from this module at
    /// all, since `replay` is headless core and `platform` is not.
    ///
    /// [`the_mirrored_browser_replay_limit_still_matches_platform`] is what
    /// holds the two together. Without it this would be precisely the
    /// mirrored constant with a "keep these in step" comment and no guard
    /// that `docs/ARCHITECTURE.md`'s "Mirrored structures and the two-layer
    /// guard" section exists to rule out.
    const BROWSER_REPLAY_LIMIT: usize = 3;

    /// Mirrors `platform::browser_storage::MAX_BROWSER_REPLAY_BYTES` for the
    /// same reason and under the same guard as [`BROWSER_REPLAY_LIMIT`]:
    /// `replay` is headless core and cannot name `platform` at all, let
    /// alone a `wasm32`-only module inside it.
    ///
    /// It is mirrored rather than omitted because one encoding now faces two
    /// caps, so a report that quoted only the native one would say nothing
    /// about the store where these same bytes are tightest.
    const MAX_BROWSER_REPLAY_BYTES: usize = 800 * 1024;

    /// Reads the real declaration out of `platform/mod.rs` as text, because
    /// source text is the only view of a `wasm32`-only constant that a
    /// native `cargo test` has - and the only view of a `client` module that
    /// a `--no-default-features` run has, where this test still runs and
    /// still checks. See [`crate::architecture`].
    #[test]
    fn the_mirrored_browser_replay_limit_still_matches_platform() {
        let declaration = format!("const BROWSER_REPLAY_LIMIT: usize = {BROWSER_REPLAY_LIMIT};");
        let platform = crate::architecture::source_file("platform/mod.rs");

        assert!(
            platform.contains(&declaration),
            "platform/mod.rs no longer declares `{declaration}`; the report \
             this module prints would quote a browser retention limit the \
             browser adapter does not use"
        );
    }

    /// The companion guard to
    /// [`the_mirrored_browser_replay_limit_still_matches_platform`], for the
    /// per-record cap the report checks every scenario against.
    #[test]
    fn the_mirrored_browser_replay_cap_still_matches_platform() {
        let declaration = "const MAX_BROWSER_REPLAY_BYTES: usize = 800 * 1024;";
        let browser_storage = crate::architecture::source_file("platform/browser_storage.rs");

        assert!(
            browser_storage.contains(declaration),
            "platform/browser_storage.rs no longer declares `{declaration}`; the report this \
             module prints would measure browser headroom against a cap the browser adapter \
             does not enforce"
        );
        assert_eq!(
            MAX_BROWSER_REPLAY_BYTES,
            800 * 1024,
            "the mirrored value must stay the arithmetic the declaration above spells out"
        );
    }

    /// Primarily a report, not a pass/fail correctness test: it prints a size
    /// table for every scenario in [`scenarios`], run with
    /// `cargo test replay_size_measurements -- --nocapture`. The assertions
    /// guard only the invariants the report's conclusion depends on - that
    /// events accumulate, that both write paths agree on one encoding, and
    /// that every modelled shape still fits the native per-replay cap - not
    /// any specific byte count, since the report documents the current
    /// numbers directly rather than pinning them here.
    ///
    /// The native-cap assertion is the one that gates. It is here because
    /// "every scenario fits" is a conclusion `docs/ARCHITECTURE.md` now
    /// states, and an unguarded conclusion is one a later frame field can
    /// quietly falsify. The browser cap is deliberately *not* asserted:
    /// several scenarios exceed it today, which is a documented and accepted
    /// tradeoff rather than a regression, so it is reported and not enforced.
    #[test]
    fn replay_size_measurements() {
        println!(
            "\n{:<52} {:>8} {:>12} {:>10} {:>9} {:>9}",
            "scenario", "events", "file bytes", "bytes/evt", "of native", "of browser"
        );
        println!("{}", "-".repeat(104));
        let mut results = Vec::new();
        for spec in scenarios() {
            let measurement = measure(&spec);
            println!(
                "{:<52} {:>8} {:>12} {:>10} {:>8.0}% {:>9.0}%",
                measurement.name,
                measurement.event_count,
                measurement.file_bytes,
                measurement.file_bytes / measurement.event_count.max(1),
                percent_of(measurement.file_bytes, MAX_REPLAY_JSON_BYTES),
                percent_of(measurement.file_bytes, MAX_BROWSER_REPLAY_BYTES),
            );
            results.push(measurement);
        }
        println!("{}", "-".repeat(104));
        println!(
            "`file bytes` is one encoding both stores hold: Replay::to_storage_json writes it \
             to a native file and platform::browser_storage encodes a record identically. The \
             last two columns are that size against each store's per-replay cap - \
             MAX_REPLAY_JSON_BYTES at {MAX_REPLAY_JSON_BYTES} bytes, and \
             MAX_BROWSER_REPLAY_BYTES at {MAX_BROWSER_REPLAY_BYTES} bytes with \
             {BROWSER_REPLAY_LIMIT} records retained."
        );

        for measurement in &results {
            assert!(measurement.event_count > 0);
            assert_eq!(
                measurement.file_bytes, measurement.browser_record_bytes,
                "{} serialized to {} bytes as a native file but {} as a browser record; the \
                 two write paths no longer agree on one encoding, which is the assumption \
                 this report and both per-replay caps now rest on",
                measurement.name, measurement.file_bytes, measurement.browser_record_bytes
            );
            assert!(
                measurement.file_bytes < MAX_REPLAY_JSON_BYTES,
                "{} writes {} bytes, at or over the {MAX_REPLAY_JSON_BYTES}-byte native \
                 per-replay limit, so a Match of this shape cannot be saved natively. Every \
                 modelled shape fitting is the conclusion docs/ARCHITECTURE.md's Replay size \
                 section now records; if this scenario is legitimately larger, that section \
                 needs rewriting, not this assertion relaxing.",
                measurement.name,
                measurement.file_bytes
            );
            if measurement.file_bytes >= MAX_BROWSER_REPLAY_BYTES {
                println!(
                    "NOTE: {} writes {} bytes, at or over the {MAX_BROWSER_REPLAY_BYTES}-byte \
                     browser per-replay limit, so a Match of this shape cannot be saved in a \
                     browser tab - see the known remaining browser tradeoff in \
                     docs/ARCHITECTURE.md's Replay size section.",
                    measurement.name, measurement.file_bytes
                );
            }
        }

        let near_limit = results
            .iter()
            .find(|measurement| measurement.name.contains("event limit"))
            .expect("the near-event-limit scenario must be present");
        println!(
            "\nEstimated bytes for {BROWSER_REPLAY_LIMIT} browser replays at the near-event-limit \
             size: {}",
            near_limit.file_bytes * BROWSER_REPLAY_LIMIT
        );
    }

    /// A size as a percentage of a cap, for the report's last two columns.
    fn percent_of(bytes: usize, cap: usize) -> f64 {
        bytes as f64 / cap as f64 * 100.0
    }
}
