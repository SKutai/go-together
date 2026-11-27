# Implemented Game Rules

This document describes what the current code does. Go Together is inspired by
Go, but it is not standard Go.

## Board and players

- The board is a 9x9 grid of intersections.
- Black is Player 1 on the right side of the screen.
- White is Player 2 on the left side.
- Black, White, and Gray stones may occupy an intersection.
- Each player has a cursor and one optional Teleport Marker. Live cursors are
  hidden during Round 0, visible from the Round 0 reveal through ordinary play,
  and hidden again once the Match is scored.
- A Teleport Marker moves only the cursor. It may be replaced and may be set on
  an occupied intersection.
- Neither Teleport Marker is drawn during live play, so each player must
  remember where their own marker is. Replays show both markers.
- The five star points marked on the grid are decorative landmarks only; they
  affect no rule.

There is no pass action. A Match ends through the hold gesture described below.

## Starting a Match

Entering or restarting a Match clears the board, score, capture totals,
selections, Teleport Markers, history, timers, no-board-change streak, and
Replay recorder. It also takes an immutable snapshot of the current Game
settings.
Editing settings later does not alter that Match; restarting uses the latest
saved values. Saved controls and preferences are not reset.

The Match enters interactive Round 0 immediately, with no pre-Match countdown.
The Match timer and Replay timeline both start at Round 0 elapsed zero and
continue into ordinary play. Opening a Help or Home modal pauses the clock,
Round 0 actions, its closing countdown, and recorded time. Closing a modal
resumes all of that at once. An input that was already held is suppressed until
it is released and pressed again, but only that action: one player's held key
never suppresses the other player's. Restart is the one exception, because it
is a single shared action rather than one per player, so a held Restart stays
suppressed for both until it is released.

Temporary side hints show only each player's current movement and Select keys.
White's hint is on the left and Black's is on the right. Each hint disappears
independently after that player first moves at least one intersection, makes an
accepted selection, sets a Teleport Marker, or teleports. A rejected Select
press, or a Round 0 re-selection of a point already held, leaves the hint up.
Modal pauses do not restore a hidden hint; a fresh Match restores both.

## Round 0: hidden opening

Round 0 starts on an empty board. Both live cursors and every selected location
are hidden from both players. Each sidebar shows only a row of dots for that
player's selection count: a line divides it, with the selections held filled on
the board side of the line, newest beside it, and the rest empty on the other.

Each player privately builds an ordered set of three distinct intersections:

1. A distinct selection is appended while fewer than three are held.
2. With three selections already held, another distinct selection removes the
   oldest point and appends the new one.
3. Selecting a point already in that player's current set changes nothing and
   plays the Round 0 wooden clack, and that player's line is drawn as a cross
   until their next selection - the same news in a form that does not need
   sound.

The two players may select the same point; that overlap remains secret. Movement
and Teleport Marker actions work normally throughout Round 0.

As soon as both players first hold three selections, `3`, `2`, and `1` each
display for one second. All Round 0 actions remain available, so either player
may keep moving, replace selections through the FIFO rule, or set and use a
Teleport Marker. The countdown never restarts. The three numbers use the same
single marimba note.

At `Go!`, the Go chord plays and the final six slots reveal atomically:

- a point selected only by Black becomes Black;
- a point selected only by White becomes White; and
- every point selected by both players becomes permanent Gray.

The six revealed stones are then swept for captures in one step:

1. every Black or White group with zero liberties on that completed board is
   found at once, checking both colors against the same starting position so
   neither is evaluated first;
2. every such group is removed simultaneously; and
3. when Capture Points is enabled, each player is credited for every opposing
   stone removed during this sweep.

