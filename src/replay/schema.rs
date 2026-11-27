//! The persisted Replay wire format: every type, constant, and field name
//! that appears on disk or in browser storage - and, since the same types
//! are what a headless consumer inspects, the public read surface over them.
//!
//! [`REPLAY_SCHEMA_VERSION`] and every `#[serde]` field name in this module
//! are part of the on-disk format. Renaming a field, reordering an enum's
//! variants, or changing the version silently invalidates existing replays.
//! Backward-compatible loading of older versions lives in
//! [`super::compat`]; structural and rules validation lives in
//! [`super::validation`].
//!
//! Types are public; fields are not. [`Replay`], [`ReplayEvent`], and
//! [`ReplayFrame`] expose accessors instead of `pub` fields so that a
//! consumer can read everything but cannot assemble a `Replay` that never
//! passed [`Replay::validate`] - see that method's docs for the ownership of
//! validation, and `Replay`'s own docs for how one is obtained.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::cue::GameCue;
use crate::match_settings::MatchSettings;
use crate::opening::{OpeningCountdownStage, OpeningSelections};
use crate::rules::{Board, Player, Point, ResolutionKind, Score};

/// The schema version every newly recorded Replay is written with.
///
/// Every version from [`LEGACY_REPLAY_SCHEMA_VERSION`] up to this one still
/// loads; [`Replay::validate`] rejects anything else with
/// [`ReplayError::UnsupportedVersion`]. A loaded Replay keeps the version it
/// was recorded with rather than being migrated, so [`Replay::version`]
/// equals this constant only for a Replay recorded by a build at least this
/// new. The version constants below each name the version that introduced
/// something a consumer can observe; their values are frozen by the files
/// that already carry them.
pub const REPLAY_SCHEMA_VERSION: u32 = 6;
/// Schema v1, the oldest version that still loads: timestamped events, each
/// with a complete frame. v1 frames did not persist a round number, so a raw
/// v1 [`ReplayFrame::current_round`] is `0`; [`Replay::playback_frame_at`]
/// reconstructs it from the resolution events and
/// [`Replay::completed_round_count`] counts them.
pub const LEGACY_REPLAY_SCHEMA_VERSION: u32 = 1;
/// Schema v2 added explicit round tracking: the Replay's completed round
/// count and every frame's [`ReplayFrame::current_round`] are persisted from
/// this version on.
pub const ROUND_TRACKING_REPLAY_SCHEMA_VERSION: u32 = 2;
/// Schema v3 added explicit [`ReplayEventKind::MatchSound`] events. Below it,
/// [`Replay::audio_cues_between`] derives best-effort cues from the board
/// change each round produced instead of reading a recorded track.
pub const EXPLICIT_AUDIO_REPLAY_SCHEMA_VERSION: u32 = 3;
/// Schema v4 added the Round 0 lifecycle: [`Replay::opening_completed`], and
/// on every frame the [`ReplayPhase`], the opening selections, and the
/// countdown stage. Older Replays never had a Round 0, so their defaults
/// (`Playing`, no selections, no countdown, `opening_completed == false`)
/// describe them accurately rather than standing in for missing data.
pub const OPENING_REPLAY_SCHEMA_VERSION: u32 = 4;
/// Schema v5 added the immutable Match settings snapshot, explicit end
/// reasons, timeout completions with their [`TimeoutCause`], per-round
/// capture deltas, and on every frame the capture totals, no-stone streak,
/// and round clock. Below it, [`Replay::end_reasons`] is exactly
/// `[EndReason::Legacy]`, [`Replay::match_settings`] is the all-disabled
/// default, and the per-frame counters are zero because they were never
/// recorded - not because nothing happened.
pub const MATCH_RULES_REPLAY_SCHEMA_VERSION: u32 = 5;
/// From this version on, an event's `frame.board` is `null` on disk whenever
/// it is identical to the previous event's board - see `compact_event_boards`
/// and `resolve_compact_event_boards` in the `compat` submodule. Measurement showed the
/// repeated, unchanged board dominates a Replay's serialized size (roughly
/// 650 of ~1,080 bytes per event, about 60%); most events within a round do
/// not change the board at all, so this alone cuts typical Replay size
/// substantially with no change to what is recorded, how seeking works, or
/// the in-memory [`ReplayFrame`] type, which always holds a complete board -
/// see [Replay size](../../docs/ARCHITECTURE.md#replay-size). Only
/// [`Replay::to_storage_value`]'s output shows the difference.
pub const COMPACT_BOARD_REPLAY_SCHEMA_VERSION: u32 = 6;
/// The label rule [`validate_label`] and [`Replay::rename`] enforce, in the
/// words shown to players; a [`LabelError`] displays as this text.
pub const REPLAY_LABEL_RULE: &str = "Start with a letter. Use up to 60 characters.";
/// The largest JSON document [`Replay::from_json`] parses. It bounds the
/// work done on untrusted bytes at the boundary where they enter; a storage
/// adapter may impose a tighter limit of its own (the browser adapter caps a
/// single record at 800 KiB).
///
/// It is also a *write* limit, which is easy to miss: the native adapter
/// checks the serialized text against it before writing a file, so this
/// value decides how long a Match may be as well as how large a file may be
/// read. Reading it as a parse bound alone is what once hid a long Match
/// that could not be saved - see
/// [Replay size](../../docs/ARCHITECTURE.md#replay-size). Both roles are
/// measured against the same compact encoding
/// [`Replay::to_storage_json`] writes.
pub const MAX_REPLAY_JSON_BYTES: usize = 8 * 1024 * 1024;
/// The most events [`Replay::validate`] accepts in one Replay.
///
/// This, rather than [`MAX_REPLAY_JSON_BYTES`], is what bounds a Replay's
/// length in practice: at the sizes `replay::measurement` reports, a Replay
/// reaches this limit well before its serialized bytes reach that one. It is
/// checked when a Replay is validated, not while one is recorded, so a Match
/// that runs past it records normally and fails once, at save.
pub const MAX_REPLAY_EVENTS: usize = 10_000;
pub(super) const MAX_REPLAY_CAPTURE_TOTAL: usize = MAX_REPLAY_EVENTS * crate::rules::CELL_COUNT;
pub(super) const MAX_END_REASONS: usize = 5;
/// The longest timeline [`Replay::validate`] accepts, in seconds (seven
/// days). The authoritative Match clock caps itself at the same value, so a
/// live Match can never outgrow what its Replay can persist.
pub const MAX_REPLAY_DURATION_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;
pub(super) const MAX_TIMESTAMP_MS: i64 = 8_640_000_000_000_000;
pub(super) const TIME_EPSILON: f64 = 0.000_001;

