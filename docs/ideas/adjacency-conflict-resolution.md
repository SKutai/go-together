# Resolving a contested point by adjacency instead of by timing

Status: raised 2026-09-19 by the project owner; **deliberately deferred
2026-09-23**, not rejected. Nothing in this file describes current behaviour;
see [`README.md`](README.md).

The reason for deferring is worth more than the idea itself, so read it before
picking this up: the owner's judgement is that it would change the game too
much, and that the change would not end at the resolution rule. Answering it
properly means answering what happens to chains, to scoring, and to balance -
and then making all of that intuitive to a player who should not have to read a
textbook to understand their own game. That is a design project, not a rule
tweak, and the "open questions" section below is the evidence: every one of
them is a fork that needs its own answer.

So do not pick this up as a small change. If it is ever taken on, it is taken
on as a deliberate redesign with the owner steering, and the first work is
deciding between neighbour count and group size, because everything else
depends on it.

## The idea

When both players select the same point, decide who gets it by counting
adjacent stones - **including diagonals** - rather than by who chose first.

## What it would replace

Today a contested point is decided by the clock. `SIMULTANEOUS_WINDOW`
(`src/rules/resolution.rs`) is 100 milliseconds:

- More than 100ms apart, the earlier selection applies first and the later one
  resolves against its result - `ResolutionKind::Sequential`.
- Within 100ms, neither player gets the point: it becomes a permanent Gray
  stone - `ResolutionKind::SamePointGray`.

So the contest is currently a reaction-time race, and the faster decider takes
every contested intersection that falls outside the tie window.

## Why it is worth considering

It turns a reflex contest into a positional one. A player who is slower - by
temperament, by motor impairment, by processing speed, or because they are
thinking harder - is not thereby worse at claiming points, and a mixed-ability
pair gets a fairer game without needing a handicap setting bolted on.

That connects to something an accessibility review of this project raised
separately: the 100ms window is the one timing rule in the game a player cannot
adjust, and it silently favours the faster player. Adjacency resolution would
dissolve that problem rather than mitigate it.

It is also more in the spirit of Go than a race is. Rewarding the player whose
existing shape already reaches a point is a positional judgement, which is what
the rest of the game is about.

## The ambiguity to resolve first

"The count of the length of adjacent stones" admits two readings, and they are
materially different games:

1. **Neighbour count.** How many of the eight surrounding intersections hold
   one of your stones. Range 0-8, local, easy to see at a glance, easy to
   explain.
2. **Group size.** The combined length of the connected groups touching the
   point. Range 0 to most of the board, global, rewards large existing shapes
   far more strongly, and is much harder to eyeball mid-match.

Reading 2 concentrates advantage: a player who is already ahead has bigger
groups and therefore wins more contests, which compounds. Reading 1 is bounded
and stays local. Decide which before anything else, because almost every other
question below depends on it.

## The cost of diagonals

This is the part to weigh carefully. Adjacency in this game is orthogonal, and
it is orthogonal everywhere: `neighbors` in `src/rules/board.rs` returns at most
four points, and it is the single notion behind groups, liberties, captures,
suicide, ko and active cleanup alike.

Counting diagonals for conflict resolution would give the game **two different
meanings of "adjacent"** - one for whether stones are connected, another for
who wins a contested point. That is a real teaching cost in a game whose
in-game help already assumes more Go literacy than a newcomer has, and it is
the kind of split that makes a rule feel arbitrary rather than principled.

Worth doing deliberately if diagonals are doing real work here - there is a
reasonable argument that "influence" should reach diagonally even though
"connection" does not, and that is exactly the distinction Go players draw
informally. But it should be a decision, not a detail.

## Open questions

- **Neighbour count or group size?** See above. Decide first.
- **What happens on a tie?** Equal counts must resolve somehow. Falling back to
  Gray keeps the current outcome and needs no new vocabulary. Falling back to
  timing keeps the race but only as a tiebreak. Doing something else is a third
  option.
- **Does this replace timing entirely, or only inside the 100ms window?** The
  smaller change is to keep `Sequential` for clearly-separated selections and
  use adjacency only where the rule currently produces Gray. The larger change
  makes adjacency decide every contest and reduces the clock to nothing. These
  are different games; the first is far cheaper to try.
- **Whose stones count?** Only your own, or your own minus your opponent's?
  The second makes a contested point near an enemy wall harder to take, which
  may be the more interesting rule.
- **Do Gray stones count?** They belong to neither player and give no
  liberties - the existing rule is that they behave like the edge of the board.
  The consistent answer is that they count for nobody, but it should be stated.
- **An optional rule or the default?** The project already models optional
  bounded rules through `MatchSettings`, each match taking an immutable
  snapshot, so shipping this as a toggle fits the existing shape and lets both
  rules be played. It is also more work than changing the rule outright.
- **Is the winner discoverable?** A player who loses a contested point should
  be able to see why. Today the outcome is a Gray stone, which is unambiguous.
  "You lost that point because they had three neighbours and you had two"
  is not visible anywhere, and the board's hidden-selection secrecy means it
  cannot be shown before the round resolves.

## What it would not break

Existing Replays are safe. Playback does not re-run the rules - each recorded
frame is a full board snapshot and `src/replay/schema.rs` states that playback
draws it directly - so a Replay recorded under today's resolution keeps
replaying exactly as it did, whatever the live rules become.

A new `ResolutionKind` variant would be needed if the outcome is to be
distinguishable from `SamePointGray` in recordings, and that enum is part of
the persisted schema. See
[`../../CONTRIBUTING.md`](../../CONTRIBUTING.md#changing-persisted-data).

Whatever is decided, `docs/GAME_RULES.md` is the document that must end up
describing it, and the in-game help pages in `src/ui/help/mod.rs` are
player-facing documentation subject to the same standard.
