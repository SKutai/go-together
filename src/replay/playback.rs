//! Playback: seeking a [`Replay`]'s recorded frames and driving
//! [`PlaybackController`], the transport clock the Replay Viewer advances
//! each frame - and the same clock a headless consumer drives from its own
//! tick.
//!
//! Nothing here re-runs [`crate::rules`] - seeking always draws the last
//! recorded [`ReplayFrame`] at or before the requested time, so it is exact
//! and immune to rule changes. Only
//! [`PlaybackController::advance_with_cues`] emits audio cues; every seek
//! method moves the position silently so scrubbing never replays a burst of
//! stale sounds.
//!
//! Every method here assumes the `Replay` it is given passed
//! [`Replay::validate`]; outside this crate that is guaranteed by how a
//! `Replay` is obtained (see its docs), and inside it by the load and write
//! paths that own validation.

use crate::cue::GameCue;
use crate::rules::{BOARD_SIZE, Cell, Point, ResolutionKind, RoundSound};

use super::schema::{
    EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION, LEGACY_REPLAY_SCHEMA_VERSION,
    MATCH_RULES_REPLAY_SCHEMA_VERSION, ROUND_TRACKING_REPLAY_SCHEMA_VERSION, Replay,
    ReplayEventKind, ReplayFrame, ReplayPhase, TIME_EPSILON,
};

/// The playback speeds [`PlaybackController::set_speed`] accepts, as
/// multiples of real time. The Replay Viewer offers exactly these.
pub const PLAYBACK_SPEEDS: [f64; 3] = [0.5, 1.0, 2.0];

impl Replay {
    /// Returns the last recorded frame at or before a clamped timeline time.
    ///
    /// Schema v1 had no `current_round`, so it is reconstructed by counting
    /// resolution events up to the requested time.
    #[cfg(test)]
    pub(crate) fn frame_at(&self, seconds: f64) -> ReplayFrame {
        self.frame_with_source_time_at(seconds).0
    }

    /// The state of the Match at `seconds` into the timeline: the last
    /// recorded frame at or before that time, with its ordinary-round clock
    /// advanced from the event that recorded it to the requested position
    /// (capped at the round-time limit when one is enabled). `seconds` is
    /// clamped to `0..=duration_seconds`, so any finite value is a valid
    /// seek. A [`LEGACY_REPLAY_SCHEMA_VERSION`] frame's round number, which
    /// v1 never recorded, is reconstructed here as one more than the number
    /// of resolution events at or before the requested time.
    ///
    /// This is the time-keyed view the Replay Viewer draws from. To walk the
    /// exact record event by event, including events that share a timestamp,
    /// iterate [`Replay::events`] instead.
    pub fn playback_frame_at(&self, seconds: f64) -> ReplayFrame {
        let target = seconds.clamp(0.0, self.duration_seconds);
        let (mut frame, source_time) = self.frame_with_source_time_at(target);
        if self.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION && frame.phase == ReplayPhase::Playing
        {
            frame.round_elapsed_seconds += (target - source_time).max(0.0);
            if self
                .match_settings
                .enabled(crate::match_settings::MatchSettingField::RoundTime)
            {
                frame.round_elapsed_seconds = frame.round_elapsed_seconds.min(f64::from(
                    self.match_settings
                        .value(crate::match_settings::MatchSettingField::RoundTime),
                ));
            }
        }
        frame
    }

    /// Identifies which recorded event is currently in effect at `seconds`,
    /// without building the [`ReplayFrame`] itself.
    ///
    /// Two calls returning the same key mean every discrete, non-interpolated
    /// part of the frame - board, cursors, markers, selections, score - is
    /// identical; only continuously-interpolated fields such as
    /// `round_elapsed_seconds` may differ. The Replay Viewer uses this to
    /// redraw the board only when playback crosses an event boundary, rather
    /// than on every frame while the interpolated clock keeps ticking. The
    /// key is an index into [`Replay::events`]; it is `None` only for an
    /// empty timeline, which a validated Replay never has.
    pub fn playback_boundary_key(&self, seconds: f64) -> Option<usize> {
        let target = seconds.clamp(0.0, self.duration_seconds);
        self.events
            .iter()
            .rposition(|event| event.at_seconds <= target + TIME_EPSILON)
    }

