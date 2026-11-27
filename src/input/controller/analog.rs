//! Analog interpretation: raw axis/trigger reading as Bevy hands it over -
//! already filtered by Bevy's default `GamepadSettings`, and given no dead
//! zone of its own here - plus Schmitt-trigger hysteresis, so a stick or
//! trigger resting near its threshold does not chatter between pressed and
//! released every frame.
//!
//! [`raw_magnitude`] is a pure function of a [`bevy::prelude::Gamepad`]
//! value, and [`controller_input_pressed`] needs only that plus a
//! [`ControllerHysteresisState`] - no live ECS `World`, entity, or App is
//! needed to call either, which is what makes them directly unit-testable.

use std::collections::HashMap;

use bevy::prelude::{Gamepad, GamepadAxis, GamepadButton, Resource};

use super::{ControllerBindings, ControllerInput, ProfileAction};
use crate::rules::Player;

/// Schmitt-trigger state for analog-derived controller inputs, keyed by
/// player slot so each side's stick/trigger noise rejection is independent.
#[derive(Resource, Default)]
pub(crate) struct ControllerHysteresisState {
    active: HashMap<(Player, ControllerInput), bool>,
}

impl ControllerHysteresisState {
    pub(super) fn is_active(&self, player: Player, input: ControllerInput) -> bool {
        *self.active.get(&(player, input)).unwrap_or(&false)
    }

    pub(super) fn set_active(&mut self, player: Player, input: ControllerInput, active: bool) {
        self.active.insert((player, input), active);
    }

    /// Releases every held analog input for a player slot, called when that
    /// slot's controller disconnects so no stale "held" state survives.
    pub(crate) fn clear_player(&mut self, player: Player) {
        self.active.retain(|(slot, _), _| *slot != player);
    }
}

/// Sanitizes one raw axis/trigger reading before any press/release threshold
/// ever sees it: a missing reading, a non-finite one (`NaN`, `+-infinity` -
/// possible from a misbehaving driver, not from any real device this project
/// has seen), or one outside the documented `-1.0..=1.0` range is treated as
/// the resting value or clamped into range. This is a deliberate policy, not
/// an accident of how `NaN` happens to compare: a stray `NaN` reading
/// silently comparing as "not pressed" would still be technically safe, but
/// leaves what "safe" means up to floating-point trivia rather than stating
/// it outright.
fn sanitized_axis(value: Option<f32>) -> f32 {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(-1.0, 1.0))
        .unwrap_or(0.0)
}

pub(super) fn raw_magnitude(input: ControllerInput, gamepad: &Gamepad) -> f32 {
    match input {
        ControllerInput::LeftStickUp => sanitized_axis(gamepad.get(GamepadAxis::LeftStickY)),
        ControllerInput::LeftStickDown => -sanitized_axis(gamepad.get(GamepadAxis::LeftStickY)),
        ControllerInput::LeftStickLeft => -sanitized_axis(gamepad.get(GamepadAxis::LeftStickX)),
        ControllerInput::LeftStickRight => sanitized_axis(gamepad.get(GamepadAxis::LeftStickX)),
        ControllerInput::RightStickUp => sanitized_axis(gamepad.get(GamepadAxis::RightStickY)),
        ControllerInput::RightStickDown => -sanitized_axis(gamepad.get(GamepadAxis::RightStickY)),
        ControllerInput::RightStickLeft => -sanitized_axis(gamepad.get(GamepadAxis::RightStickX)),
        ControllerInput::RightStickRight => sanitized_axis(gamepad.get(GamepadAxis::RightStickX)),
        ControllerInput::LeftAnalogTrigger => {
            sanitized_axis(gamepad.get(GamepadButton::LeftTrigger2))
        }
        ControllerInput::RightAnalogTrigger => {
            sanitized_axis(gamepad.get(GamepadButton::RightTrigger2))
        }
        _ => 0.0,
    }
}

/// Resolves whether a single physical input is currently "pressed", applying
/// Bevy's own press/release thresholds (`ButtonSettings`, 0.75 and 0.65 by
/// default) for buttons/D-pad and this module's centralized Schmitt-trigger
/// thresholds for stick directions and analog triggers.
fn controller_input_pressed(
    input: ControllerInput,
    gamepad: &Gamepad,
    player: Player,
    hysteresis: &mut ControllerHysteresisState,
) -> bool {
    if let Some(button) = input.digital_button() {
        return gamepad.pressed(button);
    }

    let (press_threshold, release_threshold) = input.hysteresis_thresholds();
    let value = raw_magnitude(input, gamepad);
    let was_active = hysteresis.is_active(player, input);
    let now_active = if was_active {
        value > release_threshold
    } else {
        value > press_threshold
    };
    hysteresis.set_active(player, input, now_active);
    now_active
}

