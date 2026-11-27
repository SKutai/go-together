//! Integration test proving `go_together`'s headless core works for an
//! external library consumer: this file only reaches through the published
//! API (`go_together::rules`, `go_together::authority`, `go_together::clock`,
//! `go_together::opening`, `go_together::match_settings`,
//! `go_together::replay`) and never touches Bevy, matching how a future
//! headless host would drive a Match on its own tick without a window,
//! renderer, audio device, or storage backend - including making Match time
//! pass, which the second half of this file does through
//! [`MatchState::advance_clock`] alone.

use go_together::authority::{
    ClockBoundary, LockOutcome, MatchPhase, MatchState, OrdinaryCompletion, player_index,
};
use go_together::clock::{MatchDuration, MatchInstant};
use go_together::match_settings::{MatchSettingField, MatchSettings};
use go_together::opening::{OPENING_GO_DISPLAY, OPENING_REVEAL, OpeningCountdownStage};
use go_together::replay::EndReason;
use go_together::rules::{Cell, Player, Point, Score};

#[test]
fn a_full_match_can_be_played_headlessly_from_round_zero_to_score() {
    let mut state = MatchState::default();
    assert_eq!(state.phase, MatchPhase::Opening);

    // Round 0: each player privately picks three distinct points through the
    // same command method the Bevy client calls from its own input systems.
    let black_picks = [Point::new(0, 0), Point::new(1, 0), Point::new(2, 0)];
    let white_picks = [Point::new(6, 8), Point::new(7, 8), Point::new(8, 8)];
    for point in black_picks {
        state.cursors[player_index(Player::One)] = point;
        state.try_opening_select(Player::One);
    }
    for point in white_picks {
        state.cursors[player_index(Player::Two)] = point;
        state.try_opening_select(Player::Two);
    }

    // Both windows filled, so the closing countdown already started as part
    // of that last select - no separate "start the countdown" step needed.
    assert_eq!(state.phase, MatchPhase::OpeningCountdown);

    // A client or server drives the countdown display on its own clock; a
    // headless caller can reveal as soon as it decides the countdown is over.
    state.reveal_round_zero();
    assert_eq!(state.phase, MatchPhase::Playing);
    assert_eq!(state.round, 1);
    for point in black_picks {
        assert_eq!(state.board.get(point), Cell::Black);
    }
    for point in white_picks {
        assert_eq!(state.board.get(point), Cell::White);
    }

    // Ordinary round: each player locks in a hidden selection; the round
    // resolves deterministically once both are filled.
    state.cursors = [Point::new(4, 4), Point::new(5, 5)];
    assert!(matches!(
        state.try_lock(Player::One, MatchInstant::from_secs_f64(1.0)),
        LockOutcome::Locked { point } if point == Point::new(4, 4)
    ));
    assert!(matches!(
        state.try_lock(Player::Two, MatchInstant::from_secs_f64(1.05)),
        LockOutcome::Locked { point } if point == Point::new(5, 5)
    ));
    assert!(state.locks_filled());

    let outcome = state
        .resolve_ordinary_round(OrdinaryCompletion::Normal)
        .expect("both hidden selections resolve into a round");
    assert!(outcome.end_reasons.is_empty());
    assert_eq!(state.round, 2);
    assert_eq!(state.board.get(Point::new(4, 4)), Cell::Black);
    assert_eq!(state.board.get(Point::new(5, 5)), Cell::White);
    assert_eq!(state.capture_totals, Score::default());

    // Finishing the Match scores it deterministically - no window, renderer,
    // audio device, or storage backend involved anywhere in this test.
    assert!(state.finish(vec![EndReason::Manual]));
    assert_eq!(state.phase, MatchPhase::Ended);
    assert_eq!(state.end_reasons, vec![EndReason::Manual]);
}

/// The neutral, UI-independent boundary claimed by `crate::authority`'s
/// module docs: a caller can construct, mutate, and clone the authoritative
/// state directly, with no channel, resource, or Bevy type in sight.
#[test]
fn match_state_is_a_plain_cloneable_value() {
    let mut state = MatchState::default();
    state.cursors[player_index(Player::One)] = Point::new(3, 3);
    let snapshot = state.clone();

    state.cursors[player_index(Player::One)] = Point::new(4, 4);

    assert_ne!(state.cursors, snapshot.cursors);
    assert_eq!(
        snapshot.cursors[player_index(Player::One)],
        Point::new(3, 3)
    );
}