/// One completed Match and its validated, timestamped frame timeline.
///
/// # Obtaining one
///
/// Outside this crate a `Replay` comes only from [`Replay::from_json`] or
/// [`Replay::from_value`], and both run [`Replay::validate`] before
/// returning. The fields are private, there is no other constructor, and
/// `Replay` deliberately does not implement [`serde::Deserialize`] - the
/// hand-written per-version loader in `compat` is reachable only through
/// those two functions - so every `Replay` a consumer can hold has passed
/// validation, and every other method may assume it. The one mutation,
/// [`Replay::rename`], keeps it that way by validating the new label.
/// Recording a live Match into a new `Replay` is `recording`'s
/// `MatchRecorder`, which stays crate-private for the same reason: it does
/// not validate what it produces.
///
/// # Serializing one
///
/// [`Replay::to_storage_json`] and [`Replay::to_storage_value`] are the only
/// ways to serialize one. They produce the on-disk shape for this Replay's
/// own schema version, exactly as the game writes it: fields the version
/// never had are stripped, and from
/// [`COMPACT_BOARD_REPLAY_SCHEMA_VERSION`] on an unchanged board is omitted,
/// so a loaded v1 file re-saves as a v1 file.
///
/// `Replay` implements neither [`serde::Serialize`] nor
/// [`serde::Deserialize`], and for the same reason in both directions: a
/// derived impl would describe the complete in-memory shape - every field of
/// every frame, no board compaction, fields a v1-v4 file never had - which
/// is not the persisted format, and for a typical multi-round v6 Replay
/// roughly twice as large, because the unchanged board v6 omits is about 60%
/// of each event (see `docs/ARCHITECTURE.md`'s "Replay size"). The crate
/// keeps that shape to itself; `compat`'s two private shadow structs are what
/// carry the derives, one per direction.
///
/// Field names and enum representations are persisted compatibility
/// surfaces; see the module docs of [`crate::replay`] for the version
/// history.
///
/// The fields really are unreachable from outside the crate; this block
/// fails to compile because `duration_seconds` is private (the error code is
/// checked only on a nightly toolchain; `tests/headless_replay.rs` is the
/// other half of the proof, reaching every field's value through an
/// accessor instead):
///
/// ```compile_fail,E0616
/// fn duration(replay: &go_together::replay::Replay) -> f64 {
///     replay.duration_seconds
/// }
/// ```
///
/// The in-memory shape is unreachable in the same way, because there is no
/// `Serialize` impl to reach it through:
///
/// ```compile_fail,E0277
/// fn in_memory_shape(replay: &go_together::replay::Replay) -> serde_json::Value {
///     serde_json::to_value(replay).unwrap()
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    pub(crate) version: u32,
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) started_at_ms: i64,
    pub(crate) completed_at_ms: i64,
    pub(crate) duration_seconds: f64,
    pub(crate) score: Score,
    pub(crate) round_count: usize,
    /// Whether this replay includes a completed Round 0. `round_count` remains
    /// the number of completed ordinary rounds so schemas v1-v3 retain their
    /// historical meaning.
    pub(crate) opening_completed: bool,
    pub(crate) match_settings: MatchSettings,
    pub(crate) end_reasons: Vec<EndReason>,
    pub(crate) final_board: Board,
    pub(crate) events: Vec<ReplayEvent>,
}