/// Resolves whether `player`'s assigned controller (if any) currently drives
/// `action`, per that player's profile bindings.
pub(crate) fn resolve_profile_action(
    player: Player,
    action: ProfileAction,
    bindings: &ControllerBindings,
    gamepad: Option<&Gamepad>,
    hysteresis: &mut ControllerHysteresisState,
) -> bool {
    let Some(gamepad) = gamepad else {
        return false;
    };
    let input = bindings.input(player, action);
    controller_input_pressed(input, gamepad, player, hysteresis)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gamepad_with_axis(axis: GamepadAxis, value: f32) -> Gamepad {
        let mut gamepad = Gamepad::default();
        gamepad.analog_mut().set(axis, value);
        gamepad
    }

    #[test]
    fn stick_direction_uses_hysteresis_to_reject_noise_near_threshold() {
        let mut hysteresis = ControllerHysteresisState::default();
        let low = gamepad_with_axis(GamepadAxis::LeftStickY, 0.40);
        assert!(!controller_input_pressed(
            ControllerInput::LeftStickUp,
            &low,
            Player::One,
            &mut hysteresis
        ));

        let high = gamepad_with_axis(GamepadAxis::LeftStickY, 0.60);
        assert!(controller_input_pressed(
            ControllerInput::LeftStickUp,
            &high,
            Player::One,
            &mut hysteresis
        ));

        // Once active, a dip that stays above the release threshold should
        // not flicker back to released even though it's below the press
        // threshold - this is the noise rejection hysteresis provides.
        let dip = gamepad_with_axis(GamepadAxis::LeftStickY, 0.40);
        assert!(controller_input_pressed(
            ControllerInput::LeftStickUp,
            &dip,
            Player::One,
            &mut hysteresis
        ));

        let released = gamepad_with_axis(GamepadAxis::LeftStickY, 0.20);
        assert!(!controller_input_pressed(
            ControllerInput::LeftStickUp,
            &released,
            Player::One,
            &mut hysteresis
        ));
    }

    #[test]
    fn analog_trigger_is_press_like_via_centralized_thresholds() {
        let mut hysteresis = ControllerHysteresisState::default();
        let mut gamepad = Gamepad::default();
        gamepad.analog_mut().set(GamepadButton::RightTrigger2, 0.65);

        assert!(controller_input_pressed(
            ControllerInput::RightAnalogTrigger,
            &gamepad,
            Player::One,
            &mut hysteresis
        ));

        gamepad.analog_mut().set(GamepadButton::RightTrigger2, 0.1);
        assert!(!controller_input_pressed(
            ControllerInput::RightAnalogTrigger,
            &gamepad,
            Player::One,
            &mut hysteresis
        ));
    }

    #[test]
    fn digital_face_button_is_pressed_directly() {
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::South);
        let mut hysteresis = ControllerHysteresisState::default();

        assert!(controller_input_pressed(
            ControllerInput::South,
            &gamepad,
            Player::One,
            &mut hysteresis
        ));
    }

    #[test]
    fn resolve_profile_action_is_false_without_an_assigned_gamepad() {
        let bindings = ControllerBindings::default();
        let mut hysteresis = ControllerHysteresisState::default();
        assert!(!resolve_profile_action(
            Player::One,
            ProfileAction::Select,
            &bindings,
            None,
            &mut hysteresis
        ));
    }

    #[test]
    fn sanitized_axis_treats_missing_and_non_finite_readings_as_resting() {
        assert_eq!(sanitized_axis(None), 0.0);
        assert_eq!(sanitized_axis(Some(f32::NAN)), 0.0);
        assert_eq!(sanitized_axis(Some(f32::INFINITY)), 0.0);
        assert_eq!(sanitized_axis(Some(f32::NEG_INFINITY)), 0.0);
    }

    #[test]
    fn sanitized_axis_clamps_an_out_of_range_reading_rather_than_passing_it_through() {
        assert_eq!(sanitized_axis(Some(1.4)), 1.0);
        assert_eq!(sanitized_axis(Some(-1.4)), -1.0);
        assert_eq!(sanitized_axis(Some(0.42)), 0.42);
    }

    #[test]
    fn a_nan_stick_reading_never_registers_as_pressed_and_never_panics() {
        let mut hysteresis = ControllerHysteresisState::default();
        let gamepad = gamepad_with_axis(GamepadAxis::LeftStickY, f32::NAN);
        assert!(!controller_input_pressed(
            ControllerInput::LeftStickUp,
            &gamepad,
            Player::One,
            &mut hysteresis
        ));
    }

    #[test]
    fn an_out_of_range_trigger_reading_is_clamped_not_passed_through_unbounded() {
        let mut hysteresis = ControllerHysteresisState::default();
        let mut gamepad = Gamepad::default();
        // A misbehaving driver reporting far past the documented range must
        // still compare as an ordinary, bounded "fully pressed" trigger -
        // not an unbounded value that happens to exceed every threshold.
        // Bevy's `Gamepad::get` clamps first, so this pins the whole path;
        // `sanitized_axis`'s own clamp is exercised directly above.
        gamepad.analog_mut().set(GamepadButton::RightTrigger2, 50.0);
        assert!(controller_input_pressed(
            ControllerInput::RightAnalogTrigger,
            &gamepad,
            Player::One,
            &mut hysteresis
        ));
        assert_eq!(
            raw_magnitude(ControllerInput::RightAnalogTrigger, &gamepad),
            1.0
        );
    }
}