    fn frame_with_source_time_at(&self, seconds: f64) -> (ReplayFrame, f64) {
        let target = seconds.clamp(0.0, self.duration_seconds);
        let source = self
            .events
            .iter()
            .rev()
            .find(|event| event.at_seconds <= target + TIME_EPSILON);
        let mut frame = source.map(|event| event.frame.clone()).unwrap_or_default();
        if frame.current_round == 0
            && (self.version < ROUND_TRACKING_REPLAY_SCHEMA_VERSION || !self.opening_completed)
        {
            frame.current_round = self
                .events
                .iter()
                .take_while(|event| event.at_seconds <= target + TIME_EPSILON)
                .filter(|event| matches!(event.kind, ReplayEventKind::RoundResolved { .. }))
                .count()
                + 1;
        }
        (frame, source.map_or(0.0, |event| event.at_seconds))
    }

    /// How many ordinary rounds the Match completed - Round 0, when there
    /// was one, is not counted. Read from the persisted count from
    /// [`ROUND_TRACKING_REPLAY_SCHEMA_VERSION`] on and derived from the
    /// resolution events for a v1 Replay, so it is right for every version.
    pub fn completed_round_count(&self) -> usize {
        if self.version == LEGACY_REPLAY_SCHEMA_VERSION {
            self.events
                .iter()
                .filter(|event| matches!(event.kind, ReplayEventKind::RoundResolved { .. }))
                .count()
        } else {
            self.round_count
        }
    }

    /// Whether the Match began with a completed Round 0 (simultaneous
    /// opening selections and a reveal). Always `false` below
    /// [`OPENING_REPLAY_SCHEMA_VERSION`](super::OPENING_REPLAY_SCHEMA_VERSION),
    /// which is accurate: those
    /// Matches had no Round 0.
    pub const fn opening_completed(&self) -> bool {
        self.opening_completed
    }

    /// The timeline positions playback steps between, in ascending order
    /// with duplicates removed: the times of every event whose kind
    /// [`ReplayEventKind::is_significant`]. [`PlaybackController::jump_back`]
    /// and [`PlaybackController::jump_forward`] move between these.
    pub fn significant_times(&self) -> Vec<f64> {
        self.events
            .iter()
            .filter(|event| event.kind.is_significant())
            .map(|event| event.at_seconds)
            .fold(Vec::new(), |mut times, time| {
                if times
                    .last()
                    .is_none_or(|last| f64::abs(*last - time) > TIME_EPSILON)
                {
                    times.push(time);
                }
                times
            })
    }