fn settings_with(fields: &[(MatchSettingField, u64)]) -> MatchSettings {
    let mut settings = MatchSettings::default();
    for (field, value) in fields {
        settings.set_value(*field, *value);
        settings.set_enabled(*field, true);
    }
    settings
}

fn select_full_opening(state: &mut MatchState) {
    let black_picks = [Point::new(0, 0), Point::new(1, 0), Point::new(2, 0)];
    let white_picks = [Point::new(6, 8), Point::new(7, 8), Point::new(8, 8)];
    for point in black_picks {
        state.cursors[player_index(Player::One)] = point;
        state.try_opening_select(Player::One);
    }
    for point in white_picks {
        state.cursors[player_index(Player::Two)] = point;
        state.try_opening_select(Player::Two);
    }
    assert_eq!(state.phase, MatchPhase::OpeningCountdown);
}

/// Everything a headless host does to make `delta` of Match time pass: call
/// [`MatchState::advance_clock`] until a step crosses nothing, and answer
/// each boundary the way the core leaves to its caller - resolve the round
/// a timeout ended with the completion the boundary names, and finish the
/// Match when an end reason lands. Every boundary is returned with the
/// Match-clock instant it landed at, which is what a recorder would stamp.
fn pass_time(state: &mut MatchState, delta: MatchDuration) -> Vec<(MatchInstant, ClockBoundary)> {
    let mut crossed = Vec::new();
    let mut budget = delta;
    while state.phase != MatchPhase::Ended {
        let step = state.advance_clock(budget);
        budget = step.remaining;
        let Some(boundary) = step.crossed else {
            break;
        };
        crossed.push((state.elapsed, boundary));
        match boundary {
            ClockBoundary::OpeningCountdown(_) | ClockBoundary::OpeningCountdownFinished => {}
            ClockBoundary::MatchTimeExpiredInRoundZero => {
                assert!(state.finish(vec![EndReason::MatchTimeLimit]));
            }
            ClockBoundary::RoundTimeExpired | ClockBoundary::MatchTimeExpired => {
                let completion = boundary
                    .completion()
                    .expect("a timeout boundary names the completion to resolve with");
                let outcome = state
                    .resolve_ordinary_round(completion)
                    .expect("a timed-out round always resolves");
                if !outcome.end_reasons.is_empty() {
                    assert!(state.finish(outcome.end_reasons));
                }
            }
        }
    }
    crossed
}

fn secs(seconds: f64) -> MatchDuration {
    MatchDuration::from_secs_f64(seconds)
}

fn at(seconds: f64) -> MatchInstant {
    MatchInstant::from_secs_f64(seconds)
}

