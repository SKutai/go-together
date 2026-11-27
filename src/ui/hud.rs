//! State, components and systems shared by more than one screen's HUD.
//!
//! # Responsibility
//!
//! Three vocabularies live here because several screens speak them:
//!
//! * [`HudState`] - which modal, if any, is open, and which help page is
//!   shown. [`sync_gameplay_pause`] translates it into gameplay's neutral
//!   [`crate::gameplay::GameplayPause`] every frame, which is how a modal
//!   pauses the clock and swallows gameplay input without gameplay knowing
//!   what a modal is; see [`modal_pause_state`].
//! * [`HudAction`] - the intent attached to a button. [`apply_hud_action`] is
//!   the single place that maps an intent to a screen change or a HUD modal
//!   change (help, return-home), so that behaviour cannot drift between
//!   screens. The Settings screen's own intents - the four tab switches and
//!   the reset-mappings confirmation - pass through it untouched and are
//!   routed by [`super::settings`]'s own button system, because their state
//!   lives in that screen's resource; only their click cue
//!   ([`hud_action_ui_cue`]) is decided here.
//! * [`UiRole`] and [`HudPanel`] - markers that let a refresh system fill
//!   text and toggle visibility by role rather than by a query per widget.
//!   The roles are divided: this module fills the two modals' text, and the
//!   Match screen's display pass fills the rest.
//!
//! [`HudPlugin`] also owns the one entity in this vocabulary that belongs to
//! no screen: the Help modal (`super::help`), spawned once at
//! `Startup` because it is shown from Home and from the Match, and closed
//! by [`close_hud_modals_on_screen_change`] on every screen change together
//! with the rest of [`HudState`].
//!
//! # Generated copy
//!
//! `update_shared_display` fills two roles whose text depends on the live
//! bindings: the help modal's pages, through [`help_page`], and the
//! return-home confirmation's body, through [`return_home_body`]. Both name
//! whatever keys the player has actually bound, so remapping a control updates
//! them the same frame. Neither is bindings-driven throughout - both also
//! spell out the fixed three-second hold, and the confirmation's "starting in
//! Round 1" qualifier is a rule rather than a binding - so the generated part
//! is the key names, not the whole sentence. Both are generated only while
//! their modal is open: the Help modal now exists on every screen,
//! and its copy is the better part of a kilobyte of formatted text per frame
//! that nobody could see. The frame a modal opens fills its text before
//! layout, so a remap made while it was closed is shown on its first frame.
//!
//! # Data flow
//!
//! `handle_ui_buttons` (in [`AppSystems::HudInput`]) turns this frame's
//! activations of [`HudAction`] buttons - by pointer, keyboard or
//! controller alike, see [`super::button`] - into state changes; [`sync_gameplay_pause`] (chained right after, still in
//! [`AppSystems::HudInput`], so it runs before gameplay's
//! [`AppSystems::MatchUpdate`] in the same frame) republishes that state as
//! [`crate::gameplay::GameplayPause`]; `update_shared_display` (in
//! [`AppSystems::HudDisplay`]) re-renders from HUD state near the end of the
//! frame. Opening or closing a modal always re-arms the input-release gate so
//! the same press cannot both dismiss a modal and act on the board underneath
//! it.
//!
//! That display pass also styles every button that carries a [`HudAction`],
//! on whichever screen it sits, unless the screen has marked the button
//! [`ScreenStyled`] and styles it itself. The marker is what lets this pass
//! stay unordered against the screens' own display stages.

use bevy::prelude::*;
use bevy::state::state::{StateTransitionEvent, StateTransitionSystems};

use super::button::{Activated, PointerHighlight, UiButtonPlugin};
use super::help::{
    HELP_PAGE_COUNT, help_navigation_unavailable, help_page, help_page_indicator, spawn_help_modal,
};
use super::match_screen::end_gesture_keys;
use crate::app::{AppScreen, AppSystems};
use crate::audio::{UiCue, UiSound};
use crate::gameplay::{GameState, GameplayPause, InputGate, MatchPhase};
use crate::input::keyboard::KeyboardBindings;
use crate::presentation::button_background;
use crate::rules::Player;
use crate::settings::GamePreferences;
use crate::ui::ControlBindings;

/// Owns [`HudState`], the Help modal's one entity, and the
/// screen-agnostic HUD systems.
pub(super) struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        UiButtonPlugin::require(app);
        app.init_resource::<HudState>()
            .add_systems(Startup, spawn_help_modal)
            // After the transition has been applied and its message written,
            // and before the exit and enter schedules run, all in the same
            // schedule: the modal is already closed when the leaving screen's
            // `OnExit` chain, the entering screen's `OnEnter` chain and this
            // frame's `Update` run, so state and display never disagree for a
            // frame. Both edges are stated, because `after` alone would leave
            // this unordered against the enter schedules.
            .add_systems(
                StateTransition,
                close_hud_modals_on_screen_change
                    .after(StateTransitionSystems::DependentTransitions)
                    .before(StateTransitionSystems::ExitSchedules),
            )
            .add_systems(
                Update,
                (
                    (handle_ui_buttons, sync_gameplay_pause)
                        .chain()
                        .in_set(AppSystems::HudInput),
                    update_shared_display.in_set(AppSystems::HudDisplay),
                ),
            );
    }
}

