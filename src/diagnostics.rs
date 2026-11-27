//! The boundary between subsystem-specific typed errors and what a player
//! sees versus what only a log sees.
//!
//! # Responsibility
//!
//! Subsystems - [`crate::settings`], [`crate::replay`], [`crate::platform`] -
//! keep their own typed errors ([`crate::platform::SettingsStorageError`],
//! [`crate::replay::ReplayError`], and so on) and know nothing about
//! diagnostics. Whichever call site first has both the failure and
//! `ResMut<Diagnostics>` access is the boundary: it logs the full technical
//! detail through `bevy::log` and pushes a concise, player-safe
//! [`Diagnostic`] onto [`Diagnostics`] for whichever screen wants to show it.
//!
//! One exception to "whichever call site first has both": the failures that
//! happen during startup, before `LogPlugin` exists to log through at all.
//! Those return their detail to `app::run`, which pushes the player-safe
//! half onto [`Diagnostics`] here and hands the technical half to
//! `app::startup_log` to write out from a `Startup` system. The split
//! between the two halves is exactly the same; only the moment the log call
//! happens moves.
//!
//! Logging through `bevy::log` rather than `eprintln!` matters specifically
//! for WASM: `eprintln!` produces no output at all in a browser tab (there is
//! no inherited stderr to write to), so a failure logged that way is
//! completely invisible to anyone without native tooling attached. Bevy's
//! `LogPlugin` (part of `DefaultPlugins`, added in `app::AppPlugin`)
//! wires a subscriber that reaches the browser console on WASM and the
//! terminal natively, so the same call produces visible output on both.
//!
//! # What this is not
//!
//! Not a general error type unifying every subsystem's errors - a function
//! that can fail still returns its own typed error; only the point where a
//! failure becomes user-facing goes through here. Not a toast/notification
//! UI either: existing per-screen inline text (the Settings screen's mapping
//! error, the Replay Library's rename/delete dialog error, the Match
//! screen's replay-save notice) keeps rendering its own way, usually fed by
//! the same call site that pushes onto [`Diagnostics`]. [`Diagnostics`] is
//! the one shared place all of those *could* also be read from, useful for
//! screens with no more specific place to show a failure, and a foundation
//! for later diagnostics-adjacent work (retry status, background task
//! results) rather than a redesign of how failures look today.

use bevy::prelude::*;

/// How serious a diagnostic is.
///
/// Only [`Self::Warning`] exists today: every diagnostic in this codebase is
/// a recoverable failure worth a player's attention - a settings write that
/// could not persist, a Replay that could not save or load, a storage
/// backend that turned out to be unavailable - and none of them stop play.
/// This stays a distinct type, not a hardcoded label, because a genuinely
/// fatal failure (one the game cannot meaningfully continue past) would
/// belong here as a second variant rather than as a different mechanism -
/// there simply isn't one today. The closest candidate,
/// `platform::native::asset_root` failing to locate the running executable,
/// was deliberately changed to degrade (fall back to a relative asset path)
/// rather than become the first fatal diagnostic; see its doc comment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Severity {
    Warning,
}

/// One player-facing failure notice: a concise, safe message plus how
/// serious it is. Never holds a file path, stack trace, or other internal
/// detail - that stays in the `bevy::log` call [`report_recoverable`] makes
/// alongside pushing this.
///
/// That is a checked invariant, not a convention: every site that pushes a
/// [`Diagnostic`] has a test that triggers the real failure and asserts both
/// the exact message and, through this module's test-only
/// `assert_carries_no_internal_detail`, that the message carries none of the
/// shapes internal detail arrives in. The check exists because nothing
/// reads [`Diagnostics`] back in production (see [`Diagnostics::active`]),
/// so a call site that swapped its `technical` and `message` arguments
/// would otherwise go unnoticed indefinitely - which is exactly what
/// happened to `gameplay`'s Replay-save site.
///
/// Almost every site reaches that through [`report_recoverable`]. The one
/// that does not is the startup settings-load notice `app::run` pre-loads
/// before any `App` exists: it pushes directly, because this module's
/// logging half cannot run that early - Bevy's `LogPlugin` has not installed
/// a `tracing` subscriber yet - not because it is exempt from the rule. Its
/// message is generic and pinned by a test like every other, and the
/// detailed string that failure produces still reaches both of its
/// destinations: the Settings screen's inline text, and a log line that
/// `app::startup_log` holds until a subscriber exists. See
/// `app::startup_diagnostics`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) message: String,
}