/// A Match driven through *time* - not just through commands - to
/// completion, with nothing but the public API: interactive Round 0, the
/// closing countdown stage by stage, the atomic reveal, the Go card
/// clearing, Round Time ending each ordinary round, and the Round Limit
/// ending the Match. Frames are deliberately odd sizes that straddle every
/// boundary, and each boundary still lands at its exact Match-clock instant.
#[test]
fn a_full_match_can_be_driven_through_time_headlessly_to_completion() {
    let settings = settings_with(&[
        (MatchSettingField::RoundTime, 1),
        (MatchSettingField::RoundLimit, 3),
    ]);
    let mut state = MatchState::default();
    state.begin_round_zero(&settings);
    assert_eq!(state.phase, MatchPhase::Opening);

    // Interactive Round 0 has no deadline of its own: time simply passes.
    assert!(pass_time(&mut state, secs(0.5)).is_empty());
    assert_eq!(state.elapsed, at(0.5));
    assert_eq!(state.opening_countdown_stage(), None);

    select_full_opening(&mut state);
    assert_eq!(
        state.opening_countdown_stage(),
        Some(OpeningCountdownStage::Three)
    );

    // The countdown, one odd frame at a time.
    assert!(pass_time(&mut state, secs(0.4)).is_empty());
    assert_eq!(
        pass_time(&mut state, secs(0.7)),
        vec![(
            at(1.5),
            ClockBoundary::OpeningCountdown(OpeningCountdownStage::Two)
        )]
    );
    assert_eq!(
        state.opening_countdown_stage(),
        Some(OpeningCountdownStage::Two)
    );
    assert_eq!(
        pass_time(&mut state, secs(1.9)),
        vec![
            (
                at(2.5),
                ClockBoundary::OpeningCountdown(OpeningCountdownStage::One)
            ),
            (
                at(3.5),
                ClockBoundary::OpeningCountdown(OpeningCountdownStage::Go)
            ),
        ]
    );
    assert_eq!(state.elapsed, at(0.5) + OPENING_REVEAL);
    assert_eq!(state.phase, MatchPhase::Playing);
    assert_eq!(state.round, 1);
    assert_eq!(state.board.get(Point::new(0, 0)), Cell::Black);
    assert_eq!(state.board.get(Point::new(8, 8)), Cell::White);
    assert_eq!(
        state.opening_countdown_stage(),
        Some(OpeningCountdownStage::Go)
    );
    assert_eq!(state.opening_go_remaining(), Some(OPENING_GO_DISPLAY));

    // Ordinary play under the Go card: Black locks a move that the round's
    // timeout will resolve; White never does.
    state.cursors[player_index(Player::One)] = Point::new(4, 4);
    assert!(matches!(
        state.try_lock(Player::One, state.elapsed),
        LockOutcome::Locked { .. }
    ));
    assert_eq!(
        pass_time(&mut state, secs(1.1)),
        vec![
            (at(4.25), ClockBoundary::OpeningCountdownFinished),
            (at(4.5), ClockBoundary::RoundTimeExpired),
        ]
    );
    assert_eq!(state.opening_countdown_stage(), None);
    assert_eq!(state.round, 2);
    assert_eq!(state.round_elapsed, secs(0.1));
    assert_eq!(state.board.get(Point::new(4, 4)), Cell::Black);
    assert_eq!(state.round_time_remaining_seconds(), Some(1));

    // Two more forfeited rounds reach the Round Limit; the Match ends at the
    // exact instant of the third timeout, and time stops there even though
    // the frame had more to give.
    assert_eq!(
        pass_time(&mut state, secs(5.0)),
        vec![
            (at(5.5), ClockBoundary::RoundTimeExpired),
            (at(6.5), ClockBoundary::RoundTimeExpired),
        ]
    );
    assert_eq!(state.phase, MatchPhase::Ended);
    assert_eq!(state.round, 4);
    assert_eq!(state.elapsed, at(6.5));
    assert_eq!(state.end_reasons, vec![EndReason::RoundLimit]);
    assert!(pass_time(&mut state, secs(1.0)).is_empty());
    assert_eq!(state.elapsed, at(6.5));
}

