# Accessibility findings not yet acted on

Status: recorded 2026-09-26 from an accessibility review of the whole game run
on 2026-09-18; the Contrast section was rewritten 2026-09-30, when part of it
was acted on, again on 2026-10-01, when the Replay Viewer's board marks
were, on 2026-10-02, when the one pairing that work left open was
closed, on 2026-10-04, when the last six pairings awaiting a decision
were, and on 2026-10-06, when the score badges' order was stated in the
Replay Viewer's Key; the Timing section was closed on 2026-10-05, when the
end hold began to drain instead of resetting; and the end-of-match banner,
under Feedback and comprehension, was closed on 2026-10-07, when the Match
screen began to say who won where the players already look; and the first
half of Text size was closed the same day, when every text in the game was
raised to a floor of 14px. Every finding
below survived
adversarial verification against the
code, and the severities are the *corrected* ones - the review's first pass
overstated several, and what is written here is what survived. Nothing in this
file describes a decision that has been made; see [`README.md`](README.md).

This file exists because the review's findings were recorded nowhere else.
The keyboard work that came out of it is done and merged; these are the parts
nobody has started. Measurements are as of 2026-09-18 unless noted, so
re-measure before relying on a number.

## What has already been done, so nobody redoes it

The review's headline finding was that **no single input device played the game
end to end**: every menu and button was pointer-only, while the board took no
pointer input at all. That is fixed for the keyboard and the controller - every
screen and dialog has a focus ring, and F1 opens the Help modal. Four bugs it
found are fixed too: the input gate freezing both players, the Match Time clock
never being shown, text fields silently swallowing non-ASCII, and two
player-facing strings that were not printable ASCII.

**A contrast floor exists and is enforced**, as of 2026-09-30, and the part of
this review's contrast findings that could be fixed without a design decision
has been: gray and white stones carry a ring, the Replay Viewer's disabled
labels and armed speed were brought in line with the other screens, and the
Home footer line was lightened. `CONTRIBUTING.md` states the floor,
`docs/ARCHITECTURE.md`'s "Colour and contrast" section carries the reasoning,
and `src/contrast.rs` is the checked list of what passes and what does not.
What that work could not settle is under Contrast below.

**No pairing is below the floor with nothing carrying it**, as of
2026-10-04, when the owner's decisions of 2026-10-03 on the last six landed.
`src/contrast.rs` pins that count at zero. What each was, and what answered
it:

- **Player One's red as text** - the Match sidebar heading (3.97:1 on
  `APP_BACKGROUND`), the Settings binding columns' heading (3.54:1 on
  `SURFACE_BACKGROUND`) and the Match control hint (3.78:1 on its card).
  Player Two's blue was 6.11 / 5.44 / 5.81 at the same three, so the two
  identity colours were not held to a common bar. The headings - both
  players', and the Keyboard tab's `SHARED` heading beside them - are now
  24px, where WCAG's large-text bar of 3:1 applies and today's red already
  passes. Both control hints are now written in `TEXT_MUTED` (8.92:1 on the
  card), and keep their accent frame: each hint sits on its own player's
  side of the screen, beside that player's heading, and its first line names
  the player in words (`BLACK / P1`). Both headings name the player too, so
  the accent was always reinforcement there; this was a legibility fix.

  **A lighter red was not the fix, and that is provable.** Derived
  2026-09-30. Reaching 4.5:1 on `SURFACE_BACKGROUND`, the lightest of the
  three, needs luminance 0.2298. Pure red - `srgb(1, 0, 0)`, the most
  luminous fully saturated red there is - reaches only 0.2126. So **no red at
  any saturation is compliant as normal-weight text here.** Every compliant
  candidate has to carry green and blue, which lands it in the desaturated
  coral where `DANGER` (1.0, 0.38, 0.32) already lives: the tints that measure
  6-7:1 all sit within 1.2:1 of it in lightness. The `assert_ne!` in
  `presentation.rs` only stops the two constants being literally equal, so it
  would not catch this - but the semantic collision it was written to prevent
  is real, since `DANGER` means destructive. Moving the text did not help
  either: the red's ceiling against pure black is about 4.40:1.

  The proof is about `SURFACE_BACKGROUND`, and only there is it absolute.
  Re-derived 2026-10-04: on the two darker backgrounds a fully saturated red
  does just clear 4.5:1 - pure red is 4.74:1 on `APP_BACKGROUND` and 4.51:1
  on the hint card - so "no red can fix this" was true of the Settings
  columns, not of every site. It did not change the answer: the accent is
  one colour, written as text on all three, and moving it to pure red would
  have changed every mark drawn in it as well. As it stands, `BLACK_P1_RED`
  is 3.5-4.0:1 on every background the game writes text on, so anything new
  that writes it as text below 24px will fail the guard.
