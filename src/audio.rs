//! Sound: the cue queue, the semantic cue vocabulary, and playback.
//!
//! # Responsibility
//!
//! Nothing in the crate plays a sound directly. Systems push semantic cues -
//! [`GameCue`] for match events, [`UiCue`] for interface feedback - onto
//! [`AudioCueQueue`], and this module turns the queue into audio once per
//! frame. That indirection is what lets replays record *cues* rather than
//! sounds, and lets one volume setting mute a whole class of feedback.
//!
//! # Channels
//!
//! [`AudioChannel::Game`] and [`AudioChannel::Ui`] have independent volume
//! levels. Match cues and replayed match cues share the game channel, so a
//! replay sounds exactly like the match did.
//!
//! # Buttons
//!
//! [`UiSound`] attaches cues to a button entity. Its activation cue is queued
//! by an observer of Bevy's `Activate`, which is what every activation of a
//! button is - a pointer click, or the focus ring's Enter, Space or South
//! (`ui::button` says how) - at the moment it happens, in `PreUpdate`. So a
//! click is heard even when the same activation changes screen and despawns
//! the button, and the cue is the same whichever input made it. Its hover
//! cue is queued by `emit_hover_cues`, from the button's `Hovered`, in
//! [`UiAudioSet`] - which the whole `Update` pipeline is ordered after. A
//! disabled button has its cues muted rather than removed, so an inert
//! control never sounds like it worked.
//!
//! # Assets
//!
//! Original marimba and wooden-impact samples are loaded from `assets/audio`.
//! A missing or unloadable sample is reported once and then ignored, so audio
//! problems degrade to silence instead of crashing or spamming.
//!
//! Browsers additionally gate playback until the first key, pointer button,
//! button activation (which covers a tap), or gamepad button. Cues queued
//! before that gesture are discarded, not replayed later, so autoplay
//! suspension cannot create a burst.

use std::collections::{HashMap, HashSet};

use bevy::asset::LoadState;
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::window::CursorMoved;

#[cfg(test)]
use crate::rules::RoundSound;
use crate::settings::{GamePreferences, VolumeLevel};

const BLACK_CAPTURE_NOTES: [MarimbaNote; 1] = [MarimbaNote::C4];
const WHITE_CAPTURE_NOTES: [MarimbaNote; 1] = [MarimbaNote::G4];
const BOTH_CAPTURE_NOTES: [MarimbaNote; 2] = [MarimbaNote::C4, MarimbaNote::G4];
const GRAY_CHORD_NOTES: [MarimbaNote; 3] = [MarimbaNote::C4, MarimbaNote::E4, MarimbaNote::G4];
const SAME_POINT_GRAY_NOTES: [MarimbaNote; 1] = [GRAY_CHORD_NOTES[2]];
const BLACK_TELEPORT_NOTES: [MarimbaNote; 1] = [MarimbaNote::D4];
const WHITE_TELEPORT_NOTES: [MarimbaNote; 1] = [MarimbaNote::A4];
const COUNTDOWN_NOTES: [MarimbaNote; 1] = [MarimbaNote::C5];
const GO_NOTES: [MarimbaNote; 3] = [MarimbaNote::C5, MarimbaNote::E5, MarimbaNote::G5];
const MATCH_END_NOTES: [MarimbaNote; 1] = [MarimbaNote::B3];
const MATCH_RESTART_NOTES: [MarimbaNote; 1] = [MarimbaNote::A3];
const GAME_PREVIEW_NOTES: [MarimbaNote; 1] = [MarimbaNote::E4];
const UI_CONFIRM_NOTES: [MarimbaNote; 1] = [MarimbaNote::G5];
const UI_CANCEL_NOTES: [MarimbaNote; 1] = [MarimbaNote::C4];
const UI_DESTRUCTIVE_NOTES: [MarimbaNote; 1] = [MarimbaNote::A3];
const UI_NEUTRAL_NOTES: [MarimbaNote; 1] = [MarimbaNote::E5];
const UI_HOVER_NOTES: [MarimbaNote; 1] = [MarimbaNote::D5];

