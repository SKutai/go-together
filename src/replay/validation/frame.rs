//! Whether one frame, or one event's payload, is coherent read on its own.
//!
//! Everything here is answerable from the thing in front of it plus the
//! schema version, the Replay's duration, and the Match settings: field
//! bounds, the phase and round agreeing with each other, a score that
//! includes its own capture credits, and every point lying on the board.
//! Nothing here looks at the frame before or after - that is `transitions`'
//! job - which is what lets these checks run against every event
//! unconditionally, including the first.
//!
//! The version conditions are the load-bearing part. Each schema version
//! added fields, and a file claiming an older version must not carry the
//! newer ones: the `version <` branches reject fabricated metadata just as
//! firmly as the `version >=` branches reject missing or impossible values.

use crate::match_settings::MatchSettings;
use crate::opening::{OpeningCountdownStage, OpeningSelections};
use crate::replay::schema::{
    MATCH_RULES_REPLAY_SCHEMA_VERSION, MAX_REPLAY_CAPTURE_TOTAL, MAX_REPLAY_EVENTS,
    OPENING_REPLAY_SCHEMA_VERSION, ROUND_TRACKING_REPLAY_SCHEMA_VERSION, ReplayError,
    ReplayEventKind, ReplayFrame, ReplayPhase, TIME_EPSILON,
};
use crate::rules::{BOARD_SIZE, Board, Point, Score};

impl ReplayFrame {
    pub(super) fn validate(
        &self,
        version: u32,
        replay_duration_seconds: f64,
        settings: &MatchSettings,
    ) -> Result<(), ReplayError> {
        if (ROUND_TRACKING_REPLAY_SCHEMA_VERSION..OPENING_REPLAY_SCHEMA_VERSION).contains(&version)
            && self.current_round == 0
        {
            return Err(ReplayError::InvalidData(
                "replay frame has an invalid current round".into(),
            ));
        }
        for selections in &self.opening_selections {
            selections
                .validate()
                .map_err(|message| ReplayError::InvalidData(message.into()))?;
        }
        if version < OPENING_REPLAY_SCHEMA_VERSION
            && (self.phase != ReplayPhase::Playing
                || self.opening_countdown.is_some()
                || self
                    .opening_selections
                    .iter()
                    .any(|selections| !selections.is_empty()))
        {
            return Err(ReplayError::InvalidData(
                "legacy replay contains Round 0 frame data".into(),
            ));
        }
        if version >= OPENING_REPLAY_SCHEMA_VERSION {
            match self.phase {
                ReplayPhase::Opening => {
                    if self.current_round != 0
                        || self.board != Board::default()
                        || self.score != Score::default()
                        || self.selected_moves.iter().any(Option::is_some)
                        || self.opening_countdown == Some(OpeningCountdownStage::Go)
                    {
                        return Err(ReplayError::InvalidData(
                            "Round 0 replay frame is inconsistent".into(),
                        ));
                    }
                    if self.opening_countdown.is_some()
                        && !self
                            .opening_selections
                            .iter()
                            .all(OpeningSelections::is_full)
                    {
                        return Err(ReplayError::InvalidData(
                            "Round 0 countdown requires three selections per player".into(),
                        ));
                    }
                }
                ReplayPhase::Playing | ReplayPhase::Ended => {
                    if self.current_round == 0
                        || self
                            .opening_selections
                            .iter()
                            .any(|selections| !selections.is_empty())
                    {
                        return Err(ReplayError::InvalidData(
                            "ordinary replay frame contains active Round 0 selections".into(),
                        ));
                    }
                    if self.opening_countdown.is_some()
                        && self.opening_countdown != Some(OpeningCountdownStage::Go)
                    {
                        return Err(ReplayError::InvalidData(
                            "ordinary replay frame has an invalid countdown stage".into(),
                        ));
                    }
                }
            }
        }
        if version < MATCH_RULES_REPLAY_SCHEMA_VERSION
            && (self.capture_totals != Score::default()
                || self.no_stone_streak != 0
                || self.round_elapsed_seconds != 0.0)
        {
            return Err(ReplayError::InvalidData(
                "legacy replay contains v5 frame metadata".into(),
            ));
        }
        if version >= MATCH_RULES_REPLAY_SCHEMA_VERSION {
            if self.capture_totals.black > MAX_REPLAY_CAPTURE_TOTAL
                || self.capture_totals.white > MAX_REPLAY_CAPTURE_TOTAL
            {
                return Err(ReplayError::InvalidData(
                    "replay capture total exceeds its supported bound".into(),
                ));
            }
            if self.no_stone_streak > MAX_REPLAY_EVENTS {
                return Err(ReplayError::InvalidData(
                    "replay no-stone streak exceeds its supported bound".into(),
                ));
            }
            if !self.round_elapsed_seconds.is_finite()
                || self.round_elapsed_seconds < 0.0
                || self.round_elapsed_seconds > replay_duration_seconds + TIME_EPSILON
            {
                return Err(ReplayError::InvalidData(
                    "replay round clock is invalid".into(),
                ));
            }
            if settings.enabled(crate::match_settings::MatchSettingField::RoundTime)
                && self.round_elapsed_seconds
                    > f64::from(settings.value(crate::match_settings::MatchSettingField::RoundTime))
                        + TIME_EPSILON
            {
                return Err(ReplayError::InvalidData(
                    "replay round clock exceeds the configured limit".into(),
                ));
            }
            if self.no_stone_streak > self.current_round.saturating_sub(1) {
                return Err(ReplayError::InvalidData(
                    "replay no-stone streak exceeds completed rounds".into(),
                ));
            }
            if self.phase == ReplayPhase::Opening
                && (self.capture_totals != Score::default()
                    || self.no_stone_streak != 0
                    || self.round_elapsed_seconds != 0.0)
            {
                return Err(ReplayError::InvalidData(
                    "Round 0 contains ordinary-round metadata".into(),
                ));
            }
            if self.phase != ReplayPhase::Ended && self.score != Score::default() {
                return Err(ReplayError::InvalidData(
                    "active replay frame contains a final score".into(),
                ));
            }
            if self.phase == ReplayPhase::Ended && self.score != self.current_score(settings) {
                return Err(ReplayError::InvalidData(
                    "ended frame score does not include its capture metadata".into(),
                ));
            }
        }
        for point in self
            .cursors
            .iter()
            .copied()
            .chain(self.teleport_markers.iter().flatten().copied())
            .chain(self.selected_moves.iter().flatten().copied())
        {
            validate_point(point)?;
        }
        Ok(())
    }
}