    /// The sounds a listener would hear playing forward through the interval
    /// `(start_exclusive, end_inclusive]`, in timeline order - except that an
    /// interval starting at time zero also includes cues recorded exactly at
    /// zero, so opening a Replay and pressing play hears the first sound.
    /// Empty when the interval is empty or reversed.
    ///
    /// From [`EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION`] on these are the recorded
    /// [`ReplayEventKind::MatchSound`] events; below it, best-effort cues are
    /// derived from each round's board change, teleport jumps, and the Match
    /// end. [`PlaybackController::advance_with_cues`] calls this for the
    /// interval it just played through; seeking never does.
    pub fn audio_cues_between(&self, start_exclusive: f64, end_inclusive: f64) -> Vec<GameCue> {
        if end_inclusive <= start_exclusive + TIME_EPSILON {
            return Vec::new();
        }
        self.events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                (event.at_seconds > start_exclusive + TIME_EPSILON
                    || (start_exclusive <= TIME_EPSILON && event.at_seconds.abs() <= TIME_EPSILON))
                    && event.at_seconds <= end_inclusive + TIME_EPSILON
            })
            .filter_map(|(index, event)| {
                if self.version >= EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION {
                    match &event.kind {
                        ReplayEventKind::MatchSound { cue } => Some(*cue),
                        _ => None,
                    }
                } else {
                    self.legacy_audio_cue(index)
                }
            })
            .collect()
    }

    fn legacy_audio_cue(&self, event_index: usize) -> Option<GameCue> {
        let event = self.events.get(event_index)?;
        match &event.kind {
            ReplayEventKind::JumpToMarker { player, from, to } if from != to => {
                Some(GameCue::teleport(*player))
            }
            ReplayEventKind::RoundResolved { resolution, .. } => {
                if *resolution == ResolutionKind::SamePointGray {
                    return Some(GameCue::SamePointGray);
                }
                if *resolution == ResolutionKind::KoGray {
                    return Some(GameCue::GrayCapture);
                }
                let before = self.events.get(event_index.saturating_sub(1))?;
                let mut sound = RoundSound::default();
                for y in 0..BOARD_SIZE {
                    for x in 0..BOARD_SIZE {
                        let point = Point::new(x, y);
                        let previous = before.frame.board.get(point);
                        let current = event.frame.board.get(point);
                        sound.black_capture |=
                            previous == Cell::White && matches!(current, Cell::Empty | Cell::Gray);
                        sound.white_capture |=
                            previous == Cell::Black && matches!(current, Cell::Empty | Cell::Gray);
                        sound.gray_capture |= *resolution == ResolutionKind::Simultaneous
                            && matches!(previous, Cell::Black | Cell::White)
                            && current == Cell::Gray;
                    }
                }
                GameCue::from_round_sound(sound)
            }
            ReplayEventKind::MatchEnd => Some(GameCue::MatchEnd),
            _ => None,
        }
    }

    /// The last entry of [`Replay::significant_times`] strictly before
    /// `current`, or `0.0` when there is none.
    pub fn previous_significant_time(&self, current: f64) -> f64 {
        self.significant_times()
            .into_iter()
            .rev()
            .find(|time| *time < current - TIME_EPSILON)
            .unwrap_or(0.0)
    }

    /// The first entry of [`Replay::significant_times`] strictly after
    /// `current`, or [`Replay::duration_seconds`] when there is none.
    pub fn next_significant_time(&self, current: f64) -> f64 {
        self.significant_times()
            .into_iter()
            .find(|time| *time > current + TIME_EPSILON)
            .unwrap_or(self.duration_seconds)
    }
}

/// Replay transport state: a position on one Replay's timeline, a speed,
/// and whether the position is advancing.
///
/// [`PlaybackController::default`] is paused at the start at real-time
/// speed. The controller does not hold the Replay; each method that needs
/// the timeline takes the same `&Replay` the position is meant for, and the
/// caller drives it from its own clock: call
/// [`PlaybackController::advance_with_cues`] once per tick with the elapsed
/// real seconds, draw [`Replay::playback_frame_at`] at
/// [`PlaybackController::position_seconds`], and play the cues it returned.
/// Playback pauses itself on reaching the end.
///
/// Only `advance_with_cues` emits crossed audio cues. All seek methods
/// update position silently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackController {
    pub(crate) position_seconds: f64,
    pub(crate) speed: f64,
    pub(crate) playing: bool,
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self {
            position_seconds: 0.0,
            speed: 1.0,
            playing: false,
        }
    }
}

impl PlaybackController {
    /// The current timeline position in seconds, within
    /// `0..=replay.duration_seconds()`.
    pub const fn position_seconds(&self) -> f64 {
        self.position_seconds
    }

    /// The current playback speed, one of [`PLAYBACK_SPEEDS`].
    pub const fn speed(&self) -> f64 {
        self.speed
    }

    /// Whether [`PlaybackController::advance_with_cues`] is currently moving
    /// the position.
    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    /// Starts playback, or pauses it - except at the end of the timeline,
    /// where there is nothing to play and the call does nothing.
    pub fn toggle(&mut self, replay: &Replay) {
        if self.at_end(replay) {
            return;
        }
        self.playing = !self.playing;
    }

