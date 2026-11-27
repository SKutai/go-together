//! Fusion into semantic input: [`update_semantic_input`] ORs the keyboard
//! and each player's assigned controller into one [`SemanticInput`] per
//! frame. Gameplay reads only that, which is why every action behaves
//! identically whatever it came from.
//!
//! This is the one place a [`super::keyboard`] binding and a
//! [`super::controller`] profile meet, which is why it lives beside the
//! vocabulary they both target rather than inside either device module.
//! [`profile_action_for`] is the whole of the translation: it maps each
//! player-specific [`GameAction`] onto the profile-local [`ProfileAction`] a
//! pad is bound in terms of.
//!
//! Bevy supplies the same component/event surface for supported browser
//! Gamepad API devices; opaque browser device names naturally use universal
//! positional face-button labels.

use bevy::prelude::{ButtonInput, Gamepad, KeyCode, Query, Res, ResMut};

use super::controller::{
    ControllerAssignments, ControllerBindings, ControllerHysteresisState, ProfileAction,
    resolve_profile_action,
};
use super::keyboard::KeyboardBindings;
use super::{GameAction, SemanticInput};
use crate::rules::Player;

fn profile_action_for(action: GameAction) -> Option<(Player, ProfileAction)> {
    use GameAction::*;
    Some(match action {
        PlayerOneUp => (Player::One, ProfileAction::Up),
        PlayerOneDown => (Player::One, ProfileAction::Down),
        PlayerOneLeft => (Player::One, ProfileAction::Left),
        PlayerOneRight => (Player::One, ProfileAction::Right),
        PlayerOneSelect => (Player::One, ProfileAction::Select),
        PlayerOneSetMarker => (Player::One, ProfileAction::SetMarker),
        PlayerOneJumpToMarker => (Player::One, ProfileAction::JumpToMarker),
        PlayerTwoUp => (Player::Two, ProfileAction::Up),
        PlayerTwoDown => (Player::Two, ProfileAction::Down),
        PlayerTwoLeft => (Player::Two, ProfileAction::Left),
        PlayerTwoRight => (Player::Two, ProfileAction::Right),
        PlayerTwoSelect => (Player::Two, ProfileAction::Select),
        PlayerTwoSetMarker => (Player::Two, ProfileAction::SetMarker),
        PlayerTwoJumpToMarker => (Player::Two, ProfileAction::JumpToMarker),
        RestartMatch => return None,
    })
}

/// Recomputes [`SemanticInput`] for the frame from the keyboard (always
/// live) combined with whichever controller is assigned to each player,
/// applying hysteresis to stick/trigger derived directions. Entities whose
/// [`Gamepad`] component was removed (disconnected) simply fail the
/// `gamepads.get` lookup, so their inputs release automatically.
pub(crate) fn update_semantic_input(
    keys: Res<ButtonInput<KeyCode>>,
    keyboard_bindings: Res<KeyboardBindings>,
    controller_bindings: Res<ControllerBindings>,
    assignments: Res<ControllerAssignments>,
    gamepads: Query<&Gamepad>,
    mut hysteresis: ResMut<ControllerHysteresisState>,
    mut semantic: ResMut<SemanticInput>,
) {
    semantic.previous = semantic.current;
    for action in GameAction::ALL {
        let keyboard_down = keys.pressed(keyboard_bindings.key_code(action));
        let controller_down = match profile_action_for(action) {
            Some((player, profile_action)) => {
                let gamepad = assignments
                    .assigned_entity(player)
                    .and_then(|entity| gamepads.get(entity).ok());
                resolve_profile_action(
                    player,
                    profile_action,
                    &controller_bindings,
                    gamepad,
                    &mut hysteresis,
                )
            }
            None => Player::ALL.into_iter().any(|player| {
                let gamepad = assignments
                    .assigned_entity(player)
                    .and_then(|entity| gamepads.get(entity).ok());
                resolve_profile_action(
                    player,
                    ProfileAction::Restart,
                    &controller_bindings,
                    gamepad,
                    &mut hysteresis,
                )
            }),
        };
        semantic.current[action as usize] = keyboard_down || controller_down;
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{App, GamepadButton};

    use super::*;
    use crate::input::controller::{ControllerInput, DeviceFamily};
    use crate::input::keyboard::KeyboardKey;

    #[test]
    fn bevy_input_injection_combines_remapped_controller_keyboard_and_restart() {
        let mut app = App::new();
        let mut keyboard = KeyboardBindings::default();
        keyboard.assign(GameAction::PlayerOneSetMarker, KeyboardKey::F8);
        let mut controllers = ControllerBindings::default();
        controllers.assign(
            Player::Two,
            ProfileAction::JumpToMarker,
            ControllerInput::RightBumper,
        );
        let mut assignments = ControllerAssignments::default();
        let mut white_pad = Gamepad::default();
        white_pad.digital_mut().press(GamepadButton::RightTrigger);
        white_pad.digital_mut().press(GamepadButton::North);
        let white_entity = app.world_mut().spawn(white_pad).id();
        assignments.connect(
            white_entity,
            DeviceFamily::Unknown,
            "Injected controller".into(),
        );
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::F8);

        app.insert_resource(keys)
            .insert_resource(keyboard)
            .insert_resource(controllers)
            .insert_resource(assignments)
            .insert_resource(ControllerHysteresisState::default())
            .insert_resource(SemanticInput::default())
            .add_systems(bevy::prelude::Update, update_semantic_input);

        app.update();

        let semantic = app.world().resource::<SemanticInput>();
        assert!(semantic.just_pressed(GameAction::PlayerOneSetMarker));
        assert!(semantic.just_pressed(GameAction::PlayerTwoJumpToMarker));
        assert!(semantic.just_pressed(GameAction::RestartMatch));
        assert!(!semantic.pressed(GameAction::PlayerOneJumpToMarker));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::F8);
        app.world_mut()
            .entity_mut(white_entity)
            .get_mut::<Gamepad>()
            .unwrap()
            .digital_mut()
            .release(GamepadButton::RightTrigger);
        app.update();

        let semantic = app.world().resource::<SemanticInput>();
        assert!(!semantic.pressed(GameAction::PlayerOneSetMarker));
        assert!(!semantic.pressed(GameAction::PlayerTwoJumpToMarker));
        assert!(semantic.pressed(GameAction::RestartMatch));
    }
}
