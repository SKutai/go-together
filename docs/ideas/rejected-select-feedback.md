# Showing a player that a Select was refused

Status: **the owner's idea, recorded 2026-10-06 and deliberately not
scheduled.** The finding it answers is closed, so this is not a fix waiting
to happen; it is a shape worth having if the subject is ever reopened.
Nothing here describes current behaviour - see [`README.md`](README.md).

## What the game does today

A refused Select is silent. Pressing Select on a point that is not empty,
where an active cleanup is also not legal, changes nothing on screen and
plays nothing.

That silence is deliberate and load-bearing, and
[`../SECURITY.md`](../SECURITY.md) carries the reason: after a press on a
visibly *empty* point, whether the selection took is "the
accepted-or-reserved answer that silent rejection refuses to give". A
generic refusal cue fires for the opponent-reserved case too, and so reads
out their hidden point by when it appears. The game once had a contextual
alert strip and it was removed for this;
`live_match_ui_has_no_contextual_alert_strip` in `ui::match_screen` scans
the source to keep it gone.

## Why the finding was closed rather than fixed

Reviewed with the owner on 2026-10-06. The refusals that are safe to report
are the ones the public board already explains, and for most of them the
explanation is sitting on the point: a player who presses an enemy stone or
a Gray wall has the stone in front of them and is not confused for long.

One case inside it is genuinely not obvious, and is the part worth keeping:
**a failed active cleanup on the player's own stone.** `Board::apply_cleanup`
refuses for two structural reasons - the point is not the player's own
stone, or no orthogonally adjacent opposing group currently has zero
liberties - and the second is a liberty count the player has to work out by
eye. The point looks exactly as selectable as it did a move earlier.

That was judged too small to justify what a fix costs: reversing a
deliberate decision, replacing a guard test that exists to keep a leak out,
and carrying a refusal reason out of `authority` where today none escapes.

## The owner's idea, if it is ever reopened

Two forms, the first preferred:

1. **The refused stone answers.** Play the wooden clack, and have the stone
   that was selected shrink a little and fade a little, then reverse back to
   normal.
2. **The group answers instead.** The same gesture, performed by the group
   the player was apparently trying to capture.

The owner's own objection to the second is the reason the first is
preferred: **it is often impossible to know which group a player meant.** A
stone can sit adjacent to several opposing groups, and a cleanup that
refuses did not identify one - `adjacent_zero_liberty_opponent_groups`
returned nothing, so there is no group to point at. Guessing would teach
the wrong rule.

## What whoever picks this up will meet

- **A clack is a persisted cue.** `GameCue::OpeningDuplicate` is Round 0's
  duplicate clack and means something else; reusing it would overload a
  vocabulary that Replays record. A new variant is a schema concern, and
  `GameCue` is `#[non_exhaustive]` for exactly that reason.
- **Shrink-and-fade is motion**, so the policy in `src/app/motion.rs`
  applies: it must be reducible, and whatever the refusal conveys has to
  survive with motion off. A gesture that returns the stone to exactly its
  starting state has no end state to carry it, which is the hard half of
  rule 2 - the same problem the Round 0 dots met, where the answer was a
  mark on the row's divider rather than anything about where the dots
  finished.
- **The safety argument has to be rebuilt, not assumed.** Feedback may fire
  only where the public board already explains the refusal. The structural
  way is a refusal variant that exists only for the public case, so there is
  none a reserved refusal could travel through, and a guard asserting no
  feedback path is reachable from one.
