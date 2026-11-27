//! Gamepad support: shared vocabulary and device-family classification.
//! Assignment, per-profile bindings, analog interpretation, and rebinding
//! capture live in this module's own submodules - see each one's doc comment
//! for its slice of the pipeline.
//!
//! # Responsibility
//!
//! Controllers are described by *profile-local* actions ([`ProfileAction`]:
//! Up, Select, Restart, ...) rather than player-specific ones, so one saved
//! profile applies to whichever player a pad is assigned to - see
//! [`bindings`] for the profile type this feeds. Mapping a [`ProfileAction`]
//! to the crate-wide [`crate::input::GameAction`] is not this module's job:
//! that happens once per frame in [`crate::input::fusion`], one level up.
//!
//! [`ControllerInput`] lives here rather than in any one submodule because
//! [`bindings`] (persisted profiles reference physical inputs) and
//! [`analog`]/[`capture`] (interpreting and detecting those inputs) all need
//! it. [`DeviceFamily`] lives beside it because
//! [`ControllerInput::display_name`] takes one, [`assignment`] classifies
//! each connected pad into one, and the Settings screen and the Help modal
//! read it back.
//!
//! # Direction
//!
//! Imports point one way: outward and upward, never sideways. Nothing here
//! touches the semantic vocabulary ([`crate::input::GameAction`],
//! [`crate::input::SemanticInput`]) that [`super`] owns; translating into it
//! is [`crate::input::fusion`]'s job, one level up. Nothing here imports from
//! [`super::keyboard`] either - the two device modules are siblings that
//! never reference each other. What [`super`] takes from here, it takes
//! through the re-exports below.

mod analog;
mod assignment;
mod bindings;
mod capture;

pub(crate) use analog::{ControllerHysteresisState, resolve_profile_action};
pub(crate) use assignment::{
    ControllerAssignments, ControllerSlotStatus, update_controller_assignments,
};
pub(crate) use bindings::ControllerBindings;
pub(crate) use capture::{ControllerCaptureState, capture_step};

use bevy::prelude::GamepadButton;
use serde::{Deserialize, Serialize};

use crate::presentation::{SELECT_ACTION, SET_TELEPORT_MARKER_ACTION, TELEPORT_TO_MARKER_ACTION};

/// A Schmitt-trigger press/release pair applied to a continuous analog
/// reading (stick direction magnitude or analog trigger pull) so noisy
/// values near the edge of a threshold don't rapidly flicker pressed state.
const STICK_PRESS_THRESHOLD: f32 = 0.55;
const STICK_RELEASE_THRESHOLD: f32 = 0.35;
const TRIGGER_PRESS_THRESHOLD: f32 = 0.6;
const TRIGGER_RELEASE_THRESHOLD: f32 = 0.4;

/// The local action namespace bound per controller profile. Mirrors the
/// seven per-player keyboard actions plus the shared restart gesture, but
/// stays independent from the global [`crate::input::GameAction`] identifiers so a single
/// profile shape can be assigned to either player slot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub(crate) enum ProfileAction {
    Up,
    Down,
    Left,
    Right,
    Select,
    SetMarker,
    JumpToMarker,
    Restart,
}

impl ProfileAction {
    pub(crate) const ALL: [Self; 8] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::Select,
        Self::SetMarker,
        Self::JumpToMarker,
        Self::Restart,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Up => "Move Up",
            Self::Down => "Move Down",
            Self::Left => "Move Left",
            Self::Right => "Move Right",
            Self::Select => SELECT_ACTION,
            Self::SetMarker => SET_TELEPORT_MARKER_ACTION,
            Self::JumpToMarker => TELEPORT_TO_MARKER_ACTION,
            Self::Restart => "Restart Match",
        }
    }
}

/// Physical controller inputs eligible for remapping. Deliberately limited
/// to the D-pad, the four face buttons, the bumpers, the analog triggers,
/// and directional stick tilts: menu/select/stick-click/home buttons are not
/// exposed since they carry OS/platform meaning on some devices.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ControllerInput {
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    South,
    East,
    North,
    West,
    LeftBumper,
    RightBumper,
    LeftAnalogTrigger,
    RightAnalogTrigger,
    LeftStickUp,
    LeftStickDown,
    LeftStickLeft,
    LeftStickRight,
    RightStickUp,
    RightStickDown,
    RightStickLeft,
    RightStickRight,
}

