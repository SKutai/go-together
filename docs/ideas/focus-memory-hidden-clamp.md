# The focus ring can be moved while nobody is looking

Status: **known residual, accepted 2026-09-24 by the project owner.** Not a bug
report and not scheduled work - it is here so that the next person to notice it
finds a decision rather than a surprise. Nothing in this file describes a
defect that needs fixing; see [`README.md`](README.md).

## What happens

`refresh_focus_candidates` in `src/ui/focus.rs` re-resolves the remembered
`(group, order)` key against the current candidates **every frame**, and writes
the result back into the memory - including on frames where the player has
pressed nothing and the ring is not drawn at all.

`resolve_focus` never clamps upward. When the remembered order is gone it takes
the nearest *lower* order, and failing that the group's first entry.

Put together: anything that removes controls while the ring is hidden quietly
rewrites where the ring will next appear, downward. A player who mouses around
the Replay Library - filtering the list with the search box, deleting a replay,
paging - and then reaches for the keyboard will find the ring somewhere earlier
in the screen than they left it, with no event having moved it.

## Why it is accepted rather than fixed

The Settings screen met the same mechanism first and accepted it for the same
reason, so both screens behave alike today. Fixing it properly means the core
exposing its memory for a screen to put back, which no screen does and which
would add surface area to `focus.rs` after several screens have been built on
the current behaviour.

Weighed against that, the symptom is mild. The ring reappears a row or two from
where it was, in a place that is always drawn and always valid - never on a
despawned entity, never outside the visible list, never in a hidden tab panel,
because the same pass that moved it also proved it against real layout. Nothing
is activated, nothing is lost; the player presses an arrow key and carries on.

It is also self-limiting: it can only ever move the ring *earlier* in a
screen's order, never onto something new, and the bands on each screen are
arranged so that the fallbacks are harmless controls.

## If it is ever revisited

The shape of a fix, for whoever picks this up:

- The core would need a way for a screen to say "restore this key" rather than
  only ever reading what the core resolved - some affordance on
  `UiFocusState`, which is `pub(crate)` but whose memory is private today.
- The harder half is deciding *when* a screen may restore, since the whole
  point of the per-frame rewrite is that it keeps the memory honest against a
  candidate list that changes underneath it. A restore that fights that would
  reintroduce stale keys pointing at despawned entities, which is exactly what
  keying on `(group, order)` rather than `Entity` was designed to avoid.
- Any change here touches every screen with a ring at once, so it wants its own
  task and its own pass over all of them, rather than riding along with a
  screen.

Worth noting that the accepted behaviour has one genuine virtue: it is
stateless. The memory is always a fact about the current frame's candidates
rather than a wish about a frame that has gone, and that is why no screen has
ever had to reason about a stale focus key.