impl Replay {
    /// The schema version this Replay was recorded with. See the version
    /// constants in [`crate::replay`] ([`LEGACY_REPLAY_SCHEMA_VERSION`]
    /// through [`REPLAY_SCHEMA_VERSION`]) for what each one introduced and
    /// what the accessors report below it.
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// The canonical hyphenated UUID that identifies this Replay - also its
    /// file name on native platforms and its record key in the browser.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The player-visible label; [`Replay::rename`] changes it within
    /// [`REPLAY_LABEL_RULE`].
    pub fn label(&self) -> &str {
        &self.label
    }

    /// When the Match started, in milliseconds since the Unix epoch.
    pub const fn started_at_ms(&self) -> i64 {
        self.started_at_ms
    }

    /// When the Match ended, in milliseconds since the Unix epoch; never
    /// earlier than [`Replay::started_at_ms`].
    pub const fn completed_at_ms(&self) -> i64 {
        self.completed_at_ms
    }

    /// The length of the timeline in Match-clock seconds: the timestamp of
    /// the final `MatchEnd` event, and the position every playback seek
    /// clamps to.
    pub const fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    /// The final score. Validation guarantees it equals the final frame's
    /// [`ReplayFrame::current_score`] under [`Replay::match_settings`], so
    /// it is the one score in a Replay that needs no phase to interpret.
    pub const fn final_score(&self) -> Score {
        self.score
    }

    /// The immutable Match settings the Match was played under. Below
    /// [`MATCH_RULES_REPLAY_SCHEMA_VERSION`] this is the all-disabled default,
    /// because those Replays predate optional rules.
    pub const fn match_settings(&self) -> &MatchSettings {
        &self.match_settings
    }

