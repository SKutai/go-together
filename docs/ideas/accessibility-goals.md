# Accessibility goals

Status: stated by the project owner 2026-09-19. These are the destination, not
a description of the game today; see [`README.md`](README.md). The near-term
work is deliberately narrower than this - see "Sequencing" at the end.

## The goals

1. **The whole game is playable and operable by keyboard.** Not the board only,
   which already is, but every menu, modal and screen.
2. **The desktop window is resizable and works fullscreen.**
3. **A screen reader can read any text in the game.**
4. **The game's colours are configurable** - cursors, stones, and board.
5. **Text colour, font, and font size are configurable.** Wanted, but explicitly
   the furthest out of the five.

## What each one runs into

Recorded so that whoever picks one up does not rediscover it.

**Keyboard operability** is the only goal with a design already worked out.
The hard part is not the menus, it is that
the Match screen shows live gameplay and buttons at the same time on one shared
keyboard, and Player One's default movement is the arrow keys. The answer that
survived scrutiny is that the focus ring and live gameplay are never
simultaneously active, with entry through structurally unbindable inputs
only: `KeyCode::Escape` and `KeyCode::F1`, neither of which `KeyboardKey` can
represent and both of which `KeyboardKey::from_key_code` refuses outright,
and `GamepadButton::Start`, which is absent from `ControllerInput`'s twenty
variants.

Where that stands as of 2026-09-26: **the goal is met.** Every menu screen
and the Help modal - on Home and during a Match alike - are in the
ring, as is every dialog but one. The Return Home confirmation is driven
directly by `handle_match_return` rather than by a ring, reading Escape,
Start, Enter and South itself, so it is operable without a pointer while
carrying no `Focusable`. The last gap, opening the Help modal during a
Match, closed when F1 became a reserved key: the Match HUD's two buttons,
*Home* and *Help*, still carry no `Focusable`, deliberately, because the
Match screen cannot have a ring during live play, so the way in is a key no
binding can hold rather than a ring - F1 left `KeyboardKey` as Escape was
never on it, and `handle_help_modal_keys` reads it raw on Home and the
Match. What remains is not a keyboard gap: no controller button opens it.
Start is already the pad's back, and the other structurally unbindable
buttons - Select, Mode and the stick clicks, none of which `ControllerInput`
can represent - are unassigned; choosing one is the decision for another
day, and this sentence goes once it is made. Until then opening the Help modal
mid-match takes F1 or a pointer: the Match screen carries no focus ring during
play, so a pad cannot reach the Help button either.

**Resizable and fullscreen**: as of 2026-11-28, **the goal is met.** The
desktop window can be resized down to 1040x600, maximised and made
fullscreen - by F11, which became a reserved key for it, wherever the system
lets the key through, and on macOS by the window's green button, since a Mac
with its default settings keeps F11 for itself - and the browser build's
canvas fills its page. Every screen lays itself out for the window's size
rather than being stretched: text keeps its size, and the boards and Home's
diamond grow and shrink. A window smaller than the minimum - fullscreen on a
smaller screen, a window manager that ignores the minimum, a program that
sets the size, or a browser page - is laid out as the window raised to the
minimum and drawn scaled down to fit. `docs/ARCHITECTURE.md`'s "The window's size, and what answers to
it" says how. What the goal leaves for later is remembering the window's
size, place and fullscreen state between runs, which nothing does yet, and
text that grows with the window, which is goal 5's.

Until then it was a documented constraint, not an oversight: the window was
made with `resizable: false` in both platform adapters, `CONTRIBUTING.md`'s UI
section said to check text at "the normal fixed 1040x900 window", and several
tests asserted that things fitted that exact size. So it changed the
contributor contract as well as the window - that section now asks for checks
at the smallest window, the opening one and a large one - and every
fixed-pixel layout in `src/ui/` was made a function of the window's size. The
web shell, which used to scale a fixed canvas down to fit and never up, now
sizes the canvas to the page.

**Screen readers** are three problems that look like one.

Game *state* has a ready-made answer: `src/view.rs` already defines
`PublicMatchView` and `PlayerMatchView`, complete privacy-scoped projections,
and says outright that nothing outside the module constructs either one today -
so publishing them is wiring, not modelling.

