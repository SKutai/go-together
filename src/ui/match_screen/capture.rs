//! The capture indication: what the last resolution took off the board,
//! shown where it stood.
//!
//! # Responsibility
//!
//! `update_match_display` paints every intersection from the current cell,
//! so a round that captures a group simply stops drawing it, and nothing
//! says what was taken. This module draws that from the authority's own
//! record of it, `MatchProgress::last_removals`, which every resolution
//! writes: it reads the record, never the board's history, so the fact of
//! a capture exists outside the renderer and a future screen reader can
//! announce the same record this module draws.
//!
//! # The static form is the requirement
//!
//! Under the motion policy in `app::motion`, the information has to survive
//! with motion off, so the static form is designed first and the animation
//! is built on top of it. The indication has two parts:
//!
//! * Beside the board, under each player's side, a **count** of the stones
//!   that player was credited: `Captured 3`. The rules credit every removal
//!   to the removed stone's opponent, so the line sits under the captor.
//!   It is shown whether motion is allowed or reduced: a word is the
//!   second channel `CONTRIBUTING.md` asks for, it is what a screen reader
//!   will read first, and it is the one part of the indication that does
//!   not depend on how a player feels about movement.
//! * On every point the resolution emptied, a **ghost** of the stone that
//!   stood there: the outline of the stone, in its own colour, with an X
//!   through it. The outline is drawn as the stone was - white carries the
//!   grid's ink ring outside its stroke, black does not, and each sits at
//!   its stone's footprint from `stone_geometry` - so a ghost reads as
//!   that stone, hollow. The X is what no other mark on either board has:
//!   the Replay Viewer's hidden-selection mark is also a ring around a
//!   bare middle, and without the X a ghost could be read as one; so the
//!   ghost is all or nothing - ink, stroke and cross together - never the
//!   ring alone. The empty-intersection dot stays visible in the middle,
//!   saying the point is empty now. Black and white ghosts are told apart
//!   by 17.8:1 of luminance, not hue, and every stroke is a stone colour
//!   or the grid ink already held to the wood in `contrast.rs`.
//!
//!   **The ghosts are the reduced-motion form only.** With motion reduced
//!   there is no animation, so they are the only thing that can carry
//!   which stones went, and they stay until the next resolution replaces
//!   the record. With motion allowed, the animation is that channel, and
//!   the board is left clean once it has finished: the owner, having seen
//!   both on screen, did not want marks lingering through the round that
//!   follows. That is a decision with a cost, taken on 2026-10-04 and
//!   recorded in `docs/ARCHITECTURE.md`: a player who allows motion and
//!   looks away for the half second a capture takes loses it, where a
//!   player with motion reduced would still find the marks. The count is
//!   what both players keep. Rule 2 of the policy holds as stated - the
//!   static form is the requirement whenever it is the only channel - and
//!   the ghost is still what a capture *is* on screen when nothing moves.
//!
//! Whatever is shown stays until the next resolution replaces the record,
//! so it says "what the last round took" for exactly as long as that is
//! true, and it shows through the end of a Match that a capture ended -
//! Point Target, Round Limit, a Match Time deadline - since that capture
//! is the one a player most needs to see. Nothing is shown after a Manual
//! end: a completed end hold rolls play back to its snapshot, and the
//! record then describes a round that stands, not one that just happened.
//!
//! # The animation
//!
//! When motion is allowed, each removed stone is covered by a disc that
//! looks like the stone did, and the disc plays the owner's two steps:
//!
//! 1. it fades from its own colour to its captor's, through
//!    [`captured_stone_fill`] in `presentation`;
//! 2. it expands a little, then shrinks and fades out, keeping the colour
//!    it faded into, and it is fully transparent before the shrink would
//!    have reached nothing - the fade-out takes [`VANISH_SECONDS`] of the
//!    [`SHRINK_SECONDS`] shrink, so the disc is gone at about a quarter of
//!    its size rather than collapsing to a point. The board is clean
//!    behind it.
//!
//! Every stone's disc carries the grid-ink ring inside its footprint
//! throughout, as a white stone does. That is what keeps its edge on the
//! wood: the fade passes through the wood's own luminance on the way from
//! black to white, and a bare disc would vanish into the board for a few
//! frames. The same crossing passes Gray's tone, and for those frames a
//! fading stone resembles a gray stone. The fade is eased so it crosses
//! the middle at its quickest, which still leaves about half of
//! [`FADE_SECONDS`] - under a tenth of a second - within 3:1 of Gray, and
//! then step two follows at once, which no gray stone ever does. That
//! exposure is accepted and measured by a test here rather than designed
//! away: the two ways to shorten it, starting the expansion during the
//! fade or fading the disc out as it recolours, would both merge the
//! owner's two steps into one.
//!
//! **Order.** Each stone's disc starts a little after the one before it,
//! and the order is the distance, in steps across the group, from the
//! stone that took it: the stones touching the placed or cleanup stone
//! that removed the group go first, and the wave runs away from it. That
//! is the obvious order - an onion peel from the group's boundary - made
//! specific: on a 9x9 board most captured groups are a stone or a short
//! line, every stone of which is on the boundary, so a peel would show no
//! order at all; a wave from the taking stone shows one for every group of
//! two or more, and it literally shows what took them.
//! The Round 0 reveal sweep has no taking stone, so there a group peels
//! from its boundary inward, stones with a neighbour outside the group
//! first. Groups peel independently and at once: each is its own event on
//! the board, and a small group does not wait for a large one.
//!
//! **Time.** Two seconds was the owner's ceiling and is treated as far too
//! long: a single stone takes under four tenths of a second, a group of
//! three in a line six tenths, and the deepest group a board can hold is
//! held under [`LONGEST_SECONDS`] by shortening the stagger. Those are the
//! lengths a player sees: the last slice of each shrink is invisible, so
//! the timeline ends where the disc vanishes, not where its scale would
//! have reached zero. The clock is Bevy's
//! `Time`, as the Home marimba's is - never the Match clock and never
//! `playing_delta`, which the hold gestures own - and the animation
//! reads the Match and writes nothing of it: it cannot pause, gate or
//! delay anything. It runs on under a modal, which covers it.
//!
//! **Giving way.** The marks are keyed by the round the record names. The
//! moment a different record is shown - the next round, a restart, the
//! reveal of a new Match - every mark is restyled for the new record and
//! every disc restarts or disappears, in the same frame the state changed.
//! A point that is no longer empty gets no ghost while play continues, and
//! a mark whose animation has finished, with motion allowed, is hidden the
//! frame it finishes.
//!
//! # Hidden information
//!
//! A ghost can mark a point no displayed board ever showed a stone on: a
//! stone placed and removed within one round, or swept at the Round 0
//! reveal. That is allowed because a *resolved* selection is not secret -
//! the rules hide only pending ones, and every Replay records them - and
//! it is the only way a player learns that such a stone existed. See
//! `view::PublicMatchView::last_removals`.
//!
//! # Entities
//!
//! Every mark is spawned once, hidden, when the screen is entered - one
//! root per intersection with its ghost and disc parts as children - and
//! restyled from state, like the Replay Viewer's marks: a mark spawned on
//! demand would appear a frame after the stone had already gone. Nothing
//! here writes a component whose value has not changed, so a frame with
//! no capture, and a frame after an animation has ended, writes nothing.