If both colors have a zero-liberty group, both are removed together; a group
is never spared because removing the other color first would have given it a
liberty. Gray stones are permanent neutral walls throughout: they count as
occupied when liberties are calculated and are never themselves removed.
Suicide and positional-repetition rules do not otherwise apply to this
one-time reveal sweep - a group is removed only for having zero liberties on
the completed board, not for whichever player placed it. This sweep is
distinct from the active cleanup selection described under
[Ordinary hidden selection](#ordinary-hidden-selection): the reveal sweep runs
once, automatically, on every Match; active cleanup is an ordinary-round
choice either player can make repeatedly. The cleaned board, not
the raw six-slot reveal, becomes the ko-history baseline. The opening choices
and count dots clear, both cursors become visible at their current logical
positions, and ordinary Round 1 begins. The `Go!` card stays up for 0.75
seconds, but Round 1 is already live beneath it: selections are accepted, the
Round Time clock is running, and the end gesture counts from the reveal, not
from the card clearing. No additional placement, Gray, or capture cue is added
on top of the Go chord.

## Ordinary hidden selection

Each ordinary round, beginning with Round 1, starts with no selected moves.

1. A player moves their cursor to any intersection. Selecting an intersection
   that is empty on the currently visible board records an ordinary
   placement. Selecting one of that player's own stones instead records an
   active cleanup, provided the stone currently borders at least one opposing
   group with zero liberties. A suicidal placement is allowed.
2. Pressing Select records that point, and the moment of that press, without
   showing it to the opponent.
3. Before the opponent selects, that player may move and press Select again.
   Selecting a *different* point is a new decision: it replaces the older
   selection and takes the new press's time. Pressing Select again on the
   point already selected is not a new decision and changes nothing at all -
   the point and its recorded time both stand, because the choice was made by
   the first press.
4. When both players have selections, the round resolves immediately.

Moving or teleporting after Select does not move the saved selection. An
occupied point that is not one of the selecting player's own eligible stones,
or a sequentially reserved same point, is ignored without a live error
message, sound, marker, or any other confirmation. Selection does not predict
whether an earlier hidden action will make a later selection suicidal, an
invalid cleanup, or a valid one; that is decided during resolution.

An active cleanup selection is otherwise hidden exactly like an ordinary
placement: it can be freely replaced, and both what it will do and whether it
is even still valid are decided only when the round resolves. A successful
cleanup removes every opposing group with zero liberties orthogonally
adjacent to the selected stone, all at once; it places no new stone, and the
selected stone itself is neither moved nor removed by its own cleanup. When
Capture Points is enabled, the acting player is credited for every opposing
stone removed this way. A cleanup selection that turns out invalid - for
example, because an earlier hidden action removed the targeted group, or
saved it by giving it a liberty - is silently ignored at resolution exactly as
an invalid selection is ignored at lock time: no message, and the round still
advances on whatever the other selection did.

After a successful resolution, both selections clear and the displayed round
number advances.

When Round Time is enabled, its clock starts at the Round 0 reveal and resets
after every ordinary round. While it runs, a bar below the board shows the
time left in the round: full as the round starts, it shrinks from both ends
toward the centre as the clock runs down, and it disappears when no round
clock is running. That bar is not a preference and cannot be turned off, so
no round can end on a deadline nobody was shown; a hold gesture in progress
takes its place for as long as the hold lasts, and a released end hold keeps
it while its progress drains. The numeric `Round MM:SS`
countdown beside the round number is a preference (see
[Ending and scoring](#ending-and-scoring)). At expiry, the round completes
with the selections currently available: two resolve normally, one is
attempted alone, and none advances without a placement. A pending suicidal
selection resolves normally, including self-removal. An open modal stops both
Match and Round clocks; a held input does not, because the input-release gate
suppresses one action at a time while the clocks are shared by both players.

## Sequential and simultaneous timing

Let the earlier and later selection timestamps differ by `elapsed`.

- If `elapsed > 0.1` seconds, resolution is **sequential**.
- If `elapsed <= 0.1` seconds, resolution is **simultaneous**.

Exactly 0.1 seconds is therefore simultaneous.

Each timestamp is the moment of the press that first chose the point that
player is holding, not of their most recent press. Re-selecting a point already
selected leaves the timestamp where it is, so a player cannot keep their own
time at "now" by pressing Select repeatedly, and cannot pull a round that would
otherwise resolve sequentially into the window that way.

### Sequential resolution

The earlier move is played first, including opponent captures and any complete
self-removal. The later move - an ordinary placement or an active cleanup - is
then played on that resulting board and may also capture, self-remove, or
remove an opposing group.

Because the later action is evaluated against the board the earlier one
produced, an earlier capture or self-removal can invalidate a later cleanup:
if the group it targeted was removed, or gained a liberty, by the earlier
action, the cleanup fails silently and the round still advances on the
earlier action alone.

If both players target the same point more than 0.1 seconds apart, the point is
reserved by the earlier selection and the later selection is rejected. The
later player must select another legal point.

A reservation ages from the press that first chose the point, so the earlier
player cannot reset it by pressing Select on the same point again. Once their
selection is more than 0.1 seconds old, the point stays reserved for as long as
they hold it: re-pressing cannot make that selection look recent again and so
turn the opponent's later attempt on the point into a same-point tie instead of
a rejection.

This rejection carries no message, sound, marker, or other confirmation to
either player, and it does not reveal the earlier selection. The later player
can only infer that the point was already reserved because their own
selection silently failed to register; nothing distinguishes this from
selecting a point that is simply occupied. The earlier player is never told
that their hidden point was targeted, and may go on moving, teleporting, and
replacing their own selection exactly as if the attempt had never happened.

### Simultaneous resolution

Each selection - an ordinary placement or an active cleanup - is evaluated
independently against the same pre-round board. For different points, both
selections' placements, opponent-capture sets, acting-group self-removal
sets, and cleanup removal sets are merged:

- Black's move may capture White groups, and Black's cleanup may remove
  opposing zero-liberty groups;
- White's move may capture Black groups, and White's cleanup may remove
  opposing zero-liberty groups;
- either acting group may self-remove; and
- every stone in an evaluated removal set becomes empty.

If both players perform valid cleanup selections simultaneously, both may
capture. Because each selection is judged only against the untouched
pre-round board, a cleanup remains valid even if the other player's
simultaneous action independently captures the very stone used to select it:
the selected stone still disappears, but the opposing group it targeted is
still removed.

The merged board is not reevaluated for liberties. A group removed by its
pre-round snapshot stays removed even if the other snapshot's removals would
have opened a liberty. Capture credits are deduplicated by removed position and
stone ownership.

**Experimental, subject to change after playtesting:** because neither
snapshot ever sees the other player's simultaneous placement, a colored group
can end the round with zero liberties without being removed - for example,
when each player's move independently consumes a different one of that
group's last two liberties, or when one player's move extends their own
group into a shape whose only remaining liberty is exactly the point the
other player just filled. No automatic global zero-liberty cleanup runs after
an ordinary simultaneous round to catch this. The group is simply left
standing, exactly as merged, until a player later spends an active cleanup
selection on an eligible bordering stone of their own (see
[Ordinary hidden selection](#ordinary-hidden-selection)) - or until the Match
ends, at which point a surviving zero-liberty group scores exactly like any
other stones on the board, since area scoring counts what is present and has
no notion of liberties.

This intentionally avoids giving an ordering advantage to a player whose input
arrived only a few milliseconds later.

If both players select the same empty point within 0.1 seconds, neither color
is placed; that point becomes Gray. Two cleanup selections can never target
the same point this way, since a point can hold only one player's stone.

## Liberties, captures, and suicide

Black and White stones connect orthogonally, never diagonally. A connected
same-color group has the union of all orthogonally adjacent empty
intersections as liberties.

For one ordinary move:

1. place the player's stone;
2. remove every adjacent opponent group with no liberties;
3. inspect the placed stone's own connected group; and
4. if that group still has no liberties, remove the entire acting group,
   including friendly stones connected to the new stone.

Captures happen before the suicide check, so a placement that gains liberties
by capturing survives. When Capture Points is enabled, ordinary opponent
captures are credited to the actor, while every self-removed acting stone is
credited to the opponent. Gray stones are occupied neutral walls: they do not
join either player's groups and are never captured.

An active cleanup instead:

1. finds every opposing group with zero liberties orthogonally adjacent to
   the selected stone, on the board as it stands when the cleanup resolves;
2. removes every such group at once, giving none of them priority over the
   others; and
3. leaves the selected stone itself in place - a cleanup never captures or
   self-removes its own stone, though another action in the same round can
   still remove it.

When Capture Points is enabled, the acting player is credited for every stone
removed this way, the same as an ordinary capture.

## Positional repetition

After a round is resolved, its candidate board is compared exactly with every
distinct earlier completed board from the entire Match. If any match:

1. discard the entire candidate board;
2. restore the board from before the round; and
3. turn each unique, legal attempted intersection that was empty before the
   round Gray.

For a one-player timeout only that player's point is converted. Invalid or
previously occupied attempts never overwrite an existing stone. Tentative
ordinary captures and self-removal credits are discarded and award no capture
points. A pure suicide that removes only its newly placed stone recreates the
pre-round board, so it is repetition and its attempted point becomes Gray. The
resulting Gray board is accepted without another repetition check. The round
still completes and advances.

A cleanup's attempted point already held the selecting player's own stone
before the round, so it is never empty beforehand and is never converted to
Gray by this rule. A repeated position reached through a cleanup-only round
therefore just restores the pre-round board, with the same stones it already
had.

Nothing distinguishes a deliberate attempt to recreate an earlier position
from an accidental one: selection legality never looks at Match history, so
knowingly steering a round toward repetition to plant a Gray wall is a
legitimate strategy, not an exploit.

## Gray stones

Gray stones are permanent and neutral. They are created by:

- a point held in both players' final Round 0 windows at the reveal;
- simultaneous same-point selection in an ordinary round;
- valid attempted points in a repetition-rejected round.

A Gray intersection cannot be selected, does not connect to Black or White,
cannot be captured by any means - an ordinary move, an active cleanup, or the
Round 0 reveal sweep - and scores for neither player. Gray provides no
liberties to an adjacent group, exactly like the edge of the board; a
colored group can be fully enclosed, and captured, by any combination of
Gray stones and opposing stones.

Gray stones block territory traversal but are never a region's boundary
owner: a region bordered only by Black (Gray stones and the board edge
aside) belongs to Black, one bordered only by White belongs to White, and a
region whose entire border is Gray (and the board edge) stays neutral and
unclaimed by either player.

## Ending and scoring

Beginning in Round 1, both players must hold their own Select input for three
seconds. The end gesture is disabled throughout Round 0 and its closing
countdown; Restart remains available.

The hold need not be unbroken. If either Select comes up, the hold's progress
drains at half the rate it filled rather than resetting, and pressing both
again carries on from wherever the drain has left it, so a player who can
keep contact only half the time still completes the gesture in about twelve
seconds. Each lapse does end the gesture, though, so a hold finished over
several contacts rolls back only the last contact's incidental selections
when it completes; the rounds earlier contacts resolved stand, as a released
tap's round does. The progress bar below the board drains with it, so a
lapse is seen while there is still time to act on it. This is the end
gesture's
accommodation alone: the restart hold still resets the moment its input comes
up, because restarting is destructive and takes one shared input, and nobody
is kept from playing by restart being hard to complete.

The end gesture outranks Restart. Once both Select inputs are held, a held
Restart neither clears nor pauses the end hold, and the restart hold is
cleared - any progress a Restart already held had made is discarded, not
paused - until one of the Selects comes up or the Match ends. This is a
deliberate trade, not a strictly safer rule. The semantic input ORs its
sources, so one player at a shared keyboard can hold both default Select keys
alone, and holding Restart does not let the other player stop that forced
scored end. What still does is a modal: a Select held while Help or the Home
confirmation opens and closes stays suppressed until it is released (the rule
under [Starting a Match](#starting-a-match)), so opening and closing one stops
the gesture exactly as a release would - the incidental selections stand, and
an automatic end they had already reached is committed - and, unlike a
release, returns its progress to zero outright, because a modal reads as the
gesture having been interrupted, which it was. Returning Home remains the
destructive way out. The two outcomes are not equally bad,
which is why the trade is made: a forced scored end saves the Replay and
leaves a fresh Match one restart away, while a forced restart - which needs
only one shared input - destroys the Match with no record. The gesture meant
to take two people is the one given the benefit of the doubt.

The first frame of that hold snapshots all rollback-sensitive Match state,
including the board, current selections, full board history, counters, round
clock, end metadata, and Replay recorder. This matters because a short
simultaneous Select press is also a legitimate round:

- release before three seconds and the ordinary selections stand, committing
  any automatic end they triggered exactly once;
- complete the hold and those incidental Select presses are rolled back before
  scoring with Agreed Ending as the sole reason.

An automatic end reached while the hold runs - by the incidental round, or by
a round that times out during the hold - remains provisional while both Select
inputs stay held. It does not save a Replay or emit the Match End cue
until the gesture is released or completes. While it is pending the Match is
stopped, because whether it has already ended is not yet decided: the Match and
Round clocks stop, so no Round Time or Match Time deadline can land on top of a
provisional end, and play stops with them - cursor moves, Teleport Markers and
selections are all ignored. The end gesture and Restart keep working, since
they are what settle it, and every way of settling it ends the Match or
abandons it, so play never resumes. If Restart is already held as the Selects
come up, the end stays deferred while Restart is held, or until a resumed hold
settles it; see [Restarting](#restarting).

Until an automatic end has actually been deferred, deadlines still land during
the hold. A Round Time expiry resolves the round with timeout semantics and the
hold continues; completing the hold rolls that round back too, because the
snapshot predates it. A Match Time expiry ends the Match at once with Match Time
Limit, plus any other limit the timed-out round crossed: the hold is abandoned,
its snapshot discarded, the incidental selections stand, and Agreed Ending
is not recorded.

Scoring then fills each connected empty region:

- if its Black/White boundary contains only Black, fill it Black;
- if its boundary contains only White, fill it White;
- if it touches both colors or neither color, leave it neutral;
- Gray stones block region traversal but are not a boundary owner.

The final area score counts Black and White intersections after that fill.
Empty and Gray intersections score zero. When Capture Points is enabled, each
player's cumulative capture credits are added to both the progressive and final
area score. Credits include opposing stones captured normally, all stones
self-removed by the opponent, and opposing stones removed by the Round 0 reveal
sweep. Repetition rollbacks and Gray conversions do not award captures. There
is no komi.

Optional Match rules can end play after an ordinary round reaches a configured
round count, no-board-change streak, or point target. That streak counts
consecutive completed ordinary rounds whose board is exactly the same as it
was when the round started. It resets to zero whenever a completed round
causes any of the following:

- a Black, White, or Gray stone is placed;
- one or more stones are removed by an ordinary capture;
- one or more stones are removed by suicide;
- one or more stones are removed by an active cleanup; or
- positional repetition replaces attempted placements with Gray.

It increments only when none of those happen - when the round's selections
resolve to exactly the board they started from. If several limits trigger
together, every reason is retained. Round 0 is excluded from these three
limits.

Match Time includes Round 0. At expiry during Round 0, all current opening
choices (including partial or empty choices) reveal atomically and the Match is
scored immediately. At expiry during ordinary play, the current round first
resolves with timeout semantics, then all simultaneous end reasons are
recorded.

The Game tab stores optional numeric rules as bounded whole values. Round Limit
and End After Rounds Without a Board Change accept 1-1000; Point Target
accepts 1-81; Round Time and Match Time accept 1-3600 integer seconds. An
enabled Round Time cannot exceed Match Time. A disabled rule is Off but
retains its edited value. Capture Points is an On/Off rule.

Turning Round Time or Match Time on also switches on its clock's readout on
the Preferences tab - Show Round Number for Round Time, whose panel then reads
`Round 3  |  01:30 left`, and Show Match Timer for Match Time. The readout
is revealed rather than forced: the player may switch it off again afterwards
and it stays off, and turning the rule back off leaves the preference wherever
the player last put it. Only the moment a rule is switched on writes a
preference; saving Settings or loading them never does. The bar below the
board described under
[Ordinary hidden selection](#ordinary-hidden-selection) is the round clock's
indication that no preference removes. Match Time has no bar - only its
numeric readout.

On a fresh install every optional rule is Off, and the values waiting behind the
toggles are Round Limit 100, Round Time 1:00, End After Rounds Without a Board
Change 10, Match Time 10:00, and Point Target 10.

Completed Replays record one or more player-facing end reasons in deterministic
order: Agreed Ending, Match Time Limit, Round Limit, No Board Change Limit,
and Point Target. Agreed Ending is the one the end gesture records. Replays written before v5 use Legacy Match because their exact ending
rules were not recorded.

The higher count wins; equal counts draw. The Match screen then names the
winner by colour - `Black Wins!`, `White Wins!` or `Draw!` - with every end
reason on the line beneath it, and shows each player's final score beside
their name in their own corner. A completed Match is saved as a Replay
when platform storage is available. Native Replay storage remains unlimited
until Replays are manually deleted. Browser storage keeps exactly the newest
three completed Replays; quota limits, blocked storage, or private mode can
prevent persistence without changing the Match result.

Only a Match that reaches scoring is recorded at all. Returning Home from a
Match still in progress abandons it and saves no Replay, exactly as an early
Restart does; its confirmation dialog says so and names the current end
gesture, so the way to keep the Match is never hidden behind the way to
discard it. Returning Home after scoring discards nothing, because the Replay
is already written.

## Restarting

Hold the shared Restart input continuously for three seconds during Round 0,
ordinary play, or after scoring.

- Restart before scoring abandons the current Match and does not save a
  Replay.
- Restart after scoring begins a fresh interactive Round 0.
- Controls, preferences, and persisted Game settings remain unchanged; the new
  Match snapshots those current Game settings.
- During ordinary play, the end gesture has priority if both are held at the
  same time: while both Select inputs are held, a held Restart does nothing,
  and any restart progress already made is cleared, so its hold begins from
  zero only once one of the Selects comes up or the Match has ended. See
  [Ending and scoring](#ending-and-scoring) for what that costs and why.
- A restart hold carried from Round 0 into Round 1 with both Select inputs
  down is cleared at the reveal, even on the frame it would have completed:
  the end gesture exists from that moment and outranks it.
- A provisional automatic end stays deferred while Restart is held, if Restart
  was already held as the Selects came up, and play stays stopped with it:
  completing the Restart abandons it without saving a Replay, while releasing
  the Restart without both Selects down commits it exactly once. If both
  Selects come down again first, the end hold resumes on the same snapshot,
  from whatever its drain has left.
- With no provisional end pending, Restart held on its own defers nothing.
  Rounds played while it is held, without both Selects ever down at once,
  count, and one that reaches an automatic limit ends the Match at once.
  Holding both Selects at once - even briefly, and even if one went down
  first - still starts the end gesture, Restart or no Restart, so an automatic
  end reached while it runs is provisional as described above. An end hold
  begun later takes a fresh snapshot, so completing it rolls back only what
  happened after it began.
- A Restart still held when the end hold completes is not suppressed. After
  scoring it counts from zero, so three more seconds restart the just-scored
  Match - whose Replay, if storage allowed one, is already written.
- An input already held when the fresh Match begins - the restart input
  included - is suppressed until it is released and pressed again. Only that
  input waits: every other action, from either player, is accepted at once, and
  the new Match's clock and Replay timeline start immediately.

## Differences from standard Go

Go Together deliberately differs from standard Go:

- two hidden selections are collected per round;
- every Match begins with a hidden three-choice-per-player Round 0;
- a hidden selection may be an active cleanup of one's own stone instead of a
  placement;
- a suicidal placement is legal, and removes the acting group;
- a 0.1-second window chooses sequential or simultaneous resolution;
- same-point and ko outcomes create permanent Gray stones;
- positional repetition restores the prior board and grays valid attempted
  points;
- there is no pass, komi, handicap, or configurable board size;
- players use cursors and Teleport Markers; and
- a three-second hold of both players' own Select inputs ends and scores the
  Match.