    /// Why the Match ended, in [`EndReason`]'s canonical order. Below
    /// [`MATCH_RULES_REPLAY_SCHEMA_VERSION`] it is exactly
    /// `[EndReason::Legacy]`: the old files did not persist enough to prove
    /// why play ended.
    pub fn end_reasons(&self) -> &[EndReason] {
        &self.end_reasons
    }

    /// The board at the end of the Match, after territory was filled -
    /// identical to the final event's frame.
    pub const fn final_board(&self) -> &Board {
        &self.final_board
    }

    /// Every recorded event in timeline order. The first is always
    /// `MatchStart` at time zero and the last `MatchEnd` at
    /// [`Replay::duration_seconds`]; timestamps never decrease, and several
    /// events may share one. Each carries the complete frame produced by it,
    /// so iterating here is the exact record; [`Replay::playback_frame_at`]
    /// is the time-keyed view built on top of it.
    pub fn events(&self) -> &[ReplayEvent] {
        &self.events
    }

    /// Who won, from [`Replay::final_score`].
    pub fn winner(&self) -> ReplayWinner {
        ReplayWinner::from_score(self.score)
    }

    /// Changes the label, or rejects the new one against
    /// [`REPLAY_LABEL_RULE`] and leaves the Replay unchanged. This is the only
    /// way a `Replay` changes after it is loaded, and the label rule is the
    /// same one [`Replay::validate`] checks, so a renamed Replay is still a
    /// validated one.
    pub fn rename(&mut self, label: String) -> Result<(), LabelError> {
        validate_label(&label)?;
        self.label = label;
        Ok(())
    }
}

/// Why a Match ended. The declaration order is the canonical persisted order
/// when more than one automatic limit crosses on the same completed round.
///
/// `Legacy` is not a way a Match ends: it is what schema v1-v4 Replays
/// report, because those files did not persist enough to prove why play
/// ended. The `snake_case` names are persisted from schema v5 on, and the
/// enum is `#[non_exhaustive]` because a new end condition would arrive with
/// a new schema version - a consumer matching on it needs a wildcard arm.
///
/// The authoritative core produces this ([`crate::authority::MatchState::finish`]
/// takes it, `RoundOutcome` reports it), but it is defined here, like
/// [`TimeoutCause`], because it is persisted: every `#[serde]` name in this
/// module is the on-disk contract, and keeping the persisted enums together
/// is what keeps that contract visible when one of them is edited.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EndReason {
    Manual,
    MatchTimeLimit,
    RoundLimit,
    NoStoneRoundLimit,
    PointTarget,
    Legacy,
}

impl EndReason {
    /// The player-facing name of this reason. Both screens that show one
    /// read it here: the Match screen, on the line under an ended Match's
    /// result, and the Replay Viewer's Match Settings list. `Display` prints
    /// the same text.
    ///
    /// One string serves both on purpose, so it has to read in a list as
    /// well as under a result. It is also written for a Match played in good
    /// faith (see `CONTRIBUTING.md`): `Manual` is what a completed end
    /// gesture records, and the gesture is an agreement between two players,
    /// even though one player at a shared keyboard can reach both Select
    /// keys.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Manual => "Agreed Ending",
            Self::MatchTimeLimit => "Match Time Limit",
            Self::RoundLimit => "Round Limit",
            Self::NoStoneRoundLimit => "No Board Change Limit",
            Self::PointTarget => "Point Target",
            Self::Legacy => "Legacy Match",
        }
    }

    pub(super) const fn order(self) -> u8 {
        match self {
            Self::Manual => 0,
            Self::MatchTimeLimit => 1,
            Self::RoundLimit => 2,
            Self::NoStoneRoundLimit => 3,
            Self::PointTarget => 4,
            Self::Legacy => 5,
        }
    }
}