/// Loads marimba assets and drains semantic cue queues into Bevy audio players.
pub(crate) struct GameAudioPlugin;

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct UiAudioSet;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioCueQueue>()
            .init_resource::<AudioInteractionGate>()
            .init_resource::<AudioAssetFailures>()
            .add_systems(Startup, load_audio_assets)
            .add_observer(unlock_audio_on_activation)
            .add_observer(queue_activation_cue)
            .add_systems(
                Update,
                (unlock_audio_after_interaction, emit_hover_cues)
                    .chain()
                    .in_set(UiAudioSet),
            )
            .add_systems(
                PostUpdate,
                (
                    play_queued_cues,
                    report_audio_asset_failures,
                    cleanup_failed_audio,
                )
                    .chain(),
            );
    }
}

/// Persisted semantic sounds produced by live Matches and Replay playback.
///
/// Defined in [`crate::cue`], outside both this module and [`crate::replay`],
/// so the headless core can persist `MatchSound` events without depending on
/// Bevy audio.
///
/// Re-exported here because at a call site `GameCue` is *this* module's
/// vocabulary: `gameplay` imports it beside [`AudioCueQueue`] and passes one
/// to [`AudioCueQueue::game`], so both halves of that one idiom come from one
/// place. That is the reason to keep the re-export. It is not the reason it
/// was created - it arrived with the headless-core split to leave existing
/// paths working, and that justification expired once the split was done, so
/// do not treat this as a migration shim awaiting cleanup.
///
/// [`crate::cue::GameCue`] stays the canonical path, and everything in
/// `replay` names it that way.
pub(crate) use crate::cue::GameCue;

/// Interface feedback that is never written into a Replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiCue {
    Confirm,
    Cancel,
    Destructive,
    Neutral,
    Hover,
    Preview,
    Marimba(MarimbaNote),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AudioCue {
    Game(GameCue),
    Ui(UiCue),
}

pub(crate) const fn volume_preview(channel: AudioChannel, level: VolumeLevel) -> Option<AudioCue> {
    if matches!(level, VolumeLevel::Mute) {
        return None;
    }
    Some(match channel {
        AudioChannel::Game => AudioCue::Game(GameCue::Preview),
        AudioChannel::Ui => AudioCue::Ui(UiCue::Preview),
    })
}

impl AudioCue {
    const fn channel(self) -> AudioChannel {
        match self {
            Self::Game(_) => AudioChannel::Game,
            Self::Ui(_) => AudioChannel::Ui,
        }
    }