/// Currently active, player-facing diagnostics.
///
/// A small bounded log, not a queue that must be drained by some consuming
/// system - screens read [`Self::active`] and decide for themselves whether
/// and how to show it. [`Self::push`] drops an exact duplicate of the most
/// recently pushed diagnostic instead of appending it again, so a condition
/// a system happens to check every frame cannot flood this with copies of
/// the same message; a genuinely distinct second failure is still appended,
/// never overwriting the first.
#[derive(Resource, Default)]
pub(crate) struct Diagnostics {
    active: Vec<Diagnostic>,
}

/// Small on purpose: this is a recent-history log for a single local
/// player's session, not a durable audit trail.
const MAX_ACTIVE_DIAGNOSTICS: usize = 20;

impl Diagnostics {
    /// No production screen reads this yet - every current call site that
    /// pushes here also already has its own established, tested display
    /// (the Settings screen's mapping error, a Replay Library dialog's
    /// error, the Match screen's replay-save notice), so nothing has needed
    /// a shared reader so far. Exists as the one place a future screen with
    /// no more specific home for a failure could read from; so this
    /// module's own tests can verify push/dedup behavior without inspecting
    /// unrelated screens; and so each call site's test can pin the exact
    /// player-safe message it produces, which is the only thing exercising
    /// those strings today.
    #[cfg(test)]
    pub(crate) fn active(&self) -> &[Diagnostic] {
        &self.active
    }

    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        if self.active.last() == Some(&diagnostic) {
            return;
        }
        self.active.push(diagnostic);
        if self.active.len() > MAX_ACTIVE_DIAGNOSTICS {
            self.active.remove(0);
        }
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.active.clear();
    }
}

/// Logs `technical` (full cause context, for developers and bug reports) and
/// pushes a concise, player-safe `message` onto `diagnostics` at
/// [`Severity::Warning`].
///
/// This is the shape almost every recoverable-failure call site wants: the
/// Match, Settings screen, or Replay Library keeps running, but the player
/// should be able to tell something did not persist.
///
/// The two arguments are easy to transpose and nothing in production reads
/// the result back, so each call site pins its `message` in a test; see this
/// module's test-only `assert_carries_no_internal_detail`.
pub(crate) fn report_recoverable(
    diagnostics: &mut Diagnostics,
    technical: impl std::fmt::Display,
    message: impl Into<String>,
) {
    warn!("{technical}");
    diagnostics.push(Diagnostic {
        severity: Severity::Warning,
        message: message.into(),
    });
}

/// Whether `message` looks like a subsystem's technical string rather than a
/// player-facing one.
///
/// Deliberately crude rather than exhaustive. Every way internal detail has
/// actually reached a `Diagnostic` in this crate looks the same - a call
/// site passing its *technical* string where its player-safe one belongs -
/// and every technical string here is a formatted
/// [`crate::platform::SettingsStorageError`] or [`crate::replay::ReplayError`]
/// wrapping a filesystem path or a `std::io::Error`. Those all carry a path
/// separator or the `os error N` that `std::io::Error`'s `Display` appends,
/// so three substring checks catch the mistake at the call site that made it.
///
/// Used in both directions by the tests that pin each pushed message (in
/// `app`, `gameplay`, `ui::replay::library`, and `ui::settings`): they assert
/// the player-facing message is clean *and* that the technical string the
/// same failure produced is not, so a test cannot pass because the failure it
/// set up was harmless to begin with.
#[cfg(test)]
pub(crate) fn carries_internal_detail(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    ["/", "\\", "os error"]
        .iter()
        .any(|marker| lowered.contains(marker))
}

