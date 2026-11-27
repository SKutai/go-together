//! The pure rules of Go Together: the board, legal moves, round resolution,
//! territory and scoring.
//!
//! # Responsibility
//!
//! This module knows nothing about Bevy, input or rendering. It is the
//! authority on what a round *means*, which is what makes the whole ruleset
//! unit-testable.
//!
//! # Module boundaries and dependency direction
//!
//! - `values` - [`Player`], [`Point`], [`Cell`], [`Score`], and the board
//!   dimension constants. Fundamental values with no dependency on anything
//!   else in `rules`.
//! - `board` - [`Board`]'s representation, serialization, topology
//!   (connected groups and liberties), and the individual placement,
//!   suicide, capture, and active-cleanup effects one action has on it.
//!   Depends only on `values`.
//! - `scoring` - territory filling, final scoring, and the Round 0
//!   reveal's one-time capture sweep: the parts of `Board` that evaluate a
//!   finished or revealed position rather than one player's action. Depends
//!   on `board` and `values`.
//! - `resolution` - combining zero, one, or two hidden selections into one
//!   completed round using the timing, Gray, and positional-repetition
//!   rules. Depends on `board` and `values`; does not depend on `scoring`.
//!
//! Dependencies only point toward `values`; nothing here depends on Bevy,
//! UI, audio, storage, or Replay presentation. The one crate module outside
//! `rules` that `rules` uses is [`crate::clock`], for the `MatchInstant` and
//! `MatchDuration` values `resolution` compares against
//! [`SIMULTANEOUS_WINDOW`]; `ALLOWED_EDGES` in `src/architecture.rs` pins
//! that single edge. `crate::authority` composes these pieces into a live,
//! timed Match; `crate::replay` persists their results. This module
//! re-exports every public item from its submodules at the top level
//! (`crate::rules::Board`, `crate::rules::Player`, and so on), so callers
//! outside `rules` never need submodule paths - see each submodule's own
//! docs for the reasoning behind where a boundary falls.
//!
//! # Simultaneous moves
//!
//! Both players normally select in secret; a round can also be completed by a
//! timeout with zero or one selections. [`SIMULTANEOUS_WINDOW`] is the entire
//! timing model when both players selected, compared as exact
//! [`crate::clock::MatchInstant`] microseconds rather than `f64` seconds, so
//! the boundary never depends on floating-point rounding:
//!
//! * more than the window apart - the moves are *sequential*. The earlier move
//!   is played first, including any self-removal, and the later move is applied
//!   to the resulting board;
//! * within the window - the moves are *simultaneous*. Both are evaluated from
//!   the same starting board and merged, so neither player is punished for
//!   being milliseconds slower.
//!
//! The two timestamps are the moments each player *chose* the point they
//! hold, not the moments they last pressed Select. Moving and selecting a
//! different point is a new choice and re-times the selection; re-selecting
//! the point already held is not, and keeps the original timestamp
//! (`MatchState::try_lock` in `crate::authority` decides this). Pressing
//! Select repeatedly therefore cannot hold a player's commit time at "now",
//! which would otherwise let a round that should resolve sequentially fall
//! inside the window - taking any same-point reservation with it, so that
//! the opponent's later attempt on the reserved point would be accepted into
//! a tie instead of rejected.
//!
//! # Gray stones
//!
//! Gray is how the rules resolve a genuine tie without favouring either player.
//! It appears when both players pick the *same* point simultaneously and when
//! the ko rule rejects a round. Gray stones are permanent neutral walls: they
//! belong to nobody, never connect to a player's group, cannot be captured and
//! own no territory. They count as occupied when liberties are evaluated and
//! provide no liberties to an adjacent group, exactly like the edge of the
//! board.
//!
//! # Suicide, capture and ko
//!
//! [`Board::apply_move`] removes the opponent's dead groups first, then removes
//! the acting player's entire connected group if it is left without liberties.
//! This ordering lets a capturing placement gain liberties and survive.
//! Positional repetition is enforced against every completed board in the
//! current Match, including boards produced by self-removal.
//!
//! # Compatibility
//!
//! [`Board`] has hand-written [`serde::Serialize`]/[`serde::Deserialize`]
//! impls that store cells as a flat list and validate the length on load,
//! and [`Cell`] and [`Player`] serialise by name. Replays depend on these
//! representations; see [`crate::replay`] before changing any of them.

mod board;
#[cfg(test)]
mod properties;
mod resolution;
mod scoring;
#[cfg(test)]
mod test_support;
mod values;

pub use board::{Board, CleanupError, CleanupOutcome, MoveError, MoveOutcome};
pub use resolution::{
    LockError, LockedMove, ResolutionError, ResolutionKind, RoundResolution, RoundSound,
    SIMULTANEOUS_WINDOW, SelectionOutcome, apply_selection, resolve_round, validate_lock,
};
pub use scoring::OpeningSweep;
pub use values::{BOARD_SIZE, CELL_COUNT, Cell, Player, Point, RemovedStone, Score};
