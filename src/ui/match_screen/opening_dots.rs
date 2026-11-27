//! The Round 0 dot rows: how many selections each player holds, and what
//! their last selection did.
//!
//! # Responsibility
//!
//! Round 0 hides both cursors and every selected point, so beside the board
//! each player has a row of dots saying only what is public: how many of
//! their three slots are filled, and whether their last selection added a
//! point, rolled the window forward, or changed nothing. It reads the first
//! from the lengths of `opening_selections` and the second from the
//! authority's record of it, `MatchState::last_opening_attempts` - never a
//! point - so the fact exists outside the renderer and a future screen
//! reader can announce the same record this module draws.
//!
//! # The row
//!
//! A line divides each row. The side away from the board holds the
//! selections not yet made, as empty dots packed against the line; the side
//! nearest the board holds the selections made, filled, the newest at the
//! line and the oldest at the far end. That side is a queue, which is the
//! point of the design: dots enter at the line and push along it, so the
//! rule "with three held, a new distinct selection removes the oldest" is
//! something a player can watch rather than be told.
//!
//! # The static form is the requirement
//!
//! Under the motion policy in `app::motion`, the information has to survive
//! with motion off. A count cannot carry this one: with three held, an
//! accepted roll-forward and a duplicate both end with three filled dots.
//! So the mark is not where the dots finish but the line itself: **after a
//! selection that changed nothing, the dividing line is drawn as a cross**,
//! and it stays until the player's next selection replaces it. A cross means
//! "that selection changed nothing", which is what the clack has always
//! said, now in a channel a player can see; an accepted selection clears it.
//! It is drawn from the record alone, the same with motion allowed or not,
//! and the animation below is built on top of it.
//!
//! The cross is on the line rather than on a dot for three reasons. The
//! line is where a selection is accepted - a dot fills as it crosses it -
//! so a cross there reads as the crossing being refused. It is in one place
//! however many points are held. And it needs no dot to hang on: a
//! duplicate with three held spawns a dot that has nowhere to return to and
//! simply fades, which leaves nothing on the far side to mark.
//!
//! # The animation
//!
//! With motion allowed, each selection plays the owner's design on top:
//!
//! * **Accepted with fewer than three held.** The dot beside the line slides
//!   across it, filling as it goes, and pushes the held dots one place
//!   along; the empty dots behind it close up to the line.
//! * **Accepted with three held.** A fourth dot fades into view at the far
//!   side's farthest place and slides all the way across. When it meets the
//!   three it pushes them along, and the oldest, pushed past the end of the
//!   row, falls as though pushed off a table and fades away.
//! * **A duplicate.** The dot beside the line - a fourth spawned there when
//!   three are held - lunges across the line into the newest held dot,
//!   which jolts; it pops up, falls back across the line, emptying as it
//!   goes, and fades. With fewer than three held it then fades back in
//!   where it was; with three held it has no place to return to and is
//!   simply gone.
//!
//! A sliding dot gathers speed until it meets the dots ahead and slows as
//! it pushes them, at one speed across the meeting ([`slide`]), and its fill
//! is how much of it is past the line, so a dot is full exactly when it is
//! wholly on the near side. Every size is made from a dot's diameter, its
//! border and the gap between two dots, which are the row's only lengths,
//! so the row keeps its proportions at whatever scale the interface is
//! drawn; a larger or smaller window moves the row and leaves its size
//! alone, as it does all of the interface. The clock is Bevy's
//! `Time`, never the Match clock, and the animation reads the Match and
//! writes nothing of it.
//!
//! # Meeting the clack
//!
//! The clack is a game fact, not a sound effect: `gameplay::input` records
//! it into the Replay as a `MatchSound` at the moment the duplicate is
//! rejected, and plays it that same frame. The collision is therefore timed
//! to meet the cue, never the cue moved to meet the collision, which would
//! make a Replay encode how long a dot takes to travel. That leaves a
//! duplicate's dot [`APPROACH_SECONDS`] to reach the dot it hits - two
//! frames - which is held under the 45 ms by which sound may lead picture
//! before a viewer notices (ITU-R BT.1359), and is why every refused dot
//! starts beside the line. The owner's design had a duplicate with three
//! held fade in at the far end and slide the whole way, like an accepted
//! fourth; that cannot meet the cue, so a refused fourth appears beside the
//! line instead. Everything a duplicate does after the collision - the pop,
//! the fall, the fade - is after the clack, so it can take its time.
//!
//! # Overlapping selections
//!
//! Round 0 takes input continuously and each row is its own, so a second
//! selection can arrive while a row is still moving. **It interrupts:** the
//! row's motion stops where it would have ended, which is exactly the
//! resting row the new selection starts from, and the new motion starts at
//! its beginning, in the frame the selection was made. Queueing it was
//! ruled out by the clack - a queued duplicate's collision would land after
//! its cue - and absorbing it would show a duplicate's clack with no
//! collision at all. Interrupting keeps every collision on its cue and the
//! row never behind the state it shows. The two rows never wait for each
//! other.
//!
//! # Hidden information
//!
//! Nothing here depends on a point. A duplicate always strikes the newest
//! held dot, whichever held point it repeated, and a roll-forward always
//! drops the oldest, which the rules always remove; the animation is a
//! function of the outcome and the count alone. So it shows what the dot
//! count and the clack already said, plus one thing new to the screen: that
//! a player with three held made an accepted selection, and when.
//! `docs/SECURITY.md` argues why that reveals nothing an opponent can use.
//!
//! # Entities
//!
//! Each row is spawned once on entry, hidden outside Round 0: a root, the
//! line, the two bars of the cross, and four dots - three, and the fourth a
//! roll-forward or a refused selection with three held needs. Dots are
//! drawn from frames rather than given identities, and nothing writes a
//! component whose value has not changed, so a resting row writes nothing.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::geometry::{StackItem, match_layout};
use super::layout::{CONTROL_HINT_WIDTH, MatchPlacement, place};
use crate::app::AppScreen;
use crate::app::motion::Motion;
use crate::app::visuals::WindowSize;
use crate::gameplay::{GameState, MatchPhase, OpeningAttempt, OpeningAttemptOutcome, player_index};
use crate::opening::OPENING_SELECTION_LIMIT;
use crate::presentation::{opening_dot_fill, opening_dot_frame, player_accent};
use crate::rules::Player;