use std::collections::{HashMap, VecDeque};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;

use super::layout::{CONTROL_HINT_SIDE_OFFSET, CONTROL_HINT_WIDTH};
use crate::app::AppScreen;
use crate::app::motion::Motion;
use crate::app::visuals::{
    STONE_RADIUS, STONE_RING_WIDTH, StoneMesh, StonePalette, board_position, stone_geometry,
};
use crate::gameplay::{GameState, MatchPhase, RoundRemovals};
use crate::presentation::{BLACK_STONE, TEXT_PRIMARY, WHITE_STONE, captured_stone_fill};
use crate::replay::EndReason;
use crate::rules::{Cell, Player, Point, RemovedStone};

/// Z of a mark's root: above the stones at 2 and below the cursors at 8.
const CAPTURE_MARK_Z: f32 = 3.0;
/// The ghost's stroke, twice the grid's ink so it reads as an outline and
/// not as a lattice line; the white ghost's ink ring outside it is the
/// stone's own, [`STONE_RING_WIDTH`].
const GHOST_STROKE_WIDTH: f32 = 2.0 * STONE_RING_WIDTH;
/// The X through the ghost, as heavy as its stroke.
const CROSS_BAR_WIDTH: f32 = 2.0 * STONE_RING_WIDTH;
const ANNULUS_RESOLUTION: u32 = 64;
/// Top of the per-side count line: below the Home and Help buttons, above
/// the Round 0 dot rows, which are hidden whenever a count can show.
const CAPTURE_COUNT_TOP: f32 = 140.0;
const CAPTURE_COUNT_FONT_PX: f32 = 15.0;

/// How long one stone's fade to its captor's colour takes.
const FADE_SECONDS: f32 = 0.15;
/// How much later each step of the wave starts than the one before it.
const STAGGER_SECONDS: f32 = 0.06;
const EXPAND_SECONDS: f32 = 0.08;
const EXPAND_SCALE: f32 = 1.15;
/// How long the disc would take to shrink from [`EXPAND_SCALE`] to nothing.
const SHRINK_SECONDS: f32 = 0.20;
/// How long into the shrink the disc is fully transparent - earlier than
/// the shrink ends, so the stone fades out while still visibly a disc
/// rather than collapsing to a point. The timeline ends here; the rest of
/// the shrink is never drawn.
const VANISH_SECONDS: f32 = 0.14;
const _: () = assert!(
    VANISH_SECONDS < SHRINK_SECONDS,
    "the disc fades out before it shrinks away"
);
/// The longest any capture is allowed to take, with the owner's ceiling of
/// two seconds above it; a deeper group shortens its stagger to fit.
const LONGEST_SECONDS: f32 = 1.5;
const _: () = assert!(LONGEST_SECONDS < 2.0, "the owner's ceiling is two seconds");

/// The pre-spawned mark for one intersection; the ghost and disc parts
/// are its children.
#[derive(Component)]
struct CaptureMark(Point);

/// One part of a mark, drawn relative to its root.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureMarkPart {
    /// The grid-ink ring outside a white ghost's stroke.
    GhostInk,
    /// The ghost's outline in the stone's colour.
    GhostStroke,
    /// One bar of the X through the ghost.
    GhostCross,
    /// The animated disc's ink ring, inside its footprint.
    DiscRing,
    /// The animated disc's fill, the part that fades.
    DiscFill,
}

/// The count line under one player's side of the board.
#[derive(Component)]
struct CaptureCount(Player);

/// What is shown and how far its animation has run.
#[derive(Resource, Default)]
pub(crate) struct CaptureIndication {
    /// The round whose record is shown, or `None` while nothing is.
    shown: Option<usize>,
    /// The shown record, with each stone's place in the wave.
    plan: Vec<PlannedStone>,
    /// Seconds since the discs started.
    elapsed: f32,
    /// Whether the discs are still being drawn.
    animating: bool,
}

/// One removed stone with its place in its group's wave.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlannedStone {
    stone: RemovedStone,
    /// Steps from the stones that start the wave.
    depth: u8,
    /// The deepest step in this stone's group.
    group_depth: u8,
}

/// When a group's wave happens, for a group whose deepest stone is
/// `group_depth` steps in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Timeline {
    stagger: f32,
    /// When the group's second step begins: after its last stone has faded.
    second_step: f32,
    total: f32,
}

/// How one disc is drawn at an instant of its animation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DiscFrame {
    fade: f32,
    scale: f32,
    alpha: f32,
}

fn timeline(group_depth: u8) -> Timeline {
    let steps = f32::from(group_depth);
    let fixed = FADE_SECONDS + EXPAND_SECONDS + VANISH_SECONDS;
    let stagger = if steps > 0.0 {
        STAGGER_SECONDS.min((LONGEST_SECONDS - fixed) / (2.0 * steps))
    } else {
        0.0
    };
    let second_step = FADE_SECONDS + steps * stagger;
    Timeline {
        stagger,
        second_step,
        total: second_step + steps * stagger + EXPAND_SECONDS + VANISH_SECONDS,
    }
}

/// Smoothstep: slow at both ends, quickest through the middle - which is
/// what carries the fade through the tones it should not linger on.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The disc of a stone `depth` steps into its group's wave, `elapsed`
/// seconds in, or `None` once it has faded away.
fn disc_frame(elapsed: f32, depth: u8, timeline: Timeline) -> Option<DiscFrame> {
    let start = f32::from(depth) * timeline.stagger;
    let fade = ease((elapsed - start) / FADE_SECONDS);
    let second_step = timeline.second_step + start;
    if elapsed < second_step {
        return Some(DiscFrame {
            fade,
            scale: 1.0,
            alpha: 1.0,
        });
    }
    let into = elapsed - second_step;
    if into < EXPAND_SECONDS {
        return Some(DiscFrame {
            fade: 1.0,
            scale: 1.0 + (EXPAND_SCALE - 1.0) * ease(into / EXPAND_SECONDS),
            alpha: 1.0,
        });
    }
    let into = into - EXPAND_SECONDS;
    if into < VANISH_SECONDS {
        return Some(DiscFrame {
            fade: 1.0,
            scale: EXPAND_SCALE * (1.0 - ease(into / SHRINK_SECONDS)),
            alpha: 1.0 - ease(into / VANISH_SECONDS),
        });
    }
    None
}