impl fmt::Display for EndReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// Complete visual checkpoint stored after each Replay event.
///
/// Playback draws this value directly instead of re-running game rules. A
/// frame is a full snapshot - the board, both cursors, both Teleport
/// Markers, both locked selections - so it is a record for authorized
/// viewing after the fact, never something to show a live opponent; see
/// `docs/SECURITY.md`.
///
/// # Score
///
/// There is deliberately no accessor for the persisted `score` field. On a
/// v5+ Replay it is `Score::default()` on every frame until the recorded
/// [`ReplayPhase`] is `Ended`, and only then the final score - a
/// phase-dependent meaning that reads as "0-0 for the whole Match" to anyone
/// who does not know it, and validation enforces exactly that on every frame
/// of a v5+ Replay. Older versions guarantee only that the final frame
/// carries the final score (below v4 that frame's phase is still `Playing`),
/// and say nothing about the field mid-Match. [`ReplayFrame::current_score`]
/// derives the live score from the board and capture totals instead, for
/// every phase and version; [`Replay::final_score`] is the final one.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayFrame {
    pub(crate) board: Board,
    pub(crate) cursors: [Point; 2],
    pub(crate) teleport_markers: [Option<Point>; 2],
    pub(crate) selected_moves: [Option<Point>; 2],
    #[serde(default)]
    pub(crate) opening_selections: [OpeningSelections; 2],
    #[serde(default)]
    pub(crate) phase: ReplayPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) opening_countdown: Option<OpeningCountdownStage>,
    pub(crate) score: Score,
    #[serde(default)]
    pub(crate) current_round: usize,
    #[serde(default)]
    /// Cumulative capture credits, including stones self-removed by the
    /// opposing player. The field name is part of Replay v5.
    pub(crate) capture_totals: Score,
    #[serde(default)]
    pub(crate) no_stone_streak: usize,
    #[serde(default)]
    pub(crate) round_elapsed_seconds: f64,
}

impl ReplayFrame {
    /// The board after this event.
    pub const fn board(&self) -> &Board {
        &self.board
    }

    /// Both players' cursor positions, indexed by
    /// [`crate::authority::player_index`].
    pub const fn cursors(&self) -> [Point; 2] {
        self.cursors
    }

    /// Both players' Teleport Markers, indexed like [`ReplayFrame::cursors`].
    pub const fn teleport_markers(&self) -> [Option<Point>; 2] {
        self.teleport_markers
    }

    /// Each player's locked selection for the round in progress, indexed like
    /// [`ReplayFrame::cursors`]: `None` before that player locks and again
    /// once the round completes. Hidden from the opponent during live play;
    /// a Replay reveals it after the fact.
    pub const fn selected_moves(&self) -> [Option<Point>; 2] {
        self.selected_moves
    }

    /// Each player's Round 0 selections, indexed like
    /// [`ReplayFrame::cursors`]. Empty outside the opening phase, and on
    /// every frame below [`OPENING_REPLAY_SCHEMA_VERSION`].
    pub const fn opening_selections(&self) -> &[OpeningSelections; 2] {
        &self.opening_selections
    }

    /// Where the Match was at this event. Always `Playing` below
    /// [`OPENING_REPLAY_SCHEMA_VERSION`].
    pub const fn phase(&self) -> ReplayPhase {
        self.phase
    }

    /// The Round 0 closing-countdown stage on display at this event, if any.
    pub const fn opening_countdown(&self) -> Option<OpeningCountdownStage> {
        self.opening_countdown
    }

    /// The round in progress: `0` during Round 0, `1` from the reveal on,
    /// and one more after each ordinary completion. Raw
    /// [`LEGACY_REPLAY_SCHEMA_VERSION`] frames report `0` because v1 never
    /// recorded it; [`Replay::playback_frame_at`] reconstructs it for them.
    pub const fn current_round(&self) -> usize {
        self.current_round
    }

    /// Cumulative capture credits per player, including stones the opposing
    /// player self-removed, indexed like the [`Score`] it is. Recorded from
    /// [`MATCH_RULES_REPLAY_SCHEMA_VERSION`] on; zero below it.
    pub const fn capture_totals(&self) -> Score {
        self.capture_totals
    }