/// See `carries_internal_detail`, which this asserts the absence of.
#[cfg(test)]
pub(crate) fn assert_carries_no_internal_detail(message: &str) {
    assert!(
        !carries_internal_detail(message),
        "a player-facing diagnostic must carry no path or OS error: {message}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic(message: &str) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
        }
    }

    #[test]
    fn identical_consecutive_pushes_do_not_duplicate() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(diagnostic("Could not save Replay."));
        diagnostics.push(diagnostic("Could not save Replay."));
        diagnostics.push(diagnostic("Could not save Replay."));

        assert_eq!(diagnostics.active().len(), 1);
    }

    #[test]
    fn distinct_diagnostics_all_survive() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(diagnostic("Could not save Replay."));
        diagnostics.push(diagnostic("Could not save settings."));
        diagnostics.push(diagnostic("Could not save Replay."));

        assert_eq!(diagnostics.active().len(), 3);
    }

    #[test]
    fn a_repeated_message_after_a_different_one_is_not_treated_as_a_duplicate() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(diagnostic("A"));
        diagnostics.push(diagnostic("B"));
        diagnostics.push(diagnostic("A"));

        assert_eq!(diagnostics.active().len(), 3);
    }

    #[test]
    fn the_active_log_stays_bounded() {
        let mut diagnostics = Diagnostics::default();
        for index in 0..(MAX_ACTIVE_DIAGNOSTICS + 10) {
            diagnostics.push(diagnostic(&format!("failure {index}")));
        }
        assert_eq!(diagnostics.active().len(), MAX_ACTIVE_DIAGNOSTICS);
        assert_eq!(
            diagnostics.active().last(),
            Some(&diagnostic(&format!(
                "failure {}",
                MAX_ACTIVE_DIAGNOSTICS + 9
            )))
        );
    }

    #[test]
    fn clear_removes_every_active_diagnostic() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.push(diagnostic("A"));
        diagnostics.push(diagnostic("B"));
        assert_eq!(diagnostics.active().len(), 2);

        diagnostics.clear();
        assert!(diagnostics.active().is_empty());

        // Clearing does not disturb ordinary dedup behavior afterward.
        diagnostics.push(diagnostic("A"));
        diagnostics.push(diagnostic("A"));
        assert_eq!(diagnostics.active().len(), 1);
    }

    #[test]
    fn report_recoverable_pushes_a_warning_with_the_given_message() {
        let mut diagnostics = Diagnostics::default();
        report_recoverable(
            &mut diagnostics,
            "technical: disk full at /tmp/foo",
            "Could not save your settings.",
        );

        assert_eq!(
            diagnostics.active(),
            [Diagnostic {
                severity: Severity::Warning,
                message: "Could not save your settings.".into(),
            }]
        );
    }

    /// The guard every call-site test leans on has to fail on the shapes
    /// internal detail actually arrives in, and pass on ordinary player
    /// prose - including the apostrophes and periods those sentences use.
    #[test]
    fn the_internal_detail_guard_rejects_paths_and_os_errors_but_not_prose() {
        for safe in [
            "Could not save this match's Replay. The match result itself is unaffected.",
            "Could not open this Replay.",
            "Could not rename this Replay.",
            "Could not delete this Replay.",
            "Your changes are active for this session but could not be saved. \
             They may be lost when the game closes.",
        ] {
            assert_carries_no_internal_detail(safe);
        }

        for leaked in [
            "Could not save to /Users/name/Library/go-together/replays/a.json: No space left",
            "Could not save to C:\\Users\\name\\replays\\a.json: access denied",
            "Permission denied (os error 13)",
            "Permission denied (OS error 13)",
        ] {
            assert!(carries_internal_detail(leaked), "must reject {leaked:?}");
        }
    }
}