- **The selected member of a group** - the Replay Viewer's armed speed and
  the Settings volume stop - differed from its neighbours in fill (1.52:1)
  and border (orange against blue, 1.44:1), which is hue alone. The selected
  member's label is now bracketed, `[1x]` against `0.5x` and `2x`, and
  `[Medium]` against `Mute`, `Low` and `High`, so a glyph changes with the
  state as it does on every other control. The colours did not change. Both
  groups are fixed-width buttons with centred labels, so the brackets move
  nothing around them and leave the label's own characters where they were.
- **The Replay Viewer's score badges when both cursors share a point** sit
  side by side, identical but for a two-pixel frame in the player's accent,
  1.54:1 apart. Their order was already fixed, Player One on the left, and
  the owner chose that order as the channel. As of 2026-10-06 the Viewer's
  Key says so: a row after the cursors, with a swatch of the pair, reads
  "When both cursors share a point, Black / P1's is on the left, as in the
  score at the top", and a test holds the sentence and the swatch to
  `score_badge_position` so the board cannot change without the Key.
  The order itself was weighed then against flipping it, and kept. The game
  has two conventions, not one with two outliers: where players *sit* puts
  White / Player 2 on the left (the Match screen's headings and hints, the
  Settings binding columns), while every *score readout* names Black first
  (the Viewer's result line, the Replay Library's summary and a Replay's
  default name - and, until 2026-10-07, the Match screen's end heading,
  whose scores now stand each beside its own name in its own corner). The badge pair is a score
  readout, and the result line above it gives the same two numbers in the
  same order. Flipping the result line to match the seating would have
  needed all four readouts to move, the saved name included.

**The board shows what a capture took**, as of 2026-10-03. It used to
paint every intersection from its current cell alone, so a round that
captured a group changed the board with nothing saying what moved or why,
and the Replay Viewer's event stepping was the only way to see it. Now the
rules report every removed stone with its owner and what took it
(`RemovedStone` in `src/rules/`), the authority keeps the last resolution's
removals and the public view publishes them, and the Match screen draws a
per-side count and, with motion reduced, a ghost of each removed stone -
its outline in its own colour with an X through it - until the next round
resolves. With motion allowed the owner's two-step animation plays instead:
the stone fades to its captor's colour and then expands, shrinks and fades
out, and the board is left clean behind it (the ghosts were narrowed to
reduced motion on 2026-10-04, at the owner's request, having been drawn in
both modes at first). `docs/ARCHITECTURE.md`'s "Motion, and what a capture
shows" has the reasoning, including why the wave runs from the stone that
took the group and what the narrowing costs.
What is deliberately not done: the Replay Viewer draws no such mark when it
steps across a capture, and nothing yet announces the record aloud.

**The Replay Viewer's two board marks have a shape and an edge**, as of
2026-10-01, to a design the owner chose. They had been marked by opacity -
Teleport Markers as the player's accent at 72%, hidden selections as
translucent stones - and were 1.09:1 to 2.30:1 on the wood. A marker is now
a diamond, a closed square outline of three opaque strokes turned on its
point; a hidden selection is a ring of three around a bare middle. That also
fixes what this file carried as a defect rather than a shortfall: a
selection of the player's *own* stone was a translucent stone over an opaque
one of the same colour, so the Viewer drew nothing for it at all, and it is
now 15.67:1 (Player One) and 17.35:1 (Player Two). `docs/ARCHITECTURE.md`'s
"Why the Replay Viewer's marks are a diamond and a ring" has the reasoning.
One pairing came out of that work still open - Player Two's marker on a
white stone, 2.75:1 - and was closed on 2026-10-02 by a third, innermost
stroke in their deep blue, 17.35:1 there. The same change closed the
marker's sides, which at Player Two's size had read as an X.