/// Which overlay is open, and where the player is in the help modal.
///
/// Reset wholesale by [`close_hud_modals_on_screen_change`] on every screen
/// change, so no modal survives navigation: the Help modal is one
/// application-persistent entity shown from two screens, so no screen's
/// `OnEnter` could own closing it, and the Return Home confirmation's flag
/// would otherwise outlive the Match after a confirmed return, since
/// `ConfirmReturnHome` leaves it set while it changes screen.
#[derive(Resource, Default, Debug)]
pub(crate) struct HudState {
    pub(crate) help_open: bool,
    pub(crate) help_page: usize,
    pub(crate) return_home_open: bool,
}

#[derive(Component)]
/// What a piece of HUD text means.
///
/// One marker per text entity lets a refresh system fill text in by role.
/// Two do, and they divide the roles between them so no text has two
/// writers: `update_shared_display` fills the Help and Return Home modals'
/// roles, and the Match screen's display pass fills the others.
pub(super) enum UiRole {
    Heading,
    EndReasons,
    PlayerOne,
    PlayerTwo,
    Gesture,
    Timer,
    Round,
    Countdown,
    ControlHint(Player),
    HelpTitle,
    HelpBody,
    HelpPage,
    ReturnHomeBody,
}

/// Marks a button whose fill and click cue belong to its own screen.
///
/// A button carries a [`HudAction`] so that its press is read in one place,
/// and by default that also puts it in `update_shared_display`'s hands:
/// every frame, that pass sets the fill from the pointer state and switches
/// the click cue on - or, for this module's own Help page arrows at either
/// end of the pages, draws the button disabled and switches the cue off. A
/// screen whose button means more than that - the Settings tabs, whose
/// selected tab is drawn inert and makes no sound - restyles it in a display
/// system of its own, and two display systems writing one fill is a result
/// that depends on which runs last. Nothing orders
/// [`AppSystems::HudDisplay`] against a screen's own display stage, and
/// nothing needs to: a button with this marker is skipped here, so its fill
/// and whether it clicks are written by the screen's display system alone.
///
/// The screen inserts it; this module only honours it. That keeps the
/// dependency pointing the usual way - screens speak this module's
/// vocabulary, and it knows nothing about theirs.
#[derive(Component, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScreenStyled;

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
/// What pressing a button is meant to do.
///
/// Buttons carry an intent rather than a callback, so navigation, modal state
/// and the click cue are all decided in one place.
pub(crate) enum HudAction {
    PlayMatch,
    OpenReplays,
    OpenControlMapping,
    BackToHome,
    OpenHelp,
    CloseHelp,
    PreviousHelpPage,
    NextHelpPage,
    OpenReturnHome,
    CancelReturnHome,
    ConfirmReturnHome,
    RequestResetMappings,
    CancelResetMappings,
    ConfirmResetMappings,
    SwitchToPreferencesTab,
    SwitchToGameTab,
    SwitchToKeyboardTab,
    SwitchToControllerTab,
}

/// The click cue an action should make: cancel for anything that goes back or
/// dismisses, confirm for anything that commits or advances, neutral for the
/// rest. Deriving it from the intent keeps the audio consistent for free.
pub(crate) const fn hud_action_ui_cue(action: HudAction) -> UiCue {
    match action {
        HudAction::BackToHome
        | HudAction::CloseHelp
        | HudAction::PreviousHelpPage
        | HudAction::CancelReturnHome
        | HudAction::CancelResetMappings => UiCue::Cancel,
        HudAction::RequestResetMappings
        | HudAction::SwitchToPreferencesTab
        | HudAction::SwitchToGameTab
        | HudAction::SwitchToKeyboardTab
        | HudAction::SwitchToControllerTab => UiCue::Neutral,
        HudAction::PlayMatch
        | HudAction::OpenReplays
        | HudAction::OpenControlMapping
        | HudAction::OpenHelp
        | HudAction::NextHelpPage
        | HudAction::OpenReturnHome
        | HudAction::ConfirmReturnHome
        | HudAction::ConfirmResetMappings => UiCue::Confirm,
    }
}

#[derive(Component)]
/// A HUD region whose visibility is driven by state or preference.
pub(super) enum HudPanel {
    Timer,
    Round,
    /// The line under the Match screen's heading that says why the Match
    /// ended: laid out only once it has, so during play the timer and the
    /// round panel below it stand exactly where they always have.
    EndReasons,
    Countdown,
    HelpModal,
    ReturnHomeModal,
}

/// Whether any overlay is covering the board. Gameplay pauses while this is
/// true, so a modal cannot hide a match that is still running.
pub(crate) fn modal_open(hud: &HudState) -> bool {
    hud.help_open || hud.return_home_open
}

