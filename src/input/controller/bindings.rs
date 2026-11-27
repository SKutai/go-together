//! Per-profile controller bindings: which physical input a profile-local
//! [`super::ProfileAction`] maps to, and validation of a persisted profile
//! pair.

use std::collections::HashSet;
use std::fmt;

use bevy::prelude::Resource;

use super::{ControllerInput, ProfileAction};
use crate::rules::Player;

/// Errors from validating a set of controller profile assignments, used by
/// the persistence layer in [`crate::settings`] to enforce action coverage,
/// supported-input parsing (handled by serde), and within-profile
/// uniqueness (the same physical input cannot serve two actions on one
/// profile, though it may be reused across the two profiles).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControllerBindingsError {
    IncompleteActions,
    DuplicateAction(ProfileAction),
    DuplicateInput(ControllerInput),
}

impl fmt::Display for ControllerBindingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompleteActions => {
                write!(formatter, "not every controller action is bound")
            }
            Self::DuplicateAction(action) => {
                write!(formatter, "duplicate controller action {action:?}")
            }
            Self::DuplicateInput(input) => {
                write!(formatter, "duplicate controller input {input:?}")
            }
        }
    }
}

/// Per-player controller profiles: which physical input drives each of the
/// seven player actions plus the profile-local restart gesture.
#[derive(Clone, Debug, Eq, PartialEq, Resource)]
pub(crate) struct ControllerBindings {
    profiles: [[ControllerInput; ProfileAction::ALL.len()]; 2],
}

// Positional: `profiles[player][action as usize]`, so this list must follow
// `ProfileAction`'s declaration order, which `ProfileAction::ALL` mirrors:
// Up, Down, Left, Right, Select, SetMarker, JumpToMarker, Restart.
impl Default for ControllerBindings {
    fn default() -> Self {
        let profile = [
            ControllerInput::DPadUp,
            ControllerInput::DPadDown,
            ControllerInput::DPadLeft,
            ControllerInput::DPadRight,
            ControllerInput::South,
            ControllerInput::West,
            ControllerInput::East,
            ControllerInput::North,
        ];
        Self {
            profiles: [profile, profile],
        }
    }
}

impl ControllerBindings {
    pub(crate) fn input(&self, player: Player, action: ProfileAction) -> ControllerInput {
        self.profiles[player.index()][action as usize]
    }

