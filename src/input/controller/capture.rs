//! Controller-binding capture: a digital input is accepted on its
//! `just_pressed` edge; an analog input is accepted only on the frame its
//! magnitude first crosses the press threshold from below, judged against the
//! previous frame's reading held in [`ControllerCaptureState`]. That reading
//! is re-armed on entry, so a stick or trigger already held past the
//! threshold when capture begins does not bind itself - it has to be released
//! and pushed again.

use std::collections::HashMap;

use bevy::prelude::{Gamepad, Resource};

use super::ControllerInput;
use super::analog::raw_magnitude;

/// Transient per-frame baseline used only while the control mapping screen
/// is capturing a new controller binding, so a stick or trigger already
/// resting past its press threshold when capture begins doesn't
/// immediately "capture" itself.
#[derive(Resource, Default)]
pub(crate) struct ControllerCaptureState {
    previous_magnitudes: HashMap<ControllerInput, f32>,
}

impl ControllerCaptureState {
    fn snapshot(&mut self, gamepad: &Gamepad) {
        for input in ControllerInput::ANALOG {
            self.previous_magnitudes
                .insert(input, raw_magnitude(input, gamepad));
        }
    }

    pub(crate) fn reset(&mut self) {
        self.previous_magnitudes.clear();
    }

    fn detect(&mut self, gamepad: &Gamepad) -> Option<ControllerInput> {
        for input in ControllerInput::ALL {
            if let Some(button) = input.digital_button()
                && gamepad.just_pressed(button)
            {
                return Some(input);
            }
        }
        for input in ControllerInput::ANALOG {
            let (press, _) = input.hysteresis_thresholds();
            let value = raw_magnitude(input, gamepad);
            let previous = *self.previous_magnitudes.get(&input).unwrap_or(&0.0);
            if value > press && previous <= press {
                return Some(input);
            }
        }
        None
    }
}

/// Advances controller-binding capture for one frame. `just_entered` should
/// be true only on the frame capture mode is (re)activated so the analog
/// baseline can be armed without immediately matching. Returns the newly
/// captured input, if any.
pub(crate) fn capture_step(
    capture: &mut ControllerCaptureState,
    gamepad: &Gamepad,
    just_entered: bool,
) -> Option<ControllerInput> {
    if just_entered {
        capture.snapshot(gamepad);
        return None;
    }
    let detected = capture.detect(gamepad);
    if detected.is_none() {
        capture.snapshot(gamepad);
    }
    detected
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{GamepadAxis, GamepadButton};

    /// The structural fact the menu screens' pad back button rests on: Start
    /// is not a [`ControllerInput`], so a capture can never return it and no
    /// binding can ever hold it - which is what makes it safe as back on
    /// every screen, including mid-capture, exactly as `KeyCode::Escape` is
    /// on the keyboard.
    #[test]
    fn start_is_never_a_bindable_input() {
        assert!(
            ControllerInput::ALL
                .iter()
                .all(|input| input.digital_button() != Some(GamepadButton::Start))
        );
        let mut capture = ControllerCaptureState::default();
        let mut gamepad = Gamepad::default();
        gamepad.digital_mut().press(GamepadButton::Start);
        assert_eq!(capture_step(&mut capture, &gamepad, true), None);
        assert_eq!(capture_step(&mut capture, &gamepad, false), None);
    }

    use super::*;

    #[test]
    fn capture_step_arms_baseline_then_detects_rising_edge_and_ignores_held_start() {
        let mut capture = ControllerCaptureState::default();
        let mut gamepad = Gamepad::default();
        gamepad.analog_mut().set(GamepadAxis::LeftStickX, 0.9);

        // First step just arms the baseline; a stick already pushed over
        // must not immediately "capture" itself.
        assert_eq!(capture_step(&mut capture, &gamepad, true), None);
        assert_eq!(capture_step(&mut capture, &gamepad, false), None);

        // Releasing and then pushing again is a fresh rising edge.
        gamepad.analog_mut().set(GamepadAxis::LeftStickX, 0.0);
        assert_eq!(capture_step(&mut capture, &gamepad, false), None);
        gamepad.analog_mut().set(GamepadAxis::LeftStickX, 0.9);
        assert_eq!(
            capture_step(&mut capture, &gamepad, false),
            Some(ControllerInput::LeftStickRight)
        );
    }

    #[test]
    fn capture_step_detects_digital_button_just_pressed() {
        let mut capture = ControllerCaptureState::default();
        let mut gamepad = Gamepad::default();
        assert_eq!(capture_step(&mut capture, &gamepad, true), None);
        gamepad.digital_mut().press(GamepadButton::East);
        assert_eq!(
            capture_step(&mut capture, &gamepad, false),
            Some(ControllerInput::East)
        );
    }
}