    const fn definition(self) -> CueDefinition {
        match self {
            Self::Game(GameCue::Countdown3 | GameCue::Countdown2 | GameCue::Countdown1) => {
                CueDefinition::single(&COUNTDOWN_NOTES)
            }
            Self::Game(GameCue::Go) => CueDefinition::chord(&GO_NOTES),
            Self::Game(GameCue::BlackCapture) => CueDefinition::single(&BLACK_CAPTURE_NOTES),
            Self::Game(GameCue::WhiteCapture) => CueDefinition::single(&WHITE_CAPTURE_NOTES),
            Self::Game(GameCue::BothCapture) => CueDefinition::harmony(&BOTH_CAPTURE_NOTES),
            Self::Game(GameCue::GrayCapture) => CueDefinition::chord(&GRAY_CHORD_NOTES),
            Self::Game(GameCue::SamePointGray) => CueDefinition::single(&SAME_POINT_GRAY_NOTES),
            Self::Game(GameCue::OpeningDuplicate) => {
                CueDefinition::effect(EffectSample::OpeningClack, 0.7)
            }
            Self::Game(GameCue::BlackTeleport) => CueDefinition::single(&BLACK_TELEPORT_NOTES),
            Self::Game(GameCue::WhiteTeleport) => CueDefinition::single(&WHITE_TELEPORT_NOTES),
            Self::Game(GameCue::MatchEnd) => CueDefinition::single(&MATCH_END_NOTES),
            Self::Game(GameCue::MatchRestart) => CueDefinition::single(&MATCH_RESTART_NOTES),
            Self::Game(GameCue::Preview) => CueDefinition::single(&GAME_PREVIEW_NOTES),
            Self::Ui(UiCue::Confirm) => CueDefinition::single(&UI_CONFIRM_NOTES),
            Self::Ui(UiCue::Cancel) => CueDefinition::single(&UI_CANCEL_NOTES),
            Self::Ui(UiCue::Destructive) => CueDefinition::single(&UI_DESTRUCTIVE_NOTES),
            Self::Ui(UiCue::Neutral | UiCue::Preview) => CueDefinition::single(&UI_NEUTRAL_NOTES),
            Self::Ui(UiCue::Hover) => CueDefinition {
                source: CueSource::Marimba(&UI_HOVER_NOTES),
                per_note_gain: 0.18,
            },
            Self::Ui(UiCue::Marimba(note)) => CueDefinition {
                source: CueSource::Marimba(note.as_slice()),
                per_note_gain: 0.58,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum AudioChannel {
    Game,
    Ui,
}

#[derive(Clone, Copy)]
struct CueDefinition {
    source: CueSource,
    per_note_gain: f32,
}

impl CueDefinition {
    const fn single(notes: &'static [MarimbaNote]) -> Self {
        Self {
            source: CueSource::Marimba(notes),
            per_note_gain: 0.72,
        }
    }

    const fn harmony(notes: &'static [MarimbaNote]) -> Self {
        Self {
            source: CueSource::Marimba(notes),
            per_note_gain: 0.48,
        }
    }

    /// The per-note gain is bounded: the three simultaneous notes of the
    /// largest chord, at the loudest channel setting, must not exceed unity
    /// linear volume in total (3 * 0.34 * `VolumeLevel::High.gain()` =
    /// 0.918). `volume_levels_are_separate_and_have_safe_gains` pins that
    /// bound with the literal 0.34 written out again, so change both
    /// together.
    const fn chord(notes: &'static [MarimbaNote]) -> Self {
        Self {
            source: CueSource::Marimba(notes),
            per_note_gain: 0.34,
        }
    }

    const fn effect(sample: EffectSample, gain: f32) -> Self {
        Self {
            source: CueSource::Effect(sample),
            per_note_gain: gain,
        }
    }
}

#[derive(Clone, Copy)]
enum CueSource {
    Marimba(&'static [MarimbaNote]),
    Effect(EffectSample),
}

#[cfg(test)]
impl CueSource {
    const fn marimba_notes(self) -> Option<&'static [MarimbaNote]> {
        match self {
            Self::Marimba(notes) => Some(notes),
            Self::Effect(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum EffectSample {
    OpeningClack,
}

impl EffectSample {
    const ALL: [Self; 1] = [Self::OpeningClack];

    const fn asset_path(self) -> &'static str {
        match self {
            Self::OpeningClack => "audio/opening_duplicate_clack.wav",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum MarimbaNote {
    A3,
    B3,
    C4,
    D4,
    E4,
    G4,
    A4,
    C5,
    D5,
    E5,
    G5,
}

impl MarimbaNote {
    const ALL: [Self; 11] = [
        Self::A3,
        Self::B3,
        Self::C4,
        Self::D4,
        Self::E4,
        Self::G4,
        Self::A4,
        Self::C5,
        Self::D5,
        Self::E5,
        Self::G5,
    ];

    const fn asset_path(self) -> &'static str {
        match self {
            Self::A3 => "audio/marimba_a3.wav",
            Self::B3 => "audio/marimba_b3.wav",
            Self::C4 => "audio/marimba_c4.wav",
            Self::D4 => "audio/marimba_d4.wav",
            Self::E4 => "audio/marimba_e4.wav",
            Self::G4 => "audio/marimba_g4.wav",
            Self::A4 => "audio/marimba_a4.wav",
            Self::C5 => "audio/marimba_c5.wav",
            Self::D5 => "audio/marimba_d5.wav",
            Self::E5 => "audio/marimba_e5.wav",
            Self::G5 => "audio/marimba_g5.wav",
        }
    }

    const fn as_slice(self) -> &'static [Self] {
        match self {
            Self::A3 => &[Self::A3],
            Self::B3 => &[Self::B3],
            Self::C4 => &[Self::C4],
            Self::D4 => &[Self::D4],
            Self::E4 => &[Self::E4],
            Self::G4 => &[Self::G4],
            Self::A4 => &[Self::A4],
            Self::C5 => &[Self::C5],
            Self::D5 => &[Self::D5],
            Self::E5 => &[Self::E5],
            Self::G5 => &[Self::G5],
        }
    }

    #[cfg(test)]
    const fn frequency(self) -> f32 {
        match self {
            Self::A3 => 220.0,
            Self::B3 => 246.942,
            Self::C4 => 261.626,
            Self::D4 => 293.665,
            Self::E4 => 329.628,
            Self::G4 => 391.995,
            Self::A4 => 440.0,
            Self::C5 => 523.251,
            Self::D5 => 587.330,
            Self::E5 => 659.255,
            Self::G5 => 783.991,
        }
    }
}

/// Per-frame semantic audio outbox.
///
/// Producers never need asset handles or volume logic; `PostUpdate` drains
/// this queue once after gameplay and UI systems finish.
#[derive(Resource, Default)]
pub(crate) struct AudioCueQueue {
    pending: Vec<AudioCue>,
}

impl AudioCueQueue {
    pub(crate) fn game(&mut self, cue: GameCue) {
        self.pending.push(AudioCue::Game(cue));
    }

    pub(crate) fn ui(&mut self, cue: UiCue) {
        self.pending.push(AudioCue::Ui(cue));
    }

    #[cfg(test)]
    pub(crate) fn drain(&mut self) -> Vec<AudioCue> {
        std::mem::take(&mut self.pending)
    }
}

/// Per-button sound state, read by [`queue_activation_cue`] and
/// [`emit_hover_cues`].
///
/// The activation cue is queued the moment the button is activated, by any
/// input. `Hover` is queued only when the pointer comes onto the button in a
/// frame where it actually moved, so a button that ends up under a
/// stationary pointer - a layout change, a covering element going away -
/// stays silent; staying on it through a press and release is not coming
/// onto it again. The first frame after spawning only records whether the
/// button is hovered, so a button spawned under the pointer never sounds on
/// its own. [`Self::set_enabled`] with `false` mutes both cues.
/// [`Self::preview_only`] is for the volume stops, whose click is answered by
/// the preview cue itself rather than by a click sound.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct UiSound {
    activation: Option<UiCue>,
    enabled: bool,
    initialized: bool,
    hovered: bool,
}

impl UiSound {
    pub(crate) const fn new(activation: UiCue) -> Self {
        Self {
            activation: Some(activation),
            enabled: true,
            initialized: false,
            hovered: false,
        }
    }

    pub(crate) const fn preview_only() -> Self {
        Self {
            activation: None,
            enabled: true,
            initialized: false,
            hovered: false,
        }
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// The cue for one frame of a button's hover state: `Hover` when the
/// pointer has just come onto an enabled button, and moved to get there.
fn hover_cue(
    was_hovered: bool,
    hovered: bool,
    enabled: bool,
    pointer_moved: bool,
) -> Option<UiCue> {
    (enabled && hovered && !was_hovered && pointer_moved).then_some(UiCue::Hover)
}

/// Queues an activated button's activation cue, unless it is muted. An
/// observer of Bevy's `Activate`, so it runs the moment the activation is
/// triggered - see the module docs for why that is the moment.
pub(crate) fn queue_activation_cue(
    activate: On<Activate>,
    sounds: Query<&UiSound>,
    mut cues: ResMut<AudioCueQueue>,
) {
    if let Ok(sound) = sounds.get(activate.entity)
        && sound.enabled
        && let Some(cue) = sound.activation
    {
        cues.ui(cue);
    }
}

pub(crate) fn emit_hover_cues(
    mut cursor_events: MessageReader<CursorMoved>,
    mut buttons: Query<(&Hovered, &mut UiSound)>,
    mut cues: ResMut<AudioCueQueue>,
) {
    // Drained in full: a burst of moves in one frame must not leave one
    // unread, to be counted as movement in a later frame that had none.
    let pointer_moved = cursor_events.read().count() > 0;
    for (hovered, mut sound) in &mut buttons {
        let hovered = hovered.get();
        if !sound.initialized {
            sound.hovered = hovered;
            sound.initialized = true;
            continue;
        }
        if let Some(cue) = hover_cue(sound.hovered, hovered, sound.enabled, pointer_moved) {
            cues.ui(cue);
        }
        sound.hovered = hovered;
    }
}

#[derive(Resource)]
struct AudioAssets {
    notes: HashMap<MarimbaNote, Handle<AudioSource>>,
    effects: HashMap<EffectSample, Handle<AudioSource>>,
}

#[derive(Resource, Default)]
struct AudioAssetFailures {
    notes: HashSet<MarimbaNote>,
    effects: HashSet<EffectSample>,
}

#[derive(Component)]
struct PlayingAudio;

#[derive(Resource)]
struct AudioInteractionGate {
    unlocked: bool,
}

impl Default for AudioInteractionGate {
    fn default() -> Self {
        Self {
            unlocked: !cfg!(target_arch = "wasm32"),
        }
    }
}

fn unlock_audio_after_interaction(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    mut gate: ResMut<AudioInteractionGate>,
) {
    if gate.unlocked {
        return;
    }
    gate.unlocked = keys.get_just_pressed().next().is_some()
        || mouse_buttons.get_just_pressed().next().is_some()
        || gamepads
            .iter()
            .any(|gamepad| gamepad.get_just_pressed().next().is_some());
}

/// Any button activation is a gesture too: the one a tap makes, which none
/// of the device state [`unlock_audio_after_interaction`] reads reports. It
/// lands on the release, which is also when a browser counts a tap as one.
fn unlock_audio_on_activation(_activate: On<Activate>, mut gate: ResMut<AudioInteractionGate>) {
    if !gate.unlocked {
        gate.unlocked = true;
    }
}

fn load_audio_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let notes = MarimbaNote::ALL
        .into_iter()
        .map(|note| (note, asset_server.load(note.asset_path())))
        .collect();
    let effects = EffectSample::ALL
        .into_iter()
        .map(|sample| (sample, asset_server.load(sample.asset_path())))
        .collect();
    commands.insert_resource(AudioAssets { notes, effects });
}

fn play_queued_cues(
    mut commands: Commands,
    preferences: Res<GamePreferences>,
    assets: Option<Res<AudioAssets>>,
    gate: Res<AudioInteractionGate>,
    mut cues: ResMut<AudioCueQueue>,
) {
    let Some(assets) = assets else {
        cues.pending.clear();
        return;
    };
    for cue in drain_playable_cues(&mut cues, &gate) {
        let level = match cue.channel() {
            AudioChannel::Game => preferences.game_sound_effects,
            AudioChannel::Ui => preferences.ui_sound_effects,
        };
        let channel_gain = level.gain();
        if channel_gain == 0.0 {
            continue;
        }
        let definition = cue.definition();
        match definition.source {
            CueSource::Marimba(notes) => {
                for note in notes {
                    let Some(handle) = assets.notes.get(note) else {
                        error!("Missing cached marimba handle for {}", note.asset_path());
                        continue;
                    };
                    spawn_audio(
                        &mut commands,
                        handle,
                        channel_gain * definition.per_note_gain,
                    );
                }
            }
            CueSource::Effect(sample) => {
                let Some(handle) = assets.effects.get(&sample) else {
                    error!("Missing cached audio handle for {}", sample.asset_path());
                    continue;
                };
                spawn_audio(
                    &mut commands,
                    handle,
                    channel_gain * definition.per_note_gain,
                );
            }
        }
    }
}

fn spawn_audio(commands: &mut Commands, handle: &Handle<AudioSource>, gain: f32) {
    commands.spawn((
        AudioPlayer::new(handle.clone()),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
        PlayingAudio,
    ));
}

fn drain_playable_cues(cues: &mut AudioCueQueue, gate: &AudioInteractionGate) -> Vec<AudioCue> {
    if !gate.unlocked {
        cues.pending.clear();
        return Vec::new();
    }
    std::mem::take(&mut cues.pending)
}

fn cleanup_failed_audio(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    players: Query<(Entity, &AudioPlayer), With<PlayingAudio>>,
) {
    for (entity, player) in &players {
        if matches!(asset_server.load_state(player.0.id()), LoadState::Failed(_)) {
            commands.entity(entity).despawn();
        }
    }
}

fn report_audio_asset_failures(
    asset_server: Res<AssetServer>,
    assets: Option<Res<AudioAssets>>,
    mut failures: ResMut<AudioAssetFailures>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (note, handle) in &assets.notes {
        if matches!(asset_server.load_state(handle.id()), LoadState::Failed(_))
            && failures.notes.insert(*note)
        {
            error!(
                "Could not load generated marimba asset {}",
                note.asset_path()
            );
        }
    }
    for (sample, handle) in &assets.effects {
        if matches!(asset_server.load_state(handle.id()), LoadState::Failed(_))
            && failures.effects.insert(*sample)
        {
            error!(
                "Could not load generated audio asset {}",
                sample.asset_path()
            );
        }
    }
}

#[cfg(test)]
const fn channel_level(preferences: &GamePreferences, channel: AudioChannel) -> VolumeLevel {
    match channel {
        AudioChannel::Game => preferences.game_sound_effects,
        AudioChannel::Ui => preferences.ui_sound_effects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_map_uses_expected_harmonies_and_gray_third() {
        assert_eq!(BLACK_CAPTURE_NOTES, [MarimbaNote::C4]);
        assert_eq!(WHITE_CAPTURE_NOTES, [MarimbaNote::G4]);
        assert_eq!(BOTH_CAPTURE_NOTES, [MarimbaNote::C4, MarimbaNote::G4]);
        assert_eq!(
            GRAY_CHORD_NOTES,
            [MarimbaNote::C4, MarimbaNote::E4, MarimbaNote::G4]
        );
        assert_eq!(SAME_POINT_GRAY_NOTES, [GRAY_CHORD_NOTES[2]]);
        let capture_ratio = WHITE_CAPTURE_NOTES[0].frequency() / BLACK_CAPTURE_NOTES[0].frequency();
        let teleport_ratio =
            WHITE_TELEPORT_NOTES[0].frequency() / BLACK_TELEPORT_NOTES[0].frequency();
        assert!((capture_ratio - 1.5).abs() < 0.002);
        assert!((teleport_ratio - 1.5).abs() < 0.002);
    }

    #[test]
    fn countdown_numbers_share_one_note_and_go_keeps_its_chord() {
        let countdown_definitions = [
            GameCue::Countdown3,
            GameCue::Countdown2,
            GameCue::Countdown1,
        ]
        .map(|cue| AudioCue::Game(cue).definition());

        for definition in countdown_definitions {
            let CueSource::Marimba(notes) = definition.source else {
                panic!("countdown cue must use a marimba note");
            };
            assert_eq!(notes, &COUNTDOWN_NOTES);
            assert_eq!(notes, &[MarimbaNote::C5]);
        }
        let CueSource::Marimba(notes) = AudioCue::Game(GameCue::Go).definition().source else {
            panic!("Go cue must use marimba notes");
        };
        assert_eq!(notes, &[MarimbaNote::C5, MarimbaNote::E5, MarimbaNote::G5]);
        assert!(matches!(
            AudioCue::Game(GameCue::OpeningDuplicate)
                .definition()
                .source,
            CueSource::Effect(EffectSample::OpeningClack)
        ));
    }

    #[test]
    fn every_cue_maps_to_at_least_one_cached_note() {
        let game = [
            GameCue::Countdown3,
            GameCue::Countdown2,
            GameCue::Countdown1,
            GameCue::Go,
            GameCue::BlackCapture,
            GameCue::WhiteCapture,
            GameCue::BothCapture,
            GameCue::GrayCapture,
            GameCue::SamePointGray,
            GameCue::OpeningDuplicate,
            GameCue::BlackTeleport,
            GameCue::WhiteTeleport,
            GameCue::MatchEnd,
            GameCue::MatchRestart,
            GameCue::Preview,
        ];
        let ui = [
            UiCue::Confirm,
            UiCue::Cancel,
            UiCue::Destructive,
            UiCue::Neutral,
            UiCue::Hover,
            UiCue::Preview,
            UiCue::Marimba(MarimbaNote::A3),
            UiCue::Marimba(MarimbaNote::G5),
        ];
        for cue in game
            .into_iter()
            .map(AudioCue::Game)
            .chain(ui.into_iter().map(AudioCue::Ui))
        {
            let definition = cue.definition();
            match definition.source {
                CueSource::Marimba(notes) => {
                    assert!(!notes.is_empty());
                    assert!(notes.iter().all(|note| MarimbaNote::ALL.contains(note)));
                }
                CueSource::Effect(sample) => assert!(EffectSample::ALL.contains(&sample)),
            }
        }
    }

    #[test]
    fn round_sound_maps_black_white_harmony_and_gray_priority() {
        assert_eq!(
            GameCue::from_round_sound(RoundSound {
                black_capture: true,
                ..default()
            }),
            Some(GameCue::BlackCapture)
        );
        assert_eq!(
            GameCue::from_round_sound(RoundSound {
                white_capture: true,
                ..default()
            }),
            Some(GameCue::WhiteCapture)
        );
        assert_eq!(
            GameCue::from_round_sound(RoundSound {
                black_capture: true,
                white_capture: true,
                ..default()
            }),
            Some(GameCue::BothCapture)
        );
        assert_eq!(
            GameCue::from_round_sound(RoundSound {
                black_capture: true,
                white_capture: true,
                gray_capture: true,
                ..default()
            }),
            Some(GameCue::GrayCapture)
        );
        assert_eq!(GameCue::from_round_sound(RoundSound::default()), None);
    }

    #[test]
    fn the_hover_cue_sounds_once_per_entry_and_never_for_a_muted_button() {
        assert_eq!(hover_cue(false, true, true, true), Some(UiCue::Hover));
        assert_eq!(
            hover_cue(true, true, true, true),
            None,
            "staying is no entry"
        );
        assert_eq!(
            hover_cue(false, true, true, false),
            None,
            "a button that arrives under a still pointer stays silent"
        );
        assert_eq!(hover_cue(false, true, false, true), None, "muted");
        assert_eq!(
            hover_cue(true, false, true, true),
            None,
            "leaving is silent"
        );
    }

    /// An app with the activation and hover cue paths, and one button.
    fn cue_app(sound: UiSound) -> (App, Entity) {
        let mut app = App::new();
        app.add_message::<CursorMoved>()
            .insert_resource(AudioCueQueue::default())
            .add_observer(queue_activation_cue)
            .add_systems(Update, emit_hover_cues);
        let button = app.world_mut().spawn((Hovered(false), sound)).id();
        app.update();
        (app, button)
    }

    fn drain(app: &mut App) -> Vec<AudioCue> {
        app.world_mut().resource_mut::<AudioCueQueue>().drain()
    }

    #[test]
    fn an_activation_queues_its_buttons_cue_once_and_a_muted_button_none() {
        let (mut app, button) = cue_app(UiSound::new(UiCue::Confirm));
        app.world_mut().trigger(Activate { entity: button });
        app.update();
        assert_eq!(drain(&mut app), vec![AudioCue::Ui(UiCue::Confirm)]);
        app.update();
        assert!(drain(&mut app).is_empty(), "one activation, one cue");

        app.world_mut()
            .get_mut::<UiSound>(button)
            .unwrap()
            .set_enabled(false);
        app.world_mut().trigger(Activate { entity: button });
        app.update();
        assert!(drain(&mut app).is_empty(), "a muted button is silent");

        let (mut app, stop) = cue_app(UiSound::preview_only());
        app.world_mut().trigger(Activate { entity: stop });
        app.update();
        assert!(
            drain(&mut app).is_empty(),
            "a volume stop's click is its preview, which its handler plays"
        );
    }

    #[test]
    fn the_pointer_coming_onto_a_button_queues_the_hover_cue_once() {
        let (mut app, button) = cue_app(UiSound::new(UiCue::Confirm));
        let moved = |app: &mut App| {
            app.world_mut().write_message(CursorMoved {
                window: Entity::PLACEHOLDER,
                position: Vec2::ZERO,
                delta: None,
            });
        };

        // Hovered with no motion - a layout change put the button under a
        // still pointer - stays silent.
        app.world_mut().entity_mut(button).insert(Hovered(true));
        app.update();
        assert!(drain(&mut app).is_empty());

        app.world_mut().entity_mut(button).insert(Hovered(false));
        let other = app
            .world_mut()
            .spawn((Hovered(false), UiSound::new(UiCue::Confirm)))
            .id();
        app.update();
        moved(&mut app);
        moved(&mut app);
        app.world_mut().entity_mut(button).insert(Hovered(true));
        app.update();
        assert_eq!(drain(&mut app), vec![AudioCue::Ui(UiCue::Hover)]);

        // Both moves were drained in that frame, so the next one, which had
        // none, is not mistaken for a frame with motion: the second button
        // arriving under the still pointer stays silent.
        app.world_mut().entity_mut(other).insert(Hovered(true));
        app.update();
        assert!(drain(&mut app).is_empty());
    }

    #[test]
    fn volume_levels_are_separate_and_have_safe_gains() {
        assert_eq!(VolumeLevel::default(), VolumeLevel::Medium);
        assert_eq!(VolumeLevel::Mute.gain(), 0.0);
        assert_eq!(VolumeLevel::Low.gain(), 0.25);
        assert_eq!(VolumeLevel::Medium.gain(), 0.55);
        assert_eq!(VolumeLevel::High.gain(), 0.9);
        assert!(GRAY_CHORD_NOTES.len() as f32 * 0.34 * VolumeLevel::High.gain() <= 1.0);

        let preferences = GamePreferences {
            game_sound_effects: VolumeLevel::High,
            ui_sound_effects: VolumeLevel::Low,
            ..default()
        };
        assert_eq!(
            channel_level(&preferences, AudioChannel::Game),
            VolumeLevel::High
        );
        assert_eq!(
            channel_level(&preferences, AudioChannel::Ui),
            VolumeLevel::Low
        );
        assert_eq!(
            volume_preview(AudioChannel::Game, VolumeLevel::High),
            Some(AudioCue::Game(GameCue::Preview))
        );
        assert_eq!(
            volume_preview(AudioChannel::Ui, VolumeLevel::Low),
            Some(AudioCue::Ui(UiCue::Preview))
        );
        assert_eq!(volume_preview(AudioChannel::Ui, VolumeLevel::Mute), None);
        assert_eq!(
            AudioCue::Ui(UiCue::Marimba(MarimbaNote::C5)).channel(),
            AudioChannel::Ui
        );
        assert_eq!(
            AudioCue::Ui(UiCue::Marimba(MarimbaNote::C5))
                .definition()
                .source
                .marimba_notes(),
            Some(&[MarimbaNote::C5][..])
        );
    }

    #[test]
    fn suspended_browser_audio_discards_pre_gesture_cues_without_a_burst() {
        let mut cues = AudioCueQueue::default();
        cues.game(GameCue::Countdown3);
        cues.ui(UiCue::Hover);
        let mut gate = AudioInteractionGate { unlocked: false };

        assert!(drain_playable_cues(&mut cues, &gate).is_empty());
        assert!(cues.pending.is_empty());

        gate.unlocked = true;
        cues.game(GameCue::Go);
        assert_eq!(
            drain_playable_cues(&mut cues, &gate),
            vec![AudioCue::Game(GameCue::Go)]
        );
    }

    #[test]
    fn generated_wavs_are_mono_pcm_click_free_and_unclipped() {
        for note in MarimbaNote::ALL {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(note.asset_path());
            assert_wav_integrity(&path, 0.46);
        }
        let clack = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(EffectSample::OpeningClack.asset_path());
        assert_wav_integrity(&clack, 0.18);
    }

    fn assert_wav_integrity(path: &std::path::Path, duration_seconds: f32) {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1);
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(data_len, bytes.len() - 44);
        let samples: Vec<i16> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|sample| i16::from_le_bytes(*sample))
            .collect();
        assert_eq!(samples.len(), (44_100.0 * duration_seconds) as usize);
        assert!(
            samples
                .iter()
                .all(|sample| *sample != i16::MIN && *sample != i16::MAX)
        );
        assert!(samples[0].abs() < 64);
        assert!(samples.last().unwrap().abs() < 64);
    }
}