/// A dot's diameter, its border and the gap between two neighbours: the
/// row's only lengths. Every other one here is made of these.
const DOT: f32 = 13.0;
/// The height of each row's node, which stands in its player's stack below
/// the Home and Help buttons and above the control hints;
/// `super::geometry` places it.
pub(super) const ROW_HEIGHT: f32 = DOT;
const DOT_BORDER: f32 = 2.0;
const DOT_GAP: f32 = 10.0;
const RADIUS: f32 = DOT / 2.0;
/// Centre to centre, for two neighbours on one side of the line.
const PITCH: f32 = DOT + DOT_GAP;
/// Centre to centre across the line: a gap on each side of a line as wide
/// as a dot's border.
const ACROSS: f32 = DOT + 2.0 * DOT_GAP + DOT_BORDER;
/// Each bar of the cross: the diagonal of a dot's square, so the cross
/// stands exactly as tall as the line it replaces.
const CROSS_BAR: f32 = DOT * std::f32::consts::SQRT_2;
const LIMIT: usize = OPENING_SELECTION_LIMIT;

/// How long an accepted dot takes to cross with fewer than three held.
const ADD_SECONDS: f32 = 0.24;
/// How long a fourth dot takes to fade into view, and then to slide across.
const ARRIVE_SECONDS: f32 = 0.10;
const ROLL_SECONDS: f32 = 0.32;
/// How long the oldest dot takes to fall off the row and fade, and how far
/// it falls.
const FALL_SECONDS: f32 = 0.30;
const FALL_DEPTH: f32 = 2.0 * DOT;
/// How long a duplicate's dot takes to reach the dot it hits. The clack
/// plays in the frame the duplicate is rejected, so this is how far behind
/// the sound the collision lands.
const APPROACH_SECONDS: f32 = 0.03;
/// How far sound may lead picture before a viewer notices: ITU-R BT.1359's
/// detectability threshold.
const SOUND_LEADS_PICTURE_SECONDS: f32 = 0.045;
const _: () = assert!(
    APPROACH_SECONDS <= SOUND_LEADS_PICTURE_SECONDS,
    "a duplicate's collision has to meet its clack"
);
/// The pop, fall and fade after a duplicate's collision.
const BOUNCE_SECONDS: f32 = 0.30;
/// How high the refused dot pops, and the share of the bounce it spends
/// rising.
const POP_HEIGHT: f32 = RADIUS;
const POP_SHARE: f32 = 0.3;
/// How far the struck dot and those behind it jolt, and for how long.
const JOLT: f32 = DOT_BORDER;
const JOLT_SECONDS: f32 = 0.10;
/// How long a refused dot takes to fade back in where it was.
const RETURN_SECONDS: f32 = 0.12;
/// The longest any one selection's motion may take, and the totals held
/// to it. For scale, the capture indication's single stone takes half a
/// second, and the owner has twice said that budget felt generous.
const LONGEST_SECONDS: f32 = 0.75;
const ROLL_TOTAL_SECONDS: f32 = roll_fall_starts() + FALL_SECONDS;
const REFUSE_TOTAL_SECONDS: f32 = APPROACH_SECONDS + BOUNCE_SECONDS + RETURN_SECONDS;
const _: () = assert!(
    ADD_SECONDS <= LONGEST_SECONDS
        && ROLL_TOTAL_SECONDS <= LONGEST_SECONDS
        && REFUSE_TOTAL_SECONDS <= LONGEST_SECONDS,
    "no selection's motion outlasts the longest allowed"
);
const _: () = assert!(
    ROLL_TOTAL_SECONDS >= ARRIVE_SECONDS + ROLL_SECONDS,
    "a roll-forward ends when the oldest has fallen, after the fourth has arrived"
);

/// Where along a row a dot at far-side place `slot` sits, counting from
/// the line: slot 1 is beside it. Lengths along a row are measured from the
/// end away from the board.
const fn far(slot: usize) -> f32 {
    RADIUS + (LIMIT - slot) as f32 * PITCH
}

/// Where a dot at near-side place `slot` sits, slot 1 beside the line.
/// Slot 4 is past the end of the row, where the oldest is pushed to.
const fn near(slot: usize) -> f32 {
    far(1) + ACROSS + (slot - 1) as f32 * PITCH
}

/// The middle of the dividing line.
const LINE: f32 = far(1) + ACROSS / 2.0;
/// Where a duplicate's dot touches the newest held dot.
const CONTACT: f32 = near(1) - DOT;
/// From the first far-side dot's outer edge to the last near-side dot's.
const CONTENT: f32 = near(LIMIT) + RADIUS;
/// The space either side of the content, which is centred in its row.
const ROW_PADDING: f32 = (CONTROL_HINT_WIDTH - CONTENT) / 2.0;
const _: () = assert!(ROW_PADDING >= 0.0, "a row's content fits its node");

/// The root of one player's row.
#[derive(Component)]
struct OpeningDots;

/// One of the four dots in a player's row, drawn from whichever frame is
/// handed to it; see the module docs.
#[derive(Component)]
struct OpeningDot {
    player: Player,
    index: usize,
}

/// One of the two forms a player's dividing line takes: the line, or one
/// bar of the cross that replaces it after a selection changed nothing.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
struct RowMark {
    player: Player,
    cross: bool,
}

/// What each row is showing and how far its motion has run.
#[derive(Resource, Default)]
pub(crate) struct OpeningDotRows([RowState; 2]);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct RowState {
    /// The number of the attempt the row was last restyled for, or `None`
    /// while the player has made none.
    shown: Option<u32>,
    /// The motion playing and how many seconds in, while one plays.
    playing: Option<(RowMotion, f32)>,
}

/// One selection's motion, with the number of points held before it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RowMotion {
    /// A distinct selection, with `held` fewer than three.
    Add { held: usize },
    /// A distinct selection with three held.
    RollForward,
    /// A duplicate, with `held` points held.
    Refuse { held: usize },
}

/// One dot as drawn at an instant.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DotFrame {
    /// Its centre, along the row from the end away from the board.
    x: f32,
    /// Its centre below the row's centre line; negative is above it.
    y: f32,
    alpha: f32,
}

impl DotFrame {
    fn at(x: f32) -> Self {
        Self {
            x,
            y: 0.0,
            alpha: 1.0,
        }
    }