    /// How many consecutive completed rounds have left the board exactly as
    /// it was. Recorded from [`MATCH_RULES_REPLAY_SCHEMA_VERSION`] on; zero
    /// below it.
    pub const fn no_stone_streak(&self) -> usize {
        self.no_stone_streak
    }

    /// Seconds elapsed in the ordinary round in progress, as recorded at
    /// this event; [`Replay::playback_frame_at`] advances it to the requested
    /// time. Recorded from [`MATCH_RULES_REPLAY_SCHEMA_VERSION`] on; zero
    /// below it.
    pub const fn round_elapsed_seconds(&self) -> f64 {
        self.round_elapsed_seconds
    }

    /// The score as it stood at this event, derived from the board and the
    /// capture totals under `settings` - the accessor to use for a running
    /// score, in every phase. Pass [`Replay::match_settings`] of the Replay
    /// the frame came from: whether capture credits count is a per-Match
    /// rule, and the default settings would silently drop them.
    pub fn current_score(&self, settings: &MatchSettings) -> Score {
        self.board
            .score_with_captures(self.capture_totals, settings.capture_points())
    }
}

/// Where a Match was when a frame was recorded.
///
/// The persisted, independently-versioned counterpart of
/// [`crate::authority::MatchPhase`]: Round 0 and its closing countdown are
/// both `Opening` here, because a Replay frame carries the countdown stage
/// separately. Always `Playing` below [`OPENING_REPLAY_SCHEMA_VERSION`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayPhase {
    Opening,
    #[default]
    Playing,
    Ended,
}

/// A timestamped semantic event plus the frame produced by it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayEvent {
    pub(crate) at_seconds: f64,
    pub(crate) kind: ReplayEventKind,
    pub(crate) frame: ReplayFrame,
}

impl ReplayEvent {
    /// When this event happened, in Match-clock seconds from the start.
    pub const fn at_seconds(&self) -> f64 {
        self.at_seconds
    }

    /// What happened.
    pub const fn kind(&self) -> &ReplayEventKind {
        &self.kind
    }

    /// The complete state immediately after this event.
    pub const fn frame(&self) -> &ReplayFrame {
        &self.frame
    }
}

/// Persisted event vocabulary used for stepping, compatibility, and audio.
///
/// The `snake_case` `type` tags and field names are the on-disk format. The
/// vocabulary has grown with the schema - `MatchSound` in v3, the `Opening*`
/// events in v4, `RoundTimedOut` and the `captures` payloads in v5 - and
/// existing variants have gained fields, so both the enum and its struct
/// variants are `#[non_exhaustive]`: match with a wildcard arm and `..`, and
/// a future version's additions will not break the match.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum ReplayEventKind {
    MatchStart,
    OpeningStarted,
    #[non_exhaustive]
    OpeningSelectionChanged {
        player: Player,
        point: Point,
        evicted: Option<Point>,
    },
    #[non_exhaustive]
    OpeningSelectionDuplicate {
        player: Player,
        point: Point,
    },
    #[non_exhaustive]
    OpeningCountdown {
        stage: OpeningCountdownStage,
    },
    OpeningReveal,
    OpeningCountdownFinished,
    #[non_exhaustive]
    CursorMoved {
        player: Player,
        from: Point,
        to: Point,
    },
    #[non_exhaustive]
    SelectionChanged {
        player: Player,
        point: Point,
    },
    #[non_exhaustive]
    MarkerChanged {
        player: Player,
        point: Point,
    },
    #[non_exhaustive]
    JumpToMarker {
        player: Player,
        from: Point,
        to: Point,
    },
    #[non_exhaustive]
    RoundResolved {
        resolution: ResolutionKind,
        /// Persisted field name retained for Replay v5 compatibility. Values
        /// are capture credits, including opponent self-removals.
        #[serde(default, skip_serializing_if = "capture_delta_is_zero")]
        captures: [usize; 2],
    },
    #[non_exhaustive]
    RoundTimedOut {
        resolution: ResolutionKind,
        submitted: [bool; 2],
        /// Persisted field name retained for Replay v5 compatibility. Values
        /// are capture credits, including opponent self-removals.
        #[serde(default, skip_serializing_if = "capture_delta_is_zero")]
        captures: [usize; 2],
        cause: TimeoutCause,
    },
    #[non_exhaustive]
    MatchSound {
        cue: GameCue,
    },
    MatchEnd,
}

