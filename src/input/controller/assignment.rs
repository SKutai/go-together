//! Connected-pad discovery and per-player-slot assignment.
//!
//! Connected pads are assigned in a fixed order - White/Player 2 first, then
//! Black/Player 1 - so two players plugging in get a predictable side. An
//! unplugged pad frees its slot immediately and does not auto-fill from a
//! standby pad; nothing else in the crate has to care, because gameplay only
//! ever reads the fused result in [`crate::input::fusion`].

use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent};
use bevy::prelude::{Entity, MessageReader, ResMut, Resource};

use super::DeviceFamily;
use super::analog::ControllerHysteresisState;
use crate::rules::Player;

/// One physical gamepad currently occupying a player slot.
#[derive(Clone, Debug)]
struct AssignedController {
    entity: Entity,
    family: DeviceFamily,
    name: String,
}

/// Display-friendly connection status for a player's controller slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ControllerSlotStatus {
    Unassigned,
    Connected { family: DeviceFamily, name: String },
}

/// Tracks which physical gamepad entity (if any) is assigned to each player.
/// The first controller connected fills White/Player Two (left), the second
/// fills Black/Player One (right); further controllers stay on standby.
/// Disconnecting a controller clears only its own slot; a controller that
/// was already connected and on standby does *not* automatically claim a
/// slot freed by a later disconnect - only a newly connected controller can.
#[derive(Resource, Default)]
pub(crate) struct ControllerAssignments {
    slots: [Option<AssignedController>; 2],
}

impl ControllerAssignments {
    pub(crate) fn assigned_entity(&self, player: Player) -> Option<Entity> {
        self.slots[player.index()].as_ref().map(|slot| slot.entity)
    }

    pub(crate) fn family(&self, player: Player) -> DeviceFamily {
        self.slots[player.index()]
            .as_ref()
            .map_or(DeviceFamily::Unknown, |slot| slot.family)
    }

    pub(crate) fn status(&self, player: Player) -> ControllerSlotStatus {
        match &self.slots[player.index()] {
            Some(slot) => ControllerSlotStatus::Connected {
                family: slot.family,
                name: slot.name.clone(),
            },
            None => ControllerSlotStatus::Unassigned,
        }
    }

    /// Fills the first vacant slot in White-then-Black priority order. A
    /// gamepad already assigned, or arriving when both slots are occupied,
    /// is left untouched (standby).
    pub(crate) fn connect(&mut self, entity: Entity, family: DeviceFamily, name: String) {
        if self.slots.iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|assigned| assigned.entity == entity)
        }) {
            return;
        }
        for player in Player::PRIORITY_ORDER {
            let slot = &mut self.slots[player.index()];
            if slot.is_none() {
                *slot = Some(AssignedController {
                    entity,
                    family,
                    name,
                });
                return;
            }
        }
    }

    /// Clears the assignment for a disconnected entity, if it held a slot,
    /// and reports which player's slot was vacated so callers can also
    /// release any held semantic/hysteresis state for that slot.
    pub(crate) fn disconnect(&mut self, entity: Entity) -> Option<Player> {
        for player in Player::ALL {
            let slot = &mut self.slots[player.index()];
            if slot
                .as_ref()
                .is_some_and(|assigned| assigned.entity == entity)
            {
                *slot = None;
                return Some(player);
            }
        }
        None
    }
}

/// Applies newly connected/disconnected gamepads to the player slot
/// assignments, and releases any held hysteresis state for a slot that just
/// went vacant.
pub(crate) fn update_controller_assignments(
    mut connection_events: MessageReader<GamepadConnectionEvent>,
    mut assignments: ResMut<ControllerAssignments>,
    mut hysteresis: ResMut<ControllerHysteresisState>,
) {
    for event in connection_events.read() {
        match &event.connection {
            GamepadConnection::Connected {
                name, vendor_id, ..
            } => {
                let family = DeviceFamily::classify(*vendor_id, Some(name));
                assignments.connect(event.gamepad, family, name.clone());
            }
            GamepadConnection::Disconnected => {
                if let Some(player) = assignments.disconnect(event.gamepad) {
                    hysteresis.clear_player(player);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::controller::ControllerInput;

    #[test]
    fn assignment_lifecycle_orders_white_first_then_black_then_standby() {
        let mut assignments = ControllerAssignments::default();
        let first = Entity::from_raw_u32(1).unwrap();
        let second = Entity::from_raw_u32(2).unwrap();
        let third = Entity::from_raw_u32(3).unwrap();

        assignments.connect(first, DeviceFamily::Xbox, "Pad A".into());
        assert_eq!(assignments.assigned_entity(Player::Two), Some(first));
        assert_eq!(assignments.assigned_entity(Player::One), None);

        assignments.connect(second, DeviceFamily::PlayStation, "Pad B".into());
        assert_eq!(assignments.assigned_entity(Player::One), Some(second));

        // A third pad has nowhere to go and stays on standby.
        assignments.connect(third, DeviceFamily::Nintendo, "Pad C".into());
        assert_eq!(assignments.assigned_entity(Player::One), Some(second));
        assert_eq!(assignments.assigned_entity(Player::Two), Some(first));
    }

    #[test]
    fn disconnect_clears_only_that_slot_and_standby_does_not_auto_fill() {
        let mut assignments = ControllerAssignments::default();
        let mut hysteresis = ControllerHysteresisState::default();
        let first = Entity::from_raw_u32(1).unwrap();
        let second = Entity::from_raw_u32(2).unwrap();
        let third = Entity::from_raw_u32(3).unwrap();
        assignments.connect(first, DeviceFamily::Xbox, "Pad A".into());
        assignments.connect(second, DeviceFamily::PlayStation, "Pad B".into());
        assignments.connect(third, DeviceFamily::Nintendo, "Pad C".into());
        hysteresis.set_active(Player::Two, ControllerInput::LeftStickUp, true);

        let vacated = assignments.disconnect(first);

        assert_eq!(vacated, Some(Player::Two));
        assert_eq!(assignments.assigned_entity(Player::Two), None);
        assert_eq!(assignments.assigned_entity(Player::One), Some(second));
        // The pre-existing standby pad does not automatically claim the slot.
        assert_ne!(assignments.assigned_entity(Player::Two), Some(third));

        hysteresis.clear_player(Player::Two);
        assert!(!hysteresis.is_active(Player::Two, ControllerInput::LeftStickUp));
    }

    #[test]
    fn a_newly_connected_pad_fills_a_freshly_vacant_slot() {
        let mut assignments = ControllerAssignments::default();
        let first = Entity::from_raw_u32(1).unwrap();
        assignments.connect(first, DeviceFamily::Xbox, "Pad A".into());
        assignments.disconnect(first);

        let fresh = Entity::from_raw_u32(9).unwrap();
        assignments.connect(fresh, DeviceFamily::Xbox, "Pad Fresh".into());

        assert_eq!(assignments.assigned_entity(Player::Two), Some(fresh));
    }
}