impl ControllerInput {
    pub(crate) const ALL: [Self; 20] = [
        Self::DPadUp,
        Self::DPadDown,
        Self::DPadLeft,
        Self::DPadRight,
        Self::South,
        Self::East,
        Self::North,
        Self::West,
        Self::LeftBumper,
        Self::RightBumper,
        Self::LeftAnalogTrigger,
        Self::RightAnalogTrigger,
        Self::LeftStickUp,
        Self::LeftStickDown,
        Self::LeftStickLeft,
        Self::LeftStickRight,
        Self::RightStickUp,
        Self::RightStickDown,
        Self::RightStickLeft,
        Self::RightStickRight,
    ];

    const ANALOG: [Self; 10] = [
        Self::LeftAnalogTrigger,
        Self::RightAnalogTrigger,
        Self::LeftStickUp,
        Self::LeftStickDown,
        Self::LeftStickLeft,
        Self::LeftStickRight,
        Self::RightStickUp,
        Self::RightStickDown,
        Self::RightStickLeft,
        Self::RightStickRight,
    ];

    /// The digital [`GamepadButton`] backing this input, if it is a simple
    /// on/off input rather than an analog reading.
    const fn digital_button(self) -> Option<GamepadButton> {
        match self {
            Self::DPadUp => Some(GamepadButton::DPadUp),
            Self::DPadDown => Some(GamepadButton::DPadDown),
            Self::DPadLeft => Some(GamepadButton::DPadLeft),
            Self::DPadRight => Some(GamepadButton::DPadRight),
            Self::South => Some(GamepadButton::South),
            Self::East => Some(GamepadButton::East),
            Self::North => Some(GamepadButton::North),
            Self::West => Some(GamepadButton::West),
            // Bevy's `LeftTrigger`/`RightTrigger` are the bumpers (LB/RB,
            // L1/R1); the analog triggers are `LeftTrigger2`/`RightTrigger2`,
            // read in `analog::raw_magnitude`.
            Self::LeftBumper => Some(GamepadButton::LeftTrigger),
            Self::RightBumper => Some(GamepadButton::RightTrigger),
            Self::LeftAnalogTrigger
            | Self::RightAnalogTrigger
            | Self::LeftStickUp
            | Self::LeftStickDown
            | Self::LeftStickLeft
            | Self::LeftStickRight
            | Self::RightStickUp
            | Self::RightStickDown
            | Self::RightStickLeft
            | Self::RightStickRight => None,
        }
    }

    const fn hysteresis_thresholds(self) -> (f32, f32) {
        match self {
            Self::LeftAnalogTrigger | Self::RightAnalogTrigger => {
                (TRIGGER_PRESS_THRESHOLD, TRIGGER_RELEASE_THRESHOLD)
            }
            _ => (STICK_PRESS_THRESHOLD, STICK_RELEASE_THRESHOLD),
        }
    }

    const fn is_face_button(self) -> bool {
        matches!(self, Self::South | Self::East | Self::North | Self::West)
    }

    /// A device-family-aware label. Face buttons carry different printed
    /// symbols per manufacturer; every other input has one universal name.
    pub(crate) fn display_name(self, family: DeviceFamily) -> &'static str {
        if self.is_face_button() {
            return family.face_button_label(self);
        }
        match self {
            Self::DPadUp => "D-Pad Up",
            Self::DPadDown => "D-Pad Down",
            Self::DPadLeft => "D-Pad Left",
            Self::DPadRight => "D-Pad Right",
            Self::LeftBumper => "Left Bumper",
            Self::RightBumper => "Right Bumper",
            Self::LeftAnalogTrigger => "Left Trigger",
            Self::RightAnalogTrigger => "Right Trigger",
            Self::LeftStickUp => "Left Stick Up",
            Self::LeftStickDown => "Left Stick Down",
            Self::LeftStickLeft => "Left Stick Left",
            Self::LeftStickRight => "Left Stick Right",
            Self::RightStickUp => "Right Stick Up",
            Self::RightStickDown => "Right Stick Down",
            Self::RightStickLeft => "Right Stick Left",
            Self::RightStickRight => "Right Stick Right",
            Self::South | Self::East | Self::North | Self::West => unreachable!(),
        }
    }
}

/// Conservative controller manufacturer classification used only to pick a
/// device-accurate label for the four face buttons. Unknown vendors and
/// ambiguous names always fall back to universal positional labels; no
/// identifying information is persisted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DeviceFamily {
    PlayStation,
    Xbox,
    Nintendo,
    #[default]
    Unknown,
}

const SONY_VENDOR_ID: u16 = 0x054c;

const MICROSOFT_VENDOR_ID: u16 = 0x045e;