/// Divides the record into groups - connected stones of one owner - and
/// gives every stone its step in its group's wave. See the module docs
/// for the order and why.
fn plan(stones: &[RemovedStone]) -> Vec<PlannedStone> {
    let position = |point: Point, owner: Player| {
        stones
            .iter()
            .position(|stone| stone.point == point && stone.owner == owner)
    };
    let mut planned = Vec::with_capacity(stones.len());
    let mut grouped = vec![false; stones.len()];
    for first in 0..stones.len() {
        if grouped[first] {
            continue;
        }
        let owner = stones[first].owner;
        let mut group = vec![first];
        grouped[first] = true;
        let mut next = 0;
        while next < group.len() {
            let point = stones[group[next]].point;
            next += 1;
            for neighbor in point.neighbors() {
                if let Some(index) = position(neighbor, owner)
                    && !grouped[index]
                {
                    grouped[index] = true;
                    group.push(index);
                }
            }
        }

        let in_group = |point: Point| group.iter().any(|&index| stones[index].point == point);
        // Where the wave starts. A self-removed group holds the stone that
        // took it - the placement that left it without liberties - and the
        // wave starts there alone; a captured group lies beside its taker,
        // and the wave starts from the stones that touch it. The reveal
        // sweep has no taker, so a swept group starts from its boundary.
        let is_its_own_taker = |index: usize| stones[index].by == Some(stones[index].point);
        let touches_its_taker = |index: usize| {
            let stone = stones[index];
            stone
                .by
                .is_some_and(|by| stone.point.neighbors().any(|neighbor| neighbor == by))
        };
        let on_the_boundary = |index: usize| stones[index].point.neighbors().any(|n| !in_group(n));
        let seeded = |pick: &dyn Fn(usize) -> bool| -> Vec<usize> {
            group.iter().copied().filter(|&i| pick(i)).collect()
        };
        let mut seeds = seeded(&is_its_own_taker);
        if seeds.is_empty() {
            seeds = seeded(&touches_its_taker);
        }
        if seeds.is_empty() {
            seeds = seeded(&on_the_boundary);
        }
        if seeds.is_empty() {
            seeds = group.clone();
        }

        let mut depth: Vec<Option<u8>> = vec![None; stones.len()];
        let mut queue = VecDeque::new();
        for seed in seeds {
            depth[seed] = Some(0);
            queue.push_back(seed);
        }
        while let Some(index) = queue.pop_front() {
            let here = depth[index].unwrap_or(0);
            for neighbor in stones[index].point.neighbors() {
                if let Some(other) = position(neighbor, owner)
                    && group.contains(&other)
                    && depth[other].is_none()
                {
                    depth[other] = Some(here.saturating_add(1));
                    queue.push_back(other);
                }
            }
        }
        let group_depth = group
            .iter()
            .filter_map(|&index| depth[index])
            .max()
            .unwrap_or(0);
        for index in group {
            planned.push(PlannedStone {
                stone: stones[index],
                depth: depth[index].unwrap_or(0),
                group_depth,
            });
        }
    }
    planned.sort_by_key(|planned| planned.stone.point.index());
    planned
}

/// The record the screen shows right now, or `None`: the last resolution's
/// during play, and after the Match ends only if a resolution ended it.
fn shown_record(game: &GameState) -> Option<&RoundRemovals> {
    match game.phase {
        MatchPhase::Playing => Some(&game.last_removals),
        MatchPhase::Ended
            if game
                .end_reasons
                .iter()
                .any(|reason| *reason != EndReason::Manual) =>
        {
            Some(&game.last_removals)
        }
        MatchPhase::Opening | MatchPhase::OpeningCountdown | MatchPhase::Ended => None,
    }
}

/// How many of `stones` were credited to `player`: every one the opponent
/// lost.
fn credited_to(stones: &[RemovedStone], player: Player) -> usize {
    stones
        .iter()
        .filter(|stone| stone.owner == player.opponent())
        .count()
}

fn count_text(stones: &[RemovedStone], player: Player) -> String {
    match credited_to(stones, player) {
        0 => String::new(),
        count => format!("Captured {count}"),
    }
}

const fn stone_colour(owner: Player) -> Color {
    match owner {
        Player::One => BLACK_STONE,
        Player::Two => WHITE_STONE,
    }
}

/// Length of each bar of the X: the diameter inside the ghost's stroke.
fn cross_length(owner: Player) -> f32 {
    2.0 * (STONE_RADIUS * stone_geometry(owner.cell()).fill_scale - GHOST_STROKE_WIDTH)
}

/// Spawns one hidden mark per intersection and the two count lines, and
/// forgets whatever an earlier visit to the screen was showing. Runs after
/// `spawn_match_screen` on entry.
pub(super) fn spawn_capture_marks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    palette: Res<StonePalette>,
    stone_mesh: Res<StoneMesh>,
    mut indication: ResMut<CaptureIndication>,
) {
    *indication = CaptureIndication::default();

    // Both bands are made for a black stone's footprint; a white ghost's
    // stroke is scaled in by its fill, exactly as its stone's fill is.
    let ink = meshes.add(
        Annulus::new(STONE_RADIUS - STONE_RING_WIDTH, STONE_RADIUS)
            .mesh()
            .resolution(ANNULUS_RESOLUTION),
    );
    let stroke = meshes.add(
        Annulus::new(STONE_RADIUS - GHOST_STROKE_WIDTH, STONE_RADIUS)
            .mesh()
            .resolution(ANNULUS_RESOLUTION),
    );
    let ring = STONE_RING_WIDTH / STONE_RADIUS;

    for y in 0..crate::rules::BOARD_SIZE {
        for x in 0..crate::rules::BOARD_SIZE {
            let point = Point::new(x, y);
            let position = board_position(point);
            // Its own material, since its colour is its own as it fades;
            // blended, or the alpha it fades to would be ignored.
            let fill = materials.add(ColorMaterial {
                color: BLACK_STONE,
                alpha_mode: AlphaMode2d::Blend,
                ..default()
            });
            commands
                .spawn((
                    Transform::from_xyz(position.x, position.y, CAPTURE_MARK_Z),
                    Visibility::Hidden,
                    CaptureMark(point),
                    DespawnOnExit(AppScreen::Match),
                ))
                .with_children(|mark| {
                    mark.spawn((
                        Mesh2d(ink.clone()),
                        MeshMaterial2d(palette.ring.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.0),
                        Visibility::Hidden,
                        CaptureMarkPart::GhostInk,
                    ));
                    mark.spawn((
                        Mesh2d(stroke.clone()),
                        MeshMaterial2d(palette.black.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.1),
                        Visibility::Inherited,
                        CaptureMarkPart::GhostStroke,
                    ));
                    for turn in [-1.0, 1.0] {
                        mark.spawn((
                            Sprite::from_color(
                                BLACK_STONE,
                                Vec2::new(cross_length(Player::One), CROSS_BAR_WIDTH),
                            ),
                            Transform::from_xyz(0.0, 0.0, 0.2).with_rotation(
                                Quat::from_rotation_z(turn * std::f32::consts::FRAC_PI_4),
                            ),
                            Visibility::Inherited,
                            CaptureMarkPart::GhostCross,
                        ));
                    }
                    mark.spawn((
                        Mesh2d(stone_mesh.0.clone()),
                        MeshMaterial2d(palette.ring.clone()),
                        Transform::from_xyz(0.0, 0.0, 0.3),
                        Visibility::Hidden,
                        CaptureMarkPart::DiscRing,
                    ));
                    mark.spawn((
                        Mesh2d(stone_mesh.0.clone()),
                        MeshMaterial2d(fill),
                        Transform::from_xyz(0.0, 0.0, 0.4).with_scale(Vec3::splat(1.0 - ring)),
                        Visibility::Hidden,
                        CaptureMarkPart::DiscFill,
                    ));
                });
        }
    }

    for player in Player::ALL {
        let mut node = Node {
            position_type: PositionType::Absolute,
            top: px(CAPTURE_COUNT_TOP),
            width: px(CONTROL_HINT_WIDTH),
            justify_content: JustifyContent::Center,
            ..default()
        };
        match player {
            Player::One => node.right = px(CONTROL_HINT_SIDE_OFFSET),
            Player::Two => node.left = px(CONTROL_HINT_SIDE_OFFSET),
        }
        commands.spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(CAPTURE_COUNT_FONT_PX),
                ..default()
            },
            TextColor(TEXT_PRIMARY),
            TextLayout::justify(Justify::Center),
            node,
            CaptureCount(player),
            DespawnOnExit(AppScreen::Match),
        ));
    }
}