/// Translates HUD/modal state into gameplay's neutral [`GameplayPause`].
///
/// Today's shared-screen game always blocks local input and pauses the
/// authoritative Match together; a future networked mode could instead have
/// only the local client's own modal set `input_blocked`, leaving
/// `match_paused` to a server signal instead of this function.
pub(crate) fn modal_pause_state(hud: &HudState) -> GameplayPause {
    if modal_open(hud) {
        GameplayPause::blocking()
    } else {
        GameplayPause::default()
    }
}

/// Republishes [`HudState`] as [`GameplayPause`] every frame, so gameplay
/// never needs to know a modal exists. Runs immediately after
/// [`handle_ui_buttons`] and before gameplay's match-update systems in the
/// same frame.
pub(crate) fn sync_gameplay_pause(hud: Res<HudState>, mut pause: ResMut<GameplayPause>) {
    *pause = modal_pause_state(&hud);
}

/// Resets [`HudState`] on every screen change, closing whichever modal was
/// open - the one place that happens, for every screen at once.
///
/// One screen-agnostic system rather than a reset in each screen's `OnEnter`
/// chain, for the same reason the Help modal is spawned once: the
/// modal belongs to no screen, so no screen's entry is the right owner of
/// closing it, and a per-screen reset is exactly the kind of duplicated
/// obligation a new screen would forget. Runs in `StateTransition` after
/// the transition is applied and before the exit and enter schedules,
/// reading the transition message rather than enumerating screens, and
/// fires exactly when a screen's `OnEnter` would:
/// on a change of screen, and on a same-screen transition only if it was
/// asked to run the enter and exit schedules - which a plain `NextState::set`
/// is, and `set_if_different` is not.
fn close_hud_modals_on_screen_change(
    mut transitions: MessageReader<StateTransitionEvent<AppScreen>>,
    mut hud: ResMut<HudState>,
) {
    let screen_changed = transitions.read().any(|transition| {
        transition.exited != transition.entered || transition.allow_same_state_transitions
    });
    if screen_changed {
        *hud = HudState::default();
    }
}

pub(crate) fn handle_ui_buttons(
    mut activated: Activated<HudAction>,
    mut hud: ResMut<HudState>,
    mut input_gate: ResMut<InputGate>,
    mut next_screen: ResMut<NextState<AppScreen>>,
) {
    for action in activated.read() {
        if let Some(screen) = apply_hud_action(&mut hud, &mut input_gate, *action) {
            next_screen.set(screen);
        }
    }
}

/// Applies a button intent, returning the screen to switch to, if any.
///
/// Every path that opens or closes an overlay also re-arms the input gate, so
/// the press that dismissed a modal cannot also act on the board underneath.
pub(crate) fn apply_hud_action(
    hud: &mut HudState,
    input_gate: &mut InputGate,
    action: HudAction,
) -> Option<AppScreen> {
    match action {
        HudAction::PlayMatch => return Some(AppScreen::Match),
        HudAction::OpenReplays => return Some(AppScreen::ReplayLibrary),
        HudAction::OpenControlMapping => return Some(AppScreen::ControlMapping),
        HudAction::BackToHome => return Some(AppScreen::Home),
        HudAction::OpenHelp => {
            hud.help_open = true;
            hud.help_page = 0;
            hud.return_home_open = false;
            input_gate.arm();
        }
        HudAction::CloseHelp => {
            hud.help_open = false;
            input_gate.arm();
        }
        HudAction::PreviousHelpPage => {
            hud.help_page = hud.help_page.saturating_sub(1);
        }
        HudAction::NextHelpPage => {
            hud.help_page = (hud.help_page + 1).min(HELP_PAGE_COUNT - 1);
        }
        HudAction::OpenReturnHome => {
            hud.help_open = false;
            hud.return_home_open = true;
            input_gate.arm();
        }
        HudAction::CancelReturnHome => {
            hud.return_home_open = false;
            input_gate.arm();
        }
        // Confirming requires the confirmation to still be open: with the
        // Match exit reading Escape in `PreUpdate` and the pointer's click
        // read later in `Update`, a click on Confirm can share a frame with
        // the dismissal it lost to, and it must not abandon a match the
        // player just chose to keep. (Before the keyboard exit existed this
        // could not happen - nothing else wrote to this dialog's state.)
        HudAction::ConfirmReturnHome => {
            if hud.return_home_open {
                return Some(AppScreen::Home);
            }
        }
        HudAction::RequestResetMappings
        | HudAction::CancelResetMappings
        | HudAction::ConfirmResetMappings
        | HudAction::SwitchToPreferencesTab
        | HudAction::SwitchToGameTab
        | HudAction::SwitchToKeyboardTab
        | HudAction::SwitchToControllerTab => {}
    }
    None
}

/// Every button this pass styles: one carrying a [`HudAction`] that its own
/// screen has not claimed with [`ScreenStyled`].
type SharedButtons<'w, 's> = Query<
    'w,
    's,
    (
        PointerHighlight,
        &'static HudAction,
        &'static mut BackgroundColor,
        &'static mut UiSound,
    ),
    Without<ScreenStyled>,
>;