impl ReplayEventKind {
    pub(super) fn validate(&self) -> Result<(), ReplayError> {
        match self {
            Self::CursorMoved { from, to, .. } | Self::JumpToMarker { from, to, .. } => {
                validate_point(*from)?;
                validate_point(*to)
            }
            Self::SelectionChanged { point, .. }
            | Self::MarkerChanged { point, .. }
            | Self::OpeningSelectionDuplicate { point, .. } => validate_point(*point),
            Self::OpeningSelectionChanged { point, evicted, .. } => {
                validate_point(*point)?;
                if let Some(evicted) = evicted {
                    validate_point(*evicted)?;
                }
                Ok(())
            }
            Self::MatchStart
            | Self::OpeningStarted
            | Self::OpeningCountdown { .. }
            | Self::OpeningReveal
            | Self::OpeningCountdownFinished
            | Self::RoundResolved { .. }
            | Self::RoundTimedOut { .. }
            | Self::MatchSound { .. }
            | Self::MatchEnd => Ok(()),
        }
    }
}

fn validate_point(point: Point) -> Result<(), ReplayError> {
    if point.x < BOARD_SIZE && point.y < BOARD_SIZE {
        Ok(())
    } else {
        Err(ReplayError::InvalidData(
            "replay point is outside the board".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::test_support::*;

    #[test]
    fn schema_v5_rejects_counter_bounds_and_inconsistent_final_capture_score() {
        let mut replay = sample_replay(1, 2, "Counters");
        for event in &mut replay.events {
            event.frame.capture_totals.black = MAX_REPLAY_CAPTURE_TOTAL + 1;
        }
        assert!(replay.validate().is_err());

        let mut replay = sample_replay(1, 2, "Counters");
        replay.events[0].frame.no_stone_streak = MAX_REPLAY_EVENTS + 1;
        assert!(replay.validate().is_err());

        let mut replay = sample_replay(1, 2, "Counters");
        replay
            .events
            .last_mut()
            .unwrap()
            .frame
            .round_elapsed_seconds = replay.duration_seconds + 1.0;
        assert!(replay.validate().is_err());

        let mut replay = sample_replay(1, 2, "Counters");
        replay.match_settings.set_capture_points(true);
        replay.events.last_mut().unwrap().frame.capture_totals.black = 1;
        assert!(matches!(
            replay.validate(),
            Err(ReplayError::InvalidData(ref message))
                if message.contains("final score") || message.contains("capture")
        ));
    }
}