impl ReplayEventKind {
    pub(super) const fn capture_delta(&self) -> [usize; 2] {
        match self {
            Self::RoundResolved { captures, .. } | Self::RoundTimedOut { captures, .. } => {
                *captures
            }
            _ => [0, 0],
        }
    }

    /// Whether the event is one playback steps to: `MatchStart` and
    /// `MatchEnd`; `OpeningStarted`, every opening selection change, every
    /// countdown stage, and the reveal; Teleport Marker changes and jumps;
    /// and round completions, timed out or not. Cursor movement, ordinary
    /// selection changes, duplicate opening picks, `OpeningCountdownFinished`,
    /// and sounds are not stops. [`Replay::significant_times`] is the timeline
    /// built from this.
    pub const fn is_significant(&self) -> bool {
        matches!(
            self,
            Self::MatchStart
                | Self::OpeningStarted
                | Self::OpeningSelectionChanged { .. }
                | Self::OpeningCountdown { .. }
                | Self::OpeningReveal
                | Self::MarkerChanged { .. }
                | Self::JumpToMarker { .. }
                | Self::RoundResolved { .. }
                | Self::RoundTimedOut { .. }
                | Self::MatchEnd
        )
    }
}

pub(super) fn capture_delta_is_zero(captures: &[usize; 2]) -> bool {
    *captures == [0, 0]
}

/// Which timer expired to complete an ordinary round by timeout, persisted
/// on [`ReplayEventKind::RoundTimedOut`] from
/// [`MATCH_RULES_REPLAY_SCHEMA_VERSION`] on.
///
/// Like [`EndReason`], this describes something the authoritative core
/// decided ([`crate::authority::OrdinaryCompletion::timeout_cause`] produces
/// it) and lives here because it is persisted: every `#[serde]` name in this
/// module is the on-disk contract, and keeping the persisted enums together
/// is what keeps that contract visible.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutCause {
    RoundTime,
    MatchTime,
}

/// The result of a Match, from its final score.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayWinner {
    Black,
    White,
    Draw,
}

impl ReplayWinner {
    pub(crate) fn from_score(score: Score) -> Self {
        match score.black.cmp(&score.white) {
            Ordering::Greater => Self::Black,
            Ordering::Less => Self::White,
            Ordering::Equal => Self::Draw,
        }
    }

    /// The player-facing name of this result - "Black", "White", or "Draw";
    /// `Display` prints the same text, and a default Replay label begins with
    /// it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Black => "Black",
            Self::White => "White",
            Self::Draw => "Draw",
        }
    }
}

impl fmt::Display for ReplayWinner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// Replay validation, serialization, and storage failures.
///
/// `Storage` and `StorageUnavailable` are produced by the `client` feature's
/// storage adapters; the headless loading and validation paths produce the
/// rest. `#[non_exhaustive]`, as error enums should be.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReplayError {
    /// The bytes were not the JSON shape of any supported schema version.
    Json(serde_json::Error),
    /// A storage adapter could not read, write, or delete a record.
    Storage(String),
    /// The input exceeded a size limit before it was parsed - see
    /// [`MAX_REPLAY_JSON_BYTES`].
    #[non_exhaustive]
    DataTooLarge {
        item: &'static str,
        max_bytes: usize,
    },
    /// The Replay's ID is not a canonical hyphenated UUID.
    InvalidId,
    /// The Replay parsed but failed structural or rules validation; the
    /// message says which check.
    InvalidData(String),
    /// The platform's storage cannot be used at all (no data directory, no
    /// `localStorage`).
    StorageUnavailable(String),
    /// The Replay's schema version is not one this build loads: `0`, or
    /// newer than [`REPLAY_SCHEMA_VERSION`] - the realistic case being a
    /// file written by a newer build, which is rejected whole rather than
    /// partially read.
    UnsupportedVersion(u32),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid JSON: {error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::DataTooLarge { item, max_bytes } => {
                write!(formatter, "{item} exceeds the {max_bytes}-byte limit")
            }
            Self::InvalidId => write!(formatter, "invalid replay ID"),
            Self::InvalidData(message) => write!(formatter, "{message}"),
            Self::StorageUnavailable(message) => write!(formatter, "{message}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported replay version {version}")
            }
        }
    }
}