const NINTENDO_VENDOR_ID: u16 = 0x057e;

impl DeviceFamily {
    pub(crate) fn classify(vendor_id: Option<u16>, name: Option<&str>) -> Self {
        let lower = name.map(str::to_ascii_lowercase);
        let name = lower.as_deref();
        if vendor_id == Some(SONY_VENDOR_ID)
            || confident_name_match(name, &["sony", "playstation", "dualshock", "dualsense"])
        {
            Self::PlayStation
        } else if vendor_id == Some(MICROSOFT_VENDOR_ID) || confident_name_match(name, &["xbox"]) {
            Self::Xbox
        } else if vendor_id == Some(NINTENDO_VENDOR_ID)
            || confident_name_match(
                name,
                &["nintendo", "switch", "joy-con", "joycon", "pro controller"],
            )
        {
            Self::Nintendo
        } else {
            Self::Unknown
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::PlayStation => "PlayStation controller",
            Self::Xbox => "Xbox controller",
            Self::Nintendo => "Nintendo controller",
            Self::Unknown => "Controller",
        }
    }

    fn face_button_label(self, input: ControllerInput) -> &'static str {
        match (self, input) {
            (Self::PlayStation, ControllerInput::South) => "Cross",
            (Self::PlayStation, ControllerInput::East) => "Circle",
            (Self::PlayStation, ControllerInput::West) => "Square",
            (Self::PlayStation, ControllerInput::North) => "Triangle",
            (Self::Xbox, ControllerInput::South) => "A",
            (Self::Xbox, ControllerInput::East) => "B",
            (Self::Xbox, ControllerInput::West) => "X",
            (Self::Xbox, ControllerInput::North) => "Y",
            (Self::Nintendo, ControllerInput::South) => "B",
            (Self::Nintendo, ControllerInput::East) => "A",
            (Self::Nintendo, ControllerInput::West) => "Y",
            (Self::Nintendo, ControllerInput::North) => "X",
            (Self::Unknown, ControllerInput::South) => "South Button",
            (Self::Unknown, ControllerInput::East) => "East Button",
            (Self::Unknown, ControllerInput::North) => "North Button",
            (Self::Unknown, ControllerInput::West) => "West Button",
            _ => unreachable!("face_button_label is only called for face-button inputs"),
        }
    }
}

fn confident_name_match(name: Option<&str>, patterns: &[&str]) -> bool {
    name.is_some_and(|name| patterns.iter().any(|pattern| name.contains(pattern)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_classifier_is_conservative_and_family_aware() {
        assert_eq!(
            DeviceFamily::classify(Some(0x054c), None),
            DeviceFamily::PlayStation
        );
        assert_eq!(
            DeviceFamily::classify(None, Some("Sony DualSense Wireless Controller")),
            DeviceFamily::PlayStation
        );
        assert_eq!(
            DeviceFamily::classify(Some(0x045e), None),
            DeviceFamily::Xbox
        );
        assert_eq!(
            DeviceFamily::classify(None, Some("Xbox Wireless Controller")),
            DeviceFamily::Xbox
        );
        assert_eq!(
            DeviceFamily::classify(Some(0x057e), None),
            DeviceFamily::Nintendo
        );
        assert_eq!(
            DeviceFamily::classify(None, Some("Pro Controller")),
            DeviceFamily::Nintendo
        );
        assert_eq!(
            DeviceFamily::classify(Some(0x1234), Some("Generic USB Gamepad")),
            DeviceFamily::Unknown
        );
        assert_eq!(DeviceFamily::classify(None, None), DeviceFamily::Unknown);

        assert_eq!(
            ControllerInput::South.display_name(DeviceFamily::PlayStation),
            "Cross"
        );
        assert_eq!(ControllerInput::South.display_name(DeviceFamily::Xbox), "A");
        assert_eq!(
            ControllerInput::South.display_name(DeviceFamily::Nintendo),
            "B"
        );
        assert_eq!(
            ControllerInput::East.display_name(DeviceFamily::Nintendo),
            "A"
        );
        assert_eq!(
            ControllerInput::West.display_name(DeviceFamily::Nintendo),
            "Y"
        );
        assert_eq!(
            ControllerInput::North.display_name(DeviceFamily::Nintendo),
            "X"
        );
        assert_eq!(
            ControllerInput::South.display_name(DeviceFamily::Unknown),
            "South Button"
        );
        assert_eq!(
            ControllerInput::DPadUp.display_name(DeviceFamily::PlayStation),
            "D-Pad Up"
        );
    }
}
