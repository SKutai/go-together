//! Technical detail from failures that happen before there is anything to log
//! it to, held until a `Startup` system can write it out.
//!
//! # Why this exists
//!
//! Bevy's `LogPlugin` is what installs the `tracing` subscriber - its `build`
//! calls `tracing::subscriber::set_global_default`, and nothing before that
//! point has a subscriber at all. A `warn!` made earlier is dispatched to
//! `NoSubscriber` and dropped: not filtered out by a level, not written
//! somewhere quieter, just gone.
//!
//! Several real startup failures land in that window, all of them in
//! [`crate::app::run`] or in `AppPlugin::build` before it adds
//! `DefaultPlugins`:
//!
//! * [`crate::settings::load_user_controls`] - unreadable, unsupported, or
//!   partly corrupt settings, whose detail names the settings file's path.
//! * `ReplayLibrary::load_user` - an unusable Replay directory, a stored
//!   Replay that could not be read or failed validation, and a storage
//!   reconcile that failed. A missing or corrupt catalog cache is not one of
//!   them: it is silently rebuilt.
//! * [`crate::platform::asset_root`] - the running executable could not be
//!   located, so assets fall back to a working-directory-relative path.
//!
//! The first two already have a player-facing home: the settings failure a
//! generic [`crate::diagnostics::Diagnostic`] plus the Settings screen's
//! inline error text, the Replay failures the Replay Library's warning list.
//! The asset-root fallback has none and needs none - a wrong asset root
//! surfaces later as ordinary asset-load errors. What they lacked is the
//! developer-facing half: a line in the terminal or the browser console that
//! someone can paste into a bug report, which is precisely when the path and
//! the OS error are wanted. This module is that half. It changes nothing
//! about what a player sees.
//!
//! # How
//!
//! The failing call returns its detail instead of logging it, `run` collects
//! those into [`DeferredStartupWarnings`], and
//! [`emit_deferred_startup_warnings`] - a `Startup` system, so it runs only
//! after every plugin's `build` - drains the resource and emits one `warn!`
//! per message.
//!
//! Deliberately *not* done by installing a subscriber earlier: `LogPlugin`
//! sets the global default and only the first caller wins, so an earlier
//! subscriber of ours would either take that slot and silence `LogPlugin`'s
//! (losing every log line for the rest of the run) or lose the race and be
//! useless.

use bevy::prelude::*;

/// Startup failure detail waiting for a subscriber to exist.
///
/// Filled in [`crate::app::run`] and in `AppPlugin::build`, both of which run
/// before `LogPlugin` installs one; drained once by
/// [`emit_deferred_startup_warnings`]. Empty in the overwhelmingly common
/// case where startup had nothing to complain about.
#[derive(Resource, Default)]
pub(crate) struct DeferredStartupWarnings {
    messages: Vec<String>,
}

impl DeferredStartupWarnings {
    pub(crate) fn push(&mut self, warning: impl Into<String>) {
        self.messages.push(warning.into());
    }

    #[cfg(test)]
    pub(crate) fn messages(&self) -> &[String] {
        &self.messages
    }
}

/// Emits every deferred startup warning, once, now that `LogPlugin` has
/// installed a subscriber.
///
/// Drains rather than reads: `Startup` runs once, so this is belt and braces,
/// but it makes "logged exactly once" a property of the data rather than of
/// the schedule it happens to be registered in.
///
/// `warn!` and not `error!` for the same reason
/// [`crate::diagnostics::Severity`] has only a `Warning` variant: each of
/// these failures has already degraded to a working default, and none of them
/// stops play.
pub(crate) fn emit_deferred_startup_warnings(mut deferred: ResMut<DeferredStartupWarnings>) {
    for warning in deferred.messages.drain(..) {
        warn!("{warning}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CapturedLog, capture_log_events};
    use bevy::log::Level;

    fn app_with(warnings: &[&str]) -> App {
        let mut deferred = DeferredStartupWarnings::default();
        for warning in warnings {
            deferred.push(*warning);
        }
        let mut app = App::new();
        app.insert_resource(deferred)
            .add_systems(Startup, emit_deferred_startup_warnings);
        app
    }

    /// The point of the whole module: detail collected before a subscriber
    /// existed has to come out of a real `warn!`, at `Startup`, through the
    /// production system. Asserts the emitted events themselves, not that the
    /// code path ran.
    #[test]
    fn every_deferred_warning_reaches_the_log_as_a_warning() {
        let mut app = app_with(&[
            "Could not load settings: Could not read settings from /tmp/controls.json: \
             Permission denied (os error 13). Using defaults.",
            "Could not reconcile Replay storage: disk full",
        ]);

        let captured = capture_log_events(&mut app, 1);

        assert_eq!(
            captured,
            [
                CapturedLog {
                    level: Level::WARN,
                    message: "Could not load settings: Could not read settings from \
                              /tmp/controls.json: Permission denied (os error 13). Using defaults."
                        .into(),
                },
                CapturedLog {
                    level: Level::WARN,
                    message: "Could not reconcile Replay storage: disk full".into(),
                },
            ]
        );
    }

    /// A clean startup must be silent - no empty line, no "no failures"
    /// chatter - and the resource must be drained, so no later frame can
    /// repeat what was already written.
    #[test]
    fn a_clean_startup_logs_nothing_and_leaves_the_resource_drained() {
        let mut app = app_with(&[]);

        assert_eq!(capture_log_events(&mut app, 3), []);
        assert!(
            app.world()
                .resource::<DeferredStartupWarnings>()
                .messages()
                .is_empty()
        );
    }

    /// Draining is what makes "once" true even if this system were ever
    /// registered in a schedule that repeats.
    #[test]
    fn a_warning_is_not_repeated_on_later_frames() {
        let mut app = app_with(&["Could not locate the executable: no such file"]);
        app.add_systems(Update, emit_deferred_startup_warnings);

        let captured = capture_log_events(&mut app, 5);

        assert_eq!(
            captured,
            [CapturedLog {
                level: Level::WARN,
                message: "Could not locate the executable: no such file".into(),
            }]
        );
    }
}