    /// How much of the dot is past the line: the share of its fill that
    /// is its player's colour.
    fn fill(self) -> f32 {
        ((self.x - (LINE - RADIUS)) / DOT).clamp(0.0, 1.0)
    }
}

/// Smoothstep, for the motions that start and end at rest.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where a dot sliding from `from` to `to` is, `u` of the way through its
/// slide in time. It gathers speed until it meets the dots ahead, a pitch
/// short of `to`, then slows as it pushes them the rest of the way, at one
/// speed across the meeting: each part takes time in proportion to its
/// distance, which is what makes the two speeds equal.
fn slide(from: f32, to: f32, u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    let distance = to - from;
    let meet = (distance - PITCH) / distance;
    if u < meet {
        from + (distance - PITCH) * (u / meet).powi(2)
    } else {
        let pushing = (u - meet) / (1.0 - meet);
        to - PITCH + PITCH * (1.0 - (1.0 - pushing).powi(2))
    }
}

/// How far the held dots have been pushed along by a dot arriving at `x`:
/// nothing until it meets them, a pitch short of the first place past the
/// line.
fn pushed(x: f32) -> f32 {
    (x - (near(1) - PITCH)).clamp(0.0, PITCH)
}

/// The row at rest with `held` points held: filled dots from the line
/// toward the board, empty ones from the line away from it.
fn at_rest(held: usize) -> Vec<DotFrame> {
    (1..=held)
        .map(near)
        .chain((1..=LIMIT - held).map(far))
        .map(DotFrame::at)
        .collect()
}

/// How far below the row a refused dot is, `bounced` seconds after its
/// collision: up to [`POP_HEIGHT`], then down again under the same
/// gravity.
fn bounce_height(bounced: f32) -> f32 {
    let rising = POP_SHARE * BOUNCE_SECONDS;
    let speed = 2.0 * POP_HEIGHT / rising;
    let gravity = speed / rising;
    -speed * bounced + 0.5 * gravity * bounced * bounced
}

/// When, in a roll-forward, the oldest dot is pushed off the end of the
/// row and starts to fall: once the push has carried it half a pitch, past
/// the gap that follows the last place on the near side.
const fn roll_fall_starts() -> f32 {
    let distance = near(1) - far(LIMIT);
    let meet = (distance - PITCH) / distance;
    let half_pushed = 1.0 - std::f32::consts::FRAC_1_SQRT_2;
    ARRIVE_SECONDS + ROLL_SECONDS * (meet + half_pushed * (1.0 - meet))
}

impl RowMotion {
    /// The motion for a selection that had `outcome` and left `held`
    /// points held, or `None` for a record the count contradicts.
    fn of(outcome: OpeningAttemptOutcome, held: usize) -> Option<Self> {
        match outcome {
            OpeningAttemptOutcome::Added => held.checked_sub(1).map(|held| Self::Add { held }),
            OpeningAttemptOutcome::RolledForward => Some(Self::RollForward),
            OpeningAttemptOutcome::Duplicate => (held > 0).then_some(Self::Refuse { held }),
        }
    }

    const fn seconds(self) -> f32 {
        match self {
            Self::Add { .. } => ADD_SECONDS,
            Self::RollForward => ROLL_TOTAL_SECONDS,
            // With three held, the refused dot is a fourth with nowhere to
            // return to, so it does not.
            Self::Refuse { held } if held < LIMIT => REFUSE_TOTAL_SECONDS,
            Self::Refuse { .. } => APPROACH_SECONDS + BOUNCE_SECONDS,
        }
    }

    /// The row `elapsed` seconds into this motion. At zero it is the row
    /// at rest before the selection - with a refused fourth already beside
    /// the line - and from [`Self::seconds`] on, the row at rest after it.
    fn frame(self, elapsed: f32) -> Vec<DotFrame> {
        match self {
            Self::Add { held } => {
                let progress = (elapsed / ADD_SECONDS).clamp(0.0, 1.0);
                let crossing = slide(far(1), near(1), progress);
                let push = pushed(crossing);
                let close_up = PITCH * ease(progress);
                std::iter::once(DotFrame::at(crossing))
                    .chain((1..=held).map(|slot| DotFrame::at(near(slot) + push)))
                    .chain((2..=LIMIT - held).map(|slot| DotFrame::at(far(slot) + close_up)))
                    .collect()
            }
            Self::RollForward => {
                let arriving = if elapsed < ARRIVE_SECONDS {
                    DotFrame {
                        alpha: ease(elapsed / ARRIVE_SECONDS),
                        ..DotFrame::at(far(LIMIT))
                    }
                } else {
                    let progress = (elapsed - ARRIVE_SECONDS) / ROLL_SECONDS;
                    DotFrame::at(slide(far(LIMIT), near(1), progress))
                };
                let push = pushed(arriving.x);
                let falling = elapsed - roll_fall_starts();
                let oldest = DotFrame::at(near(LIMIT) + push);
                let oldest = if falling <= 0.0 {
                    Some(oldest)
                } else if falling < FALL_SECONDS {
                    let fallen = falling / FALL_SECONDS;
                    Some(DotFrame {
                        y: FALL_DEPTH * fallen * fallen,
                        alpha: 1.0 - ease(fallen),
                        ..oldest
                    })
                } else {
                    None
                };
                std::iter::once(arriving)
                    .chain((1..LIMIT).map(|slot| DotFrame::at(near(slot) + push)))
                    .chain(oldest)
                    .collect()
            }
            Self::Refuse { held } => {
                let struck = elapsed - APPROACH_SECONDS;
                let jolt = if (0.0..JOLT_SECONDS).contains(&struck) {
                    JOLT * (std::f32::consts::PI * struck / JOLT_SECONDS).sin()
                } else {
                    0.0
                };
                let refused = if struck < 0.0 {
                    let approached = elapsed / APPROACH_SECONDS;
                    Some(DotFrame::at(
                        far(1) + (CONTACT - far(1)) * approached * approached,
                    ))
                } else if struck < BOUNCE_SECONDS {
                    let bounced = struck / BOUNCE_SECONDS;
                    let fading = ((bounced - POP_SHARE) / (1.0 - POP_SHARE)).max(0.0);
                    Some(DotFrame {
                        x: CONTACT - (CONTACT - far(1)) * bounced,
                        y: bounce_height(struck),
                        alpha: 1.0 - ease(fading),
                    })
                } else if held < LIMIT {
                    let returned = (struck - BOUNCE_SECONDS) / RETURN_SECONDS;
                    Some(DotFrame {
                        alpha: ease(returned),
                        ..DotFrame::at(far(1))
                    })
                } else {
                    None
                };
                (1..=held)
                    .map(|slot| DotFrame::at(near(slot) + jolt))
                    .chain((2..=LIMIT - held).map(|slot| DotFrame::at(far(slot))))
                    .chain(refused)
                    .collect()
            }
        }
    }
}

