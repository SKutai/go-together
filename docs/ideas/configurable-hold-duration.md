# A settable hold duration

Status: **considered and parked 2026-10-05 by the project owner.** Not
rejected and not scheduled; the owner's position is that the game is good
without it for now, and that anyone who finds this and wants to pick it up
is welcome to. Nothing here describes current behaviour - see
[`README.md`](README.md).

## The idea

Let a player change how long the end gesture has to be held, in Settings,
instead of `HOLD_DURATION`'s fixed three seconds.

## Why it was raised, and why it was parked

The accessibility review found that three seconds was fixed **and** that
the accumulator reset to zero on a single unregistered frame, with nothing
in Settings touching either. Under shipped defaults the dual-Select hold is
the only route to a scored match, so a player who could not hold steadily
could not finish a game at all.

The second half of that was fixed on its own: the end hold now decays at
half the rate it fills rather than resetting, so intermittent contact still
completes. That was judged enough. Three seconds is also, in the owner's
words, "a fair amount of time to think" - the length is a design choice
about deliberateness, not an obstacle that happened to be there.

So this is parked for a reason rather than forgotten, and the reason is
that the barrier was the cliff and not the length.

## What it would cost, for whoever picks it up

- **A settings schema bump.** `GamePreferences` is at v6; v7 would need the
  same care the Reduce Motion preference needed. Watch
  `settings::compat`'s per-section gates: the Reduce Motion bump found that
  gating a section on the *current* version constant silently loads every
  existing player's file with that section's defaults.
- **The Replay.** `HOLD_DURATION` is a constant the whole crate reads. A
  Replay records what happened, not how long a gesture had to be held, so
  playback should be unaffected - but that is worth confirming rather than
  assuming.
- **The help copy.** The Ending page tells players to hold for 3 seconds,
  and `docs/GAME_RULES.md` says three seconds in several places. A
  configurable duration makes every one of those sentences conditional,
  which is more copy than it sounds, and the help pages have little room
  left - `docs/TESTING.md` carries the margins.

## What would make it worth doing

Evidence that decay is not enough: a player who still cannot complete the
gesture with it. That is the thing to look for before building this, and
nobody has reported it yet.
