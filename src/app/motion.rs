//! The motion policy: when the client may animate, and what it owes a
//! player who asked it not to.
//!
//! # The policy
//!
//! Go Together had no motion in its core loop before the capture
//! indication, and an accessibility review recorded that as a result worth
//! keeping. So motion here is held to two rules, and anything that moves,
//! scales, fades or pulses on screen is expected to follow both:
//!
//! 1. **Motion is reducible.** A player can turn it off, with the Reduce
//!    Motion preference, and the platform can ask for it to be off, with
//!    the browser's `prefers-reduced-motion` setting. Either is enough:
//!    [`MotionPolicy::reduced`] is true when the preference is on *or* the
//!    platform asks, and the in-game preference cannot re-enable motion the
//!    platform asked to reduce. With motion reduced, nothing on screen
//!    changes continuously over time; every change is a step.
//! 2. **The information survives with motion off.** An animation is the
//!    richer presentation of a change the player could learn without it. A
//!    change that only an animation shows has moved a defect from one
//!    channel to another, not fixed it. So the static form - a mark, a
//!    count, a word - is the requirement, and the animation is built on
//!    top of it. The capture indication in `ui::match_screen::capture` is
//!    the pattern: the marks and the per-side count are what a capture
//!    *is* on screen, and the fade and shrink are what it looks like when
//!    motion is allowed.
//!
//! The Round 0 dot rows in `ui::match_screen::opening_dots` are the second
//! piece of motion built under it, and the harder case of rule 2: with three
//! held, an accepted selection and a refused one end with the same three
//! dots, so the static form there is a mark on the row's dividing line
//! rather than anything about where the dots finish.
//!
//! # What the policy does not govern
//!
//! The hold-progress bar below the board, the round-time bar that shares its
//! slot, and the Match and Round clock texts all change continuously while a
//! Match runs, and none of them asks [`Motion::reduced`]. They are readouts,
//! not animations: each frame of the bar is a different measurement of the
//! authoritative hold timer, as each tick of the clock text is a different
//! time, so the motion *is* the information rather than a presentation of a
//! change the player could learn another way. Rule 2 asks for the static
//! form of a change, and a readout's static form is the readout. Rule 1's
//! step would be a bar that jumps between values at some interval nothing
//! else in the game uses, which would make a steady hold look like a
//! stutter. So the bars and the clocks are exempt on principle, in the way a
//! clock face is: the policy governs motion added on top of a state change,
//! and a readout has no such layer.
//!
//! The end hold's drain is the case that made this worth writing down. Since
//! 2026-10-05 a released end hold drains at half the rate it filled rather
//! than resetting (`gameplay::holds`), so the bar now moves while the player
//! does nothing, which looks more like an animation than a bar that only
//! moved under a held key did. It is still a readout. The timer really is
//! draining; the bar is the one thing telling the player that their progress
//! is leaving while they can still act on it; and the reduced-motion
//! alternative - a bar that vanished on release - is exactly the cliff the
//! drain was introduced to remove, so freezing it under Reduce Motion would
//! show a bar that lies. Nor is photosensitivity in question: the bar is a
//! single slow change of width in one fixed colour, with no flash in it.
//!
//! # Where the two signals come from
//!
//! The preference is `GamePreferences::reduce_motion`, persisted from
//! settings schema v6 and switched on the Preferences tab. The platform
//! signal is `platform::prefers_reduced_motion`: on the web it is the
//! browser's media query, read once at startup and again on entering a
//! Match, so a change of the operating-system setting applies from the
//! next Match rather than mid-animation; on native nothing in the
//! dependency tree can read the operating system's setting, so the platform
//! half is always off there and the preference is the whole policy.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::platform::prefers_reduced_motion;
use crate::settings::GamePreferences;

/// The platform's half of the motion policy, as last read. See the module
/// docs for the policy itself.
#[derive(Resource, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct MotionPolicy {
    /// Whether the platform asked for reduced motion the last time
    /// [`read_platform_motion_signal`] ran.
    platform_reduces_motion: bool,
}

impl MotionPolicy {
    /// A policy with the platform's answer fixed, for tests.
    #[cfg(test)]
    pub(crate) const fn with_platform_reducing_motion(platform_reduces_motion: bool) -> Self {
        Self {
            platform_reduces_motion,
        }
    }

    /// Whether motion should be left out: the player asked, or the platform
    /// did. Either is enough.
    pub(crate) const fn reduced(&self, preferences: &GamePreferences) -> bool {
        preferences.reduce_motion || self.platform_reduces_motion
    }
}

/// The two halves of the policy together, for a system that animates:
/// `motion.reduced()` is the one question it asks.
#[derive(SystemParam)]
pub(crate) struct Motion<'w> {
    policy: Res<'w, MotionPolicy>,
    preferences: Res<'w, GamePreferences>,
}

impl Motion<'_> {
    /// See [`MotionPolicy::reduced`].
    pub(crate) fn reduced(&self) -> bool {
        self.policy.reduced(&self.preferences)
    }
}

/// Reads the platform's reduced-motion signal into [`MotionPolicy`]. Runs at
/// startup and on entering a Match; see the module docs for why it is not
/// polled.
pub(crate) fn read_platform_motion_signal(mut policy: ResMut<MotionPolicy>) {
    let platform_reduces_motion = prefers_reduced_motion();
    if policy.platform_reduces_motion != platform_reduces_motion {
        policy.platform_reduces_motion = platform_reduces_motion;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Either signal reduces motion, and the preference cannot override a
    /// platform that asked for less.
    #[test]
    fn motion_is_reduced_when_either_the_player_or_the_platform_asks() {
        let on = GamePreferences {
            reduce_motion: true,
            ..GamePreferences::default()
        };
        let off = GamePreferences::default();

        assert!(!MotionPolicy::default().reduced(&off));
        assert!(MotionPolicy::default().reduced(&on));
        let platform = MotionPolicy::with_platform_reducing_motion(true);
        assert!(platform.reduced(&off));
        assert!(platform.reduced(&on));
    }

    /// The native platform never asks, so a native build's policy is the
    /// preference alone - which is what the module docs promise.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_native_platform_never_asks_for_reduced_motion() {
        let mut app = App::new();
        app.insert_resource(MotionPolicy::with_platform_reducing_motion(true))
            .add_systems(Update, read_platform_motion_signal);
        app.update();
        assert_eq!(
            *app.world().resource::<MotionPolicy>(),
            MotionPolicy::default()
        );
    }

    #[test]
    fn the_system_param_answers_for_both_halves() {
        fn ask(motion: Motion) -> bool {
            motion.reduced()
        }
        let mut world = World::new();
        world.insert_resource(MotionPolicy::default());
        world.insert_resource(GamePreferences::default());
        assert!(!world.run_system_once(ask).unwrap());
        world.resource_mut::<GamePreferences>().reduce_motion = true;
        assert!(world.run_system_once(ask).unwrap());
    }

    use bevy::ecs::system::RunSystemOnce;
}