**Photosensitivity came back clean** when the review ran, because the game
had essentially no animation then: the Home marimba, and the web shell's
loading pulse, which is gated on `prefers-reduced-motion`. That result was
spent deliberately on 2026-10-03, when the capture indication became the
first motion in the core loop, and it was spent under a policy rather than
by accident. The policy is in `src/app/motion.rs`: motion is reducible, by
the Reduce Motion preference or the browser's `prefers-reduced-motion`,
and the information every animation carries also has a static form. The
replacement argument for photosensitivity is this: a captured stone's disc
makes one opposing-luminance change (its own colour to its captor's) and
then fades on the wood, stones start seventy milliseconds apart, and a disc
is about 2,500 square pixels, so even a dozen stones captured at once are
one flash in any one-second window against WCAG 2.3.1's three. The marimba
now follows the same policy. A future animation should keep both halves of
the argument, and re-measure if it adds a repeating or whole-screen change.
The Round 0 dot rows, the second animation under the policy, keep both:
each row's motion is one 13-pixel dot moving at a time, its fill changes
once as it crosses, and a refused dot fades rather than flickers, so a
player pressing Select as fast as they can interrupts one small motion with
the next rather than flashing anything.

**Round 0 no longer needs the clack to tell a refused selection from an
accepted one**, as of 2026-10-03, to a design the owner made. Each sidebar's
dots are divided by a line, filled on the board side and empty on the
other, and after a selection that changed nothing the line is drawn as a
cross until that player's next selection. With three held the dots end the
same either way, so the cross is the whole of the difference with motion
reduced; with motion allowed the owner's animation plays on top, timed so
that a refused dot's collision meets the clack rather than moving it. The
help's Round 0 page now teaches the cross beside the sound.
`docs/ARCHITECTURE.md`'s "Round 0's dot rows" has the reasoning, and
`docs/SECURITY.md` the argument that it shows an opponent nothing they can
use. What is deliberately not done: the Replay Viewer draws no dots, so a
Replay still carries a duplicate as the clack alone, and nothing yet
announces the record aloud.

**The end hold drains instead of resetting**, as of 2026-10-05, to the
owner's decision. The dual-Select hold had reset to zero on any single frame
on which both Selects were not read as held, and under shipped defaults it is
the only route to a scored match, so a player with a tremor, a weak grip or a
flaky key could not finish a game at all. A released end hold now drains at
half the rate it filled - not one-to-one, at which a player making contact
half the time never completes; at half rate they complete in about twelve
seconds, which is the case this exists for - and pressing both Selects again
carries on from wherever the drain has left it. The progress bar drains with
the timer, so a lapse is seen while there is still time to act on it, and
it names the gesture that is running rather than the timer that is
non-zero, so a restart hold run over a draining end hold is reported as the
restart. What the owner deliberately left as it was: the restart hold still
resets, being destructive and one shared input; three seconds stays, since
the barrier was the cliff rather than the length and changing both would
have hidden which helped; it is not configurable
([`configurable-hold-duration.md`](configurable-hold-duration.md) parks that
with reasons); and a modal still ends the gesture outright, because a player
reads a modal as the gesture having been interrupted. `src/gameplay/holds.rs`'s
module doc carries the reasoning, `docs/GAME_RULES.md` the rule, and the
help's Ending page tells players the bar drains rather than empties.
`src/app/motion.rs` says why the draining bar is a readout and not motion
the policy governs.

**The help defines its own words**, as of 2026-10-05, to the owner's
decision that brevity is a preference and clarity a requirement. Two pages
of terms sit between the controls page and the first rules page: point,
stone, adjacent, group, liberty and capture on the first, Gray, suicide, ko,
territory and the Teleport Marker on the second. Each is written for this
game rather than for Go - suicide is allowed, there is no separate ko rule,
Gray is permanent and gives no liberties - in the order each builds on the
last, and the pages sit before any rules page uses one of the words. The
list grew past the review's four because reading the rules pages as a
stranger to Go found more: "intersection" and "adjacent" are what "liberty"
is defined with, "territory" and "area score" were used two pages before the
Ending page explains the fill, and nothing anywhere said what a Teleport
Marker is. `src/ui/help/mod.rs`'s module docs say why the pages are a primer at
the front rather than a glossary at the back, and two tests pin the entries
and their placement. What is deliberately not done: the Replay Viewer's Key
still says "provide no liberties" in its Gray row, which is its own legend
and was outside that task; and `resolve`, `active cleanup` and `Capture
Points` were left where the pages already explain them.

**Text has a floor of 14px**, as of 2026-10-07, stated in `CONTRIBUTING.md`.
The smallest text had been an 11px validation error. What that did not
touch - any way for a player to make text larger, and the web canvas drawing
everything smaller in a browser window smaller than the smallest one the game
is laid out for - is under Text size below.

## Still open

### Contrast

Nothing here is a pairing `src/contrast.rs` counts as awaiting a decision -
that count is zero. This is where something carries the meaning, but less
well than it could.

- **The two player accents against each other** are 1.54:1. That matters less
  than it sounds wherever something else also tells the players apart - the
  cursors differ in size (68 against 52) and in border, the Replay Viewer's
  Teleport Markers likewise, its hidden-selection rings in which of their
  strokes is dark, and the sidebars, hint cards and binding columns carry a
  written name - but it is why a colour-only cue anywhere else would fail,
  as the score badges showed.

Worth knowing for the configurable-colours goal: the guard checks the shipped
palette. A palette a player can change will need the same table run against
whatever they choose, or a constraint on what they may choose.

### Text size

The finding had two halves - the text was small, and nothing could make it
larger - and only the first is closed.

- ~~**The smallest text was 11px, with 12-13px carrying substantive Settings
  copy.**~~ **Closed 2026-10-07**, by a floor of 14px that `CONTRIBUTING.md`
  states and `no_text_is_set_below_the_floor` in `src/architecture.rs`
  holds. Ten places set text below it, all in Settings and the Replay
  Library: a Game-tab field's validation error at 11px, set smaller than the
  description above it; those descriptions and the `MM:SS` and `Min` captions
  at 12px; the volume descriptions, the storage note and the controller
  status line at 13px; a Library card's Match ID at 12px; and at 13px the
  Library's browser-only retention note and the Match ID in the delete
  dialog, which a grep for literal sizes misses because it is set through a
  named constant. Gameplay was already clear of the floor.

  Measured in the running game before and after, almost nothing moved: under
  2px on the Preferences and Controller tabs and in the Library, where the
  cards are a fixed height, and nothing on the Keyboard tab. The Game tab is
  the exception, because its panel clips rather than grows: a row whose
  error line wraps is the one that pushes, and the longest message now wraps
  to two lines, taking the panel from 493px to 518.5px of its 535px.
  Raising the sizes also exposed a fault that predated them: whenever Round
  Time was 10:00 or more, the Match Time row's controls did not fit beside
  its copy, flexbox squeezed every child to make room, and the 12px caption
  was drawn as `Min` above `60:00`. The controls now keep their width and
  the copy gives way, and
  `the_game_tab_fits_its_panel_at_its_most_crowded` in
  `src/ui/settings/game.rs` holds both.
- **No text-scale mechanism exists anywhere** - no font-size setting and no
  OS scaling honoured, and the one `UiScale` the game sets only ever draws a
  window smaller than the minimum scaled down - so a player who needs more
  than 14px has no in-game remedy. Still open. Until 2026-11-28 the window
  was also a fixed 1040x900 and not resizable on either platform; it can now
  be resized, maximised and made fullscreen, and every screen keeps its text
  at its size in any window rather than enlarging it. This is goal 5 in
  [`accessibility-goals.md`](accessibility-goals.md), and it waits on a font
  and licensing decision rather than on code.
- **On the web, the floor holds in the game's pixels but not on screen,
  below the smallest window.** Found 2026-10-07, when `web/shell.js` scaled
  the fixed 1040x900 canvas by `Math.min(1, available width / 1040,
  available height / 900)`, so that whenever the browser's page area was
  smaller than the game, every piece of text was drawn proportionally
  smaller than its size: a page area 700 CSS px tall gave a scale under
  0.78, so 14px text was drawn at under 11 CSS px, and downsampled as well.
  Since 2026-11-28, as goal 2 asked, the canvas fills the page and the game lays
  itself out at its size, so text keeps its size in any page at least 1040x600
  CSS px, the smallest window the screens are laid out for. Below that the
  canvas is laid out as the page raised to the minimum and scaled down to fit,
  as the whole canvas used to be, so the finding still holds there: a page 500
  CSS px tall leaves the canvas 436 to 457 of them under the status lines, which
  scales it by at most 0.76, so 14px text is drawn at under 11 CSS px. In Chrome
  on 2026-11-28 an 800x500 page gave 0.727 before the game had started, set by
  the 436 px of height the status lines left it while they were taller, and
  0.754 once it had, set by its 784 px of width; an 855x500 page, at 200% zoom,
  gave 0.762, set by 457 px of height. Browser zoom still cannot help there,
  because zooming in shrinks the page area in CSS px. A desktop window that ends
  up smaller than the minimum - fullscreen on a smaller screen, or under a
  window manager that ignores the minimum - is drawn the same way.

### Information carried only by sound

- ~~**Teleport-to-Marker is confirmed by a marimba note only.**~~ **Closed
  2026-10-05 as not a gap**, by the project owner, on the reasoning below.
  Two parts of the original finding were also wrong, and the corrections are
  why it closes.

  The no-op is **not** indistinguishable from a successful jump:
  `MatchState::jump_to_teleport_marker` returns `None` when the cursor is
  already on the marker, so nothing is recorded and **no cue plays at all**.
  A jump is a note; a no-op is silence.

  And in ordinary play the jump has a visual, the finding's "no private
  visual" notwithstanding: the player's own cursor moves to the marker, which
  is a state change rather than an animation and so survives Reduce Motion.

  That leaves Round 0, where both cursors are hidden from both players. A
  visual confirmation there is not merely absent, it is **impossible**: the
  screen is shared, so anything drawn would tell the opponent that a player
  teleported and probably where. This is the same structural limit that
  governs the screen-reader work in
  [`accessibility-goals.md`](accessibility-goals.md).

  And it costs nothing, because **both outcomes leave the cursor in the same
  place.** Press Jump with a marker set and either you move to it or you were
  already on it; either way you are on the marker afterwards. There is no
  information in the confirmation to lose.

  One narrow case does survive, and the owner chose on 2026-10-05 to leave it
  rather than act on it: pressing Jump with **no marker set** is also silent
  and also moves nothing, so in Round 0 a player who believes they set a
  marker and did not cannot tell that from being already on it. Both leave
  them somewhere they did not intend, with nothing said. Any fix meets the
  same shared-screen wall as the rest of this finding.
- ~~**Muting Game Sound Effects deletes that information.**~~ **Closed
  2026-10-05 by the project owner**, because the information it named has
  since been given visual form and the premise no longer holds. Every member
  of `GameCue` was checked against what is on screen when it plays:

  | cue | what shows it without sound |
  | --- | --- |
  | `Countdown3` / `2` / `1` / `Go` | the countdown card |
  | the four capture cues | the removed stones' marks and the per-side count |
  | `SamePointGray` | the Gray stone appearing |
  | `OpeningDuplicate` | the dot row's divider drawn as a cross |
  | the two teleport cues | the player's own cursor moving (see above) |
  | `MatchEnd` | the end banner |
  | `MatchRestart` | the board resetting |
  | `Preview` | a Settings preview, carrying no Match state |

  So a player on Mute now learns every state change the cues accompany. What
  sound still carries alone is narrower and was never what this finding
  claimed: *which rule* produced a Gray stone is distinguished by cue and not
  on the board. That is a comprehension question rather than a deletion, it
  is not fully distinguished by sound either, and nobody has asked for it.

### Feedback and comprehension

- ~~**A rejected Select is silent.**~~ **Closed 2026-10-06 by the project
  owner.** The finding argued that the occupied-point case costs nothing to
  report. Most of it does not survive looking at: a player who presses an
  enemy stone or a Gray wall has the stone in front of them, and the board
  is the explanation. The part with merit is narrower - a failed active
  cleanup on the player's own stone, where the refusal turns on a liberty
  count the player must work out by eye - and it was judged too small to
  justify reversing a deliberate decision, replacing the guard test that
  keeps the old leak out, and carrying a refusal reason out of `authority`.
  The owner's idea for a solution, if this is ever reopened, is recorded in
  [`rejected-select-feedback.md`](rejected-select-feedback.md) together with
  the three things that make it harder than it looks.
- ~~**Go jargon is undefined in game.**~~ **Closed 2026-10-05** by two pages
  of terms in the help, to the owner's decision that players must not be
  left confused for the sake of a shorter help; see "The help defines its
  own words" above. The count here was right as a count of what a player
  reads: "liberty" appeared seven times in the help's copy, all on the rules
  pages. A grep over the help's source that day, `src/ui/help.rs` (since
  moved to `src/ui/help/mod.rs`), found eleven, because four were test
  assertions pinning that copy.
