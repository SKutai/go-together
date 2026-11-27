//! Deterministic Replay-save timing measurement on real, fully valid
//! Replays - run with `cargo test --release replay_save_timing_measurements
//! -- --nocapture` for numbers that reflect an optimized build.
//!
//! Unlike [`crate::replay::measurement`], which measures *size* on
//! schema-faithful but not rules-derived Replays (full `Replay::validate()`
//! would reject them), this module drives the real
//! [`GameState`]/[`advance_match_state_by`] pipeline directly - no Bevy
//! `App`, window, or input, though still a `client` build - with round
//! timeouts and
//! no player input, producing genuinely valid, fully-forfeited Matches of
//! whatever length is requested - `Replay::validate()` accepts them exactly
//! as it would a real completed Match, so `validate`'s own cost (which
//! re-derives every ordinary round through [`crate::rules::resolve_round`])
//! is part of what gets measured, not skipped.
//!
//! See [Replay-save timing](../../docs/ARCHITECTURE.md#replay-save-timing)
//! for the resulting measurements and conclusion.

use std::time::Instant;

use super::countdown::{advance_match_state_by, begin_active_match};
use super::{GameState, MatchRules, finish_match};
use crate::clock::MatchDuration;
use crate::match_settings::{MatchSettingField, MatchSettings};
use crate::platform::atomic_write;
use crate::replay::{EndReason, Replay};

fn round_limited_settings(round_limit: u16) -> MatchSettings {
    let mut settings = MatchSettings::default();
    settings.set_value(MatchSettingField::RoundTime, 1);
    settings.set_enabled(MatchSettingField::RoundTime, true);
    settings.set_value(MatchSettingField::RoundLimit, u64::from(round_limit));
    settings.set_enabled(MatchSettingField::RoundLimit, true);
    settings
}

/// Plays a real Match to completion via repeated one-second round timeouts
/// with no player input (every round is forfeited), then returns the
/// resulting, fully valid `Replay`.
pub(super) fn valid_replay_with_rounds(round_limit: u16) -> Replay {
    let settings = round_limited_settings(round_limit);
    let mut game = GameState::with_match_rules(MatchRules::snapshot(&settings));
    begin_active_match(&mut game);
    for _ in 0..round_limit {
        if game.pending_replay.is_some() {
            break;
        }
        advance_match_state_by(&mut game, MatchDuration::from_secs(1), false);
    }
    game.pending_replay
        .take()
        .or_else(|| finish_match(&mut game, vec![EndReason::Manual]))
        .expect("round_limit one-second timeouts always reach an end condition")
}

pub(super) struct SaveTiming {
    pub(super) event_count: usize,
    pub(super) validate_micros: u128,
    pub(super) serialize_micros: u128,
    pub(super) write_micros: u128,
    pub(super) total_micros: u128,
}

/// Times each stage of native Replay persistence separately, on the same
/// already-built `Replay` (so recorder-completion cost, which happens
/// earlier and is not part of saving, is excluded from all three): schema
/// validation, building+serializing the compact on-disk JSON
/// (`to_storage_json`, which includes the board-compaction pass from
/// `COMPACT_BOARD_REPLAY_SCHEMA_VERSION`), and the filesystem write.
///
/// The write stage calls [`atomic_write`] - the very function
/// `NativeReplayStorage::save` uses - rather than a plain `std::fs::write`,
/// because the two are not comparable: `atomic_write` fsyncs the temporary
/// file and its parent directory, so a buffered `fs::write` would report a
/// fraction of what production actually pays.
///
/// The three stages together are the whole of a native save, counted once
/// each: `to_storage_json` does not validate internally, so the total
/// contains exactly the one `validate` call the write path actually makes -
/// see [`Replay::validate`] for the ownership policy that guarantees it.
///
/// Note that the serialize stage is dominated by *building* the storage
/// value - cloning it down to the file's schema version and running the
/// board-compaction pass - not by encoding that value to text. Measurement
/// on 2026-09-11 found that dropping pretty-printing, which halved every
/// file, moved this column by only a few percent; see
/// [Replay-save timing](../../docs/ARCHITECTURE.md#replay-save-timing).
pub(super) fn measure_save(replay: &Replay, root: &std::path::Path) -> SaveTiming {
    let event_count = replay.events.len();

    let validate_start = Instant::now();
    replay.validate().unwrap();
    let validate_micros = validate_start.elapsed().as_micros();

    let serialize_start = Instant::now();
    let json = replay.to_storage_json().unwrap();
    let serialize_micros = serialize_start.elapsed().as_micros();

    let path = root.join(format!("{}.json", replay.id));
    let write_start = Instant::now();
    atomic_write(&path, &json).unwrap();
    let write_micros = write_start.elapsed().as_micros();

    SaveTiming {
        event_count,
        validate_micros,
        serialize_micros,
        write_micros,
        total_micros: validate_micros + serialize_micros + write_micros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_replay_with_rounds_produces_a_replay_that_passes_full_validation() {
        let replay = valid_replay_with_rounds(5);
        replay.validate().unwrap();
        // At least the 5 round-timeout events plus MatchStart and MatchEnd.
        assert!(replay.events.len() >= 7);
    }

    /// Not a pass/fail correctness test: prints a timing table for
    /// small/typical/near-round-limit Matches. The only assertions guard the
    /// report's own consistency (each Replay is actually valid, timings are
    /// non-negative), not any specific duration - deliberately, since wall-clock
    /// timing is machine-dependent and must never gate CI.
    #[test]
    fn replay_save_timing_measurements() {
        let root =
            std::env::temp_dir().join(format!("go-together-save-timing-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();

        println!(
            "\n{:<28} {:>8} {:>14} {:>14} {:>14} {:>14}",
            "scenario", "events", "validate us", "serialize us", "write us", "total us"
        );
        println!("{}", "-".repeat(100));
        for (name, round_limit) in [
            ("Short match (10 rounds)", 10),
            ("Typical match (100 rounds)", 100),
            ("Near maximum round limit (1000)", 1000),
        ] {
            let replay = valid_replay_with_rounds(round_limit);
            let timing = measure_save(&replay, &root);
            println!(
                "{:<28} {:>8} {:>14} {:>14} {:>14} {:>14}",
                name,
                timing.event_count,
                timing.validate_micros,
                timing.serialize_micros,
                timing.write_micros,
                timing.total_micros,
            );
        }
        println!("{}", "-".repeat(100));
        println!(
            "A frame at 60 FPS budgets ~16,667 us; a stall much smaller than that is not \
             perceptible as a dropped frame."
        );

        std::fs::remove_dir_all(&root).unwrap();
    }
}
