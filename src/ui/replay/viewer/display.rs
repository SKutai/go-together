//! Keeps every viewer entity in sync with the frame currently being replayed.
//!
//! [`PlaybackController`](crate::replay::PlaybackController) holds the
//! timeline position and [`Replay::playback_frame_at`] resolves it to a
//! recorded [`ReplayFrame`], and the systems here restyle the entities
//! [`super::layout`] and [`super::modals`] already spawned to match it. They
//! never spawn, never despawn and never move playback.
//!
//! # The board redraws on event boundaries, not every frame
//!
//! The board, its stone rings, cursors, markers and selections are entirely
//! determined by which recorded event is currently in effect - they never
//! change value between two events, only `round_elapsed_seconds` does. The
//! score badges are too, and by the window's size as well: they are drawn
//! at their own size whatever the board's zoom and placed in window pixels
//! off their cursors, so where they stand in the world moves with the
//! board's framing. [`update_viewer_display`] caches the last-drawn
//! `(replay_id, boundary key, window size)` and only calls
//! [`update_viewer_board_visuals`] when that changes or the board has just
//! been spawned, redrawing immediately (no one-frame lag) on any crossing,
//! including a silent seek, on any change of the window's size, and on
//! every entry to the screen. See
//! [Presentation: continuous vs change-driven display](../../../../docs/ARCHITECTURE.md#presentation-continuous-vs-change-driven-display)
//! for the full reasoning, including why `replay_id` has to be part of the
//! cache key and why a new board has to be drawn whatever the key.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use super::geometry::{
    ViewerLayout, score_badge_position, score_badge_scale, viewer_board_position,
    viewer_cursor_position, viewer_selection_position,
};
use super::layout::{current_layout, speed_label};
use super::{
    REPLAY_FIELD_SEPARATOR, ReplayViewerState, ViewerAction, ViewerButtonLabel, ViewerCountdown,
    ViewerCursor, ViewerIntersection, ViewerKeyModal, ViewerMarker, ViewerMatchSettingsModal,
    ViewerScoreBadge, ViewerSelection, ViewerStoneRing, ViewerText, format_setting_duration,
    player_index, player_score, viewer_action_disabled,
};
use crate::app::visuals::{StonePalette, WindowSize, format_elapsed_time, stone_geometry};
use crate::audio::UiSound;
use crate::match_settings::MatchSettingField;
use crate::presentation::{
    BLUE_BORDER, BUTTON_DISABLED_BORDER, Highlight, ORANGE, TEXT_MUTED, TEXT_PRIMARY,
    button_background,
};
use crate::replay::{MATCH_RULES_REPLAY_SCHEMA_VERSION, Replay, ReplayFrame, ReplayPhase};
use crate::replay_library::format_local_timestamp;
use crate::rules::{Player, Point};
use crate::ui::button::PointerHighlight;
use crate::ui::widgets::selected_label;
/// One entity per intersection, restyled from the reconstructed frame.
type ViewerStones<'w, 's> = Query<
    'w,
    's,
    (
        &'static ViewerIntersection,
        &'static mut MeshMaterial2d<ColorMaterial>,
        &'static mut Transform,
    ),
>;

/// The ring behind each intersection, shown only under a white or gray stone.
///
/// It names every other board marker in its filter rather than adding itself
/// to theirs: each of those queries requires its own marker, so this one
/// excluding all five is enough to prove it shares no entity with any.
type ViewerStoneRings<'w, 's> = Query<
    'w,
    's,
    (
        &'static ViewerStoneRing,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (
        Without<ViewerIntersection>,
        Without<ViewerCursor>,
        Without<ViewerScoreBadge>,
        Without<ViewerMarker>,
        Without<ViewerSelection>,
    ),
>;

/// Both replay cursors.
type ViewerCursors<'w, 's> = Query<
    'w,
    's,
    (&'static ViewerCursor, &'static mut Transform),
    (
        Without<ViewerIntersection>,
        Without<ViewerScoreBadge>,
        Without<ViewerMarker>,
        Without<ViewerSelection>,
    ),
>;

/// The score badge that trails each cursor.
type ViewerScoreBadges<'w, 's> = Query<
    'w,
    's,
    (
        &'static ViewerScoreBadge,
        &'static mut Transform,
        &'static Children,
    ),
    (
        Without<ViewerIntersection>,
        Without<ViewerCursor>,
        Without<ViewerMarker>,
        Without<ViewerSelection>,
    ),
>;

/// Teleport Markers, which are replay-only visuals. Each is one entity with
/// its outline's segments as children; this pass moves it and shows or hides
/// it, and never writes the turn it was spawned with.
type ViewerMarkers<'w, 's> = Query<
    'w,
    's,
    (
        &'static ViewerMarker,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (
        Without<ViewerIntersection>,
        Without<ViewerCursor>,
        Without<ViewerScoreBadge>,
        Without<ViewerSelection>,
    ),
>;

/// Hidden selections, also replay-only: the match is over, so nothing is
/// secret any more. Each mark is one entity with its three strokes as
/// children, so placing or hiding it takes all three with it. A slot has
/// two marks - the ordinary one and the smaller one shown when both players
/// selected the point - and this pass shows at most one of them.
type ViewerSelections<'w, 's> = Query<
    'w,
    's,
    (
        &'static ViewerSelection,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (
        Without<ViewerIntersection>,
        Without<ViewerCursor>,
        Without<ViewerScoreBadge>,
        Without<ViewerMarker>,
    ),
>;

/// Everything drawn on the replay board.
///
/// The mutually exclusive `Without` filters are load bearing: all of these
/// entities carry a `Transform`, so Bevy needs proof that no two queries can
/// ever match the same entity before it will run the system.
#[derive(SystemParam)]
pub(super) struct ViewerBoard<'w, 's> {
    intersections: ViewerStones<'w, 's>,
    rings: ViewerStoneRings<'w, 's>,
    cursors: ViewerCursors<'w, 's>,
    score_badges: ViewerScoreBadges<'w, 's>,
    badge_texts: Query<'w, 's, &'static mut Text2d>,
    markers: ViewerMarkers<'w, 's>,
    selections: ViewerSelections<'w, 's>,
    /// Matches on the frame the board was spawned, which spawns its marks
    /// with it: see [`ViewerBoardBoundary`].
    spawned: Query<'w, 's, (), Added<ViewerScoreBadge>>,
}

/// Transport buttons, including the child label whose colour tracks the
/// button's enabled state and whether the button currently carries
/// [`InteractionDisabled`].
type ViewerButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        PointerHighlight,
        &'static ViewerAction,
        &'static Children,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        &'static mut UiSound,
        Has<InteractionDisabled>,
    ),
>;

type ViewerCountdownNode<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<ViewerCountdown>,
        Without<ViewerKeyModal>,
        Without<ViewerMatchSettingsModal>,
    ),
>;

/// The header, transport bar, Round 0 countdown, and the Key and Match
/// Settings modals that frame the board.
#[derive(SystemParam)]
pub(super) struct ViewerOverlay<'w, 's> {
    texts: Query<'w, 's, (&'static ViewerText, &'static mut Text)>,
    buttons: ViewerButtons<'w, 's>,
    button_labels: Query<'w, 's, &'static mut TextColor, With<ViewerButtonLabel>>,
    /// The labels whose words never change with playback - every control's
    /// but Play's - of which only the speeds' are rewritten, to bracket the
    /// armed one.
    fixed_labels: Query<'w, 's, &'static mut Text, (With<ViewerButtonLabel>, Without<ViewerText>)>,
    key_modal:
        Query<'w, 's, &'static mut Node, (With<ViewerKeyModal>, Without<ViewerMatchSettingsModal>)>,
    match_settings_modal:
        Query<'w, 's, &'static mut Node, (With<ViewerMatchSettingsModal>, Without<ViewerKeyModal>)>,
    countdown: ViewerCountdownNode<'w, 's>,
}