type MarkRoots<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static CaptureMark, &'static mut Visibility),
    (Without<CaptureMarkPart>, Without<CaptureCount>),
>;

type MarkParts<'w, 's> = Query<
    'w,
    's,
    (
        &'static ChildOf,
        &'static CaptureMarkPart,
        &'static mut Visibility,
        &'static mut Transform,
        Option<&'static mut MeshMaterial2d<ColorMaterial>>,
        Option<&'static mut Sprite>,
    ),
    (Without<CaptureMark>, Without<CaptureCount>),
>;

type CountLines<'w, 's> = Query<
    'w,
    's,
    (&'static CaptureCount, &'static mut Text),
    (Without<CaptureMark>, Without<CaptureMarkPart>),
>;

/// The mark roots, their parts and the count lines: three queries, each
/// over entities carrying its own marker and `Without` the other two, so
/// their borrows are provably disjoint.
#[derive(SystemParam)]
pub(super) struct CaptureEntities<'w, 's> {
    roots: MarkRoots<'w, 's>,
    parts: MarkParts<'w, 's>,
    counts: CountLines<'w, 's>,
}

fn set_visibility(visibility: &mut Mut<Visibility>, target: Visibility) {
    if **visibility != target {
        **visibility = target;
    }
}

fn set_scale(transform: &mut Mut<Transform>, scale: f32) {
    let target = Vec3::splat(scale);
    if transform.scale != target {
        transform.scale = target;
    }
}

fn set_material(material: &mut Mut<MeshMaterial2d<ColorMaterial>>, handle: &Handle<ColorMaterial>) {
    if material.0 != *handle {
        material.0 = handle.clone();
    }
}