impl RowState {
    /// Takes in this frame's record for the row and moves its motion on:
    /// a new attempt starts its motion at zero, in the frame it was made,
    /// interrupting any still playing; otherwise the motion advances by
    /// `delta`. Reduced motion stops it, and so does reaching its end.
    fn advance(&mut self, attempt: Option<OpeningAttempt>, held: usize, delta: f32, reduced: bool) {
        let key = attempt.map(|attempt| attempt.number);
        if key != self.shown {
            self.shown = key;
            self.playing = attempt
                .and_then(|attempt| RowMotion::of(attempt.outcome, held))
                .map(|motion| (motion, 0.0));
        } else if let Some((_, elapsed)) = &mut self.playing {
            *elapsed += delta;
        }
        if reduced
            || self
                .playing
                .is_some_and(|(motion, elapsed)| elapsed >= motion.seconds())
        {
            self.playing = None;
        }
    }

    fn frame(&self, held: usize) -> Vec<DotFrame> {
        match self.playing {
            Some((motion, elapsed)) => motion.frame(elapsed),
            None => at_rest(held),
        }
    }
}

/// Where a length `along` a row lands in `player`'s row node, from its
/// left edge. Player Two's row is left of the board and Player One's right
/// of it, so one runs left to right and the other mirrors it.
fn column(player: Player, along: f32) -> f32 {
    match player {
        Player::Two => ROW_PADDING + along,
        Player::One => CONTROL_HINT_WIDTH - ROW_PADDING - along,
    }
}

fn dot_node(player: Player, frame: DotFrame) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(column(player, frame.x) - RADIUS),
        top: px(frame.y),
        width: px(DOT),
        height: px(DOT),
        border: UiRect::all(px(DOT_BORDER)),
        border_radius: BorderRadius::all(percent(50)),
        ..default()
    }
}

/// Spawns both rows, resting and empty, and forgets whatever an earlier
/// visit to the screen was showing. Runs on entry, after the rest of the
/// HUD, so a falling dot passes over the control hints rather than under.
pub(super) fn spawn_opening_dots(
    mut commands: Commands,
    mut rows: ResMut<OpeningDotRows>,
    window: Res<WindowSize>,
) {
    *rows = OpeningDotRows::default();
    let layout = match_layout(window.layout());
    for player in [Player::Two, Player::One] {
        let placement = MatchPlacement::Stack(player, StackItem::OpeningDots);
        let mut node = Node {
            position_type: PositionType::Absolute,
            width: px(CONTROL_HINT_WIDTH),
            height: px(ROW_HEIGHT),
            ..default()
        };
        place(&layout, placement, &mut node);
        let accent = player_accent(player);
        commands
            .spawn((
                node,
                placement,
                Visibility::Visible,
                OpeningDots,
                DespawnOnExit(AppScreen::Match),
            ))
            .with_children(|row| {
                row.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(column(player, LINE) - DOT_BORDER / 2.0),
                        width: px(DOT_BORDER),
                        height: px(DOT),
                        ..default()
                    },
                    BackgroundColor(accent),
                    Visibility::Inherited,
                    RowMark {
                        player,
                        cross: false,
                    },
                ));
                for degrees in [45.0, -45.0] {
                    row.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(column(player, LINE) - CROSS_BAR / 2.0),
                            top: px(RADIUS - DOT_BORDER / 2.0),
                            width: px(CROSS_BAR),
                            height: px(DOT_BORDER),
                            ..default()
                        },
                        UiTransform::from_rotation(Rot2::degrees(degrees)),
                        BackgroundColor(accent),
                        Visibility::Hidden,
                        RowMark {
                            player,
                            cross: true,
                        },
                    ));
                }
                let resting = at_rest(0);
                for index in 0..=LIMIT {
                    let frame = resting.get(index).copied();
                    let shown = frame.unwrap_or(DotFrame::at(far(LIMIT)));
                    row.spawn((
                        dot_node(player, shown),
                        BackgroundColor(opening_dot_fill(player, shown.fill(), shown.alpha)),
                        BorderColor::all(opening_dot_frame(player, shown.alpha)),
                        if frame.is_some() {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        },
                        OpeningDot { player, index },
                    ));
                }
            });
    }
}

type RowRoots<'w, 's> = Query<
    'w,
    's,
    &'static mut Visibility,
    (With<OpeningDots>, Without<OpeningDot>, Without<RowMark>),
>;

type RowDots<'w, 's> = Query<
    'w,
    's,
    (
        &'static OpeningDot,
        &'static mut Node,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        &'static mut Visibility,
    ),
    (Without<OpeningDots>, Without<RowMark>),
>;

type RowMarks<'w, 's> = Query<
    'w,
    's,
    (&'static RowMark, &'static mut Visibility),
    (Without<OpeningDots>, Without<OpeningDot>),
>;

/// The row roots, their dots and their marks: three queries, each over
/// entities carrying its own marker and `Without` the other two, so their
/// borrows are provably disjoint.
#[derive(SystemParam)]
pub(super) struct OpeningDotEntities<'w, 's> {
    roots: RowRoots<'w, 's>,
    dots: RowDots<'w, 's>,
    marks: RowMarks<'w, 's>,
}

fn set_visibility(visibility: &mut Mut<Visibility>, target: Visibility) {
    if **visibility != target {
        **visibility = target;
    }
}