Button *labels* are worse than they look. The crate's buttons are
`bevy_ui_widgets`' `Button`, which brings the button role and no name, and
`bevy_ui` names only its own, deprecated, button from its text. So
`ui::button` names each one once, when it spawns, from its text children
(`name_new_buttons`); a name freezes at spawn-frame text, so a binding row is
named by its action but never announces its key, and the Replay Viewer's Play
never becomes Pause. Keeping names current is part of this goal.

The *browser* needs a second mechanism. Only the macOS and Windows AccessKit
adapters are vendored - `accesskit_winit` pulls in no web backend and none
exists - so a canvas cannot be served that way. The owner accepted browser
support on 2026-09-19 after it turned out to be far smaller than "write a
screen reader": the approach is to mirror text into hidden `aria-live` DOM
elements beside the canvas, which the player's own screen reader then reads.
`index.html` already has one such element for the loading status, so the
pattern exists. It needs `web-sys` features beyond the current
`["Storage", "Window"]`, and it is a genuinely separate implementation from the
desktop one, not a port of it.

### The constraint that governs all three

**A screen reader in a shared-screen game is a shared output channel.** Two
players sit at one machine with one pair of speakers, so anything announced is
heard by both - and in the browser a live region is also inspectable in
devtools by either player.

So only `PublicMatchView` may ever be published. `PlayerMatchView` must not be,
and `docs/SECURITY.md` already spells out why in a form that applies exactly
here: `PlayerMatchView::own_selected_point` carries a timing rule, because
whether it is `Some` after a press on a visibly empty point is the
accepted-or-rejected answer that silent rejection deliberately refuses to give.
Announcing it aloud hands that to the opponent.

The consequence is real and should not be papered over: a blind player can be
given the board, score, clock, round and phase, but **not their own pending
selection read back**, because saying it tells the other player. That is a
structural limit of hidden information on a shared screen, not an
implementation gap.

There is one way out, accepted in principle and written up separately in
[`announce-own-selections.md`](announce-own-selections.md): a per-player,
default-off setting that announces the owner's own hidden state, on the
assumption they are wearing headphones. It serves a blind player against a
sighted one and cannot serve two blind players, since a machine has one screen
reader and one audio output. Read that file before building any of this - it
also records why headphones make the announcement private but not the data.

**Configurable colours** would turn `src/presentation.rs` from a set of
constants into state. Worth knowing before starting: the cursors already carry
non-colour channels - different sizes (68 vs 52) and opposed contrast rings -
and so do the Replay Viewer's Teleport Markers and hidden-selection rings,
whose outside strokes are black for one player and white for the other. Any
configurability must not let a player switch those off by accident, or
choose two colours that collapse them. Player identity is also load-bearing
in the Replay Viewer, whose Key modal names each mark's shape and then
describes it by colour name.

**Configurable text** is the largest. Every size in the UI is a hard-coded
`FontSize::Px`, and Bevy's bundled fallback font does not cover non-ASCII -
which is why `CONTRIBUTING.md` requires printable ASCII in game text. A
configurable font means shipping one, which means a licensing and asset
decision, not just a setting. Since 2026-10-07 no size is below 14px, and
`no_text_is_set_below_the_floor` in `src/architecture.rs` holds that by
reading each `FontSize::Px` as a number where it is written. The game sets
one `UiScale`, in one place - `frame_main_camera` in `app::visuals` - and
only to draw a window smaller than the minimum scaled down: it is 1 for every
window at least the minimum, and never above 1. A scale applied on top - that
`UiScale`, or sizes computed from a preference - is invisible to that scan,
so whoever builds this goal will need to say what the floor means once sizes
vary: most likely that the unscaled sizes stay at or above it and that no
scale but the below-minimum one goes below 1.

## Sequencing

The owner's instruction as of 2026-09-19 is to **cover the specific
accessibility findings already identified first**, and not to start this
general work until those are done. Goal 1 is the exception only because the
findings and the goal happen to meet there: the keyboard work is both the
largest identified finding and the first goal.

Treat the rest of this file as the reason a finding-sized change should not be
allowed to grow into a goal-sized one. If a change starts pulling in
configurable colour or text that grows with the window, that is scope creep,
and the answer is a separate change later rather than a bigger one now. Goal
2 was taken on its own, as a planned piece of work, though the web canvas
finding it touched was still open: it narrowed that finding to pages smaller
than the minimum, where it still holds.