/// Draws the shown record and advances its animation. Runs in
/// `AppSystems::MatchDisplay`, so it sees the round that resolved this
/// frame; it is not ordered against `update_match_display`, with which it
/// shares no entity.
pub(super) fn update_capture_indication(
    game: Res<GameState>,
    motion: Motion,
    time: Res<Time>,
    palette: Res<StonePalette>,
    mut indication: ResMut<CaptureIndication>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut entities: CaptureEntities,
) {
    let record = shown_record(&game);
    let key = record.map(|record| record.round);
    let restyle = key != indication.shown;
    if restyle {
        indication.shown = key;
        indication.plan = record
            .map(|record| plan(&record.stones))
            .unwrap_or_default();
        indication.elapsed = 0.0;
        indication.animating = !indication.plan.is_empty();
    } else if indication.animating {
        // The frame a record first shows is drawn at zero: the stone as it
        // was. Every frame after that is drawn as far along as the clock
        // has come, which keeps the discs in step with it.
        indication.elapsed += time.delta_secs();
    }
    let reduced = motion.reduced();
    if indication.animating && reduced {
        indication.animating = false;
    }

    // A mark is drawn while its ghost is wanted (motion reduced) or its
    // disc is still animating (motion allowed); with motion allowed and the
    // animation over, the board is left clean.
    let ended = game.phase == MatchPhase::Ended;
    let animating = indication.animating;
    let mut planned_by_root: HashMap<Entity, PlannedStone> = HashMap::new();
    for (entity, mark, mut visibility) in &mut entities.roots {
        let planned = indication
            .plan
            .iter()
            .find(|planned| planned.stone.point == mark.0)
            .copied();
        let shown = planned.is_some()
            && (ended || game.board.get(mark.0) == Cell::Empty)
            && (reduced || animating);
        set_visibility(
            &mut visibility,
            if shown {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        );
        if let Some(planned) = planned {
            planned_by_root.insert(entity, planned);
        }
    }

    let ring = STONE_RING_WIDTH / STONE_RADIUS;
    let elapsed = indication.elapsed;
    let ghost = if reduced {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (child_of, part, mut visibility, mut transform, material, sprite) in &mut entities.parts {
        let Some(planned) = planned_by_root.get(&child_of.parent()) else {
            // A mark that left the plan is hidden by its root; its discs
            // are hidden in their own right too, so nothing stale is drawn
            // on the frame a later record brings the mark back.
            if restyle && matches!(part, CaptureMarkPart::DiscRing | CaptureMarkPart::DiscFill) {
                set_visibility(&mut visibility, Visibility::Hidden);
            }
            continue;
        };
        let owner = planned.stone.owner;
        let geometry = stone_geometry(owner.cell());
        match part {
            CaptureMarkPart::GhostInk => {
                set_visibility(
                    &mut visibility,
                    match geometry.ring_scale {
                        Some(_) => ghost,
                        None => Visibility::Hidden,
                    },
                );
                if restyle {
                    set_scale(&mut transform, geometry.ring_scale.unwrap_or(1.0));
                }
            }
            CaptureMarkPart::GhostStroke => {
                set_visibility(&mut visibility, ghost);
                if restyle {
                    if let Some(mut material) = material {
                        set_material(&mut material, palette.fill(owner.cell()));
                    }
                    set_scale(&mut transform, geometry.fill_scale);
                }
            }
            CaptureMarkPart::GhostCross => {
                set_visibility(&mut visibility, ghost);
                if restyle && let Some(mut sprite) = sprite {
                    let colour = stone_colour(owner);
                    if sprite.color != colour {
                        sprite.color = colour;
                    }
                    let size = Some(Vec2::new(cross_length(owner), CROSS_BAR_WIDTH));
                    if sprite.custom_size != size {
                        sprite.custom_size = size;
                    }
                }
            }
            CaptureMarkPart::DiscRing | CaptureMarkPart::DiscFill => {
                let frame = if animating {
                    disc_frame(elapsed, planned.depth, timeline(planned.group_depth))
                } else {
                    None
                };
                let Some(frame) = frame else {
                    set_visibility(&mut visibility, Visibility::Hidden);
                    continue;
                };
                set_visibility(&mut visibility, Visibility::Inherited);
                let base = if *part == CaptureMarkPart::DiscRing {
                    1.0
                } else {
                    1.0 - ring
                };
                set_scale(&mut transform, base * frame.scale);
                if *part == CaptureMarkPart::DiscFill
                    && let Some(material) = material
                {
                    let colour = captured_stone_fill(owner, frame.fade, frame.alpha);
                    if materials
                        .get(&material.0)
                        .is_some_and(|fill| fill.color != colour)
                        && let Some(mut fill) = materials.get_mut(&material.0)
                    {
                        fill.color = colour;
                    }
                }
            }
        }
    }

    for (count, mut text) in &mut entities.counts {
        let value = record
            .map(|record| count_text(&record.stones, count.0))
            .unwrap_or_default();
        if **text != value {
            **text = value;
        }
    }

    if indication.animating {
        let longest = indication
            .plan
            .iter()
            .map(|planned| timeline(planned.group_depth).total)
            .fold(0.0, f32::max);
        if indication.elapsed >= longest {
            indication.animating = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::motion::MotionPolicy;
    use crate::app::visuals::{StoneMesh, test_stone_palette};
    use crate::rules::{BOARD_SIZE, RemovedStone};
    use crate::settings::GamePreferences;
    use crate::test_support::advance_test_app;

    fn removed(x: usize, y: usize, owner: Player, by: Option<(usize, usize)>) -> RemovedStone {
        RemovedStone {
            point: Point::new(x, y),
            owner,
            by: by.map(|(x, y)| Point::new(x, y)),
        }
    }

    fn depths(stones: &[RemovedStone]) -> Vec<((usize, usize), u8, u8)> {
        plan(stones)
            .into_iter()
            .map(|planned| {
                (
                    (planned.stone.point.x, planned.stone.point.y),
                    planned.depth,
                    planned.group_depth,
                )
            })
            .collect()
    }

    #[test]
    fn a_wave_runs_from_the_stone_that_took_the_group() {
        // A line of three captured by a stone at its left end: the wave
        // runs along the line, one step per stone.
        let line: Vec<_> = (1..4)
            .map(|x| removed(x, 4, Player::One, Some((0, 4))))
            .collect();
        assert_eq!(
            depths(&line),
            vec![((1, 4), 0, 2), ((2, 4), 1, 2), ((3, 4), 2, 2)]
        );

        // A self-removal starts from the placed stone itself, inside the
        // group, and runs outward.
        let suicide = vec![
            removed(3, 0, Player::Two, Some((4, 0))),
            removed(4, 0, Player::Two, Some((4, 0))),
            removed(5, 0, Player::Two, Some((4, 0))),
        ];
        assert_eq!(
            depths(&suicide),
            vec![((3, 0), 1, 1), ((4, 0), 0, 1), ((5, 0), 1, 1)]
        );
    }

    #[test]
    fn the_reveal_sweep_peels_from_the_boundary_and_groups_peel_independently() {
        // A 3x3 block swept at the reveal: no taking stone, so the outer
        // eight go first and the middle last.
        let block: Vec<_> = (3..6)
            .flat_map(|y| (3..6).map(move |x| removed(x, y, Player::One, None)))
            .collect();
        let planned = depths(&block);
        for (point, depth, group_depth) in &planned {
            assert_eq!(*group_depth, 1);
            assert_eq!(*depth, u8::from(*point == (4, 4)), "{point:?}");
        }

        // Two groups of two colours in one round each start at once, and a
        // single stone beside a line does not wait for the line.
        let mixed = vec![
            removed(0, 0, Player::One, Some((0, 1))),
            removed(7, 7, Player::Two, Some((6, 7))),
            removed(8, 7, Player::Two, Some((6, 7))),
        ];
        assert_eq!(
            depths(&mixed),
            vec![((0, 0), 0, 0), ((7, 7), 0, 1), ((8, 7), 1, 1)]
        );
    }

    #[test]
    fn the_wave_never_outlasts_the_longest_allowed_and_a_stone_alone_is_quick() {
        let alone = timeline(0);
        assert_eq!(alone.stagger, 0.0);
        assert!((alone.total - (FADE_SECONDS + EXPAND_SECONDS + VANISH_SECONDS)).abs() < 1e-6);
        assert!(alone.total < 0.4);

        let three = timeline(2);
        assert_eq!(three.stagger, STAGGER_SECONDS);
        assert!(three.total < 0.65);

        for depth in 0..=80 {
            let timeline = timeline(depth);
            assert!(
                timeline.total <= LONGEST_SECONDS + 1e-5,
                "depth {depth}: {timeline:?}"
            );
            assert!(timeline.stagger >= 0.0);
        }
    }

    #[test]
    fn a_disc_fades_then_expands_then_shrinks_away() {
        let alone = timeline(0);
        let at = |elapsed| disc_frame(elapsed, 0, alone).unwrap();

        assert_eq!(
            at(0.0),
            DiscFrame {
                fade: 0.0,
                scale: 1.0,
                alpha: 1.0
            },
            "the disc begins as the stone was"
        );
        let halfway = at(FADE_SECONDS / 2.0);
        assert!(halfway.fade > 0.0 && halfway.fade < 1.0);
        assert_eq!((halfway.scale, halfway.alpha), (1.0, 1.0));
        assert_eq!(at(FADE_SECONDS).fade, 1.0);

        let expanding = at(FADE_SECONDS + EXPAND_SECONDS / 2.0);
        assert!(expanding.scale > 1.0 && expanding.scale < EXPAND_SCALE);
        assert_eq!(expanding.alpha, 1.0);

        let shrinking = at(FADE_SECONDS + EXPAND_SECONDS + VANISH_SECONDS / 2.0);
        assert!(shrinking.scale < EXPAND_SCALE && shrinking.scale > 0.0);
        assert!(shrinking.alpha < 1.0 && shrinking.alpha > 0.0);
        assert_eq!(shrinking.fade, 1.0, "the colour it faded into is kept");

        // The disc is fully transparent before the shrink would have
        // reached nothing: on its last drawn frame it is still a quarter
        // of its size, and then it is gone.
        let last = at(FADE_SECONDS + EXPAND_SECONDS + VANISH_SECONDS - 0.001);
        assert!(last.alpha < 0.01, "{last:?}");
        assert!(last.scale > 0.2, "{last:?}");
        assert_eq!(last.fade, 1.0);
        assert_eq!(disc_frame(alone.total, 0, alone), None);
        assert!(
            alone.total < FADE_SECONDS + EXPAND_SECONDS + SHRINK_SECONDS,
            "the timeline ends where the disc vanishes"
        );

        // A deeper stone starts its fade later and is still whole until then.
        let deep = timeline(2);
        assert_eq!(disc_frame(deep.stagger, 2, deep).unwrap().fade, 0.0);
        assert_eq!(
            disc_frame(2.0 * deep.stagger + FADE_SECONDS, 2, deep)
                .unwrap()
                .fade,
            1.0
        );
    }

    /// The fade crosses Gray's tone, and the module docs put a number on
    /// how long it lingers there; this keeps the number honest.
    #[test]
    fn the_eased_fade_spends_under_a_tenth_of_a_second_near_gray() {
        use crate::presentation::GRAY_STONE;
        let luminance = |colour: Color| {
            let linear = colour.to_linear();
            0.2126 * linear.red + 0.7152 * linear.green + 0.0722 * linear.blue
        };
        let contrast = |a: Color, b: Color| {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        let samples = 1000;
        let near_gray = (0..samples)
            .filter(|step| {
                let elapsed = FADE_SECONDS * *step as f32 / samples as f32;
                let frame = disc_frame(elapsed, 0, timeline(0)).unwrap();
                contrast(
                    captured_stone_fill(Player::One, frame.fade, 1.0),
                    GRAY_STONE,
                ) < 3.0
            })
            .count();
        let share = near_gray as f32 / samples as f32;
        assert!(share > 0.0, "a black-to-white fade cannot avoid gray");
        assert!(share < 0.55, "{share} of the fade is within 3:1 of gray");
        assert!(
            share * FADE_SECONDS < 0.1,
            "{} seconds within 3:1 of gray",
            share * FADE_SECONDS
        );
    }

    #[test]
    fn the_count_names_the_captor_and_says_nothing_for_nothing() {
        let stones = vec![
            removed(0, 0, Player::One, Some((0, 1))),
            removed(1, 0, Player::One, Some((0, 1))),
            removed(8, 8, Player::Two, Some((7, 8))),
        ];
        assert_eq!(count_text(&stones, Player::Two), "Captured 2");
        assert_eq!(count_text(&stones, Player::One), "Captured 1");
        assert_eq!(count_text(&[], Player::One), "");
    }

    // ---- The entities ---------------------------------------------------

    fn capture_test_app(reduce_motion: bool) -> App {
        let mut app = App::new();
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Mesh>>();
        let palette = {
            let mut materials = app.world_mut().resource_mut::<Assets<ColorMaterial>>();
            test_stone_palette(&mut materials)
        };
        let stone_mesh = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            StoneMesh(meshes.add(Circle::new(STONE_RADIUS)))
        };
        app.insert_resource(palette)
            .insert_resource(stone_mesh)
            .insert_resource(GameState::default())
            .insert_resource(GamePreferences {
                reduce_motion,
                ..GamePreferences::default()
            })
            .insert_resource(MotionPolicy::default())
            .insert_resource(Time::<()>::default())
            .init_resource::<CaptureIndication>()
            .add_systems(Startup, spawn_capture_marks)
            .add_systems(Update, update_capture_indication);
        app
    }

    /// A capture of two black stones at (0,0) and (1,0) by White at (2,0),
    /// written straight into the state as the authority would leave it.
    fn capture_two_black_stones(app: &mut App, round: usize) {
        let mut game = app.world_mut().resource_mut::<GameState>();
        game.phase = MatchPhase::Playing;
        game.board.set(Point::new(0, 0), Cell::Empty);
        game.board.set(Point::new(1, 0), Cell::Empty);
        game.board.set(Point::new(2, 0), Cell::White);
        game.last_removals = RoundRemovals {
            round,
            stones: vec![
                removed(0, 0, Player::One, Some((2, 0))),
                removed(1, 0, Player::One, Some((2, 0))),
            ],
        };
    }

    fn root_visible(app: &mut App, x: usize, y: usize) -> bool {
        app.world_mut()
            .query::<(&CaptureMark, &Visibility)>()
            .iter(app.world())
            .find(|(mark, _)| mark.0 == Point::new(x, y))
            .map(|(_, visibility)| *visibility == Visibility::Visible)
            .expect("one mark per intersection")
    }

    /// One part as drawn: its visibility, scale, material and sprite.
    type PartState = (
        Visibility,
        Vec3,
        Option<Handle<ColorMaterial>>,
        Option<Sprite>,
    );

    fn part(app: &mut App, x: usize, y: usize, wanted: CaptureMarkPart) -> Vec<PartState> {
        let root = app
            .world_mut()
            .query::<(Entity, &CaptureMark)>()
            .iter(app.world())
            .find(|(_, mark)| mark.0 == Point::new(x, y))
            .map(|(entity, _)| entity)
            .expect("one mark per intersection");
        app.world_mut()
            .query::<(
                &ChildOf,
                &CaptureMarkPart,
                &Visibility,
                &Transform,
                Option<&MeshMaterial2d<ColorMaterial>>,
                Option<&Sprite>,
            )>()
            .iter(app.world())
            .filter(|(child_of, part, ..)| child_of.parent() == root && **part == wanted)
            .map(|(_, _, visibility, transform, material, sprite)| {
                (
                    *visibility,
                    transform.scale,
                    material.map(|material| material.0.clone()),
                    sprite.cloned(),
                )
            })
            .collect()
    }

    fn disc_fill_colour(app: &mut App, x: usize, y: usize) -> Option<Color> {
        let (visibility, _, handle, _) = part(app, x, y, CaptureMarkPart::DiscFill).remove(0);
        (visibility == Visibility::Inherited).then(|| {
            app.world()
                .resource::<Assets<ColorMaterial>>()
                .get(handle.as_ref().unwrap())
                .unwrap()
                .color
        })
    }

    /// The one part of a kind a mark has.
    fn only<T>(mut parts: Vec<T>) -> T {
        assert_eq!(parts.len(), 1, "one part of this kind per mark");
        parts.remove(0)
    }

    fn counts(app: &mut App) -> [String; 2] {
        let mut texts = [String::new(), String::new()];
        for (count, text) in app
            .world_mut()
            .query::<(&CaptureCount, &Text)>()
            .iter(app.world())
        {
            texts[count.0.index()] = text.0.clone();
        }
        texts
    }

    #[test]
    fn nothing_is_shown_and_nothing_spawns_while_the_record_is_empty() {
        let mut app = capture_test_app(false);
        app.update();
        let entities = app.world_mut().query::<Entity>().iter(app.world()).count();
        assert_eq!(
            app.world_mut()
                .query::<&CaptureMark>()
                .iter(app.world())
                .count(),
            BOARD_SIZE * BOARD_SIZE
        );
        for _ in 0..3 {
            advance_test_app(&mut app, 0.1);
        }
        assert!(!root_visible(&mut app, 4, 4));
        assert_eq!(counts(&mut app), [String::new(), String::new()]);
        assert_eq!(
            app.world_mut().query::<Entity>().iter(app.world()).count(),
            entities,
            "marks are spawned once, on entry"
        );
        assert!(
            app.world_mut()
                .query::<(&CaptureMark, &Visibility)>()
                .iter(app.world())
                .all(|(_, visibility)| *visibility == Visibility::Hidden)
        );
    }

    #[test]
    fn a_capture_marks_each_emptied_point_as_its_stone_and_counts_it_for_the_captor() {
        let mut app = capture_test_app(true);
        capture_two_black_stones(&mut app, 3);
        app.update();

        assert!(root_visible(&mut app, 0, 0));
        assert!(root_visible(&mut app, 1, 0));
        assert!(
            !root_visible(&mut app, 2, 0),
            "the capturing stone is not marked"
        );
        let palette_black = app.world().resource::<StonePalette>().black.clone();
        let (visibility, scale, material, _) =
            only(part(&mut app, 0, 0, CaptureMarkPart::GhostStroke));
        assert_eq!(visibility, Visibility::Inherited);
        assert_eq!(material.unwrap(), palette_black);
        assert_eq!(scale, Vec3::splat(stone_geometry(Cell::Black).fill_scale));
        let (ink, ..) = only(part(&mut app, 0, 0, CaptureMarkPart::GhostInk));
        assert_eq!(ink, Visibility::Hidden, "a black stone carries no ring");
        let bars = part(&mut app, 0, 0, CaptureMarkPart::GhostCross);
        assert_eq!(bars.len(), 2);
        for (_, _, _, sprite) in bars {
            let sprite = sprite.unwrap();
            assert_eq!(sprite.color, BLACK_STONE);
            assert_eq!(
                sprite.custom_size,
                Some(Vec2::new(cross_length(Player::One), CROSS_BAR_WIDTH))
            );
        }
        assert_eq!(counts(&mut app), [String::new(), "Captured 2".into()]);
    }

    #[test]
    fn a_white_ghost_carries_its_stones_ink_ring_outside_a_white_stroke() {
        let mut app = capture_test_app(true);
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.phase = MatchPhase::Playing;
            game.last_removals = RoundRemovals {
                round: 1,
                stones: vec![removed(4, 4, Player::Two, Some((4, 5)))],
            };
        }
        app.update();

        let palette = app.world().resource::<StonePalette>();
        let (white, ring) = (palette.white.clone(), palette.ring.clone());
        let (ink_visibility, ink_scale, ink_material, _) =
            only(part(&mut app, 4, 4, CaptureMarkPart::GhostInk));
        assert_eq!(ink_visibility, Visibility::Inherited);
        assert_eq!(ink_material.unwrap(), ring);
        assert_eq!(ink_scale, Vec3::splat(1.0));
        let (_, stroke_scale, stroke_material, _) =
            only(part(&mut app, 4, 4, CaptureMarkPart::GhostStroke));
        assert_eq!(stroke_material.unwrap(), white);
        assert_eq!(
            stroke_scale,
            Vec3::splat(stone_geometry(Cell::White).fill_scale)
        );
        assert!(cross_length(Player::Two) < cross_length(Player::One));
        assert_eq!(counts(&mut app), ["Captured 1".into(), String::new()]);
    }

    #[test]
    fn reduced_motion_shows_the_marks_and_never_a_disc() {
        let mut app = capture_test_app(true);
        capture_two_black_stones(&mut app, 1);
        app.update();
        assert!(!app.world().resource::<CaptureIndication>().animating);
        for _ in 0..10 {
            assert!(root_visible(&mut app, 0, 0));
            assert_eq!(disc_fill_colour(&mut app, 0, 0), None);
            let (ring, ..) = only(part(&mut app, 0, 0, CaptureMarkPart::DiscRing));
            assert_eq!(ring, Visibility::Hidden);
            advance_test_app(&mut app, 0.05);
        }
    }

    #[test]
    fn with_motion_the_disc_fades_to_the_captors_colour_then_shrinks_and_leaves_the_board_clean() {
        let mut app = capture_test_app(false);
        capture_two_black_stones(&mut app, 1);
        app.update();
        assert!(app.world().resource::<CaptureIndication>().animating);
        assert_eq!(
            disc_fill_colour(&mut app, 0, 0),
            Some(BLACK_STONE),
            "the disc begins as the stone was"
        );
        // No ghost under it: with motion allowed the animation is the
        // channel, and the count is what both modes share.
        for ghost in [
            CaptureMarkPart::GhostInk,
            CaptureMarkPart::GhostStroke,
            CaptureMarkPart::GhostCross,
        ] {
            for (visibility, ..) in part(&mut app, 0, 0, ghost) {
                assert_eq!(visibility, Visibility::Hidden, "{ghost:?}");
            }
        }
        assert_eq!(counts(&mut app), [String::new(), "Captured 2".into()]);
        let (_, ring_scale, ring_material, _) =
            only(part(&mut app, 0, 0, CaptureMarkPart::DiscRing));
        assert_eq!(ring_scale, Vec3::splat(1.0));
        assert_eq!(
            ring_material.unwrap(),
            app.world().resource::<StonePalette>().ring.clone()
        );

        // The wave starts at (1, 0), the stone touching White's capturing
        // stone at (2, 0), and reaches the corner one step later.
        advance_test_app(&mut app, FADE_SECONDS / 2.0);
        let midway = disc_fill_colour(&mut app, 1, 0).unwrap();
        assert_ne!(midway, BLACK_STONE);
        assert_ne!(midway, WHITE_STONE);

        advance_test_app(&mut app, FADE_SECONDS / 2.0);
        assert_eq!(
            disc_fill_colour(&mut app, 1, 0),
            Some(WHITE_STONE),
            "a black stone fades to its captor's white"
        );
        assert_ne!(
            disc_fill_colour(&mut app, 0, 0),
            Some(WHITE_STONE),
            "the corner, one step further from the taker, is still on its way"
        );

        let total = timeline(1).total;
        advance_test_app(&mut app, total);
        advance_test_app(&mut app, 0.0);
        assert!(!app.world().resource::<CaptureIndication>().animating);
        assert_eq!(disc_fill_colour(&mut app, 0, 0), None);
        assert_eq!(disc_fill_colour(&mut app, 1, 0), None);
        assert!(!root_visible(&mut app, 0, 0), "the board is left clean");
        assert!(!root_visible(&mut app, 1, 0));
        assert_eq!(
            counts(&mut app),
            [String::new(), "Captured 2".into()],
            "the count stays"
        );
    }

    /// The same record in both modes: the ghosts are the reduced-motion
    /// form only, and the count is common to both. Reduced motion here is
    /// the platform's answer rather than the preference, so the policy's
    /// other half is exercised too.
    #[test]
    fn the_ghosts_are_the_reduced_motion_form_only_and_the_count_is_common() {
        let mut app = capture_test_app(false);
        app.insert_resource(MotionPolicy::with_platform_reducing_motion(true));
        capture_two_black_stones(&mut app, 1);
        app.update();
        assert!(!app.world().resource::<CaptureIndication>().animating);
        assert!(root_visible(&mut app, 0, 0));
        let (stroke, ..) = only(part(&mut app, 0, 0, CaptureMarkPart::GhostStroke));
        assert_eq!(stroke, Visibility::Inherited);
        for (cross, ..) in part(&mut app, 0, 0, CaptureMarkPart::GhostCross) {
            assert_eq!(cross, Visibility::Inherited, "the X goes with the ring");
        }
        assert_eq!(disc_fill_colour(&mut app, 0, 0), None);
        assert_eq!(counts(&mut app), [String::new(), "Captured 2".into()]);
        for _ in 0..5 {
            advance_test_app(&mut app, 0.2);
        }
        assert!(
            root_visible(&mut app, 0, 0),
            "the ghosts stay as long as the record"
        );
    }

    /// One record giving way to the next: a round resolving while the
    /// previous capture is still animating. The old marks and discs go in
    /// the same frame, the new record's marks appear with their animation
    /// at its start, and a point the new round re-occupied is not marked.
    #[test]
    fn a_new_round_resolving_mid_animation_gives_way_cleanly() {
        let mut app = capture_test_app(false);
        capture_two_black_stones(&mut app, 1);
        app.update();
        advance_test_app(&mut app, FADE_SECONDS / 2.0);
        assert!(root_visible(&mut app, 1, 0));
        assert!(disc_fill_colour(&mut app, 1, 0).is_some());
        assert!(app.world().resource::<CaptureIndication>().elapsed > 0.0);

        // Round 2: Black plays (1, 0) back and captures White's stone at
        // (2, 0); the record is now that capture alone.
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.board.set(Point::new(1, 0), Cell::Black);
            game.board.set(Point::new(2, 0), Cell::Empty);
            game.last_removals = RoundRemovals {
                round: 2,
                stones: vec![removed(2, 0, Player::Two, Some((1, 0)))],
            };
        }
        advance_test_app(&mut app, 0.016);

        assert!(
            !root_visible(&mut app, 0, 0),
            "the old round's mark is gone"
        );
        assert!(
            !root_visible(&mut app, 1, 0),
            "a re-occupied point is not marked"
        );
        assert!(root_visible(&mut app, 2, 0));
        assert_eq!(
            disc_fill_colour(&mut app, 2, 0),
            Some(WHITE_STONE),
            "the new disc starts as the white stone was"
        );
        let indication = app.world().resource::<CaptureIndication>();
        assert_eq!(indication.shown, Some(2));
        assert!(indication.animating);
        assert!(
            indication.elapsed < FADE_SECONDS / 2.0,
            "the clock restarted"
        );
        assert_eq!(counts(&mut app), ["Captured 1".into(), String::new()]);

        // The old points' discs are not merely hidden by their root: they
        // are hidden in their own right, so a later record at those points
        // starts from nothing stale.
        let (disc, ..) = only(part(&mut app, 0, 0, CaptureMarkPart::DiscFill));
        assert_eq!(disc, Visibility::Hidden);
    }

    #[test]
    fn a_round_that_removes_nothing_clears_the_marks_and_the_count() {
        let mut app = capture_test_app(false);
        capture_two_black_stones(&mut app, 1);
        app.update();
        assert!(root_visible(&mut app, 0, 0));
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.last_removals = RoundRemovals {
                round: 2,
                stones: Vec::new(),
            };
        }
        app.update();
        assert!(!root_visible(&mut app, 0, 0));
        assert!(!root_visible(&mut app, 1, 0));
        assert_eq!(counts(&mut app), [String::new(), String::new()]);
        assert!(!app.world().resource::<CaptureIndication>().animating);
    }

    #[test]
    fn the_record_shows_through_an_ending_capture_but_not_a_manual_end_or_round_zero() {
        let mut app = capture_test_app(true);
        capture_two_black_stones(&mut app, 5);
        app.update();
        assert!(root_visible(&mut app, 0, 0));

        // A Point Target crossed by that capture ends the Match; the board
        // is then territory-filled, and the mark stays over the fill.
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.phase = MatchPhase::Ended;
            game.end_reasons = vec![EndReason::PointTarget];
            game.board.set(Point::new(0, 0), Cell::White);
        }
        app.update();
        assert!(root_visible(&mut app, 0, 0));
        assert_eq!(counts(&mut app), [String::new(), "Captured 2".into()]);

        // A Manual end stands on a rolled-back snapshot: nothing is shown.
        app.world_mut().resource_mut::<GameState>().end_reasons = vec![EndReason::Manual];
        app.update();
        assert!(!root_visible(&mut app, 0, 0));
        assert_eq!(counts(&mut app), [String::new(), String::new()]);

        // A fresh Match's Round 0 shows nothing either, and forgets the key,
        // so the reveal's record - Round 0's - is new when it comes.
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.phase = MatchPhase::Opening;
            game.last_removals = RoundRemovals::default();
        }
        app.update();
        assert_eq!(app.world().resource::<CaptureIndication>().shown, None);
        {
            let mut game = app.world_mut().resource_mut::<GameState>();
            game.phase = MatchPhase::Playing;
            game.board.set(Point::new(0, 0), Cell::Empty);
            game.last_removals = RoundRemovals {
                round: 0,
                stones: vec![removed(0, 0, Player::One, None)],
            };
        }
        app.update();
        assert!(
            root_visible(&mut app, 0, 0),
            "the reveal sweep's record is shown"
        );
        assert_eq!(app.world().resource::<CaptureIndication>().shown, Some(0));
    }

    #[test]
    fn disc_fills_blend_so_the_fade_away_is_drawn() {
        let mut app = capture_test_app(false);
        app.update();
        let (_, _, handle, _) = part(&mut app, 0, 0, CaptureMarkPart::DiscFill).remove(0);
        let material = app
            .world()
            .resource::<Assets<ColorMaterial>>()
            .get(handle.as_ref().unwrap())
            .unwrap();
        assert_eq!(material.alpha_mode, AlphaMode2d::Blend);
    }

    #[test]
    fn re_entering_the_screen_forgets_what_was_shown() {
        let mut app = capture_test_app(false);
        capture_two_black_stones(&mut app, 1);
        app.update();
        assert_eq!(app.world().resource::<CaptureIndication>().shown, Some(1));

        // Leaving the screen despawns the marks; entering spawns fresh ones
        // and the indication starts over, so the same record is shown
        // again from the start rather than assumed already drawn.
        let marks: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<CaptureMark>>()
            .iter(app.world())
            .collect();
        for entity in marks {
            app.world_mut().entity_mut(entity).despawn();
        }
        app.world_mut()
            .run_system_once(spawn_capture_marks)
            .expect("the spawner runs again");
        assert_eq!(app.world().resource::<CaptureIndication>().shown, None);
        app.update();
        assert_eq!(app.world().resource::<CaptureIndication>().shown, Some(1));
        assert!(root_visible(&mut app, 0, 0));
    }

    #[test]
    fn an_idle_frame_writes_nothing() {
        #[derive(Resource, Default)]
        struct Writes(usize);
        type ChangedParts<'w, 's> = Query<
            'w,
            's,
            Entity,
            (
                Or<(Changed<Visibility>, Changed<Transform>, Changed<Sprite>)>,
                Or<(With<CaptureMark>, With<CaptureMarkPart>)>,
            ),
        >;
        fn count_writes(
            mut writes: ResMut<Writes>,
            changed: ChangedParts,
            texts: Query<Entity, (Changed<Text>, With<CaptureCount>)>,
        ) {
            writes.0 = changed.iter().count() + texts.iter().count();
        }
        let mut app = capture_test_app(false);
        app.init_resource::<Writes>()
            .add_systems(Update, count_writes.after(update_capture_indication));
        capture_two_black_stones(&mut app, 1);
        app.update();
        assert!(app.world().resource::<Writes>().0 > 0);
        advance_test_app(&mut app, timeline(1).total + 0.1);
        advance_test_app(&mut app, 0.1);
        advance_test_app(&mut app, 0.1);
        assert_eq!(app.world().resource::<Writes>().0, 0);
    }

    #[test]
    fn capture_queries_initialize_without_component_conflicts() {
        IntoSystem::into_system(update_capture_indication).initialize(&mut World::new());
    }

    use bevy::ecs::system::RunSystemOnce;
}
