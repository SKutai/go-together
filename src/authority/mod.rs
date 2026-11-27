//! The authoritative, deterministic Match state and rules: the headless
//! core of a Match, with no Bevy dependency.
//!
//! [`MatchState`] is a plain, `Clone`-able struct - it can be constructed,
//! mutated, and asserted on in a unit test, or from any other Rust program,
//! without a Bevy `App`, exactly like [`crate::rules`]. It owns the board,
//! cursors, Teleport Markers, hidden selections, positional-repetition
//! history, round and Match clocks, capture totals, the immutable
//! [`MatchRules`] snapshot, and the official end reasons. This is everything
//! a future server-authoritative or networked mode would need to reproduce,
//! and nothing else: local runtime concerns - device input, modal state,
//! display-only hints, storage status, audio, and Replay recording - stay on
//! the Bevy client's own `GameState`, which wraps one `MatchState` for the
//! live game (see `crate::gameplay` and `docs/ARCHITECTURE.md`).
//!
//! # Module boundaries and dependency direction
//!
//! - `values` - [`MatchPhase`], the immutable [`MatchRules`] snapshot,
//!   [`OrdinaryCompletion`], the per-player indexing every array here
//!   uses ([`player_index`], [`PLAYER_STARTS`]), and the two records a
//!   renderer and a reader share ([`RoundRemovals`], [`OpeningAttempt`]).
//!   Fundamental values with no dependency on anything else in
//!   `authority`.
//! - `outcomes` - the vocabulary the commands answer in: [`CursorMove`],
//!   [`TeleportJump`], [`OpeningSelectOutcome`], [`LockOutcome`],
//!   [`RoundOutcome`], [`ClockBoundary`], and [`ClockStep`]. These are one
//!   module because they exist for one reason - a command describes what
//!   happened instead of causing a side effect - which is the contract the
//!   next section argues, not an accident of where they were declared.
//!   Depends only on `values`.
//! - `state` - [`MatchState`], the [`MatchProgress`] it separates out for
//!   rollback, and every command that moves them:
//!   [`MatchState::begin_round_zero`], the cursor and Teleport Marker
//!   commands, [`MatchState::try_opening_select`] and the
//!   [`MatchState::start_opening_countdown`] it triggers,
//!   [`MatchState::reveal_round_zero`], [`MatchState::try_lock`],
//!   [`MatchState::resolve_ordinary_round`], and [`MatchState::finish`].
//!   Depends on `values` and `outcomes`.
//! - `timing` - [`MatchState::advance_clock`] and the per-phase advance it
//!   dispatches to, the closing-countdown readers
//!   ([`MatchState::opening_countdown_stage`],
//!   [`MatchState::opening_go_remaining`]), and the deadline arithmetic
//!   behind both, which stays private to that module. Adds inherent methods
//!   to `state`'s [`MatchState`]; nothing depends back on it.
//!
//! Dependencies only point toward `values`. This module re-exports every
//! public item from its submodules at the top level
//! (`crate::authority::MatchState`, `crate::authority::ClockStep`, and so
//! on), so callers outside `authority` never need submodule paths - the
//! public API is the set of `pub use` lines below. Shared test fixtures live
//! in `test_support`, mirroring [`crate::rules`].
//!
//! What deliberately did *not* move down into those submodules is the rest
//! of this block. The commands-and-outcomes contract, the trust argument a
//! future networked host would build on, the clock vocabulary, and the
//! rollback contract are four parts of one design: each is a reason the
//! others are shaped the way they are, and splitting them so every piece sat
//! beside its own code would leave four descriptions that individually look
//! arbitrary. They are argued here, together, and each submodule's own `//!`
//! points back at them.
//!
//! # Commands and outcomes
//!
//! Player intentions are applied through explicit, narrowly-typed methods
//! ([`MatchState::move_cursor`], [`MatchState::set_teleport_marker`],
//! [`MatchState::jump_to_teleport_marker`], [`MatchState::try_lock`],
//! [`MatchState::try_opening_select`]) rather than direct field mutation from
//! the orchestration layer. Each returns a small structured outcome
//! describing what happened (or `None`/a rejection variant if nothing did),
//! instead of reaching out to record a Replay event or queue audio itself.
//! [`MatchState::resolve_ordinary_round`] and [`MatchState::finish`] do the
//! same for round completion and scoring: [`RoundOutcome::sound`] is the pure
//! capture/Gray summary the rules produced, not a Bevy audio cue - mapping it
//! to one is the client's job. Orchestration (the Bevy client, or a future
//! headless host) reads these outcomes to decide what to display, which
//! sound to queue, and what to hand a Replay recorder - this module never
//! imports Replay's recording types or Bevy's audio types, so a Replay or a
//! sound is always a *record of* what the authoritative state did, never a
//! dependency of it.
//!
//! Time passes the same way. [`MatchState::advance_clock`] advances the
//! Match's own clocks to the next authoritative boundary - a closing
//! countdown stage, the atomic reveal at `Go`, the Go card clearing, or a
//! Round Time or Match Time deadline - applies that transition, and returns
//! a [`ClockStep`] naming it as a [`ClockBoundary`]. It deliberately stops
//! *before* resolving the round a deadline ended or finishing the Match,
//! because those are exactly the steps the caller interposes on: the Bevy
//! client records the post-resolution frame and may defer an automatic end
//! to an in-flight end gesture between them, and a headless host
//! answers with the same [`MatchState::resolve_ordinary_round`] and
//! [`MatchState::finish`] calls it already makes for locks. A caller drives
//! a Match through time by stepping until nothing is crossed;
//! `tests/headless_match.rs` does exactly that with no client feature.
//!
//! Every command method already takes its player identity and Match-clock
//! time as *parameters supplied by the trusted caller* - `try_lock(player,
//! now)`, never `try_lock()` reading a stored "current player" or a clock
//! itself. Today the Bevy client is that trusted caller: one local process
//! plays the role of host for both players, so it is always the one that
//! decides whose turn produced a command and what time it happened at. A
//! future networked host would keep exactly this shape but stop trusting the
//! network for two of those inputs: a remote client should submit only its
//! *intention* (which point, which action) and never be trusted to also
//! choose its player identity, assign its own authoritative timestamp,
//! replace arbitrary `MatchState`, or claim a move already succeeded. The
//! host - not the client - would still attach the identity and
//! [`crate::clock::MatchInstant`] before calling these same methods, exactly
//! as `gameplay` does today. This crate does not implement networking,
//! authentication, or that intent/command split as distinct types yet; the
//! command methods' existing "identity and time are caller-supplied
//! parameters, never read from untrusted state" shape is what a future
//! split would build on without changing Match rules. See
//! `docs/SECURITY.md` for the same boundary from the untrusted-input side.
//!
//! # Clocks
//!
//! Every time-sensitive method takes an explicit [`crate::clock::MatchInstant`]
//! (an absolute Match-clock timestamp) or [`crate::clock::MatchDuration`] (a
//! delta) - whole microseconds, not `f64` seconds - rather than reading a
//! clock itself. In the Bevy client that value always originates from Bevy's
//! `Time::delta()`, converted once at the Bevy boundary in
//! `crate::gameplay::countdown`; nothing here assumes that source. A future
//! server driving the same methods from its own tick would supply the same
//! shape of value. What the caller supplies is only *how much* time passed:
//! everything about *what that time means* - the countdown thresholds in
//! [`crate::opening`], the reveal, the Go card, both deadlines, and the cap
//! that keeps `elapsed` within what a Replay can persist - is decided inside
//! [`MatchState::advance_clock`], in the same integer arithmetic, so no host
//! re-derives Match timing and no `f64` enters an authoritative decision.
//! See `crate::clock`'s own module docs for the full vocabulary, including
//! why comparisons need no epsilon fudge and how these values relate to
//! Replay's separately-persisted `f64`-seconds timestamps.
//!
//! # Rollback
//!
//! [`MatchState`] separates the fields that change only as the result of
//! moves and round resolution into [`MatchProgress`] from the fields that
//! must keep advancing (or stay put) regardless - cursors, Teleport Markers,
//! Round 0 selections, the phase, the immutable [`MatchRules`] snapshot, and
//! the Match and closing-countdown clocks. The Round Time clock is the one
//! clock deliberately inside [`MatchProgress`]: it measures the round a
//! rollback restores, so it is restored with it. Cloning one `MatchProgress`
//! is the rollback mechanism the Bevy client uses for its end-gesture hold:
//! capturing and restoring one clone cannot omit a field the way a
//! hand-written list of fields to snapshot can, so a new field added to
//! `MatchProgress` is rolled back correctly with no extra code, while a
//! field added anywhere else in `MatchState` is never rolled back by
//! construction.
//!
//! # Privacy
//!
//! `MatchProgress::locks` (each player's hidden selection) is deliberately
//! `pub(crate)`, not `pub`: a headless consumer of this crate's public API
//! sees the board, cursors, round, phase, the derived score and everything
//! else, but never both players' pending hidden choices through a general
//! accessor. `pub(crate)` - rather than something narrower like `pub(in
//! crate::gameplay)` - is the tightest visibility Rust allows here, since
//! `locks` is declared in this module and `gameplay` is a sibling, not a
//! descendant, of it; [`crate::view`] is the actual privacy boundary for
//! anything that is not itself the trusted authoritative host. See
//! [`crate::view`]'s module docs for [`crate::view::PublicMatchView`] and
//! [`crate::view::PlayerMatchView`], the read-only projections a future
//! networked opponent, spectator, or broadcast should be built on instead of
//! reaching into `MatchState` directly - and `docs/SECURITY.md` for the
//! trust boundary a future untrusted client's *commands* would need, which
//! this crate does not implement yet.
//!
//! The local shared-screen UI is deliberately *not* one of those audiences.
//! It is the trusted host's own renderer, drawing for both players on one
//! monitor, so it reads `MatchState` directly by design: the union of both
//! players' authorized views is very nearly `MatchState` itself, and routing
//! per-frame rendering through the projections would clone a `Board` and a
//! `Vec<EndReason>` every frame to defend against an audience separation
//! that does not exist when both players share a screen. What that renderer
//! owes players is a *rendering discipline*, not a type boundary: during
//! live play it must never draw either player's hidden data - the pending
//! ordinary lock, the Round 0 selection points (only their count and fill
//! state), or either Teleport Marker. [`crate::view`] neither enforces that
//! discipline nor is needed to.
mod outcomes;
mod state;
mod timing;
mod values;

#[cfg(test)]
mod test_support;

// The public API. Every path a consumer used before this module became a
// directory still resolves here; no call site outside `authority` names a
// submodule.
pub use outcomes::{
    ClockBoundary, ClockStep, CursorMove, LockOutcome, OpeningSelectOutcome, RoundOutcome,
    TeleportJump,
};
pub use state::{MatchProgress, MatchState};
pub use values::{
    MatchPhase, MatchRules, OpeningAttempt, OpeningAttemptOutcome, OrdinaryCompletion,
    PLAYER_STARTS, RoundRemovals, player_index,
};