impl std::error::Error for ReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Storage(_)
            | Self::DataTooLarge { .. }
            | Self::InvalidId
            | Self::InvalidData(_)
            | Self::StorageUnavailable(_)
            | Self::UnsupportedVersion(_) => None,
        }
    }
}

/// Why a Replay label was rejected. See [`REPLAY_LABEL_RULE`] for the rule
/// shown to players; every variant displays as that rule.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum LabelError {
    Empty,
    TooLong,
    MustStartWithLetter,
    InvalidCharacter,
}

impl fmt::Display for LabelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{REPLAY_LABEL_RULE}")
    }
}

impl std::error::Error for LabelError {}

impl From<LabelError> for ReplayError {
    fn from(error: LabelError) -> Self {
        Self::InvalidData(error.to_string())
    }
}

pub(crate) fn is_printable_ascii(character: char) -> bool {
    (' '..='~').contains(&character)
}

/// Checks a label against [`REPLAY_LABEL_RULE`]: non-empty, at most 60
/// characters, starting with an ASCII letter, printable ASCII throughout.
/// [`Replay::rename`] applies exactly this check; a host can apply it to a
/// requested label before loading the Replay it is for.
pub fn validate_label(label: &str) -> Result<(), LabelError> {
    if label.is_empty() {
        return Err(LabelError::Empty);
    }
    if label.chars().count() > 60 {
        return Err(LabelError::TooLong);
    }
    if !label
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
    {
        return Err(LabelError::MustStartWithLetter);
    }
    if label
        .chars()
        .find(|character| !is_printable_ascii(*character))
        .is_some()
    {
        return Err(LabelError::InvalidCharacter);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_validation_accepts_every_printable_ascii_character_after_a_letter() {
        assert!(validate_label("A").is_ok());
        assert!(validate_label(&format!("A{}", "b".repeat(59))).is_ok());
        assert!(validate_label("Game: 1").is_ok());
        for byte in 0x20_u8..=0x7e {
            assert!(
                validate_label(&format!("A{}", char::from(byte))).is_ok(),
                "printable ASCII byte {byte:#04x} should be accepted"
            );
        }
    }

    #[test]
    fn label_validation_rejects_invalid_boundaries_and_characters() {
        assert!(matches!(validate_label(""), Err(LabelError::Empty)));
        assert!(matches!(
            validate_label(&format!("A{}", "b".repeat(60))),
            Err(LabelError::TooLong)
        ));
        for byte in 0x20_u8..=0x7e {
            let character = char::from(byte);
            if !character.is_ascii_alphabetic() {
                assert!(
                    matches!(
                        validate_label(&character.to_string()),
                        Err(LabelError::MustStartWithLetter)
                    ),
                    "printable ASCII byte {byte:#04x} cannot be the first character"
                );
            }
        }
        for byte in 0_u8..=0x1f {
            assert!(
                matches!(
                    validate_label(&format!("A{}", char::from(byte))),
                    Err(LabelError::InvalidCharacter)
                ),
                "control byte {byte:#04x} should be rejected"
            );
        }
        assert!(matches!(
            validate_label("A\u{7f}"),
            Err(LabelError::InvalidCharacter)
        ));
        assert!(matches!(
            validate_label("Café"),
            Err(LabelError::InvalidCharacter)
        ));
        assert_eq!(LabelError::Empty.to_string(), REPLAY_LABEL_RULE);
    }
}