/// The determinism the core promises: the boundaries a Match crosses, and
/// the instants they land at, depend only on the total Match time passed -
/// never on how a host happened to split it into ticks.
#[test]
fn the_timeline_does_not_depend_on_how_time_is_subdivided() {
    let settings = settings_with(&[
        (MatchSettingField::RoundTime, 1),
        (MatchSettingField::RoundLimit, 4),
    ]);
    let subdivisions = [0.37, 0.13, 0.5, 1.0, 0.6, 0.9, 0.75, 1.2, 0.05, 2.5, 1.0];
    let total: f64 = subdivisions.iter().sum();

    let mut one_tick = MatchState::default();
    one_tick.begin_round_zero(&settings);
    select_full_opening(&mut one_tick);
    let one_tick_timeline = pass_time(&mut one_tick, secs(total));

    let mut many_ticks = MatchState::default();
    many_ticks.begin_round_zero(&settings);
    select_full_opening(&mut many_ticks);
    let many_ticks_timeline = subdivisions
        .iter()
        .flat_map(|&seconds| pass_time(&mut many_ticks, secs(seconds)))
        .collect::<Vec<_>>();

    assert_eq!(one_tick_timeline, many_ticks_timeline);
    assert_eq!(
        one_tick_timeline
            .iter()
            .map(|(_, boundary)| *boundary)
            .collect::<Vec<_>>(),
        vec![
            ClockBoundary::OpeningCountdown(OpeningCountdownStage::Two),
            ClockBoundary::OpeningCountdown(OpeningCountdownStage::One),
            ClockBoundary::OpeningCountdown(OpeningCountdownStage::Go),
            ClockBoundary::OpeningCountdownFinished,
            ClockBoundary::RoundTimeExpired,
            ClockBoundary::RoundTimeExpired,
            ClockBoundary::RoundTimeExpired,
            ClockBoundary::RoundTimeExpired,
        ]
    );
    assert_eq!(one_tick.phase, MatchPhase::Ended);
    assert_eq!(one_tick.phase, many_ticks.phase);
    assert_eq!(one_tick.elapsed, many_ticks.elapsed);
    assert_eq!(one_tick.round, many_ticks.round);
    assert_eq!(one_tick.board, many_ticks.board);
    assert_eq!(one_tick.end_reasons, many_ticks.end_reasons);
}

/// Match Time can land before ordinary play begins - here with only a
/// partial opening chosen - and during play; both end the Match at the
/// exact limit through the public API alone.
#[test]
fn match_time_ends_a_match_in_round_zero_and_in_ordinary_play() {
    let mut in_round_zero = MatchState::default();
    in_round_zero.begin_round_zero(&settings_with(&[(MatchSettingField::MatchTime, 1)]));
    in_round_zero.cursors[player_index(Player::One)] = Point::new(2, 2);
    in_round_zero.try_opening_select(Player::One);
    assert_eq!(
        pass_time(&mut in_round_zero, secs(1.5)),
        vec![(at(1.0), ClockBoundary::MatchTimeExpiredInRoundZero)]
    );
    assert_eq!(in_round_zero.phase, MatchPhase::Ended);
    assert_eq!(in_round_zero.elapsed, at(1.0));
    assert_eq!(in_round_zero.round, 1);
    assert_eq!(in_round_zero.board.get(Point::new(2, 2)), Cell::Black);
    assert_eq!(in_round_zero.end_reasons, vec![EndReason::MatchTimeLimit]);

    let mut in_play = MatchState::default();
    in_play.begin_round_zero(&settings_with(&[(MatchSettingField::MatchTime, 5)]));
    select_full_opening(&mut in_play);
    let timeline = pass_time(&mut in_play, secs(9.0));
    assert_eq!(
        timeline.last(),
        Some(&(at(5.0), ClockBoundary::MatchTimeExpired))
    );
    assert_eq!(in_play.phase, MatchPhase::Ended);
    assert_eq!(in_play.elapsed, at(5.0));
    assert_eq!(
        in_play.round, 2,
        "the round Match Time cut short still resolves"
    );
    assert_eq!(in_play.end_reasons, vec![EndReason::MatchTimeLimit]);
}