- ~~**The end-of-match banner is terse**, reading like `Player 1 wins! - Manual`,
  and mixes "Player 1" with "BLACK" in one line. The end-reason labels
  themselves are deliberate player-facing strings, not leaked identifiers.~~
  **Closed 2026-10-07**, to a design the owner chose that day. The result is
  split across the places a player already looks: the heading names the
  winner by colour (`White Wins!`, `Black Wins!`, `Draw!`), the line under it
  gives every end reason, and each player's final score joins their own name
  in their own corner, mirrored so the names stay outermost (`WHITE 23` on
  the left, `24 BLACK` on the right). The last sentence of the finding was
  half right: `Manual` was the enum's name rather than a player's word, and
  it now reads `Agreed Ending`, in the Replay Viewer's Match Settings list
  as well, since both read the one `EndReason::label`. Closing it also
  measured a fault the finding had not recorded: with every automatic limit
  crossing at once, the old one-line banner was 110 characters - 2,239 px of
  34 px text in a 1,040 px window - and wrapped to three lines over both
  corner headings and the clocks. The reason line is now 16 px and held to
  the board's width, and a test lays out the longest list it can hold with
  the game's own font.

### Timing

- ~~**`HOLD_DURATION` is three seconds, fixed, with no tolerance window**: the
  accumulator resets to zero on a single frame where the input is not
  registered, rather than decaying. Nothing in Settings adjusts, extends or
  disables it. Under shipped defaults the dual-Select hold is the only route
  to a scored match, since every automatic end rule ships disabled.~~
  **Closed 2026-10-05** by the project owner: the end hold now drains at half
  the rate it fills rather than resetting, which was the barrier; see "The
  end hold drains instead of resetting" above. Three seconds stays, fixed,
  because the barrier was the cliff and not the length, and
  [`configurable-hold-duration.md`](configurable-hold-duration.md) parks the
  setting with the owner's reasons. One correction the review's first pass
  got wrong still stands: the gesture does **not** require two people - one
  player can hold both default keys, since the semantic input ORs its
  sources.

### Not a barrier, but the review argued about it

**The 100ms simultaneity window** decides a contested point by reaction time and
is the one timing rule a player cannot adjust. Whether that is unfair or simply
the game's premise was argued both ways; it connects to
[`adjacency-conflict-resolution.md`](adjacency-conflict-resolution.md), which the
owner has deferred with reasons.

## The one thing keyboard work did not touch

**A mouse-only player still cannot play the board.** Gameplay reads keyboard and
controller only; intersections carry no `Pickable` and no picking observer. This
is the other half of the review's headline finding, and nothing has been done
about it. It is a larger piece of work than it sounds - a pointer would need a
cursor-move-then-select model that does not leak either player's hidden
selection - and it has never been scoped.