    pub(crate) fn iter_profile(
        &self,
        player: Player,
    ) -> impl Iterator<Item = (ProfileAction, ControllerInput)> + '_ {
        ProfileAction::ALL
            .into_iter()
            .map(move |action| (action, self.input(player, action)))
    }

    /// Assigns `input` to `action` within `player`'s profile. If another
    /// action on the *same* profile already used that input, the two swap so
    /// every action on a profile always maps to a distinct input. The same
    /// physical input may still be bound on the other profile.
    pub(crate) fn assign(
        &mut self,
        player: Player,
        action: ProfileAction,
        input: ControllerInput,
    ) -> Option<ProfileAction> {
        let idx = player.index();
        let old_input = self.profiles[idx][action as usize];
        if old_input == input {
            return None;
        }

        let conflict = ProfileAction::ALL.into_iter().find(|candidate| {
            *candidate != action && self.profiles[idx][*candidate as usize] == input
        });
        self.profiles[idx][action as usize] = input;
        if let Some(conflicting_action) = conflict {
            self.profiles[idx][conflicting_action as usize] = old_input;
        }
        conflict
    }

    pub(crate) fn reset_to_defaults(&mut self) {
        *self = Self::default();
    }

    /// Builds and validates a full set of profiles from persisted entries.
    /// Used by [`crate::settings`]'s controller-section loader, which reads
    /// the section schema v2 introduced from every v2-and-newer file.
    pub(crate) fn try_from_entries(
        player_one: Vec<(ProfileAction, ControllerInput)>,
        player_two: Vec<(ProfileAction, ControllerInput)>,
    ) -> Result<Self, ControllerBindingsError> {
        Ok(Self {
            profiles: [
                Self::validate_profile(player_one)?,
                Self::validate_profile(player_two)?,
            ],
        })
    }

    fn validate_profile(
        entries: Vec<(ProfileAction, ControllerInput)>,
    ) -> Result<[ControllerInput; ProfileAction::ALL.len()], ControllerBindingsError> {
        if entries.len() != ProfileAction::ALL.len() {
            return Err(ControllerBindingsError::IncompleteActions);
        }

        let mut seen_actions = HashSet::new();
        let mut seen_inputs = HashSet::new();
        let mut profile = Self::default().profiles[0];
        for (action, input) in entries {
            if !seen_actions.insert(action) {
                return Err(ControllerBindingsError::DuplicateAction(action));
            }
            if !seen_inputs.insert(input) {
                return Err(ControllerBindingsError::DuplicateInput(input));
            }
            profile[action as usize] = input;
        }
        if ProfileAction::ALL
            .into_iter()
            .any(|action| !seen_actions.contains(&action))
        {
            return Err(ControllerBindingsError::IncompleteActions);
        }
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_defaults_cover_every_action_with_eligible_inputs() {
        let bindings = ControllerBindings::default();
        for player in Player::ALL {
            let bound: Vec<_> = bindings.iter_profile(player).collect();
            assert_eq!(bound.len(), ProfileAction::ALL.len());
            let inputs: HashSet<_> = bound.iter().map(|(_, input)| *input).collect();
            assert_eq!(
                inputs.len(),
                ProfileAction::ALL.len(),
                "profile inputs must be distinct"
            );
            for (_, input) in &bound {
                assert!(ControllerInput::ALL.contains(input));
            }
        }
        assert_eq!(
            bindings.input(Player::One, ProfileAction::Select),
            ControllerInput::South
        );
        assert_eq!(
            bindings.input(Player::One, ProfileAction::SetMarker),
            ControllerInput::West
        );
        assert_eq!(
            bindings.input(Player::One, ProfileAction::JumpToMarker),
            ControllerInput::East
        );
        assert_eq!(
            bindings.input(Player::One, ProfileAction::Restart),
            ControllerInput::North
        );
        assert_eq!(
            bindings.input(Player::One, ProfileAction::Up),
            ControllerInput::DPadUp
        );
        assert_eq!(
            bindings.input(Player::Two, ProfileAction::Up),
            ControllerInput::DPadUp
        );
    }

    #[test]
    fn assigning_an_in_use_controller_input_swaps_within_profile_only() {
        let mut bindings = ControllerBindings::default();

        let swapped = bindings.assign(Player::One, ProfileAction::Select, ControllerInput::West);

        assert_eq!(swapped, Some(ProfileAction::SetMarker));
        assert_eq!(
            bindings.input(Player::One, ProfileAction::Select),
            ControllerInput::West
        );
        assert_eq!(
            bindings.input(Player::One, ProfileAction::SetMarker),
            ControllerInput::South
        );
        // The other profile is untouched by a swap on Player::One's profile.
        assert_eq!(
            bindings.input(Player::Two, ProfileAction::Select),
            ControllerInput::South
        );
    }

    #[test]
    fn same_physical_input_is_allowed_across_profiles_but_not_within_one() {
        let mut bindings = ControllerBindings::default();
        // Both profiles default South to Select already; binding Player::Two's
        // Restart to the same South input Player::One uses for Select must succeed.
        let swapped = bindings.assign(Player::Two, ProfileAction::Restart, ControllerInput::South);
        assert_eq!(swapped, Some(ProfileAction::Select));
        assert_eq!(
            bindings.input(Player::Two, ProfileAction::Restart),
            ControllerInput::South
        );
        assert_eq!(
            bindings.input(Player::One, ProfileAction::Select),
            ControllerInput::South
        );
    }

    #[test]
    fn validation_rejects_incomplete_duplicate_action_and_duplicate_input() {
        let complete = ControllerBindings::default();
        let entries: Vec<_> = complete.iter_profile(Player::One).collect();

        let mut incomplete = entries.clone();
        incomplete.pop();
        assert_eq!(
            ControllerBindings::try_from_entries(incomplete, entries.clone()),
            Err(ControllerBindingsError::IncompleteActions)
        );

        let mut duplicate_action = entries.clone();
        duplicate_action[1].0 = duplicate_action[0].0;
        assert_eq!(
            ControllerBindings::try_from_entries(duplicate_action, entries.clone()),
            Err(ControllerBindingsError::DuplicateAction(entries[0].0))
        );

        let mut duplicate_input = entries.clone();
        duplicate_input[1].1 = duplicate_input[0].1;
        assert_eq!(
            ControllerBindings::try_from_entries(duplicate_input, entries.clone()),
            Err(ControllerBindingsError::DuplicateInput(entries[0].1))
        );

        assert!(ControllerBindings::try_from_entries(entries.clone(), entries).is_ok());
    }
}