/// The board, cursors, markers, selections and score badges only ever change
/// value at a recorded event boundary (see [`Replay::playback_boundary_key`]),
/// never while playback merely interpolates `round_elapsed_seconds` between
/// two events - or, for the score badges, when the window's size changes,
/// since they are placed in its pixels. Caching the last-drawn boundary and
/// size here lets [`update_viewer_display`] skip re-touching all of those
/// entities on every one of the many frames that fall between two
/// boundaries, while still redrawing immediately, with no one-frame lag, the
/// instant playback actually crosses one, including on a silent seek, or
/// the window changes size.
///
/// The Viewer only runs while `AppScreen::ReplayViewer` is active, so
/// leaving the screen (which clears `ReplayViewerState`) does not itself run
/// this system again to reset the cache, and two things follow from that.
/// The cached key includes `replay_id`, not just the event index: without
/// it, opening a *different* Replay that happens to land on the same event
/// index as whatever was last drawn would keep showing the previous
/// Replay's board. And a board just spawned is drawn whatever the key:
/// every entry spawns the board afresh, its cursors and badges at the
/// world's origin, so opening the *same* Replay again, at the start where
/// it was left, would otherwise match the key and leave them there until
/// playback crossed an event.
///
/// The comparison borrows this cached key rather than rebuilding it, so the
/// steady-state frame allocates nothing; `replay_id` is cloned only when the
/// key actually changes and is stored. The gate saves 180 entity iterations:
/// 81 intersections and their 81 rings, two cursors, two score badges, two
/// markers and the twelve selection marks. Their writes are already
/// compare-guarded inside [`update_viewer_board_visuals`], so paying two
/// `String` allocations per frame to reach it would have been at best a wash.
#[derive(Default)]
pub(super) struct ViewerBoardBoundary(Option<(Option<String>, Option<usize>, Vec2)>);