/// Draws both rows from the Match and advances their motion. Runs in
/// `AppSystems::MatchDisplay`, so it sees a selection in the frame it was
/// made - the frame its clack is queued in - and shares no entity with the
/// other display systems there.
pub(super) fn update_opening_dots(
    game: Res<GameState>,
    motion: Motion,
    time: Res<Time>,
    mut rows: ResMut<OpeningDotRows>,
    mut entities: OpeningDotEntities,
) {
    let reduced = motion.reduced();
    let mut frames: [Vec<DotFrame>; 2] = Default::default();
    let mut marked = [false; 2];
    for player in Player::ALL {
        let index = player_index(player);
        let held = game.opening_selections[index].len();
        let attempt = game.last_opening_attempts[index];
        let row = &mut rows.0[index];
        row.advance(attempt, held, time.delta_secs(), reduced);
        frames[index] = row.frame(held);
        marked[index] =
            attempt.is_some_and(|attempt| attempt.outcome == OpeningAttemptOutcome::Duplicate);
    }

    let shown = if matches!(
        game.phase,
        MatchPhase::Opening | MatchPhase::OpeningCountdown
    ) {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut entities.roots {
        set_visibility(&mut visibility, shown);
    }
    for (mark, mut visibility) in &mut entities.marks {
        let target = if marked[player_index(mark.player)] == mark.cross {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        set_visibility(&mut visibility, target);
    }

    for (dot, mut node, mut background, mut border, mut visibility) in &mut entities.dots {
        let Some(frame) = frames[player_index(dot.player)].get(dot.index).copied() else {
            set_visibility(&mut visibility, Visibility::Hidden);
            continue;
        };
        if frame.alpha <= 0.0 {
            set_visibility(&mut visibility, Visibility::Hidden);
            continue;
        }
        set_visibility(&mut visibility, Visibility::Inherited);
        let target = dot_node(dot.player, frame);
        if node.left != target.left {
            node.left = target.left;
        }
        if node.top != target.top {
            node.top = target.top;
        }
        let fill = opening_dot_fill(dot.player, frame.fill(), frame.alpha);
        if background.0 != fill {
            background.0 = fill;
        }
        let frame_colour = BorderColor::all(opening_dot_frame(dot.player, frame.alpha));
        if *border != frame_colour {
            *border = frame_colour;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::motion::MotionPolicy;
    use crate::presentation::DEEP_SURFACE_BACKGROUND;
    use crate::rules::Point;
    use crate::settings::GamePreferences;
    use crate::test_support::advance_test_app;
    use bevy::ecs::system::RunSystemOnce;

    const EPSILON: f32 = 1e-3;

    /// A row's visible dots as `(x, y, fill, alpha)`, in order along it,
    /// so two frames that draw the same row compare equal however their
    /// dots were assigned.
    fn drawn(frame: &[DotFrame]) -> Vec<(f32, f32, f32, f32)> {
        let mut dots: Vec<_> = frame
            .iter()
            .filter(|dot| dot.alpha > 0.0)
            .map(|dot| (dot.x, dot.y, dot.fill(), dot.alpha))
            .collect();
        dots.sort_by(|a, b| a.0.total_cmp(&b.0));
        dots
    }

    fn assert_same_row(actual: &[DotFrame], expected: &[DotFrame], what: &str) {
        let (actual, expected) = (drawn(actual), drawn(expected));
        assert_eq!(
            actual.len(),
            expected.len(),
            "{what}: {actual:?} {expected:?}"
        );
        for (a, e) in actual.iter().zip(&expected) {
            assert!(
                (a.0 - e.0).abs() < EPSILON
                    && (a.1 - e.1).abs() < EPSILON
                    && (a.2 - e.2).abs() < EPSILON
                    && (a.3 - e.3).abs() < EPSILON,
                "{what}: {actual:?} is not {expected:?}"
            );
        }
    }

    const MOTIONS: [RowMotion; 7] = [
        RowMotion::Add { held: 0 },
        RowMotion::Add { held: 1 },
        RowMotion::Add { held: 2 },
        RowMotion::RollForward,
        RowMotion::Refuse { held: 1 },
        RowMotion::Refuse { held: 2 },
        RowMotion::Refuse { held: 3 },
    ];

    /// Samples a motion from its start to a little past its end.
    fn samples(motion: RowMotion) -> impl Iterator<Item = (f32, Vec<DotFrame>)> {
        let steps = 600;
        (0..=steps).map(move |step| {
            let elapsed = motion.seconds() * 1.05 * step as f32 / steps as f32;
            (elapsed, motion.frame(elapsed))
        })
    }

    // ---- The row, at rest and in motion ------------------------------------

    #[test]
    fn at_rest_made_selections_sit_toward_the_board_and_the_rest_against_the_line() {
        assert!(far(1) < LINE && LINE < near(1));
        for held in 0..=LIMIT {
            let row = at_rest(held);
            assert_eq!(row.len(), LIMIT, "three dots at rest, whatever is held");
            let filled: Vec<f32> = row
                .iter()
                .filter(|dot| dot.fill() == 1.0)
                .map(|dot| dot.x)
                .collect();
            let empty: Vec<f32> = row
                .iter()
                .filter(|dot| dot.fill() == 0.0)
                .map(|dot| dot.x)
                .collect();
            assert_eq!(filled, (1..=held).map(near).collect::<Vec<_>>());
            assert_eq!(empty, (1..=LIMIT - held).map(far).collect::<Vec<_>>());
            assert!(row.iter().all(|dot| dot.y == 0.0 && dot.alpha == 1.0));
        }
        // The near side is a queue: the newest beside the line, the oldest
        // at the far end of it.
        assert!(near(1) < near(2) && near(2) < near(LIMIT));
    }

    #[test]
    fn every_motion_starts_from_the_row_before_it_and_ends_at_the_row_after_it() {
        for motion in MOTIONS {
            let (before, after) = match motion {
                RowMotion::Add { held } => (at_rest(held), at_rest(held + 1)),
                RowMotion::RollForward => (at_rest(LIMIT), at_rest(LIMIT)),
                RowMotion::Refuse { held } => (at_rest(held), at_rest(held)),
            };
            let mut start = before.clone();
            if motion == (RowMotion::Refuse { held: LIMIT }) {
                // A refused fourth is beside the line from its first frame.
                start.push(DotFrame::at(far(1)));
            }
            assert_same_row(&motion.frame(0.0), &start, &format!("{motion:?} start"));
            assert_same_row(
                &motion.frame(motion.seconds()),
                &after,
                &format!("{motion:?} end"),
            );
            assert!(
                motion.seconds() <= LONGEST_SECONDS,
                "{motion:?} takes {}",
                motion.seconds()
            );
        }
    }

    #[test]
    fn a_dot_fills_as_it_crosses_the_line_and_only_then() {
        let mut last_fill = 0.0;
        for (_, frame) in samples(RowMotion::Add { held: 1 }) {
            let crossing = frame[0];
            if crossing.x <= LINE - RADIUS {
                assert_eq!(crossing.fill(), 0.0, "{crossing:?}");
            }
            if crossing.x >= LINE + RADIUS {
                assert_eq!(crossing.fill(), 1.0, "{crossing:?}");
            }
            assert!(crossing.fill() >= last_fill, "it never empties on the way");
            last_fill = crossing.fill();
        }
        assert_eq!(last_fill, 1.0);
    }

    /// A sliding dot meets the held dots at their own spacing and pushes
    /// them from there: never closer than a pitch, and never moving them
    /// before it arrives. And its speed is continuous across the meeting.
    #[test]
    fn a_sliding_dot_pushes_the_held_dots_from_a_pitch_away() {
        for motion in [RowMotion::Add { held: 2 }, RowMotion::RollForward] {
            for (elapsed, frame) in samples(motion) {
                let arriving = frame[0];
                let newest = frame[1];
                assert!(
                    newest.x - arriving.x >= PITCH - EPSILON,
                    "{motion:?} at {elapsed}: {frame:?}"
                );
                if newest.x - arriving.x > PITCH + EPSILON {
                    assert_eq!(newest.x, near(1), "pushed before it arrived");
                }
            }
        }
        let (from, to) = (far(LIMIT), near(1));
        let peak = 2.0 * (to - from);
        let step = 1e-3;
        let mut u = 0.0;
        while u < 1.0 {
            let speed = (slide(from, to, u + step) - slide(from, to, u)) / step;
            assert!(speed >= 0.0 && speed <= peak * 1.01, "{speed} at {u}");
            u += step;
        }
    }

    /// The clack plays in the frame the duplicate is rejected, so the
    /// collision has to land within what a viewer accepts as the same
    /// moment - and must be a touch, never an overlap.
    #[test]
    fn a_duplicates_collision_meets_its_clack() {
        for held in 1..=LIMIT {
            let motion = RowMotion::Refuse { held };
            // Centre to centre, the newest held dot and the refused one.
            // The refused dot is drawn last, and the newest held dot first.
            let apart = |elapsed: f32| {
                let frame = motion.frame(elapsed);
                assert_eq!(frame.len(), LIMIT.max(held + 1), "{held} held: {frame:?}");
                frame[0].x - frame[frame.len() - 1].x
            };
            for (elapsed, _) in samples(motion).filter(|(elapsed, _)| *elapsed < BOUNCE_SECONDS) {
                assert!(
                    apart(elapsed) >= DOT - EPSILON,
                    "{held} held, {elapsed}: the dots overlap"
                );
                if elapsed < APPROACH_SECONDS {
                    assert!(
                        apart(elapsed) > DOT,
                        "{held} held, {elapsed}: touched early"
                    );
                }
            }
            assert!(
                (apart(APPROACH_SECONDS) - DOT).abs() < EPSILON,
                "they touch at the end of the approach"
            );
            // And the end of the approach is held under how far sound may
            // lead picture by a compile-time assertion beside the constants.
            // At the moment of contact it is partly across, and it is
            // empty again by the time it has fallen back.
            let at_contact = *motion.frame(APPROACH_SECONDS).last().unwrap();
            assert!(at_contact.fill() > 0.0 && at_contact.fill() < 1.0);
            let fallen = *motion
                .frame(APPROACH_SECONDS + 0.99 * BOUNCE_SECONDS)
                .last()
                .unwrap();
            assert_eq!(fallen.fill(), 0.0);
            assert!(fallen.y > 0.0, "it falls below the row");
        }
    }

    #[test]
    fn a_roll_forward_drops_the_oldest_off_the_end_and_it_fades() {
        let motion = RowMotion::RollForward;
        let arriving = motion.frame(ARRIVE_SECONDS / 2.0)[0];
        assert_eq!(arriving.x, far(LIMIT), "the fourth arrives at the far end");
        assert!(arriving.alpha > 0.0 && arriving.alpha < 1.0);

        let starts = roll_fall_starts();
        let oldest = |elapsed: f32| motion.frame(elapsed).get(LIMIT).copied();
        let edge = oldest(starts).unwrap();
        assert!((edge.x - (near(LIMIT) + PITCH / 2.0)).abs() < EPSILON);
        assert_eq!(edge.y, 0.0);
        let mut last = edge;
        for step in 1..20 {
            let falling = oldest(starts + FALL_SECONDS * step as f32 / 20.0).unwrap();
            assert!(falling.y > last.y && falling.alpha < last.alpha);
            last = falling;
        }
        assert_eq!(oldest(starts + FALL_SECONDS), None, "and it is gone");
    }

    #[test]
    fn the_two_rows_mirror_each_other_about_the_board() {
        for along in [0.0, far(1), LINE, near(LIMIT)] {
            assert_eq!(
                column(Player::One, along) + column(Player::Two, along),
                CONTROL_HINT_WIDTH
            );
        }
        // Player Two's row is left of the board, so its near side is on
        // the right; Player One's is the other way round.
        assert!(column(Player::Two, near(1)) > column(Player::Two, far(1)));
        assert!(column(Player::One, near(1)) < column(Player::One, far(1)));
    }

    #[test]
    fn a_new_attempt_interrupts_and_a_record_the_count_contradicts_draws_nothing_moving() {
        let attempt = |number, outcome| Some(OpeningAttempt { number, outcome });
        let mut row = RowState::default();
        row.advance(attempt(1, OpeningAttemptOutcome::Added), 1, 0.016, false);
        assert_eq!(row.playing, Some((RowMotion::Add { held: 0 }, 0.0)));
        row.advance(attempt(1, OpeningAttemptOutcome::Added), 1, 0.1, false);
        assert_eq!(row.playing, Some((RowMotion::Add { held: 0 }, 0.1)));
        row.advance(
            attempt(2, OpeningAttemptOutcome::Duplicate),
            1,
            0.016,
            false,
        );
        assert_eq!(
            row.playing,
            Some((RowMotion::Refuse { held: 1 }, 0.0)),
            "a new attempt starts at its own beginning, in its own frame"
        );
        row.advance(attempt(2, OpeningAttemptOutcome::Duplicate), 1, 0.1, true);
        assert_eq!(row.playing, None, "reduced motion stops it");

        assert_eq!(RowMotion::of(OpeningAttemptOutcome::Added, 0), None);
        assert_eq!(RowMotion::of(OpeningAttemptOutcome::Duplicate, 0), None);
    }

    // ---- The entities ------------------------------------------------------

    fn dots_test_app(reduce_motion: bool) -> App {
        let mut app = App::new();
        app.insert_resource(GameState::default())
            .insert_resource(GamePreferences {
                reduce_motion,
                ..GamePreferences::default()
            })
            .insert_resource(MotionPolicy::default())
            .insert_resource(Time::<()>::default())
            .init_resource::<WindowSize>()
            .init_resource::<OpeningDotRows>()
            .add_systems(Startup, spawn_opening_dots)
            .add_systems(Update, update_opening_dots);
        app.update();
        app
    }

    /// Selects `(x, y)` as `player` through the authority, as a Select
    /// press does, without running a frame.
    fn select(app: &mut App, player: Player, x: usize, y: usize) {
        let mut game = app.world_mut().resource_mut::<GameState>();
        game.cursors[player_index(player)] = Point::new(x, y);
        game.try_opening_select(player);
    }

    /// What one player's row draws: its visible dots as `(left, top,
    /// fill)`, in order across the screen, and whether the line and the
    /// cross are on show.
    #[derive(Debug, PartialEq)]
    struct DrawnRow {
        dots: Vec<(f32, f32, Color)>,
        line: bool,
        cross: bool,
    }

    fn row(app: &mut App, player: Player) -> DrawnRow {
        let px = |value: Val| match value {
            Val::Px(px) => px,
            other => panic!("dots are placed in pixels, not {other:?}"),
        };
        let mut dots: Vec<_> = app
            .world_mut()
            .query::<(&OpeningDot, &Node, &BackgroundColor, &Visibility)>()
            .iter(app.world())
            .filter(|(dot, _, _, visibility)| {
                dot.player == player && **visibility != Visibility::Hidden
            })
            .map(|(_, node, background, _)| (px(node.left), px(node.top), background.0))
            .collect();
        dots.sort_by(|a, b| a.0.total_cmp(&b.0));
        let marks: Vec<_> = app
            .world_mut()
            .query::<(&RowMark, &Visibility)>()
            .iter(app.world())
            .filter(|(mark, _)| mark.player == player)
            .map(|(mark, visibility)| (mark.cross, *visibility != Visibility::Hidden))
            .collect();
        DrawnRow {
            dots,
            line: marks.iter().any(|(cross, shown)| !cross && *shown),
            cross: marks
                .iter()
                .filter(|(cross, shown)| *cross && *shown)
                .count()
                == 2,
        }
    }

    fn filled(row: &DrawnRow) -> usize {
        row.dots
            .iter()
            .filter(|(.., fill)| *fill != DEEP_SURFACE_BACKGROUND)
            .count()
    }

    #[test]
    fn both_rows_spawn_resting_and_empty_with_a_line_and_no_cross() {
        let mut app = dots_test_app(false);
        let world = app.world_mut();
        assert_eq!(world.query::<&OpeningDots>().iter(world).count(), 2);
        assert_eq!(world.query::<&OpeningDot>().iter(world).count(), 8);
        assert_eq!(world.query::<&RowMark>().iter(world).count(), 6);
        assert_eq!(
            world
                .query_filtered::<Entity, (With<OpeningDot>, With<Text>)>()
                .iter(world)
                .count(),
            0,
            "a dot says nothing in words"
        );
        for player in Player::ALL {
            let drawn = row(&mut app, player);
            assert_eq!(drawn.dots.len(), LIMIT);
            assert_eq!(filled(&drawn), 0);
            assert!(drawn.line && !drawn.cross, "{drawn:?}");
        }
    }

    /// All four kinds of Round 0 selection - a new point or a duplicate,
    /// with fewer than three held or with three - each as its static form:
    /// with motion reduced, every selection is a step to the row it leaves,
    /// and the cross is there exactly when the last selection changed
    /// nothing.
    #[test]
    fn with_motion_reduced_the_cross_alone_says_a_selection_changed_nothing() {
        let mut app = dots_test_app(true);
        let step = |app: &mut App, x: usize, y: usize| {
            select(app, Player::One, x, y);
            advance_test_app(app, 1.0 / 60.0);
            let drawn = row(app, Player::One);
            // Motion is reduced, so a second frame draws exactly the first.
            advance_test_app(app, 1.0 / 60.0);
            assert_eq!(row(app, Player::One), drawn, "nothing moves");
            drawn
        };

        let accepted_below_three = step(&mut app, 0, 0);
        assert_eq!(filled(&accepted_below_three), 1);
        assert!(accepted_below_three.line && !accepted_below_three.cross);

        let refused_below_three = step(&mut app, 0, 0);
        assert_eq!(filled(&refused_below_three), 1);
        assert!(refused_below_three.cross && !refused_below_three.line);
        assert_eq!(refused_below_three.dots, accepted_below_three.dots);

        step(&mut app, 1, 0);
        let third = step(&mut app, 2, 0);
        assert_eq!(filled(&third), 3);

        let refused_at_three = step(&mut app, 1, 0);
        assert!(refused_at_three.cross && !refused_at_three.line);
        let rolled_forward = step(&mut app, 3, 0);
        assert!(rolled_forward.line && !rolled_forward.cross);
        // The difference between the two is the mark alone: the dots are
        // the same three, in the same places.
        assert_eq!(rolled_forward.dots, refused_at_three.dots);
        assert_eq!(rolled_forward.dots, third.dots);

        assert_eq!(
            row(&mut app, Player::Two),
            DrawnRow {
                dots: row(&mut app, Player::Two).dots,
                line: true,
                cross: false,
            },
            "the other player's row is untouched"
        );
        assert_eq!(filled(&row(&mut app, Player::Two)), 0);
    }

    #[test]
    fn with_motion_a_duplicate_strikes_the_newest_dot_and_the_cross_shows_at_once() {
        let mut app = dots_test_app(false);
        select(&mut app, Player::Two, 0, 0);
        select(&mut app, Player::Two, 1, 0);
        // The frame a selection is first seen draws its motion's first
        // frame; the next one is a second further on, and at rest.
        advance_test_app(&mut app, 1.0 / 60.0);
        advance_test_app(&mut app, 1.0);
        let resting = row(&mut app, Player::Two);
        assert_eq!(filled(&resting), 2);

        select(&mut app, Player::Two, 1, 0);
        advance_test_app(&mut app, 1.0 / 60.0);
        let first = row(&mut app, Player::Two);
        assert!(
            first.cross,
            "the cross is the static form, not the animation's"
        );
        assert_eq!(
            first.dots, resting.dots,
            "the motion's first frame is the row at rest"
        );

        advance_test_app(&mut app, APPROACH_SECONDS);
        let struck = row(&mut app, Player::Two);
        assert_ne!(struck.dots, resting.dots, "with motion allowed, it moves");

        advance_test_app(&mut app, 1.0);
        advance_test_app(&mut app, 1.0 / 60.0);
        let after = row(&mut app, Player::Two);
        assert_eq!(after.dots, resting.dots, "and comes back to rest");
        assert!(after.cross);
    }

    /// Re-selecting the oldest held point and re-selecting the newest draw
    /// the same row in every frame: the motion cannot say which it was.
    #[test]
    fn a_duplicate_looks_the_same_whichever_held_point_it_repeated() {
        let mut apps = [dots_test_app(false), dots_test_app(false)];
        for app in &mut apps {
            for x in 0..3 {
                select(app, Player::One, x, 4);
            }
            advance_test_app(app, 1.0);
        }
        select(&mut apps[0], Player::One, 0, 4);
        select(&mut apps[1], Player::One, 2, 4);
        for _ in 0..40 {
            let rows: Vec<_> = apps
                .iter_mut()
                .map(|app| {
                    advance_test_app(app, 1.0 / 60.0);
                    row(app, Player::One)
                })
                .collect();
            assert_eq!(rows[0], rows[1]);
        }
    }

    #[test]
    fn a_selection_mid_motion_starts_its_own_from_the_row_at_rest() {
        let mut app = dots_test_app(false);
        select(&mut app, Player::One, 0, 0);
        advance_test_app(&mut app, 1.0 / 60.0);
        advance_test_app(&mut app, ADD_SECONDS / 2.0);
        assert!(
            app.world().resource::<OpeningDotRows>().0[0]
                .playing
                .is_some()
        );

        select(&mut app, Player::One, 0, 0);
        advance_test_app(&mut app, 1.0 / 60.0);
        assert_eq!(
            app.world().resource::<OpeningDotRows>().0[0].playing,
            Some((RowMotion::Refuse { held: 1 }, 0.0))
        );
        let interrupted = row(&mut app, Player::One);
        let mut settled = dots_test_app(true);
        select(&mut settled, Player::One, 0, 0);
        advance_test_app(&mut settled, 1.0 / 60.0);
        assert_eq!(
            interrupted.dots,
            row(&mut settled, Player::One).dots,
            "the interrupted motion jumps to the end it was heading for"
        );
    }

    #[test]
    fn the_rows_show_only_in_round_zero_and_a_new_match_starts_them_empty() {
        let mut app = dots_test_app(true);
        select(&mut app, Player::One, 0, 0);
        select(&mut app, Player::One, 0, 0);
        app.update();
        assert!(row(&mut app, Player::One).cross);

        app.world_mut().resource_mut::<GameState>().phase = MatchPhase::OpeningCountdown;
        app.update();
        let shown = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&Visibility, With<OpeningDots>>()
                .iter(app.world())
                .all(|visibility| *visibility == Visibility::Visible)
        };
        assert!(shown(&mut app));
        app.world_mut()
            .resource_mut::<GameState>()
            .reveal_round_zero();
        app.update();
        assert!(
            app.world_mut()
                .query_filtered::<&Visibility, With<OpeningDots>>()
                .iter(app.world())
                .all(|visibility| *visibility == Visibility::Hidden)
        );

        *app.world_mut().resource_mut::<GameState>() = GameState::default();
        app.update();
        assert!(shown(&mut app));
        let fresh = row(&mut app, Player::One);
        assert_eq!(filled(&fresh), 0);
        assert!(fresh.line && !fresh.cross);
        assert_eq!(app.world().resource::<OpeningDotRows>().0[0].shown, None);
    }

    #[test]
    fn a_resting_row_writes_nothing_and_a_selection_writes_only_its_own_row() {
        #[derive(Resource, Default)]
        struct Writes(Vec<Player>);
        type Changed<'w, 's> = Query<
            'w,
            's,
            &'static OpeningDot,
            Or<(
                bevy::prelude::Changed<Node>,
                bevy::prelude::Changed<BackgroundColor>,
                bevy::prelude::Changed<BorderColor>,
                bevy::prelude::Changed<Visibility>,
            )>,
        >;
        fn count_writes(mut writes: ResMut<Writes>, changed: Changed) {
            writes.0 = changed.iter().map(|dot| dot.player).collect();
        }
        let mut app = dots_test_app(true);
        app.init_resource::<Writes>()
            .add_systems(Update, count_writes.after(update_opening_dots));
        app.update();
        app.update();
        assert!(app.world().resource::<Writes>().0.is_empty());

        select(&mut app, Player::Two, 4, 4);
        app.update();
        let writes = &app.world().resource::<Writes>().0;
        assert!(!writes.is_empty());
        assert!(writes.iter().all(|player| *player == Player::Two));
        app.update();
        assert!(app.world().resource::<Writes>().0.is_empty());
    }

    #[test]
    fn re_entering_the_screen_forgets_what_was_shown() {
        let mut app = dots_test_app(false);
        select(&mut app, Player::One, 0, 0);
        app.update();
        assert!(
            app.world().resource::<OpeningDotRows>().0[0]
                .shown
                .is_some()
        );
        app.world_mut()
            .run_system_once(spawn_opening_dots)
            .expect("the spawner runs again");
        assert_eq!(
            app.world().resource::<OpeningDotRows>().0[0],
            RowState::default()
        );
    }

    #[test]
    fn opening_dot_queries_initialize_without_component_conflicts() {
        IntoSystem::into_system(update_opening_dots).initialize(&mut World::new());
    }
}
