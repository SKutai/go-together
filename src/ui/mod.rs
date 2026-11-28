//! Every screen the player sees, and the shared vocabulary those screens use.
//!
//! # Responsibility
//!
//! `ui` owns layout, copy, styling and the systems that keep widgets in sync
//! with state. It never decides rules: it reads [`crate::gameplay`],
//! [`crate::replay`] and [`crate::replay_library`] state and writes back only
//! through explicit intents such as [`hud::HudAction`].
//!
//! # Submodules
//!
//! * [`hud`] - state and components shared by more than one screen: the modal
//!   flags, the [`hud::HudAction`] button vocabulary, the text roles, and
//!   the one Help modal entity, spawned once for both screens that
//!   show it.
//! * [`button`] - how a button is pressed: the pointer's press and release,
//!   and the one [`button::ButtonActivation`] that the pointer, the keyboard
//!   and the controller all produce for the screens' button handlers.
//! * [`focus`] - keyboard and controller operation of menu buttons: the
//!   [`focus::Focusable`] marker a screen puts on its buttons, and the focus
//!   ring over them that activates one.
//! * [`widgets`] - the two reusable button spawners.
//! * [`countdown_card`] - the Round 0 countdown card the Match screen and
//!   the Replay Viewer both draw over their boards, and where it stands.
//! * [`arrangement`] - the rule by which the Match screen and the Replay
//!   Viewer each choose between their tall and wide arrangements.
//! * [`help`] - the paginated Help modal, one entity shown on Home
//!   and Match.
//! * [`home`] - the title screen, including the marimba Easter egg.
//! * [`settings`] - display preferences, the Game tab's Match rules, and
//!   keyboard and controller remapping.
//! * [`match_screen`] - the live board, cursors, HUD and hold progress.
//! * [`replay`] - the Replay Library and the Replay Viewer.
//! * [`text_input`] - the caret-aware single-line editor behind the Replay
//!   Library's search and rename fields and the Game tab's numeric fields.
//! * [`text_metrics`] - how long and how tall a line of the game's text is
//!   drawn, for a layout that has to know before `bevy_ui` lays it out.
//! * [`line_breaks`] - where `bevy_ui` breaks the lines of the game's text,
//!   followed exactly, for a layout that has to know how many lines a text
//!   takes.
//!
//! # Bevy lifecycle
//!
//! [`UiPlugin`] adds one plugin per feature. Each screen plugin owns its
//! screen's `OnEnter`/`OnExit` chains. Home, Settings and Match schedule their
//! update systems into the shared [`AppSystems`](crate::app::AppSystems)
//! stages; the two Replay screens run their own chains after
//! [`UiAudioSet`](crate::audio::UiAudioSet) outside those stages, and
//! [`button::UiButtonPlugin`], [`hud::HudPlugin`] and
//! [`focus::UiFocusPlugin`] are screen-agnostic. No screen plugin has to
//! know about any other: the shared plugins are the vocabulary they speak
//! instead, and a screen opts into keyboard focus by inserting
//! [`focus::Focusable`] in its own layout, not by talking to another
//! plugin.

mod arrangement;
pub(crate) mod button;
mod countdown_card;
mod focus;
pub(crate) mod help;
mod home;
pub(crate) mod hud;
mod line_breaks;
pub(crate) mod match_screen;
mod replay;
pub(crate) mod settings;
mod text_input;
mod text_metrics;
mod widgets;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::input::controller::{ControllerAssignments, ControllerBindings};
use crate::input::keyboard::KeyboardBindings;

/// The three resources needed to name any control in player-facing text.
///
/// Labels always combine all three: what the key is bound to, what the
/// controller is bound to, and which controller (if any) is assigned to that
/// player - the last one decides whether a binding is shown with Xbox,
/// PlayStation, Nintendo or generic positional face-button names. Grouping
/// them keeps the display systems that render controls to a readable
/// parameter list.
#[derive(SystemParam)]
pub(super) struct ControlBindings<'w> {
    pub(super) keyboard: Res<'w, KeyboardBindings>,
    pub(super) controller: Res<'w, ControllerBindings>,
    pub(super) assignments: Res<'w, ControllerAssignments>,
}

/// Adds every screen plugin.
///
/// `binding_warning` is the message produced when persisted controls could not
/// be read at startup. It is threaded to the settings screen because that is
/// the only place the player can act on it.
pub(crate) struct UiPlugin {
    binding_warning: Option<String>,
}

impl UiPlugin {
    pub(crate) fn new(binding_warning: Option<String>) -> Self {
        Self { binding_warning }
    }
}

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // First, so every screen's buttons are spawned with the components
        // `button` makes `Button` require.
        app.add_plugins(button::UiButtonPlugin)
            .add_plugins(hud::HudPlugin)
            .add_plugins(focus::UiFocusPlugin)
            .add_plugins(home::HomePlugin)
            .add_plugins(settings::SettingsPlugin::new(self.binding_warning.clone()))
            .add_plugins(match_screen::MatchScreenPlugin)
            .add_plugins(replay::ReplayUiPlugin);
    }
}