pub(super) fn update_viewer_display(
    mut commands: Commands,
    viewer: Res<ReplayViewerState>,
    window: Res<WindowSize>,
    palette: Res<StonePalette>,
    mut board: ViewerBoard,
    mut overlay: ViewerOverlay,
    mut last_boundary: Local<ViewerBoardBoundary>,
) {
    let Some(replay) = viewer.replay.as_ref() else {
        last_boundary.0 = None;
        return;
    };
    let position = viewer.controller.position_seconds;
    let frame = replay.playback_frame_at(position);
    let boundary = replay.playback_boundary_key(position);
    let size = window.layout();
    let unchanged = board.spawned.is_empty()
        && last_boundary
            .0
            .as_ref()
            .is_some_and(|(id, index, drawn_at)| {
                id.as_deref() == viewer.replay_id.as_deref()
                    && *index == boundary
                    && *drawn_at == size
            });
    if !unchanged {
        last_boundary.0 = Some((viewer.replay_id.clone(), boundary, size));
        let layout = current_layout(&window, replay);
        update_viewer_board_visuals(replay, &frame, &palette, &layout, &mut board);
    }
    for (role, mut text) in &mut overlay.texts {
        let value = match role {
            ViewerText::Label => replay.label.clone(),
            ViewerText::Timer => {
                viewer_timer_text(viewer.controller.position_seconds, replay.duration_seconds)
            }
            ViewerText::Round => viewer_round_text(replay, &frame),
            ViewerText::Result => replay_result_text(replay),
            ViewerText::Countdown => viewer_countdown_text(&frame).into(),
            ViewerText::PlayPause => {
                if viewer.controller.playing {
                    "Pause".into()
                } else {
                    "Play".into()
                }
            }
        };
        if **text != value {
            **text = value;
        }
    }
    // The pointer's highlight (hover/press) can change every frame with no
    // board or playback change at all, so button styling stays a true per-frame
    // system for responsiveness - only the actual write is guarded.
    for (
        entity,
        highlight,
        action,
        children,
        mut background,
        mut border,
        mut sound,
        was_disabled,
    ) in &mut overlay.buttons
    {
        let disabled = viewer_action_disabled(*action, &viewer, replay);
        // A disabled control is not in the focus ring. The focus core
        // filters `bevy_ui`'s `InteractionDisabled`, so the same
        // computation that greys a button also takes it out of the ring
        // and puts it back - written only on change, as it is an archetype
        // move. The pointer reads the same marker: `ui::button` neither
        // presses nor activates a button that carries it.
        if disabled != was_disabled {
            if disabled {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
        let selected = !disabled
            && (matches!(action, ViewerAction::Speed(speed) if *speed == viewer.controller.speed)
                || (*action == ViewerAction::PlayPause && viewer.controller.playing));
        let (target_background, target_border, target_label) =
            viewer_button_colors(highlight.get(), selected, disabled);
        if background.0 != target_background {
            background.0 = target_background;
        }
        let target_border = BorderColor::all(target_border);
        if *border != target_border {
            *border = target_border;
        }
        sound.set_enabled(!disabled);
        for child in children.iter() {
            if let Ok(mut color) = overlay.button_labels.get_mut(child)
                && color.0 != target_label
            {
                color.0 = target_label;
            }
            if let ViewerAction::Speed(speed) = *action
                && let Ok(mut text) = overlay.fixed_labels.get_mut(child)
            {
                let label = selected_label(speed_label(speed), selected);
                if **text != label {
                    **text = label;
                }
            }
        }
    }
    let key_modal_display = if viewer.key_open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut overlay.key_modal {
        if node.display != key_modal_display {
            node.display = key_modal_display;
        }
    }
    let match_settings_modal_display = if viewer.match_settings_open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut overlay.match_settings_modal {
        if node.display != match_settings_modal_display {
            node.display = match_settings_modal_display;
        }
    }
    let countdown_display = if frame.opening_countdown.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut overlay.countdown {
        if node.display != countdown_display {
            node.display = countdown_display;
        }
    }
}

/// Fill, border and label colour of a viewer control.
///
/// `selected` is the armed speed or a running Play, and is carried on the
/// border as well as the fill - orange in place of blue, as every other
/// screen marks its active control. Both are hue, which is why the armed
/// speed's label is bracketed as well, and Play's reads Pause; the display
/// pass writes those. A disabled control keeps a label a player can still
/// read: it says what is unavailable.
fn viewer_button_colors(
    highlight: Highlight,
    selected: bool,
    disabled: bool,
) -> (Color, Color, Color) {
    let border = if disabled {
        BUTTON_DISABLED_BORDER
    } else if selected {
        ORANGE
    } else {
        BLUE_BORDER
    };
    let label = if disabled { TEXT_MUTED } else { TEXT_PRIMARY };
    (
        button_background(highlight, selected, disabled),
        border,
        label,
    )
}

/// The discrete board visuals: stones, cursors, score badges, teleport
/// markers and hidden selections, the badges placed and scaled for
/// `layout`. Called only when [`Replay::playback_boundary_key`] or the
/// window's size changes, or the board has just been spawned - see
/// [`ViewerBoardBoundary`].
fn update_viewer_board_visuals(
    replay: &Replay,
    frame: &ReplayFrame,
    palette: &StonePalette,
    layout: &ViewerLayout,
    board: &mut ViewerBoard,
) {
    for (intersection, mut material, mut transform) in &mut board.intersections {
        let cell = frame.board.get(intersection.0);
        let handle = palette.fill(cell);
        if material.0 != *handle {
            material.0 = handle.clone();
        }
        let target_scale = Vec3::splat(stone_geometry(cell).fill_scale);
        if transform.scale != target_scale {
            transform.scale = target_scale;
        }
    }
    for (ring, mut transform, mut visibility) in &mut board.rings {
        let ring_scale = stone_geometry(frame.board.get(ring.0)).ring_scale;
        if let Some(scale) = ring_scale {
            let target_scale = Vec3::splat(scale);
            if transform.scale != target_scale {
                transform.scale = target_scale;
            }
        }
        let target_visibility = if ring_scale.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != target_visibility {
            *visibility = target_visibility;
        }
    }
    for (cursor, mut transform) in &mut board.cursors {
        let position = viewer_cursor_position(frame, cursor.0);
        if transform.translation.x != position.x || transform.translation.y != position.y {
            transform.translation.x = position.x;
            transform.translation.y = position.y;
        }
    }
    let cursors_overlap = frame.cursors[0] == frame.cursors[1];
    let score = frame.current_score(&replay.match_settings);
    let badge_scale = score_badge_scale(layout);
    for (badge, mut transform, children) in &mut board.score_badges {
        let cursor_position = viewer_cursor_position(frame, badge.0);
        let position = score_badge_position(cursor_position, badge.0, cursors_overlap, layout);
        if transform.translation.x != position.x || transform.translation.y != position.y {
            transform.translation.x = position.x;
            transform.translation.y = position.y;
        }
        if transform.scale != badge_scale {
            transform.scale = badge_scale;
        }
        let value = player_score(score, badge.0).to_string();
        for child in children.iter() {
            if let Ok(mut text) = board.badge_texts.get_mut(child)
                && **text != value
            {
                **text = value.clone();
            }
        }
    }
    for (marker, mut transform, mut visibility) in &mut board.markers {
        if let Some(point) = frame.teleport_markers[player_index(marker.0)] {
            let position = viewer_board_position(point);
            if transform.translation.x != position.x || transform.translation.y != position.y {
                transform.translation.x = position.x;
                transform.translation.y = position.y;
            }
            if *visibility != Visibility::Visible {
                *visibility = Visibility::Visible;
            }
        } else if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
    }
    for (selection, mut transform, mut visibility) in &mut board.selections {
        // Each slot has an ordinary mark and a smaller shared one; whichever
        // the frame calls for is placed and shown, and the other hidden.
        let shown = viewer_selection_point(frame, selection.player, selection.slot)
            .map(|point| {
                (
                    point,
                    viewer_selection_overlaps(frame, selection.player, point),
                )
            })
            .filter(|(_, shared)| *shared == selection.shared);
        if let Some((point, shared)) = shown {
            let position = viewer_selection_position(point, selection.player, shared);
            if transform.translation.x != position.x || transform.translation.y != position.y {
                transform.translation.x = position.x;
                transform.translation.y = position.y;
            }
            if *visibility != Visibility::Visible {
                *visibility = Visibility::Visible;
            }
        } else if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
    }
}

/// The timer under the label: how far into the Replay playback stands, and
/// how long it is.
fn viewer_timer_text(position_seconds: f64, duration_seconds: f64) -> String {
    format!(
        "{} / {}",
        format_elapsed_time(position_seconds),
        format_elapsed_time(duration_seconds)
    )
}

fn viewer_round_text(replay: &Replay, frame: &ReplayFrame) -> String {
    let base = format!("Round {}", frame.current_round);
    if replay.version < MATCH_RULES_REPLAY_SCHEMA_VERSION
        || frame.phase != ReplayPhase::Playing
        || !replay.match_settings.enabled(MatchSettingField::RoundTime)
    {
        return base;
    }
    let limit = f64::from(replay.match_settings.value(MatchSettingField::RoundTime));
    let remaining = (limit - frame.round_elapsed_seconds).max(0.0).ceil() as u16;
    format!("{base} - {} remaining", format_setting_duration(remaining))
}

fn viewer_countdown_text(frame: &ReplayFrame) -> &'static str {
    frame
        .opening_countdown
        .map(crate::opening::OpeningCountdownStage::label)
        .unwrap_or_default()
}

fn viewer_selection_point(frame: &ReplayFrame, player: Player, slot: usize) -> Option<Point> {
    if frame.phase == ReplayPhase::Opening {
        frame.opening_selections[player_index(player)]
            .as_slice()
            .get(slot)
            .copied()
    } else if slot == 0 {
        frame.selected_moves[player_index(player)]
    } else {
        None
    }
}

fn viewer_selection_overlaps(frame: &ReplayFrame, player: Player, point: Point) -> bool {
    let other = match player {
        Player::One => Player::Two,
        Player::Two => Player::One,
    };
    if frame.phase == ReplayPhase::Opening {
        frame.opening_selections[player_index(other)].contains(point)
    } else {
        frame.selected_moves[player_index(other)] == Some(point)
    }
}

fn replay_result_text(replay: &Replay) -> String {
    format!(
        "{}{REPLAY_FIELD_SEPARATOR}Black {} - White {}{REPLAY_FIELD_SEPARATOR}completed {}",
        replay.winner().label(),
        replay.score.black,
        replay.score.white,
        format_local_timestamp(replay.completed_at_ms)
    )
}

#[cfg(test)]
mod tests {
    use super::super::super::library::{replay_match_id_text, replay_summary_text};
    use super::super::super::test_support::fixed_metadata_replay;
    use super::super::layout::PLAYBACK_SPEEDS;
    use super::super::layout::spawn_viewer_screen;
    use super::*;
    use crate::app::visuals::StoneMesh;
    use crate::replay::PlaybackController;
    use crate::rules::{Cell, Score};

    use crate::opening::OpeningCountdownStage;
    use crate::replay::MatchRecorder;

    /// Each longest text the Viewer's layout makes room for in its header
    /// is one this pass writes, built here with the screen's own functions:
    /// the timer at 999:59 of 999:59, the round line in round 9,999 with
    /// Round Time on, and the result line for a winner with three-digit
    /// scores and a four-digit year - character for character but for
    /// which digit stands where, since the result's timestamp is local
    /// time. The round line is as long however much of the round is left,
    /// and as short with Round Time off or in a Replay from before it was
    /// recorded; the result line is no longer for a draw or for the other
    /// winner. And each grows past what the layout names its assumptions
    /// at: 1,000 minutes, round 10,000 and a score of 1,000.
    #[test]
    fn the_layouts_longest_texts_are_the_screens_longest_texts() {
        use super::super::geometry::{LONGEST_RESULT, LONGEST_ROUND, LONGEST_TIMER};
        use crate::match_settings::DURATION_MAX_SECONDS;

        let as_nines = |text: &str| -> String {
            text.chars()
                .map(|c| if c.is_ascii_digit() { '9' } else { c })
                .collect()
        };
        assert_eq!(viewer_timer_text(59_999.0, 59_999.0), LONGEST_TIMER);
        assert!(viewer_timer_text(60_000.0, 60_000.0).len() > LONGEST_TIMER.len());

        let mut replay = fixed_metadata_replay();
        assert!(replay.version >= MATCH_RULES_REPLAY_SCHEMA_VERSION);
        replay.match_settings.set_value(
            MatchSettingField::RoundTime,
            u64::from(DURATION_MAX_SECONDS),
        );
        replay
            .match_settings
            .set_enabled(MatchSettingField::RoundTime, true);
        let playing = |round: usize, elapsed: f64| ReplayFrame {
            phase: ReplayPhase::Playing,
            current_round: round,
            round_elapsed_seconds: elapsed,
            ..default()
        };
        assert_eq!(
            viewer_round_text(&replay, &playing(9_999, 0.0)),
            LONGEST_ROUND
        );
        for elapsed in [0.5, 59.5, 3_599.5, 3_600.0] {
            let round = viewer_round_text(&replay, &playing(9_999, elapsed));
            assert_eq!(as_nines(&round), as_nines(LONGEST_ROUND), "{elapsed}");
        }
        assert!(viewer_round_text(&replay, &playing(10_000, 0.0)).len() > LONGEST_ROUND.len());
        let mut no_round_time = replay.clone();
        no_round_time
            .match_settings
            .set_enabled(MatchSettingField::RoundTime, false);
        let mut before_round_time = replay.clone();
        before_round_time.version = MATCH_RULES_REPLAY_SCHEMA_VERSION - 1;
        for shorter in [no_round_time, before_round_time] {
            let round = viewer_round_text(&shorter, &playing(9_999, 0.0));
            assert!(round.len() < LONGEST_ROUND.len(), "{round:?}");
        }

        replay.completed_at_ms = 1_795_000_000_000;
        replay.score = Score {
            black: 999,
            white: 998,
        };
        assert_eq!(
            as_nines(&replay_result_text(&replay)),
            as_nines(LONGEST_RESULT)
        );
        for (black, white) in [(998, 999), (999, 999)] {
            replay.score = Score { black, white };
            assert!(replay_result_text(&replay).len() <= LONGEST_RESULT.len());
        }
        replay.score = Score {
            black: 1_000,
            white: 999,
        };
        assert!(replay_result_text(&replay).len() > LONGEST_RESULT.len());
    }

    #[test]
    fn round_zero_viewer_exposes_all_hidden_selections_and_countdown_while_live_play_does_not() {
        let mut frame = ReplayFrame {
            cursors: [Point::new(6, 4), Point::new(2, 4)],
            phase: ReplayPhase::Opening,
            current_round: 0,
            opening_countdown: Some(OpeningCountdownStage::Two),
            ..default()
        };
        for point in [Point::new(0, 0), Point::new(1, 0), Point::new(2, 0)] {
            frame.opening_selections[Player::One.index()].add(point);
        }
        for point in [Point::new(1, 0), Point::new(7, 8)] {
            frame.opening_selections[Player::Two.index()].add(point);
        }

        assert_eq!(viewer_countdown_text(&frame), "2");
        assert_eq!(
            (0..3)
                .filter_map(|slot| viewer_selection_point(&frame, Player::One, slot))
                .collect::<Vec<_>>(),
            vec![Point::new(0, 0), Point::new(1, 0), Point::new(2, 0)]
        );
        assert_eq!(
            (0..3)
                .filter_map(|slot| viewer_selection_point(&frame, Player::Two, slot))
                .collect::<Vec<_>>(),
            vec![Point::new(1, 0), Point::new(7, 8)]
        );
        assert!(viewer_selection_overlaps(
            &frame,
            Player::One,
            Point::new(1, 0)
        ));
        assert!(!viewer_selection_overlaps(
            &frame,
            Player::One,
            Point::new(0, 0)
        ));

        frame.phase = ReplayPhase::Playing;
        frame.current_round = 1;
        frame.opening_selections = Default::default();
        frame.opening_countdown = Some(OpeningCountdownStage::Go);
        assert_eq!(viewer_countdown_text(&frame), "Go!");
        assert!((0..3).all(|slot| viewer_selection_point(&frame, Player::One, slot).is_none()));
    }

    #[test]
    fn viewer_display_queries_initialize_without_component_conflicts() {
        let mut world = World::new();
        let mut system = IntoSystem::into_system(update_viewer_display);

        system.initialize(&mut world);
    }

    #[test]
    fn replay_generated_metadata_and_speed_labels_are_ascii_safe() {
        let replay = fixed_metadata_replay();

        assert!(REPLAY_FIELD_SEPARATOR.is_ascii());
        assert!(
            replay_summary_text(&crate::replay_library::ReplayCatalogEntry::from_replay(
                &replay
            ))
            .is_ascii()
        );
        assert!(replay_match_id_text(&replay.id).is_ascii());
        assert!(replay_result_text(&replay).is_ascii());
        assert_eq!(
            PLAYBACK_SPEEDS.map(|(_, label)| label),
            ["0.5x", "1x", "2x"]
        );
        assert!(PLAYBACK_SPEEDS.iter().all(|(_, label)| label.is_ascii()));
        // The Viewer's speed buttons must offer exactly the speeds the
        // transport accepts, or a button would silently do nothing.
        assert_eq!(
            PLAYBACK_SPEEDS.map(|(speed, _)| speed),
            crate::replay::PLAYBACK_SPEEDS
        );
    }

    #[test]
    fn replay_viewer_round_is_derived_at_the_current_timeline_position() {
        let replay = fixed_metadata_replay();
        assert_eq!(viewer_round_text(&replay, &replay.frame_at(0.0)), "Round 1");
    }

    #[test]
    fn round_time_display_is_v5_only_and_uses_ceiling_seconds() {
        let mut replay = fixed_metadata_replay();
        replay
            .match_settings
            .set_value(MatchSettingField::RoundTime, 60);
        replay
            .match_settings
            .set_enabled(MatchSettingField::RoundTime, true);
        let mut frame = ReplayFrame {
            phase: ReplayPhase::Playing,
            current_round: 3,
            round_elapsed_seconds: 0.01,
            ..default()
        };

        assert_eq!(
            viewer_round_text(&replay, &frame),
            "Round 3 - 01:00 remaining"
        );
        frame.round_elapsed_seconds = 0.99;
        assert_eq!(
            viewer_round_text(&replay, &frame),
            "Round 3 - 01:00 remaining"
        );
        frame.round_elapsed_seconds = 1.0;
        assert_eq!(
            viewer_round_text(&replay, &frame),
            "Round 3 - 00:59 remaining"
        );

        replay.version = 4;
        assert_eq!(viewer_round_text(&replay, &frame), "Round 3");
    }

    #[test]
    fn round_time_display_uses_interpolated_sparse_replay_frame() {
        let mut replay = fixed_metadata_replay();
        replay.duration_seconds = 5.0;
        replay
            .match_settings
            .set_value(MatchSettingField::RoundTime, 60);
        replay
            .match_settings
            .set_enabled(MatchSettingField::RoundTime, true);
        replay.events = vec![
            crate::replay::ReplayEvent {
                at_seconds: 0.0,
                kind: crate::replay::ReplayEventKind::MatchStart,
                frame: ReplayFrame {
                    phase: ReplayPhase::Playing,
                    current_round: 3,
                    round_elapsed_seconds: 10.0,
                    ..default()
                },
            },
            crate::replay::ReplayEvent {
                at_seconds: 5.0,
                kind: crate::replay::ReplayEventKind::MatchEnd,
                frame: ReplayFrame {
                    phase: ReplayPhase::Ended,
                    current_round: 3,
                    round_elapsed_seconds: 15.0,
                    ..default()
                },
            },
        ];

        let frame = replay.playback_frame_at(2.25);

        assert_eq!(frame.round_elapsed_seconds, 12.25);
        assert_eq!(
            viewer_round_text(&replay, &frame),
            "Round 3 - 00:48 remaining"
        );
    }

    #[test]
    fn capture_scores_and_timeout_steps_use_recorded_v5_frames() {
        let mut replay = fixed_metadata_replay();
        replay.match_settings.set_capture_points(true);
        let mut timed_out = ReplayFrame {
            phase: ReplayPhase::Playing,
            current_round: 2,
            capture_totals: Score { black: 3, white: 2 },
            ..default()
        };
        timed_out.board.set(Point::new(0, 0), Cell::Black);
        replay.events = vec![
            crate::replay::ReplayEvent {
                at_seconds: 0.0,
                kind: crate::replay::ReplayEventKind::MatchStart,
                frame: ReplayFrame {
                    phase: ReplayPhase::Playing,
                    current_round: 1,
                    ..default()
                },
            },
            crate::replay::ReplayEvent {
                at_seconds: 0.5,
                kind: crate::replay::ReplayEventKind::RoundTimedOut {
                    resolution: crate::rules::ResolutionKind::Sequential,
                    submitted: [true, false],
                    captures: [0, 0],
                    cause: crate::replay::TimeoutCause::RoundTime,
                },
                frame: timed_out.clone(),
            },
        ];

        let frame = replay.frame_at(0.5);
        assert_eq!(frame.current_round, 2);
        assert_eq!(
            frame.current_score(&replay.match_settings),
            Score { black: 4, white: 2 }
        );
        let mut controller = PlaybackController::default();
        controller.jump_forward(&replay);
        assert_eq!(controller.position_seconds, 0.5);
        assert!(!controller.playing);
    }

    /// Counts what one [`update_viewer_display`] pass actually rewrote,
    /// mirroring `match_screen`'s `record_display_changes`: entities marked
    /// changed since this system's own previous run.
    #[derive(Resource, Default)]
    struct ViewerDisplayChangeCounts {
        intersections_changed: usize,
        cursors_changed: usize,
    }

    fn record_viewer_display_changes(
        intersections: Query<
            Entity,
            (
                With<ViewerIntersection>,
                Changed<MeshMaterial2d<ColorMaterial>>,
            ),
        >,
        cursors: Query<Entity, (With<ViewerCursor>, Changed<Transform>)>,
        mut counts: ResMut<ViewerDisplayChangeCounts>,
    ) {
        counts.intersections_changed = intersections.iter().count();
        counts.cursors_changed = cursors.iter().count();
    }

    /// Builds a minimal headless app with the real [`spawn_viewer_screen`]
    /// and [`update_viewer_display`] systems, a loaded `replay`, and
    /// [`record_viewer_display_changes`] chained right after so tests can
    /// inspect exactly what one display pass touched.
    fn viewer_display_test_app(replay: Replay, position_seconds: f64) -> App {
        let mut app = App::new();
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Mesh>>();
        let palette = {
            let mut materials = app.world_mut().resource_mut::<Assets<ColorMaterial>>();
            crate::app::visuals::test_stone_palette(&mut materials)
        };
        let stone_mesh = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            StoneMesh(meshes.add(Circle::new(1.0)))
        };
        app.insert_resource(palette)
            .insert_resource(stone_mesh)
            .init_resource::<WindowSize>()
            .insert_resource(ReplayViewerState {
                replay_id: Some(replay.id.clone()),
                replay: Some(replay),
                controller: PlaybackController {
                    position_seconds,
                    ..default()
                },
                ..default()
            })
            .init_resource::<ViewerDisplayChangeCounts>()
            .add_systems(Startup, spawn_viewer_screen)
            .add_systems(
                Update,
                (update_viewer_display, record_viewer_display_changes).chain(),
            );
        app
    }

    fn seek(app: &mut App, position_seconds: f64) {
        app.world_mut()
            .resource_mut::<ReplayViewerState>()
            .controller
            .position_seconds = position_seconds;
    }

    /// The actions whose buttons carry `InteractionDisabled` after the last
    /// display pass, in drawn order.
    fn disabled_actions(app: &mut App) -> Vec<ViewerAction> {
        let mut disabled: Vec<_> = app
            .world_mut()
            .query_filtered::<(&ViewerAction, &crate::ui::focus::Focusable), With<InteractionDisabled>>()
            .iter(app.world())
            .map(|(action, focusable)| (focusable.group, focusable.order, *action))
            .collect();
        disabled.sort_by_key(|(group, order, _)| (*group, *order));
        disabled.into_iter().map(|(_, _, action)| action).collect()
    }

    /// The display pass carries a control's disabled state into the focus
    /// ring by marking it `InteractionDisabled`, and unmarks it the frame
    /// it is enabled again: the two backward controls at the start of a
    /// replay, the three forward ones at its end, and every screen control
    /// while an overlay is open.
    #[test]
    fn disabled_transport_controls_carry_interaction_disabled_and_shed_it_when_enabled() {
        let replay = boundary_test_replay();
        let duration = replay.duration_seconds;
        let mut app = viewer_display_test_app(replay, 0.0);
        app.update();
        assert_eq!(
            disabled_actions(&mut app),
            [ViewerAction::Rewind, ViewerAction::JumpBack]
        );

        seek(&mut app, duration);
        app.update();
        assert_eq!(
            disabled_actions(&mut app),
            [
                ViewerAction::PlayPause,
                ViewerAction::JumpForward,
                ViewerAction::FastForward
            ]
        );

        seek(&mut app, 0.5);
        app.update();
        assert!(disabled_actions(&mut app).is_empty());

        app.world_mut().resource_mut::<ReplayViewerState>().key_open = true;
        app.update();
        let disabled = disabled_actions(&mut app);
        assert_eq!(
            disabled.len(),
            12,
            "every control but the open overlay's Close"
        );
        assert!(!disabled.contains(&ViewerAction::CloseKey));
        assert!(disabled.contains(&ViewerAction::CloseMatchSettings));

        app.world_mut().resource_mut::<ReplayViewerState>().key_open = false;
        app.update();
        assert!(disabled_actions(&mut app).is_empty());
    }

    /// What the display pass left on one control: its fill, its border and
    /// its label's colour.
    fn control_colors(app: &mut App, action: ViewerAction) -> (Color, Color, Color) {
        let (background, border, children) = app
            .world_mut()
            .query::<(&ViewerAction, &BackgroundColor, &BorderColor, &Children)>()
            .iter(app.world())
            .find(|(candidate, ..)| **candidate == action)
            .map(|(_, background, border, children)| (background.0, border.top, children.to_vec()))
            .expect("the control is on screen");
        let label = children
            .into_iter()
            .find_map(|child| {
                app.world()
                    .get::<ViewerButtonLabel>(child)
                    .and_then(|_| app.world().get::<TextColor>(child))
            })
            .expect("every control has a label")
            .0;
        (background, border, label)
    }

    /// The armed speed never changes its label, so its fill alone used to
    /// carry which speed is armed. It is on the border too, as every other
    /// screen marks an active control, and it moves with the speed.
    #[test]
    fn the_armed_speed_is_marked_on_the_border_as_well_as_the_fill() {
        use crate::presentation::{BUTTON_BACKGROUND, ORANGE_ACTIVE};

        let mut app = viewer_display_test_app(boundary_test_replay(), 0.5);
        app.update();
        for (speed, _) in PLAYBACK_SPEEDS {
            let expected = if speed == 1.0 {
                (ORANGE_ACTIVE, ORANGE, TEXT_PRIMARY)
            } else {
                (BUTTON_BACKGROUND, BLUE_BORDER, TEXT_PRIMARY)
            };
            assert_eq!(
                control_colors(&mut app, ViewerAction::Speed(speed)),
                expected,
                "{speed}x while 1x is armed"
            );
        }
        assert_eq!(
            control_colors(&mut app, ViewerAction::PlayPause),
            (BUTTON_BACKGROUND, BLUE_BORDER, TEXT_PRIMARY),
            "a paused Play is not marked"
        );

        {
            let mut viewer = app.world_mut().resource_mut::<ReplayViewerState>();
            viewer.controller.speed = 2.0;
            viewer.controller.playing = true;
        }
        app.update();
        assert_eq!(
            control_colors(&mut app, ViewerAction::Speed(2.0)),
            (ORANGE_ACTIVE, ORANGE, TEXT_PRIMARY)
        );
        assert_eq!(
            control_colors(&mut app, ViewerAction::Speed(1.0)),
            (BUTTON_BACKGROUND, BLUE_BORDER, TEXT_PRIMARY)
        );
        assert_eq!(
            control_colors(&mut app, ViewerAction::PlayPause),
            (ORANGE_ACTIVE, ORANGE, TEXT_PRIMARY),
            "a running Play is marked the same way"
        );
    }

    /// What one control's label says.
    fn control_label(app: &mut App, action: ViewerAction) -> String {
        let children = app
            .world_mut()
            .query::<(&ViewerAction, &Children)>()
            .iter(app.world())
            .find(|(candidate, _)| **candidate == action)
            .map(|(_, children)| children.to_vec())
            .expect("the control is on screen");
        children
            .into_iter()
            .find_map(|child| {
                app.world()
                    .get::<ViewerButtonLabel>(child)
                    .and_then(|_| app.world().get::<Text>(child))
            })
            .expect("every control has a label")
            .0
            .clone()
    }

    /// The fill and border that mark the armed speed are hue alone, 1.52:1
    /// and 1.44:1 from the others', so its label says it too: bracketed,
    /// as `[1x]`, and only while it is the armed one.
    #[test]
    fn the_armed_speed_is_bracketed_in_its_label() {
        let mut app = viewer_display_test_app(boundary_test_replay(), 0.5);
        app.update();
        let labels = |app: &mut App| {
            PLAYBACK_SPEEDS.map(|(speed, _)| control_label(app, ViewerAction::Speed(speed)))
        };
        assert_eq!(labels(&mut app), ["0.5x", "[1x]", "2x"]);

        app.world_mut()
            .resource_mut::<ReplayViewerState>()
            .controller
            .speed = 0.5;
        app.update();
        assert_eq!(labels(&mut app), ["[0.5x]", "1x", "2x"]);

        // With the Key open every control is disabled and nothing is drawn
        // as selected, so the brackets go with the orange.
        app.world_mut().resource_mut::<ReplayViewerState>().key_open = true;
        app.update();
        assert_eq!(labels(&mut app), ["0.5x", "1x", "2x"]);
        app.world_mut().resource_mut::<ReplayViewerState>().key_open = false;
        app.update();
        assert_eq!(labels(&mut app), ["[0.5x]", "1x", "2x"]);
        assert_eq!(control_label(&mut app, ViewerAction::Rewind), "Rewind");
    }

    /// A disabled control still says what it is: its label is the muted
    /// text Settings gives a disabled control, not a dimmer one
    /// only this screen used.
    #[test]
    fn a_disabled_control_keeps_a_readable_label() {
        use crate::presentation::BUTTON_DISABLED;

        let replay = boundary_test_replay();
        let duration = replay.duration_seconds;
        let mut app = viewer_display_test_app(replay, duration);
        app.update();

        // The positive half: these three really are disabled here, so the
        // colours below are a disabled control's and not a live one's.
        assert_eq!(
            disabled_actions(&mut app),
            [
                ViewerAction::PlayPause,
                ViewerAction::JumpForward,
                ViewerAction::FastForward
            ],
            "the forward controls are dead at the end of a replay"
        );
        for action in [
            ViewerAction::PlayPause,
            ViewerAction::JumpForward,
            ViewerAction::FastForward,
        ] {
            assert_eq!(
                control_colors(&mut app, action),
                (BUTTON_DISABLED, BUTTON_DISABLED_BORDER, TEXT_MUTED),
                "{action:?}"
            );
        }
        // The armed speed stays live and marked beside them.
        assert_eq!(control_colors(&mut app, ViewerAction::Speed(1.0)).1, ORANGE);
    }

    /// A tiny fixture replay with three well-separated boundaries: the
    /// default `MatchStart` frame at `t=0`, a cursor move to `(6, 4)` at
    /// `t=1`, and a `MatchEnd` frame with a stone placed at `(4, 4)` at
    /// `t=3`. Built from the same `MatchRecorder`/`frame()` fixtures the
    /// `replay` module's own tests use, rather than a hand-rolled schema.
    fn boundary_test_replay() -> Replay {
        crate::replay::test_support::sample_replay(1, 2, "Boundary fixture")
    }

    #[test]
    fn paused_replay_does_not_rebuild_the_board_every_frame() {
        let mut app = viewer_display_test_app(boundary_test_replay(), 0.5);
        app.update(); // Startup spawn plus the first, everything-is-new pass.
        app.update(); // Position unchanged: this pass should touch nothing.

        let counts = app.world().resource::<ViewerDisplayChangeCounts>();
        assert_eq!(counts.intersections_changed, 0);
        assert_eq!(counts.cursors_changed, 0);
    }

    #[test]
    fn crossing_a_replay_frame_boundary_updates_the_board_exactly_when_needed() {
        let mut app = viewer_display_test_app(boundary_test_replay(), 0.5);
        app.update();

        // 0.5 -> 1.5 crosses the cursor-move boundary at t=1: cursors must
        // move, but the board (unchanged between these two events) must not
        // be rewritten.
        seek(&mut app, 1.5);
        app.update();
        {
            let counts = app.world().resource::<ViewerDisplayChangeCounts>();
            assert_eq!(counts.intersections_changed, 0);
            assert!(counts.cursors_changed >= 1);
        }

        // 1.5 -> 2.9 stays within the same boundary: nothing should move.
        seek(&mut app, 2.9);
        app.update();
        {
            let counts = app.world().resource::<ViewerDisplayChangeCounts>();
            assert_eq!(counts.intersections_changed, 0);
            assert_eq!(counts.cursors_changed, 0);
        }

        // 2.9 -> 3.0 crosses the MatchEnd boundary: the board must update,
        // immediately and with no one-frame lag.
        seek(&mut app, 3.0);
        app.update();
        let counts = app.world().resource::<ViewerDisplayChangeCounts>();
        assert!(counts.intersections_changed >= 1);
    }

    #[test]
    fn seeking_refreshes_the_correct_board_state_immediately() {
        let mut app = viewer_display_test_app(boundary_test_replay(), 0.0);
        app.update();

        seek(&mut app, 3.0);
        app.update();

        let palette = app.world().resource::<StonePalette>();
        let black_handle = palette.black.clone();
        let mut query = app
            .world_mut()
            .query::<(&ViewerIntersection, &MeshMaterial2d<ColorMaterial>)>();
        let material_at_4_4 = query
            .iter(app.world())
            .find(|(intersection, _)| intersection.0 == Point::new(4, 4))
            .map(|(_, material)| material.0.clone());
        assert_eq!(material_at_4_4, Some(black_handle));
    }

    #[test]
    fn a_ring_is_drawn_behind_white_and_gray_stones_and_no_others() {
        let (black, white, gray) = (Point::new(1, 1), Point::new(3, 4), Point::new(7, 2));
        let mut start = crate::replay::test_support::frame();
        start.board.set(black, Cell::Black);
        start.board.set(white, Cell::White);
        start.board.set(gray, Cell::Gray);
        start.phase = ReplayPhase::Playing;
        let mut recorder = MatchRecorder::new(1, start.clone());
        let replay = recorder.complete(2, 1.0, start).unwrap();

        let mut app = viewer_display_test_app(replay, 0.0);
        app.update();

        let shown: std::collections::BTreeMap<(usize, usize), f32> = app
            .world_mut()
            .query::<(&ViewerStoneRing, &Transform, &Visibility)>()
            .iter(app.world())
            .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
            .map(|(ring, transform, _)| ((ring.0.x, ring.0.y), transform.scale.x))
            .collect();
        let expected = [white, gray]
            .into_iter()
            .zip([Cell::White, Cell::Gray])
            .map(|(point, cell)| {
                let scale = stone_geometry(cell)
                    .ring_scale
                    .expect("white and gray stones carry a ring");
                ((point.x, point.y), scale)
            })
            .collect();
        assert_eq!(
            shown, expected,
            "exactly the white and the gray stone show a ring, each at its own footprint"
        );
        let rings = app
            .world_mut()
            .query::<&ViewerStoneRing>()
            .iter(app.world())
            .count();
        assert_eq!(rings, crate::rules::BOARD_SIZE * crate::rules::BOARD_SIZE);
    }

    /// A one-frame replay showing `start` from its first instant.
    fn replay_showing(start: ReplayFrame) -> Replay {
        let mut recorder = MatchRecorder::new(1, start.clone());
        recorder.complete(2, 1.0, start).unwrap()
    }

    /// Every hidden-selection mark on show after a display pass, as
    /// (player, slot, whether it is the smaller shared mark, where), in a
    /// stable order.
    fn shown_selections(app: &mut App) -> Vec<(Player, usize, bool, Vec2)> {
        let mut shown: Vec<_> = app
            .world_mut()
            .query::<(&ViewerSelection, &Transform, &Visibility)>()
            .iter(app.world())
            .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
            .map(|(selection, transform, _)| {
                assert_eq!(transform.scale, Vec3::ONE, "a mark is never scaled");
                (
                    selection.player,
                    selection.slot,
                    selection.shared,
                    transform.translation.truncate(),
                )
            })
            .collect();
        shown.sort_by_key(|(player, slot, ..)| (player_index(*player), *slot));
        shown
    }

    fn shown_markers(app: &mut App) -> Vec<Player> {
        let mut shown: Vec<_> = app
            .world_mut()
            .query::<(&ViewerMarker, &Visibility)>()
            .iter(app.world())
            .filter(|(_, visibility)| **visibility == Visibility::Visible)
            .map(|(marker, _)| marker.0)
            .collect();
        shown.sort_by_key(|player| player_index(*player));
        shown
    }

    /// The case the mark's shape exists for. A translucent stone over an
    /// opaque stone of its own colour was nothing on screen; the ring is
    /// shown at that stone's point, at full size, whatever the board holds
    /// there. That its strokes are opaque and leave the middle bare, which
    /// is what makes them show on the stone, is pinned where they are made:
    /// in `presentation` and in `layout`.
    #[test]
    fn a_hidden_selection_of_the_players_own_stone_is_drawn_on_that_stone() {
        let (own_black, own_white) = (Point::new(2, 2), Point::new(6, 5));
        let mut start = crate::replay::test_support::frame();
        start.board.set(own_black, Cell::Black);
        start.board.set(own_white, Cell::White);
        start.selected_moves = [Some(own_black), Some(own_white)];

        let mut app = viewer_display_test_app(replay_showing(start), 0.0);
        app.update();

        assert_eq!(
            shown_selections(&mut app),
            [
                (Player::One, 0, false, viewer_board_position(own_black)),
                (Player::Two, 0, false, viewer_board_position(own_white)),
            ]
        );
    }

    /// The lowest and the highest z at which any part of one kind of mark
    /// is drawn: each parent's z plus each of its children's.
    fn drawn_z<C: Component>(app: &mut App) -> (f32, f32) {
        let parents: Vec<(f32, Vec<Entity>)> = app
            .world_mut()
            .query_filtered::<(&Transform, &Children), With<C>>()
            .iter(app.world())
            .map(|(transform, children)| (transform.translation.z, children.to_vec()))
            .collect();
        assert!(!parents.is_empty());
        let mut span = (f32::MAX, f32::MIN);
        for (parent_z, children) in parents {
            for child in children {
                let z = parent_z + app.world().get::<Transform>(child).unwrap().translation.z;
                span = (span.0.min(z), span.1.max(z));
            }
        }
        span
    }

    /// The stacking `layout`'s module docs argue for, read back from the
    /// screen as it is spawned rather than from its constants: every stone
    /// lies under every layer of a cursor, every layer of a cursor under
    /// every layer of a marker, every layer of a marker under every stroke
    /// of a selection, and all of it under the score badges.
    #[test]
    fn marks_are_stacked_cursors_then_markers_then_selections_as_spawned() {
        let mut app =
            viewer_display_test_app(replay_showing(crate::replay::test_support::frame()), 0.0);
        app.update();
        let stones = app
            .world_mut()
            .query_filtered::<&Transform, With<ViewerIntersection>>()
            .iter(app.world())
            .map(|transform| transform.translation.z)
            .fold(f32::MIN, f32::max);
        let cursors = drawn_z::<ViewerCursor>(&mut app);
        let markers = drawn_z::<ViewerMarker>(&mut app);
        let selections = drawn_z::<ViewerSelection>(&mut app);
        let badges = app
            .world_mut()
            .query_filtered::<&Transform, With<ViewerScoreBadge>>()
            .iter(app.world())
            .map(|transform| transform.translation.z)
            .fold(f32::MAX, f32::min);
        assert!(stones < cursors.0, "a stone is drawn over a cursor");
        assert!(cursors.1 < markers.0, "a cursor is drawn over a marker");
        assert!(
            markers.1 < selections.0,
            "a marker is drawn over a selection"
        );
        assert!(
            selections.1 < badges,
            "a selection is drawn over a score badge"
        );
    }

    /// Round 0: every held selection is shown, and a point both players
    /// chose shows both - as the smaller marks, side by side, Player One's
    /// on the left - rather than one over the other.
    #[test]
    fn two_selections_of_one_point_are_both_shown_side_by_side() {
        let (shared, only_one, only_two) = (Point::new(3, 7), Point::new(1, 7), Point::new(7, 7));
        let mut start = ReplayFrame {
            phase: ReplayPhase::Opening,
            current_round: 0,
            ..crate::replay::test_support::frame()
        };
        start.opening_selections[Player::One.index()].add(only_one);
        start.opening_selections[Player::One.index()].add(shared);
        start.opening_selections[Player::Two.index()].add(shared);
        start.opening_selections[Player::Two.index()].add(only_two);

        let mut app = viewer_display_test_app(replay_showing(start), 0.0);
        app.update();

        let centre = viewer_board_position(shared);
        let left = viewer_selection_position(shared, Player::One, true);
        let right = viewer_selection_position(shared, Player::Two, true);
        assert!(left.x < centre.x && centre.x < right.x);
        assert_eq!(
            shown_selections(&mut app),
            [
                (Player::One, 0, false, viewer_board_position(only_one)),
                (Player::One, 1, true, left),
                (Player::Two, 0, true, right),
                (Player::Two, 1, false, viewer_board_position(only_two)),
            ]
        );
    }

    /// A frame of ordinary play with both selections on one point is drawn
    /// the same way. A recorded Match never shows one - the round resolves
    /// at the same instant - but `Replay::validate` accepts such a frame
    /// with a time of its own, so the Viewer has to draw it sensibly.
    #[test]
    fn two_ordinary_selections_of_one_point_are_side_by_side_too() {
        let shared = Point::new(4, 4);
        let mut start = crate::replay::test_support::frame();
        assert_eq!(start.phase, ReplayPhase::Playing);
        start.selected_moves = [Some(shared), Some(shared)];

        let mut app = viewer_display_test_app(replay_showing(start), 0.0);
        app.update();

        assert_eq!(
            shown_selections(&mut app),
            Player::ALL.map(|player| {
                (
                    player,
                    0,
                    true,
                    viewer_selection_position(shared, player, true),
                )
            })
        );
    }

    /// A Teleport Marker is shown where its frame puts it, and the display
    /// pass only ever moves it: the 45-degree turn it was spawned with,
    /// which is what makes it a diamond, survives.
    #[test]
    fn teleport_markers_follow_the_frame_and_stay_on_their_points() {
        let marked = Point::new(5, 3);
        let mut start = crate::replay::test_support::frame();
        start.teleport_markers = [Some(marked), None];

        let mut app = viewer_display_test_app(replay_showing(start), 0.0);
        app.update();

        assert_eq!(shown_markers(&mut app), [Player::One]);
        let markers: Vec<_> = app
            .world_mut()
            .query::<(&ViewerMarker, &Transform)>()
            .iter(app.world())
            .map(|(marker, transform)| (marker.0, *transform))
            .collect();
        assert_eq!(markers.len(), 2);
        for (player, transform) in markers {
            assert_eq!(
                transform.rotation,
                Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
                "{player:?}"
            );
            if player == Player::One {
                assert_eq!(
                    transform.translation.truncate(),
                    viewer_board_position(marked)
                );
            }
        }
    }

    /// A mark one frame shows is taken away again by a frame that has none:
    /// for a selection, the round resolved; for a marker, which a recorded
    /// Match never unsets, a seek back to before it was set - the frames
    /// here stand in for that. Without this a mark's being hidden is only
    /// ever its spawn state.
    #[test]
    fn marks_are_hidden_again_when_a_later_frame_has_none() {
        let mut start = crate::replay::test_support::frame();
        start.teleport_markers = [Some(Point::new(4, 4)), Some(Point::new(5, 5))];
        start.selected_moves = [Some(Point::new(2, 2)), Some(Point::new(6, 6))];
        let end = crate::replay::test_support::frame();
        let mut recorder = MatchRecorder::new(1, start);
        let replay = recorder.complete(2, 1.0, end).unwrap();

        let mut app = viewer_display_test_app(replay, 0.0);
        app.update();
        // The positive half: both kinds of mark really were on show.
        assert_eq!(shown_selections(&mut app).len(), 2);
        assert_eq!(shown_markers(&mut app), Player::ALL);

        seek(&mut app, 1.0);
        app.update();
        assert!(shown_selections(&mut app).is_empty());
        assert!(shown_markers(&mut app).is_empty());
    }

    /// Where both players' markers are after a display pass, Player One's
    /// first.
    fn marker_positions(app: &mut App) -> Vec<Vec2> {
        let mut markers: Vec<_> = app
            .world_mut()
            .query::<(&ViewerMarker, &Transform)>()
            .iter(app.world())
            .map(|(marker, transform)| (player_index(marker.0), transform.translation.truncate()))
            .collect();
        markers.sort_by_key(|(player, _)| *player);
        markers.into_iter().map(|(_, position)| position).collect()
    }

    /// A mark that stays on show is moved when a later frame puts it
    /// elsewhere - a revised selection, a marker set again - including
    /// along a column, where only its y changes; and both players' markers
    /// are placed, not only Player One's.
    #[test]
    fn marks_on_show_follow_a_later_frame() {
        let at = |x, y| Some(Point::new(x, y));
        let mut start = crate::replay::test_support::frame();
        start.teleport_markers = [at(4, 1), at(6, 1)];
        start.selected_moves = [at(2, 2), at(7, 2)];
        let mut end = crate::replay::test_support::frame();
        end.teleport_markers = [at(4, 7), at(6, 5)];
        end.selected_moves = [at(2, 6), at(7, 5)];
        let mut recorder = MatchRecorder::new(1, start);
        let replay = recorder.complete(2, 1.0, end).unwrap();

        let mut app = viewer_display_test_app(replay, 0.0);
        app.update();
        assert_eq!(
            marker_positions(&mut app),
            [
                viewer_board_position(Point::new(4, 1)),
                viewer_board_position(Point::new(6, 1)),
            ]
        );
        assert_eq!(shown_markers(&mut app), Player::ALL);

        seek(&mut app, 1.0);
        app.update();
        assert_eq!(
            shown_selections(&mut app),
            [
                (
                    Player::One,
                    0,
                    false,
                    viewer_board_position(Point::new(2, 6))
                ),
                (
                    Player::Two,
                    0,
                    false,
                    viewer_board_position(Point::new(7, 5))
                ),
            ]
        );
        assert_eq!(
            marker_positions(&mut app),
            [
                viewer_board_position(Point::new(4, 7)),
                viewer_board_position(Point::new(6, 5)),
            ]
        );
    }

    /// Round 0 as a recording runs, frame by frame: a point becomes shared
    /// when the second player chooses it, stops being shared when the
    /// first player's fourth choice pushes it out, and is gone when play
    /// begins. At each step every held selection shows exactly one of its
    /// slot's two marks - the smaller pair while the point is shared, the
    /// ordinary mark before and after - and none is left behind, forwards
    /// or on a seek back.
    #[test]
    fn a_slot_shows_one_mark_as_its_selection_becomes_shared_and_parts_again() {
        let (x, y, z, w, v) = (
            Point::new(1, 1),
            Point::new(3, 3),
            Point::new(5, 5),
            Point::new(7, 7),
            Point::new(8, 2),
        );
        let opening = |one: &[Point], two: &[Point]| {
            let mut frame = ReplayFrame {
                phase: ReplayPhase::Opening,
                current_round: 0,
                ..crate::replay::test_support::frame()
            };
            for point in one {
                frame.opening_selections[Player::One.index()].add(*point);
            }
            for point in two {
                frame.opening_selections[Player::Two.index()].add(*point);
            }
            frame
        };
        let mut playing = crate::replay::test_support::frame();
        playing.selected_moves = [Some(y), None];
        let frames = [
            // Player One holds X and Y, Player Two holds Z.
            opening(&[x, y], &[z]),
            // Player Two chooses X as well: X is shared.
            opening(&[x, y], &[z, x]),
            // Player One's third and fourth choices push X out of their
            // three: X is Player Two's alone again.
            opening(&[x, y, w, v], &[z, x]),
            playing,
        ];
        let mut replay = replay_showing(crate::replay::test_support::frame());
        replay.events = frames
            .into_iter()
            .enumerate()
            .map(|(index, frame)| crate::replay::ReplayEvent {
                at_seconds: index as f64,
                kind: crate::replay::ReplayEventKind::MatchStart,
                frame,
            })
            .collect();
        replay.duration_seconds = 3.0;

        let centre = viewer_board_position;
        let side = |player| viewer_selection_position(x, player, true);
        let unshared = vec![
            (Player::One, 0, false, centre(x)),
            (Player::One, 1, false, centre(y)),
            (Player::Two, 0, false, centre(z)),
        ];
        let shared = vec![
            (Player::One, 0, true, side(Player::One)),
            (Player::One, 1, false, centre(y)),
            (Player::Two, 0, false, centre(z)),
            (Player::Two, 1, true, side(Player::Two)),
        ];
        let parted = vec![
            (Player::One, 0, false, centre(y)),
            (Player::One, 1, false, centre(w)),
            (Player::One, 2, false, centre(v)),
            (Player::Two, 0, false, centre(z)),
            (Player::Two, 1, false, centre(x)),
        ];
        let play = vec![(Player::One, 0, false, centre(y))];

        let mut app = viewer_display_test_app(replay, 0.0);
        for (at, expected) in [
            (0.0, &unshared),
            (1.0, &shared),
            (2.0, &parted),
            (3.0, &play),
            (1.0, &shared),
            (0.0, &unshared),
        ] {
            seek(&mut app, at);
            app.update();
            assert_eq!(&shown_selections(&mut app), expected, "at {at}");
        }
    }

    #[test]
    fn a_new_replay_at_the_same_boundary_index_is_never_mistaken_for_a_cached_one() {
        // Two different replays whose very first (t=0) frame already differs
        // - one has a stone on the board, one does not - so if the boundary
        // cache survived from one replay into the next without resetting,
        // the second replay would incorrectly keep showing the first one's
        // board.
        let mut blank_start = crate::replay::test_support::frame();
        blank_start.phase = ReplayPhase::Playing;
        let mut recorder = MatchRecorder::new(1, blank_start.clone());
        let blank_replay = recorder.complete(2, 1.0, blank_start).unwrap();

        let mut stone_start = crate::replay::test_support::frame();
        stone_start.board.set(Point::new(4, 4), Cell::Black);
        stone_start.phase = ReplayPhase::Playing;
        let mut recorder = MatchRecorder::new(1, stone_start.clone());
        let stone_replay = recorder.complete(2, 1.0, stone_start).unwrap();

        let mut app = viewer_display_test_app(blank_replay, 0.0);
        app.update();
        {
            let palette = app.world().resource::<StonePalette>();
            let empty_handle = palette.empty.clone();
            let mut query = app
                .world_mut()
                .query::<(&ViewerIntersection, &MeshMaterial2d<ColorMaterial>)>();
            let material = query
                .iter(app.world())
                .find(|(intersection, _)| intersection.0 == Point::new(4, 4))
                .map(|(_, material)| material.0.clone());
            assert_eq!(material, Some(empty_handle));
        }

        // Simulate leaving and re-entering the Viewer with a different
        // Replay: `update_viewer_display` must treat a fresh `replay` as
        // needing a full redraw even if its boundary key happens to match
        // whatever was last drawn.
        {
            let mut viewer = app.world_mut().resource_mut::<ReplayViewerState>();
            viewer.replay_id = Some(stone_replay.id.clone());
            viewer.replay = Some(stone_replay);
            viewer.controller.position_seconds = 0.0;
        }
        app.update();

        let palette = app.world().resource::<StonePalette>();
        let black_handle = palette.black.clone();
        let mut query = app
            .world_mut()
            .query::<(&ViewerIntersection, &MeshMaterial2d<ColorMaterial>)>();
        let material = query
            .iter(app.world())
            .find(|(intersection, _)| intersection.0 == Point::new(4, 4))
            .map(|(_, material)| material.0.clone());
        assert_eq!(
            material,
            Some(black_handle),
            "opening a new replay must never retain the previous replay's stale board"
        );
    }
}