/// A timeout is an obligation, not a fait accompli: the clock stops at the
/// deadline and waits for the caller to resolve the round - so the Bevy
/// client can record the post-resolution frame and defer an automatic end
/// to an in-flight end gesture first - and a caller that has not yet done
/// so is told the same thing again rather than having time slip past the
/// boundary.
#[test]
fn a_timeout_boundary_waits_for_the_caller_to_resolve_the_round() {
    let mut state = MatchState::default();
    state.begin_round_zero(&settings_with(&[(MatchSettingField::RoundTime, 1)]));
    select_full_opening(&mut state);
    assert_eq!(
        pass_time(&mut state, OPENING_REVEAL)
            .last()
            .map(|(_, boundary)| *boundary),
        Some(ClockBoundary::OpeningCountdown(OpeningCountdownStage::Go))
    );

    let step = state.advance_clock(secs(2.0));
    assert_eq!(step.consumed, OPENING_GO_DISPLAY);
    assert_eq!(step.crossed, Some(ClockBoundary::OpeningCountdownFinished));
    let step = state.advance_clock(step.remaining);
    assert_eq!(step.consumed, secs(1.0).saturating_sub(OPENING_GO_DISPLAY));
    assert_eq!(step.remaining, secs(1.0));
    assert_eq!(step.crossed, Some(ClockBoundary::RoundTimeExpired));
    assert_eq!(state.round, 1);

    let repeated = state.advance_clock(step.remaining);
    assert_eq!(repeated.consumed, MatchDuration::ZERO);
    assert_eq!(repeated.remaining, secs(1.0));
    assert_eq!(repeated.crossed, Some(ClockBoundary::RoundTimeExpired));
    assert_eq!(state.elapsed, at(4.0));

    let outcome = state
        .resolve_ordinary_round(OrdinaryCompletion::RoundTimeout)
        .expect("the timed-out round resolves");
    assert!(outcome.end_reasons.is_empty());
    assert_eq!(state.round, 2);

    let idle = state.advance_clock(MatchDuration::ZERO);
    assert_eq!(idle.consumed, MatchDuration::ZERO);
    assert_eq!(idle.crossed, None);
    let next = state.advance_clock(repeated.remaining);
    assert_eq!(next.consumed, secs(1.0));
    assert_eq!(next.remaining, MatchDuration::ZERO);
    assert_eq!(next.crossed, Some(ClockBoundary::RoundTimeExpired));
    assert_eq!(state.elapsed, at(5.0));
}

/// A capture is public information, and a headless host gets it twice over:
/// once in the outcome of the command that resolved the round, and again on
/// the public view, where it stays until the next resolution replaces it.
#[test]
fn a_capture_is_reported_in_the_outcome_and_published_on_the_public_view() {
    use go_together::authority::RoundRemovals;
    use go_together::rules::RemovedStone;
    use go_together::view::PublicMatchView;

    let mut state = MatchState::default();
    select_full_opening(&mut state);
    state.reveal_round_zero();
    assert_eq!(
        PublicMatchView::of(&state).last_removals,
        RoundRemovals::default(),
        "the opening picks are far apart, so the reveal sweep removed nothing"
    );

    // Black's three opening stones lie along the bottom edge. Wall off
    // every liberty but (3, 0), then let White take that one while Black
    // plays at the far corner: a sequential round that captures the group.
    for x in 0..3 {
        state.progress.board.set(Point::new(x, 1), Cell::White);
    }
    state.cursors = [Point::new(8, 0), Point::new(3, 0)];
    assert!(matches!(
        state.try_lock(Player::One, MatchInstant::from_secs_f64(1.0)),
        LockOutcome::Locked { .. }
    ));
    assert!(matches!(
        state.try_lock(Player::Two, MatchInstant::from_secs_f64(1.5)),
        LockOutcome::Locked { .. }
    ));
    let outcome = state
        .resolve_ordinary_round(OrdinaryCompletion::Normal)
        .expect("both selections resolve");

    let taken: Vec<RemovedStone> = (0..3)
        .map(|x| RemovedStone {
            point: Point::new(x, 0),
            owner: Player::One,
            by: Some(Point::new(3, 0)),
        })
        .collect();
    assert_eq!(outcome.removed, taken);
    assert_eq!(outcome.capture_credits, [0, 3]);
    let view = PublicMatchView::of(&state);
    assert_eq!(
        view.last_removals,
        RoundRemovals {
            round: 1,
            stones: taken,
        }
    );
    assert_eq!(view.board.get(Point::new(0, 0)), Cell::Empty);

    // The next resolution replaces the record, even when it removes nothing.
    state.cursors = [Point::new(8, 8), Point::new(7, 7)];
    state.try_lock(Player::One, MatchInstant::from_secs_f64(2.0));
    state.try_lock(Player::Two, MatchInstant::from_secs_f64(2.5));
    let quiet = state
        .resolve_ordinary_round(OrdinaryCompletion::Normal)
        .expect("both selections resolve");
    assert!(quiet.removed.is_empty());
    assert_eq!(
        PublicMatchView::of(&state).last_removals,
        RoundRemovals {
            round: 2,
            stones: Vec::new(),
        }
    );
}
