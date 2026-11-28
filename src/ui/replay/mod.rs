//! The replay feature's two screens: the library of saved matches and the
//! playback viewer.
//!
//! # Responsibility
//!
//! [`crate::replay`] owns the on-disk schema and the playback clock, and
//! [`crate::replay_library`] owns the saved collection and its
//! filtering/sorting. This module owns only presentation and intent: listing,
//! searching, renaming, deleting and stepping through a replay.
//!
//! # Submodules
//!
//! * [`library`] - the browsable list, its toolbar and the rename/delete
//!   dialogs.
//! * [`viewer`] - board reconstruction and transport controls for one replay.
//!
//! Both screens edit text through [`crate::ui::text_input`] - a sibling
//! module, not a submodule - the caret-aware editor also used by the numeric
//! settings fields.
//!
//! # Separation from live play
//!
//! Replay rendering never touches [`crate::gameplay`] state, and the viewer has
//! its own board geometry and its own cursor/marker/selection entities. Only
//! completed matches ever reach the library: an abandoned match is dropped
//! before it can be saved.

mod library;
mod viewer;

#[cfg(test)]
mod test_support;

/// Separator between metadata fields on a replay card and in the viewer
/// header, kept here so both screens format a replay identically.
const REPLAY_FIELD_SEPARATOR: &str = " | ";

use bevy::prelude::*;

use library::ReplayLibraryPlugin;
use viewer::ReplayViewerPlugin;

/// Adds both replay screens.
///
/// Each screen keeps its own chained pipeline - intent, then state, then
/// display, with the Viewer's placing its furniture for the window's size
/// first - and runs only while its screen is active, so a paused viewer costs
/// nothing while the player is browsing the library.
pub(super) struct ReplayUiPlugin;

impl Plugin for ReplayUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ReplayLibraryPlugin)
            .add_plugins(ReplayViewerPlugin);
    }
}