pub(super) fn update_shared_display(
    game: Res<GameState>,
    hud: Res<HudState>,
    preferences: Res<GamePreferences>,
    controls: ControlBindings,
    mut texts: Query<(&UiRole, &mut Text)>,
    mut panels: Query<(&HudPanel, &mut Node)>,
    mut buttons: SharedButtons,
) {
    let (timer_visible, round_visible) = match_hud_visibility(&preferences);
    for (role, mut text) in &mut texts {
        // A closed modal's copy is not generated at all - see the module
        // docs. It is filled on the frame the modal opens, before layout.
        let value = match role {
            UiRole::HelpTitle | UiRole::HelpBody | UiRole::HelpPage if !hud.help_open => {
                continue;
            }
            UiRole::ReturnHomeBody if !hud.return_home_open => continue,
            UiRole::HelpTitle => {
                help_page(
                    hud.help_page,
                    &controls.keyboard,
                    &controls.controller,
                    &controls.assignments,
                )
                .0
            }
            UiRole::HelpBody => {
                help_page(
                    hud.help_page,
                    &controls.keyboard,
                    &controls.controller,
                    &controls.assignments,
                )
                .1
            }
            UiRole::HelpPage => help_page_indicator(hud.help_page),
            UiRole::ReturnHomeBody => return_home_body(game.phase, &controls.keyboard),
            UiRole::Heading
            | UiRole::EndReasons
            | UiRole::PlayerOne
            | UiRole::PlayerTwo
            | UiRole::Gesture
            | UiRole::Timer
            | UiRole::Round
            | UiRole::Countdown
            | UiRole::ControlHint(_) => continue,
        };
        if **text != value {
            **text = value;
        }
    }

    for (panel, mut node) in &mut panels {
        let target = match panel {
            HudPanel::Timer if timer_visible => Display::Flex,
            HudPanel::Round if round_visible => Display::Flex,
            HudPanel::EndReasons if game.phase == MatchPhase::Ended => Display::Flex,
            HudPanel::Countdown if crate::gameplay::countdown_text(&game).is_some() => {
                Display::Flex
            }
            HudPanel::HelpModal if hud.help_open => Display::Flex,
            HudPanel::ReturnHomeModal if hud.return_home_open => Display::Flex,
            HudPanel::Timer
            | HudPanel::Round
            | HudPanel::EndReasons
            | HudPanel::Countdown
            | HudPanel::HelpModal
            | HudPanel::ReturnHomeModal => Display::None,
        };
        if node.display != target {
            node.display = target;
        }
    }

    // The pointer's highlight (hover/press) can change every frame with no
    // HUD-state change at all, so this stays a true per-frame system for
    // responsiveness. The background write is guarded so an unchanged colour
    // does not mark the component changed; `set_enabled` is a plain bool store
    // and is left unconditional. A `ScreenStyled` button is not in this
    // query: its screen writes both.
    for (highlight, action, mut background, mut sound) in &mut buttons {
        let unavailable = help_navigation_unavailable(*action, hud.help_page);
        let target = button_background(highlight.get(), false, unavailable);
        if background.0 != target {
            background.0 = target;
        }
        sound.set_enabled(!unavailable);
    }
}

/// The return-home confirmation's body, which depends on the Match phase.
///
/// An `Ended` Match is already scored and already saved, so leaving it costs
/// nothing and the copy is a plain question. Every other phase is a Match in
/// progress, where Home is a quit-now button: it abandons the Match and saves
/// no Replay. That is deliberate, but a player who reaches for Home because
/// they cannot find how to end a Match properly would lose the Replay while
/// looking for the very thing that would have produced one. So the warning
/// also names the way out.
///
/// The keys come from [`end_gesture_keys`], which reads the live
/// [`KeyboardBindings`], so this modal cannot name keys the Match screen's
/// own bottom hint does not and a remap updates both at once. "Starting in
/// Round 1" is not decoration: `gameplay::holds` ignores the gesture during
/// `Opening` and `OpeningCountdown`, and Round 0 - where nothing has happened
/// yet and the bottom hint is deliberately blank - is exactly where a lost
/// player reaches for Home. Without the qualifier the instruction would be
/// false in the case it exists to serve.
/// Both bodies end with the fixed keyboard shortcuts `ui`'s focus module
/// reads while this confirmation is open (Enter confirms, Escape cancels;
/// they are not bindings, so they are literals here like the menu keys in
/// `help`). The pad equivalents live on the help modal's shared-controller
/// page instead of adding controller prose to this one.
fn return_home_body(phase: MatchPhase, keyboard: &KeyboardBindings) -> String {
    if phase == MatchPhase::Ended {
        return "Leave the completed match and return to the title screen?\n\n\
                Enter confirms. Escape cancels."
            .into();
    }
    format!(
        "Your current match will be abandoned and no Replay is saved. This \
         cannot be undone.\n\n\
         Only a match that is ended properly is recorded. Cancel if you meant \
         to score and save this one: starting in Round 1, hold {} for 3 \
         seconds to end the match.\n\n\
         Enter confirms. Escape cancels.",
        end_gesture_keys(keyboard)
    )
}

