//! How a Match ended, and whether the file's account of it is possible.
//!
//! An [`EndReason`] list is not free-form: the reasons have a fixed order,
//! cannot repeat, and `Manual` cannot share a list with anything else, so a
//! list can be checked against the vocabulary alone. Beyond that, most
//! endings are *deterministic* - a round limit, no-stone limit, or point
//! target is crossed, or the match-time deadline completes the round, and
//! the Match has to stop, at that exact round and no other. This module
//! derives which reasons a given completion forces, and the caller compares
//! that against what the file claims.
//!
//! That is why the settings live in this reasoning at all: the limits are
//! what make an ending predictable, and a Replay that crossed one earlier
//! than it says, or claims a limit its settings never enabled, is rejected.
//!
//! `validate_end_reasons` is also reached from `compat`, through the
//! re-export in [`super`]: a file's end reasons are checked while it is
//! being deserialized, before there is a [`Replay`] to hand to
//! [`Replay::validate`].

use crate::cue::GameCue;
use crate::replay::schema::{
    EndReason, MATCH_RULES_REPLAY_SCHEMA_VERSION, MAX_END_REASONS, Replay, ReplayError,
    ReplayEvent, ReplayEventKind, TIME_EPSILON, TimeoutCause,
};

/// The end-reason vocabulary on its own terms: a non-empty list within its
/// bound, in the fixed order, without repeats, and drawn only from the
/// reasons its schema version allows.
///
/// This is `pub(crate)` rather than `pub(super)` only so that [`super`] can
/// re-export it to `compat`, which checks a file's reasons one level further
/// out, while it is still being deserialized. `end_reasons` is itself a
/// private module, so that re-export remains the only path to this function.
pub(crate) fn validate_end_reasons(version: u32, reasons: &[EndReason]) -> Result<(), ReplayError> {
    if reasons.is_empty() || reasons.len() > MAX_END_REASONS {
        return Err(ReplayError::InvalidData(
            "replay end reasons are empty or exceed their supported bound".into(),
        ));
    }
    if reasons
        .windows(2)
        .any(|pair| pair[0].order() >= pair[1].order())
    {
        return Err(ReplayError::InvalidData(
            "replay end reasons are duplicated or out of order".into(),
        ));
    }
    if version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
        if reasons.contains(&EndReason::Legacy)
            || (reasons.contains(&EndReason::Manual) && reasons.len() != 1)
        {
            return Err(ReplayError::InvalidData(
                "Replay v5 has incompatible end reasons".into(),
            ));
        }
    } else if reasons != [EndReason::Legacy] {
        return Err(ReplayError::InvalidData(
            "legacy replay has non-legacy end metadata".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_v5_end_metadata(replay: &Replay) -> Result<(), ReplayError> {
    use crate::match_settings::MatchSettingField;

    let has = |reason| replay.end_reasons.contains(&reason);
    if has(EndReason::MatchTimeLimit)
        && (!replay.match_settings.enabled(MatchSettingField::MatchTime)
            || (replay.duration_seconds
                - f64::from(replay.match_settings.value(MatchSettingField::MatchTime)))
            .abs()
                > TIME_EPSILON)
    {
        return Err(ReplayError::InvalidData(
            "match-time end reason does not match its settings or duration".into(),
        ));
    }
    if replay.match_settings.enabled(MatchSettingField::MatchTime)
        && replay.duration_seconds
            >= f64::from(replay.match_settings.value(MatchSettingField::MatchTime)) - TIME_EPSILON
        && !has(EndReason::MatchTimeLimit)
    {
        return Err(ReplayError::InvalidData(
            "expired match time is missing its end reason".into(),
        ));
    }
    Ok(())
}

/// The index of the [`ReplayEventKind::OpeningReveal`] event when, and only
/// when, the Match Time limit expired during Round 0: the Replay's sole end
/// reason is [`EndReason::MatchTimeLimit`], match time is enabled, the reveal
/// sits exactly at the deadline, and the only events after it are the
/// `MatchEnd` sound and [`ReplayEventKind::MatchEnd`], both at that same
/// instant.
///
/// [`Replay::validate`] passes this through to `opening`'s per-event check,
/// which then accepts that one reveal without a full set of selections and
/// without reaching countdown stage One, and consults it again at the end,
/// where `[MatchTimeLimit]` becomes the expected end reason of a Replay with
/// no deterministic ordinary completion.
pub(super) fn opening_match_time_deadline_reveal_index(replay: &Replay) -> Option<usize> {
    use crate::match_settings::MatchSettingField;

    if replay.end_reasons != [EndReason::MatchTimeLimit]
        || !replay.match_settings.enabled(MatchSettingField::MatchTime)
    {
        return None;
    }
    let deadline = f64::from(replay.match_settings.value(MatchSettingField::MatchTime));
    let reveal_index = replay
        .events
        .iter()
        .position(|event| matches!(event.kind, ReplayEventKind::OpeningReveal))?;
    let reveal = &replay.events[reveal_index];
    if (reveal.at_seconds - deadline).abs() > TIME_EPSILON {
        return None;
    }
    let [sound, end] = &replay.events[reveal_index + 1..] else {
        return None;
    };
    if (sound.at_seconds - deadline).abs() > TIME_EPSILON
        || (end.at_seconds - deadline).abs() > TIME_EPSILON
        || !matches!(
            sound.kind,
            ReplayEventKind::MatchSound {
                cue: GameCue::MatchEnd
            }
        )
        || !matches!(end.kind, ReplayEventKind::MatchEnd)
    {
        return None;
    }
    Some(reveal_index)
}

pub(super) fn deterministic_completion_end_reasons(
    replay: &Replay,
    event: &ReplayEvent,
    completed_rounds: usize,
) -> Vec<EndReason> {
    use crate::match_settings::MatchSettingField;

    let mut reasons = Vec::new();
    if matches!(
        event.kind,
        ReplayEventKind::RoundTimedOut {
            cause: TimeoutCause::MatchTime,
            ..
        }
    ) {
        reasons.push(EndReason::MatchTimeLimit);
    }
    if replay.match_settings.enabled(MatchSettingField::RoundLimit)
        && completed_rounds
            >= usize::from(replay.match_settings.value(MatchSettingField::RoundLimit))
    {
        reasons.push(EndReason::RoundLimit);
    }
    if replay
        .match_settings
        .enabled(MatchSettingField::NoStoneRoundLimit)
        && event.frame.no_stone_streak
            >= usize::from(
                replay
                    .match_settings
                    .value(MatchSettingField::NoStoneRoundLimit),
            )
    {
        reasons.push(EndReason::NoStoneRoundLimit);
    }
    if replay
        .match_settings
        .enabled(MatchSettingField::PointTarget)
    {
        let target = usize::from(replay.match_settings.value(MatchSettingField::PointTarget));
        let score = event.frame.current_score(&replay.match_settings);
        if score.black >= target || score.white >= target {
            reasons.push(EndReason::PointTarget);
        }
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::test_support::*;

    #[test]
    fn schema_v5_rejects_invalid_settings_and_end_reason_lists() {
        let replay = sample_replay(1, 2, "Validation");
        let base = replay.to_in_memory_value().unwrap();

        let mut missing_settings = base.clone();
        missing_settings
            .as_object_mut()
            .unwrap()
            .remove("match_settings");
        assert!(Replay::parse_value(missing_settings).is_err());

        let mut invalid_settings = base.clone();
        invalid_settings["match_settings"]["round_limit"]["value"] = serde_json::json!(0);
        assert!(Replay::parse_value(invalid_settings).is_err());

        let mut invalid_duration_relationship = base.clone();
        invalid_duration_relationship["match_settings"]["round_time"] =
            serde_json::json!({"enabled": true, "value": 60});
        invalid_duration_relationship["match_settings"]["match_time"] =
            serde_json::json!({"enabled": true, "value": 30});
        assert!(Replay::parse_value(invalid_duration_relationship).is_err());

        let mut missing_frame_counter = base.clone();
        missing_frame_counter["events"][0]["frame"]
            .as_object_mut()
            .unwrap()
            .remove("capture_totals");
        assert!(Replay::parse_value(missing_frame_counter).is_err());

        for reasons in [
            serde_json::json!([]),
            serde_json::json!(["round_limit", "round_limit"]),
            serde_json::json!(["point_target", "round_limit"]),
            serde_json::json!(["manual", "round_limit"]),
        ] {
            let mut invalid = base.clone();
            invalid["end_reasons"] = reasons;
            assert!(Replay::parse_value(invalid).is_err());
        }
    }

    #[test]
    fn v5_rejects_an_early_no_stone_limit_crossing_that_later_resets() {
        assert!(matches!(
            no_stone_then_placement_replay().validate(),
            Err(ReplayError::InvalidData(ref message))
                if message
                    == "automatic match limit was crossed before the final ordinary completion"
        ));
    }

    #[test]
    fn v5_rejects_a_point_target_crossing_that_is_later_lost() {
        assert!(matches!(
            point_target_then_loss_replay().validate(),
            Err(ReplayError::InvalidData(ref message))
                if message
                    == "automatic match limit was crossed before the final ordinary completion"
        ));
    }

    #[test]
    fn v5_rejects_an_extra_round_after_the_round_limit() {
        assert!(matches!(
            extra_round_after_round_limit_replay().validate(),
            Err(ReplayError::InvalidData(ref message))
                if message
                    == "automatic match limit was crossed before the final ordinary completion"
        ));
    }
}