    /// Stops the position advancing without moving it. Unconditional, unlike
    /// [`PlaybackController::toggle`]: pausing at the start or the end is a
    /// no-op that succeeds.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub(crate) fn advance(&mut self, delta_seconds: f64, duration: f64) {
        if !self.playing {
            return;
        }
        self.position_seconds += delta_seconds.max(0.0) * self.speed;
        if self.position_seconds >= duration - TIME_EPSILON {
            self.position_seconds = duration;
            self.playing = false;
        }
    }

    /// The per-tick transport step. When playing, moves the position forward
    /// by `delta_seconds` of real time at the current speed - pausing at the
    /// end of the timeline - and returns the audio cues playback crossed
    /// doing so, in order. When paused, or when `delta_seconds` is not
    /// positive, nothing moves and nothing is returned.
    pub fn advance_with_cues(&mut self, delta_seconds: f64, replay: &Replay) -> Vec<GameCue> {
        let previous = self.position_seconds;
        let was_playing = self.playing;
        self.advance(delta_seconds, replay.duration_seconds);
        if was_playing && self.position_seconds > previous {
            replay.audio_cues_between(previous, self.position_seconds)
        } else {
            Vec::new()
        }
    }

    /// Seeks to the start and pauses; does nothing when already there.
    pub fn rewind(&mut self) {
        if self.at_start() {
            return;
        }
        self.position_seconds = 0.0;
        self.playing = false;
    }

    /// Seeks to the previous [`Replay::significant_times`] entry (or the
    /// start) and pauses; does nothing when already at the start.
    pub fn jump_back(&mut self, replay: &Replay) {
        if self.at_start() {
            return;
        }
        self.position_seconds = replay.previous_significant_time(self.position_seconds);
        self.playing = false;
    }

    /// Seeks to the next [`Replay::significant_times`] entry (or the end)
    /// and pauses; does nothing when already at the end.
    pub fn jump_forward(&mut self, replay: &Replay) {
        if self.at_end(replay) {
            return;
        }
        self.position_seconds = replay.next_significant_time(self.position_seconds);
        self.playing = false;
    }

    /// Seeks to the end and pauses; does nothing when already there.
    pub fn fast_forward(&mut self, replay: &Replay) {
        if self.at_end(replay) {
            return;
        }
        self.position_seconds = replay.duration_seconds;
        self.playing = false;
    }

    /// Whether the position is at the start of the timeline (within the
    /// float tolerance every timeline comparison uses).
    pub fn at_start(&self) -> bool {
        self.position_seconds <= TIME_EPSILON
    }

    /// Whether the position is at the end of `replay`'s timeline (within the
    /// float tolerance every timeline comparison uses).
    pub fn at_end(&self, replay: &Replay) -> bool {
        self.position_seconds >= replay.duration_seconds - TIME_EPSILON
    }

    /// Sets the playback speed to one of [`PLAYBACK_SPEEDS`]. Any other
    /// value is ignored and the speed is left unchanged.
    pub fn set_speed(&mut self, speed: f64) {
        if PLAYBACK_SPEEDS.contains(&speed) {
            self.speed = speed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_settings::MatchSettings;
    use crate::replay::test_support::*;
    use crate::replay::{MatchRecorder, ReplayEvent};
    use crate::rules::Score;

    #[test]
    fn replay_frame_score_is_derived_from_current_board_state() {
        let mut current = frame();
        for black in [point(3, 4), point(5, 4), point(4, 3), point(4, 5)] {
            current.board.set(black, Cell::Black);
        }
        current.board.set(point(0, 0), Cell::White);
        current.board.set(point(8, 8), Cell::Gray);
        current.score = Score::default();

        assert_eq!(
            current.current_score(&MatchSettings::default()),
            Score { black: 4, white: 1 }
        );

        current.board.fill_territory();
        current.score = current.board.score();
        assert_eq!(
            current.current_score(&MatchSettings::default()),
            current.score
        );
        assert_eq!(
            current.current_score(&MatchSettings::default()),
            Score { black: 5, white: 1 }
        );
    }

    #[test]
    fn playback_speed_jump_rewind_and_end_pause_are_consistent() {
        let replay = sample_replay(1, 2, "Match");
        let mut controller = PlaybackController::default();
        controller.toggle(&replay);
        controller.set_speed(2.0);
        controller.advance(0.5, replay.duration_seconds);
        assert_eq!(controller.position_seconds, 1.0);
        assert!(controller.playing);

        controller.jump_forward(&replay);
        assert_eq!(controller.position_seconds, replay.duration_seconds);
        assert!(!controller.playing);
        controller.rewind();
        assert_eq!(controller.position_seconds, 0.0);
        assert!(!controller.playing);

        controller.toggle(&replay);
        controller.set_speed(0.5);
        controller.advance(99.0, replay.duration_seconds);
        assert_eq!(controller.position_seconds, replay.duration_seconds);
        assert!(!controller.playing);
        controller.toggle(&replay);
        assert_eq!(controller.position_seconds, replay.duration_seconds);
        assert!(!controller.playing);
    }

    #[test]
    fn fast_forward_seeks_exact_final_frame_and_remains_paused() {
        let mut recorder = MatchRecorder::new(1, frame());
        let mut final_frame = frame();
        final_frame.cursors = [point(2, 3), point(6, 5)];
        final_frame.teleport_markers = [Some(point(1, 2)), Some(point(7, 6))];
        final_frame.selected_moves = [None, None];
        // Each round places a further distinct stone, so the board actually
        // changes every time and the no-stone streak legitimately stays 0.
        for (current_round, stone_point) in (2..=4).zip([point(4, 4), point(5, 4), point(6, 4)]) {
            final_frame.board.set(stone_point, Cell::Black);
            final_frame.current_round = current_round;
            recorder.record(
                current_round as f64 - 1.0,
                ReplayEventKind::RoundResolved {
                    resolution: ResolutionKind::Sequential,
                    captures: [0, 0],
                },
                final_frame.clone(),
            );
        }
        final_frame.score = final_frame.board.score();
        final_frame.phase = ReplayPhase::Ended;
        let replay = recorder.complete(2, 7.25, final_frame.clone()).unwrap();
        replay.validate().unwrap();
        let mut controller = PlaybackController {
            position_seconds: 2.5,
            speed: 2.0,
            playing: true,
        };

        controller.fast_forward(&replay);

        assert_eq!(controller.position_seconds, replay.duration_seconds);
        assert!(!controller.playing);
        assert_eq!(replay.frame_at(controller.position_seconds), final_frame);
    }

    #[test]
    fn playback_boundaries_use_float_tolerance_and_inert_actions() {
        let replay = sample_replay(1, 2, "Match");
        let mut controller = PlaybackController {
            position_seconds: TIME_EPSILON / 2.0,
            speed: 1.0,
            playing: true,
        };
        let at_start = controller;

        assert!(controller.at_start());
        controller.rewind();
        controller.jump_back(&replay);
        assert_eq!(controller, at_start);

        controller.position_seconds = replay.duration_seconds - TIME_EPSILON / 2.0;
        controller.playing = false;
        let at_end = controller;
        assert!(controller.at_end(&replay));
        controller.jump_forward(&replay);
        controller.fast_forward(&replay);
        controller.toggle(&replay);
        assert_eq!(controller, at_end);
    }

    /// `pause` is the bare stop the Viewer's modals need: no start/end guard
    /// and no position change, unlike every seek method.
    #[test]
    fn pause_stops_playback_without_moving_or_guarding_the_position() {
        let replay = sample_replay(1, 2, "Match");
        for position_seconds in [0.0, 1.5, replay.duration_seconds] {
            let mut controller = PlaybackController {
                position_seconds,
                speed: 2.0,
                playing: true,
            };
            controller.pause();
            assert!(!controller.is_playing());
            assert_eq!(controller.position_seconds(), position_seconds);
            assert_eq!(controller.speed(), 2.0);
            controller.pause();
            assert!(!controller.is_playing());
            assert_eq!(controller.position_seconds(), position_seconds);
        }
    }

    /// `set_speed`'s allow-list is `PLAYBACK_SPEEDS` itself, and anything
    /// else is silently ignored rather than clamped or rejected.
    #[test]
    fn set_speed_accepts_exactly_the_published_speeds() {
        let mut controller = PlaybackController::default();
        for speed in PLAYBACK_SPEEDS {
            controller.set_speed(speed);
            assert_eq!(controller.speed(), speed);
        }
        let last_accepted = *PLAYBACK_SPEEDS.last().unwrap();
        for rejected in [0.0, 0.25, 1.5, 4.0, -1.0, f64::NAN] {
            controller.set_speed(rejected);
            assert_eq!(controller.speed(), last_accepted);
        }
    }

    #[test]
    fn playback_seek_rewind_jump_and_end_reconstruct_current_round() {
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
        let mut playback = PlaybackController::default();

        assert_eq!(replay.frame_at(playback.position_seconds).current_round, 1);
        playback.jump_forward(&replay);
        assert_eq!(playback.position_seconds, 1.0);
        assert_eq!(replay.frame_at(playback.position_seconds).current_round, 2);
        playback.jump_forward(&replay);
        assert_eq!(playback.position_seconds, 2.0);
        assert_eq!(replay.frame_at(playback.position_seconds).current_round, 2);
        playback.jump_back(&replay);
        assert_eq!(playback.position_seconds, 1.0);
        playback.rewind();
        assert_eq!(replay.frame_at(playback.position_seconds).current_round, 1);
    }

    #[test]
    fn playback_actions_reconstruct_progressive_scores_through_match_end() {
        let mut recorder = MatchRecorder::new(1, frame());
        let mut resolved = frame();
        for black in [point(3, 4), point(5, 4), point(4, 3), point(4, 5)] {
            resolved.board.set(black, Cell::Black);
        }
        resolved.board.set(point(0, 0), Cell::White);
        resolved.board.set(point(8, 8), Cell::Gray);
        resolved.current_round = 2;
        recorder.record(
            1.0,
            ReplayEventKind::RoundResolved {
                resolution: ResolutionKind::SamePointGray,
                captures: [0, 0],
            },
            resolved.clone(),
        );
        let mut final_frame = resolved;
        final_frame.board.fill_territory();
        final_frame.score = final_frame.board.score();
        let final_score = final_frame.score;
        let replay = recorder.complete(3, 2.0, final_frame).unwrap();
        let mut playback = PlaybackController::default();

        assert_eq!(
            replay
                .frame_at(playback.position_seconds)
                .current_score(&replay.match_settings),
            Score::default()
        );
        playback.toggle(&replay);
        playback.advance(0.5, replay.duration_seconds);
        assert!(playback.playing);
        assert_eq!(
            replay
                .frame_at(playback.position_seconds)
                .current_score(&replay.match_settings),
            Score::default()
        );
        playback.advance(0.5, replay.duration_seconds);
        assert_eq!(
            replay
                .frame_at(playback.position_seconds)
                .current_score(&replay.match_settings),
            Score { black: 4, white: 1 }
        );
        playback.jump_forward(&replay);
        assert!(!playback.playing);
        assert_eq!(
            replay
                .frame_at(playback.position_seconds)
                .current_score(&replay.match_settings),
            final_score
        );
        assert_eq!(final_score, replay.score);
        playback.jump_back(&replay);
        assert_eq!(
            replay
                .frame_at(playback.position_seconds)
                .current_score(&replay.match_settings),
            Score { black: 4, white: 1 }
        );
        playback.rewind();
        assert_eq!(
            replay
                .frame_at(playback.position_seconds)
                .current_score(&replay.match_settings),
            Score::default()
        );
    }

    #[test]
    fn replay_audio_crosses_events_once_at_every_speed_and_across_frame_skips() {
        let replay = replay_with_audio_track();
        let expected = vec![
            GameCue::BlackTeleport,
            GameCue::BothCapture,
            GameCue::GrayCapture,
            GameCue::MatchEnd,
        ];
        for speed in [0.5, 1.0, 2.0] {
            let mut playback = PlaybackController::default();
            playback.set_speed(speed);
            playback.toggle(&replay);
            assert_eq!(
                playback.advance_with_cues(4.0, &replay),
                expected,
                "speed {speed}"
            );
            assert!(playback.advance_with_cues(1.0, &replay).is_empty());
        }
    }

    #[test]
    fn replay_open_pause_and_seek_are_silent_but_play_after_rewind_repeats_future() {
        let replay = replay_with_audio_track();
        let mut playback = PlaybackController::default();
        assert!(playback.advance_with_cues(1.0, &replay).is_empty());

        playback.toggle(&replay);
        assert_eq!(
            playback.advance_with_cues(1.1, &replay),
            vec![GameCue::BlackTeleport, GameCue::BothCapture]
        );
        playback.toggle(&replay);
        assert!(playback.advance_with_cues(1.0, &replay).is_empty());

        playback.jump_forward(&replay);
        assert!(!playback.playing);
        playback.fast_forward(&replay);
        assert!(playback.advance_with_cues(1.0, &replay).is_empty());
        playback.rewind();
        assert!(playback.advance_with_cues(1.0, &replay).is_empty());

        playback.toggle(&replay);
        assert_eq!(
            playback.advance_with_cues(1.1, &replay),
            vec![GameCue::BlackTeleport, GameCue::BothCapture]
        );
    }

    #[test]
    fn playback_round_clock_interpolates_and_resets_at_event_boundaries() {
        let replay = round_timeout_replay();

        assert_eq!(replay.playback_frame_at(0.0).round_elapsed_seconds, 0.0);
        assert_eq!(replay.playback_frame_at(1.0).round_elapsed_seconds, 1.0);
        assert_eq!(replay.playback_frame_at(2.0).round_elapsed_seconds, 2.0);
        assert_eq!(replay.playback_frame_at(4.5).round_elapsed_seconds, 4.5);
        assert_eq!(replay.playback_frame_at(5.0).round_elapsed_seconds, 0.0);
        assert_eq!(replay.playback_frame_at(5.5).round_elapsed_seconds, 0.5);
        assert_eq!(replay.playback_frame_at(7.0).round_elapsed_seconds, 2.0);

        let opening = round_zero_replay();
        assert_eq!(opening.playback_frame_at(2.0).phase, ReplayPhase::Opening);
        assert_eq!(opening.playback_frame_at(2.0).round_elapsed_seconds, 0.0);
        assert_eq!(replay.playback_frame_at(99.0).phase, ReplayPhase::Ended);
        assert_eq!(replay.playback_frame_at(99.0).round_elapsed_seconds, 2.0);
    }

    #[test]
    fn round_zero_audio_plays_only_during_forward_playback_including_time_zero() {
        let mut replay = round_zero_replay();
        replay.events.insert(
            2,
            ReplayEvent {
                at_seconds: 0.0,
                kind: ReplayEventKind::MatchSound {
                    cue: GameCue::OpeningDuplicate,
                },
                frame: replay.events[1].frame.clone(),
            },
        );
        replay.validate().unwrap();
        let mut playback = PlaybackController::default();

        playback.toggle(&replay);
        assert_eq!(
            playback.advance_with_cues(3.3, &replay),
            vec![
                GameCue::OpeningDuplicate,
                GameCue::Countdown3,
                GameCue::Countdown2,
                GameCue::Countdown1,
                GameCue::Go
            ]
        );
        playback.rewind();
        playback.jump_forward(&replay);
        playback.fast_forward(&replay);
        assert!(playback.advance_with_cues(1.0, &replay).is_empty());
    }
}