/// Visibility of the Match timer and the round panel, as a `(timer, round)`
/// pair, and both halves are the player's preference and nothing else. A
/// running Round Time clock does not force the round panel on here: an
/// enabled timed rule reveals its readout by switching the preference on at
/// the moment the rule is enabled (`ui::settings`'s `apply_game_toggle`),
/// which leaves the player free to hide it again. The indication of a
/// running round clock that cannot be hidden is the bar `ui::match_screen`
/// draws below the board, not this panel.
pub(crate) fn match_hud_visibility(preferences: &GamePreferences) -> (bool, bool) {
    (preferences.show_match_timer, preferences.show_round_number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MatchDuration;
    use crate::gameplay::countdown::{
        advance_match_state_by, begin_active_match, match_timer_running,
    };

    use crate::gameplay::{PLAYER_STARTS, initialize_home_state};
    use crate::input::keyboard::KeyboardKey;
    use crate::input::{GameAction, SemanticInput};
    use crate::rules::{Board, Cell, Point};

    use crate::ui::settings::{PreferenceToggle, toggle_preference};

    #[test]
    fn hidden_timer_continues_tracking_and_preference_changes_are_immediate() {
        let mut game = GameState::default();
        let mut preferences = GamePreferences::default();
        begin_active_match(&mut game);

        assert_eq!(match_hud_visibility(&preferences), (false, true));
        advance_match_state_by(&mut game, MatchDuration::from_secs(3), false);

        assert_eq!(game.elapsed.as_secs_f64(), 3.0);
        toggle_preference(&mut preferences, PreferenceToggle::MatchTimer);
        assert_eq!(match_hud_visibility(&preferences), (true, true));
        assert_eq!(game.elapsed.as_secs_f64(), 3.0);
    }

    /// A running round clock no longer forces the round panel on: the
    /// preference alone decides, so a player who turned the panel off keeps
    /// it off, and hiding it stops nothing - the countdown underneath keeps
    /// its rounded-up seconds.
    #[test]
    fn a_running_round_clock_leaves_the_round_panel_to_the_preference() {
        let mut settings = crate::match_settings::MatchSettings::default();
        settings.set_value(crate::match_settings::MatchSettingField::RoundTime, 60);
        settings.set_enabled(crate::match_settings::MatchSettingField::RoundTime, true);
        let mut game = GameState::with_match_settings(&settings);
        let hidden = GamePreferences {
            show_round_number: false,
            ..default()
        };
        let shown = GamePreferences {
            show_round_number: true,
            ..default()
        };

        assert_eq!(match_hud_visibility(&hidden), (false, false));
        begin_active_match(&mut game);
        game.round_elapsed = MatchDuration::from_secs_f64(59.01);
        assert_eq!(game.round_time_remaining_seconds(), Some(1));
        assert_eq!(match_hud_visibility(&hidden), (false, false));
        assert_eq!(match_hud_visibility(&shown), (false, true));
    }

    #[test]
    fn modal_pause_state_blocks_input_and_match_together_for_any_open_modal() {
        let mut hud = HudState::default();
        assert_eq!(modal_pause_state(&hud), GameplayPause::default());

        hud.help_open = true;
        assert_eq!(modal_pause_state(&hud), GameplayPause::blocking());

        hud.help_open = false;
        hud.return_home_open = true;
        assert_eq!(modal_pause_state(&hud), GameplayPause::blocking());

        hud.return_home_open = false;
        assert_eq!(modal_pause_state(&hud), GameplayPause::default());
    }

    #[test]
    fn help_pages_stop_at_boundaries() {
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();

        assert!(help_navigation_unavailable(
            HudAction::PreviousHelpPage,
            hud.help_page
        ));
        assert!(!help_navigation_unavailable(
            HudAction::NextHelpPage,
            hud.help_page
        ));
        apply_hud_action(&mut hud, &mut input_gate, HudAction::PreviousHelpPage);
        assert_eq!(hud.help_page, 0);

        for _ in 0..HELP_PAGE_COUNT + 2 {
            apply_hud_action(&mut hud, &mut input_gate, HudAction::NextHelpPage);
        }
        assert_eq!(hud.help_page, HELP_PAGE_COUNT - 1);
        assert!(!help_navigation_unavailable(
            HudAction::PreviousHelpPage,
            hud.help_page
        ));
        assert!(help_navigation_unavailable(
            HudAction::NextHelpPage,
            hud.help_page
        ));
        assert_eq!(help_page_indicator(hud.help_page), "Page 11 of 11");

        apply_hud_action(&mut hud, &mut input_gate, HudAction::CloseHelp);
        apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenHelp);
        assert_eq!(hud.help_page, 0);
        assert_eq!(help_page_indicator(hud.help_page), "Page 1 of 11");
    }

    #[test]
    fn control_mapping_navigation_does_not_start_a_match() {
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();

        assert_eq!(
            apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenControlMapping),
            Some(AppScreen::ControlMapping)
        );
    }

    #[test]
    fn return_home_confirmation_pauses_and_cancel_preserves_match() {
        let mut game = GameState::default();
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();
        begin_active_match(&mut game);
        game.board.set(Point::new(4, 4), Cell::Black);
        game.cursors = [Point::new(3, 4), Point::new(5, 4)];
        // The restart hold, because it is the one a modal freezes: a modal
        // interrupts a running end hold outright, returning its timer to
        // zero as it opens.
        game.restart_hold = MatchDuration::from_secs_f64(1.25);
        game.elapsed = crate::clock::MatchInstant::from_secs_f64(42.0);

        assert_eq!(
            apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenReturnHome),
            None
        );
        assert!(hud.return_home_open);
        assert!(modal_pause_state(&hud).input_blocked);
        assert!(input_gate.any_gated());
        assert!(!match_timer_running(
            game.phase,
            modal_pause_state(&hud).match_paused
        ));

        advance_match_state_by(
            &mut game,
            MatchDuration::from_secs(10),
            modal_pause_state(&hud).match_paused,
        );
        assert_eq!(game.elapsed.as_secs_f64(), 42.0);
        assert_eq!(game.restart_hold, MatchDuration::from_secs_f64(1.25));

        assert_eq!(
            apply_hud_action(&mut hud, &mut input_gate, HudAction::CancelReturnHome),
            None
        );
        assert!(!hud.return_home_open);
        // Closing the modal lifts the pause but leaves the gate armed, so the
        // press that dismissed it cannot also reach the board. The gate then
        // narrows to the inputs actually held, one action at a time.
        assert!(!modal_pause_state(&hud).input_blocked);
        assert!(input_gate.any_gated());
        assert_eq!(game.board.get(Point::new(4, 4)), Cell::Black);
        assert_eq!(game.cursors, [Point::new(3, 4), Point::new(5, 4)]);
        assert_eq!(game.elapsed.as_secs_f64(), 42.0);

        // With nothing actually held, the next gate update releases every
        // action it armed.
        input_gate.release_unheld(&SemanticInput::default());
        assert!(!input_gate.any_gated());
        assert!(match_timer_running(
            game.phase,
            modal_pause_state(&hud).match_paused
        ));
    }

    #[test]
    fn confirmed_return_home_cleans_abandoned_match_and_modal_state() {
        let mut game = GameState::default();
        let mut hud = HudState::default();
        let mut input_gate = InputGate::default();
        game.board.set(Point::new(4, 4), Cell::White);
        game.teleport_markers = [Some(Point::new(3, 3)), None];
        game.elapsed = crate::clock::MatchInstant::from_secs_f64(90.0);

        // A confirm with no confirmation open is a lost race against the
        // dismissal (the keyboard exit closes the dialog in PreUpdate, a
        // same-frame click is read later) and must do nothing.
        assert_eq!(
            apply_hud_action(&mut hud, &mut input_gate, HudAction::ConfirmReturnHome),
            None
        );

        let _ = apply_hud_action(&mut hud, &mut input_gate, HudAction::OpenReturnHome);
        assert_eq!(
            apply_hud_action(&mut hud, &mut input_gate, HudAction::ConfirmReturnHome),
            Some(AppScreen::Home)
        );
        // Mirrors the transition to Home: `prepare_home_screen` resets the
        // Match state and `close_hud_modals_on_screen_change` resets the
        // HUD, two systems on the same screen change. The confirmation's
        // flag is still set at this point - that reset is what clears it.
        assert!(hud.return_home_open);
        initialize_home_state(&mut game, &mut input_gate);
        hud = HudState::default();

        assert_eq!(game.board, Board::default());
        assert_eq!(game.cursors, PLAYER_STARTS);
        assert_eq!(game.teleport_markers, [None, None]);
        assert_eq!(
            game.completed_history,
            std::collections::HashSet::from([Board::default()])
        );
        assert_eq!(game.phase, MatchPhase::Opening);
        assert_eq!(game.elapsed.as_secs_f64(), 0.0);
        assert!(!hud.help_open);
        assert!(!hud.return_home_open);
        assert!(!input_gate.any_gated());
    }

    #[test]
    fn unfinished_return_home_warns_that_no_replay_is_saved_and_follows_remapped_selects() {
        let mut keyboard = KeyboardBindings::default();

        // Pinned whole, because this is the last thing a player reads before
        // Home throws a Match away. It owes them three things: what is lost,
        // that nothing is written, and the gesture that would have written
        // it.
        assert_eq!(
            return_home_body(MatchPhase::Playing, &keyboard),
            "Your current match will be abandoned and no Replay is saved. This cannot be \
             undone.\n\nOnly a match that is ended properly is recorded. Cancel if you meant \
             to score and save this one: starting in Round 1, hold M and 2 for 3 seconds to \
             end the match.\n\nEnter confirms. Escape cancels."
        );

        // The instruction follows the player's bindings, not the defaults -
        // the whole reason it is generated rather than written out.
        keyboard.assign(GameAction::PlayerOneSelect, KeyboardKey::F8);
        keyboard.assign(GameAction::PlayerTwoSelect, KeyboardKey::UpArrow);
        let remapped = return_home_body(MatchPhase::Playing, &keyboard);
        assert!(remapped.contains("hold F8 and Up for 3 seconds to end the match."));
        assert!(!remapped.contains("M and 2"));

        // Printable ASCII only: the bundled fallback font covers nothing else.
        assert!(
            remapped
                .chars()
                .all(|character| character == '\n' || (' '..='~').contains(&character))
        );
    }

    #[test]
    fn every_unfinished_phase_warns_and_a_scored_match_is_left_alone() {
        let keyboard = KeyboardBindings::default();

        // Round 0 is the likeliest place to reach for Home, and
        // `gameplay::holds` ignores the end gesture in both of its phases, so
        // the instruction has to carry the qualifier there or it is a lie
        // exactly where it is needed.
        for phase in [
            MatchPhase::Opening,
            MatchPhase::OpeningCountdown,
            MatchPhase::Playing,
        ] {
            let body = return_home_body(phase, &keyboard);
            assert!(body.contains("no Replay is saved"), "{phase:?}");
            assert!(body.contains("starting in Round 1, hold"), "{phase:?}");
        }

        // An ended Match is already scored and already saved, so the same
        // warning would be false and the end gesture is spent.
        let ended = return_home_body(MatchPhase::Ended, &keyboard);
        assert_eq!(
            ended,
            "Leave the completed match and return to the title screen?\n\n\
             Enter confirms. Escape cancels."
        );
        assert!(!ended.contains("Replay"));
        assert!(!ended.contains("to end the match"));
    }

    /// The plugin's own registration, through real transitions: a screen
    /// change closes every HUD modal and forgets the help page, whichever
    /// screen is left or entered - and has done so before the entering
    /// screen's own `OnEnter` chain runs, which is the ordering the
    /// registration claims; a frame with no transition leaves a freshly
    /// opened modal alone; and the reset tracks `OnEnter` exactly on a
    /// same-screen transition - a plain `set` re-runs the enter schedules
    /// and resets (in Bevy 0.20 as in 0.19), and `set_if_different` runs
    /// neither. The Help modal is one entity, spawned at `Startup`, and
    /// the same entity survives every transition. (The `OnEnter` half holds
    /// by the declared `before(ExitSchedules)` edge; without it this app
    /// happens to pick the same order, so a dropped edge would pass here by
    /// scheduler luck rather than by guarantee - the edge, not this test, is
    /// the guarantee.)
    #[test]
    fn a_screen_change_closes_every_hud_modal_and_the_modal_entity_persists() {
        /// What an entering screen's `OnEnter` chain saw of the modal flags.
        #[derive(Resource, Default)]
        struct SeenAtEnter(Option<bool>);

        fn record_modal_state_at_enter(hud: Res<HudState>, mut seen: ResMut<SeenAtEnter>) {
            seen.0 = Some(modal_open(&hud));
        }

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_plugins(HudPlugin)
            .init_resource::<SeenAtEnter>()
            .insert_resource(GameState::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(InputGate::default())
            .insert_resource(GameplayPause::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(crate::input::controller::ControllerBindings::default())
            .insert_resource(crate::input::controller::ControllerAssignments::default());
        for screen in AppScreen::ALL {
            app.add_systems(OnEnter(screen), record_modal_state_at_enter);
        }
        app.update();
        let modal_entities = |app: &mut App| -> Vec<Entity> {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<HudPanel>>()
                .iter(world)
                .collect()
        };
        let modal = match modal_entities(&mut app)[..] {
            [modal] => modal,
            ref other => panic!("one Help modal, spawned at Startup, not {other:?}"),
        };
        let open_both = |app: &mut App| {
            let mut hud = app.world_mut().resource_mut::<HudState>();
            hud.help_open = true;
            hud.help_page = 4;
            hud.return_home_open = true;
        };

        open_both(&mut app);
        app.update();
        let hud = app.world().resource::<HudState>();
        assert!(
            hud.help_open && hud.return_home_open && hud.help_page == 4,
            "no transition, no reset"
        );

        for (from, to) in [
            (AppScreen::Home, AppScreen::Match),
            (AppScreen::Match, AppScreen::Home),
            (AppScreen::Home, AppScreen::ReplayLibrary),
        ] {
            assert_eq!(*app.world().resource::<State<AppScreen>>().get(), from);
            open_both(&mut app);
            app.world_mut().resource_mut::<SeenAtEnter>().0 = None;
            app.world_mut()
                .resource_mut::<NextState<AppScreen>>()
                .set(to);
            app.update();
            let hud = app.world().resource::<HudState>();
            assert!(!hud.help_open, "{from:?} -> {to:?}");
            assert!(!hud.return_home_open, "{from:?} -> {to:?}");
            assert_eq!(hud.help_page, 0, "{from:?} -> {to:?}");
            assert_eq!(
                app.world().resource::<SeenAtEnter>().0,
                Some(false),
                "{to:?}'s OnEnter chain must find every modal already closed"
            );
            assert_eq!(
                modal_entities(&mut app),
                [modal],
                "the same modal entity outlives {from:?} -> {to:?}"
            );
        }

        // A same-screen `set_if_different` runs no `OnEnter`, and no reset...
        open_both(&mut app);
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set_if_different(AppScreen::ReplayLibrary);
        app.update();
        assert!(app.world().resource::<HudState>().help_open);
        // ...while a plain `set` re-enters the screen, and resets with it.
        app.world_mut()
            .resource_mut::<NextState<AppScreen>>()
            .set(AppScreen::ReplayLibrary);
        app.update();
        assert!(!app.world().resource::<HudState>().help_open);
        assert_eq!(modal_entities(&mut app), [modal]);
    }

    /// A closed modal's copy is not generated: its text entities are left
    /// untouched while it is closed and filled on the frame it opens.
    #[test]
    fn modal_copy_is_generated_only_while_its_modal_is_open() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<AppScreen>()
            .add_plugins(HudPlugin)
            .insert_resource(GameState::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(InputGate::default())
            .insert_resource(GameplayPause::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(crate::input::controller::ControllerBindings::default())
            .insert_resource(crate::input::controller::ControllerAssignments::default());
        app.update();
        app.update();
        let help_texts = |app: &mut App| -> Vec<String> {
            let world = app.world_mut();
            let mut texts: Vec<(u8, String)> = world
                .query::<(&UiRole, &Text)>()
                .iter(world)
                .filter_map(|(role, text)| {
                    let rank = match role {
                        UiRole::HelpTitle => 0,
                        UiRole::HelpBody => 1,
                        UiRole::HelpPage => 2,
                        _ => return None,
                    };
                    Some((rank, text.0.clone()))
                })
                .collect();
            texts.sort();
            texts.into_iter().map(|(_, text)| text).collect()
        };
        assert_eq!(
            help_texts(&mut app),
            ["", "", ""],
            "closed, the modal's text is never generated"
        );

        app.world_mut().resource_mut::<HudState>().help_open = true;
        app.update();
        let [title, body, page] = <[String; 3]>::try_from(help_texts(&mut app)).unwrap();
        assert_eq!(title, "Keyboard Controls");
        assert!(body.starts_with("PLAYER 1 - BLACK"));
        assert_eq!(page, "Page 1 of 11");
    }

    /// The shared pass styles a `HudAction` button from its pointer state,
    /// and leaves one its screen has claimed exactly as the screen left it -
    /// with the pointer really over each in turn, under real layout.
    #[test]
    fn the_shared_display_pass_leaves_a_screen_styled_button_alone() {
        use crate::input::controller::{ControllerAssignments, ControllerBindings};
        use crate::presentation::{BUTTON_DISABLED, BUTTON_HOVERED};
        use crate::ui::button::hover;
        use crate::ui::focus::real_layout_app;
        use bevy::ui_widgets::Button;

        let mut app = real_layout_app();
        app.insert_resource(GameState::default())
            .insert_resource(HudState::default())
            .insert_resource(GamePreferences::default())
            .insert_resource(KeyboardBindings::default())
            .insert_resource(ControllerBindings::default())
            .insert_resource(ControllerAssignments::default())
            .add_systems(Update, update_shared_display);
        let button = || {
            (
                Button,
                Node {
                    width: px(200),
                    height: px(40),
                    ..default()
                },
                HudAction::SwitchToGameTab,
                UiSound::new(hud_action_ui_cue(HudAction::SwitchToGameTab)),
                BackgroundColor(BUTTON_DISABLED),
            )
        };
        let mut shared = Entity::PLACEHOLDER;
        let mut claimed = Entity::PLACEHOLDER;
        app.world_mut()
            .spawn(Node {
                column_gap: px(20),
                ..default()
            })
            .with_children(|row| {
                shared = row.spawn(button()).id();
                claimed = row.spawn((button(), ScreenStyled)).id();
            });
        app.update();

        let fill = |app: &App, entity| app.world().get::<BackgroundColor>(entity).unwrap().0;
        hover(&mut app, shared);
        assert_eq!(
            fill(&app, shared),
            BUTTON_HOVERED,
            "an unclaimed button takes the hover fill, so the pass really ran"
        );
        hover(&mut app, claimed);
        let fill = |entity| app.world().get::<BackgroundColor>(entity).unwrap().0;
        assert_ne!(
            BUTTON_HOVERED, BUTTON_DISABLED,
            "the two fills must differ for either assertion here to mean anything"
        );
        assert_eq!(
            fill(claimed),
            BUTTON_DISABLED,
            "a claimed button keeps the fill its screen gave it"
        );
    }

    #[test]
    fn hud_actions_use_positive_and_back_navigation_cues() {
        assert_eq!(hud_action_ui_cue(HudAction::PlayMatch), UiCue::Confirm);
        assert_eq!(hud_action_ui_cue(HudAction::NextHelpPage), UiCue::Confirm);
        assert_eq!(hud_action_ui_cue(HudAction::BackToHome), UiCue::Cancel);
        assert_eq!(
            hud_action_ui_cue(HudAction::CancelReturnHome),
            UiCue::Cancel
        );
        assert_eq!(
            hud_action_ui_cue(HudAction::SwitchToPreferencesTab),
            UiCue::Neutral
        );
    }
}
