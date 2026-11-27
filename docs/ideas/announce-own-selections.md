# Announcing a player's own hidden state, for a player on headphones

Status: accepted in principle by the project owner 2026-09-19, with the
limitation below understood and accepted. Not scheduled. Depends on
screen-reader support existing first; see
[`accessibility-goals.md`](accessibility-goals.md). Nothing here describes
current behaviour - see [`README.md`](README.md).

## The problem it solves

A screen reader on a shared-screen game is a shared output channel. Two players
sit at one machine with one pair of speakers, so anything announced is heard by
both. That is why only `PublicMatchView` may be published by default: a blind
player can be told the board, score, clock, round and phase, but not their own
pending selection, because saying it aloud hands it to the opponent.

Headphones change that. If the announcement reaches one player's ears only,
their own hidden state can be read back to them without reaching anyone else.

## The shape

A per-player, default-off setting - "announce my own selections", worded so the
headphone assumption is part of the setting rather than buried in a manual.
When Player One enables it, Player One's own Round 0 selections, own pending
ordinary selection and own Teleport Marker become available to the
accessibility layer. Player Two's data is unaffected by Player One's choice.

## Who it serves, and who it does not

The owner's read, which is the right one: **this works for one blind player
against a sighted player, and breaks down for two blind players.**

Two blind players cannot be served by it. An operating system has one screen
reader and one default audio output; there is no practical way to route Player
One's announcements to one headset and Player Two's to another. Two
simultaneous screen readers on one machine is not a configuration anyone runs.

That is not a reason to skip it. One blind player playing with a sighted friend
is a real pairing and is currently impossible; supporting it is worth doing
even though the symmetric case stays out of reach. Say so plainly rather than
implying the feature makes the game fully accessible to two blind players.

## The limitation to be honest about

**Headphones make the announcement private, not the data.**

Enabling this puts the owning player's hidden state into the accessibility tree
on desktop, or into a DOM live region in the browser. Both are readable with
off-the-shelf tools - an accessibility inspector, or devtools - by anyone with
access to the machine. Today that state is not exposed anywhere, so a curious
opponent would need a debugger; afterwards they would need a menu item.

That is a genuine, if small, change to the secrecy property the game is built
on, and it is the reason the setting is per-player and default-off: nobody who
does not opt in is affected, and the player who opts in is the one making the
trade about their own information. The setting's own description should carry
that, not just the headphone assumption.

`docs/SECURITY.md`'s trust-boundary section and
`PlayerMatchView::own_selected_point`'s timing rule both need a sentence when
this lands. The timing rule in particular becomes conditional rather than
absolute - "no live view may surface it while its round is unresolved" would
gain "unless its owner has opted in" - and that is exactly the kind of
weakening that must be written down where the rule lives rather than only in a
commit message.

## What it costs structurally

**It would be the first per-player preference.** `GamePreferences`
(`src/settings/config.rs`) is entirely global today - every one of its four
fields applies to both players at once. Per-player data in settings does have
precedent: `ControllerBindings` holds `profiles: [[ControllerInput; _]; 2]`
indexed by player. So the shape exists, just not in the preferences struct.

Settings are at schema v5, so this is a v6 with a migration, under the standing
rule in `CONTRIBUTING.md`'s persisted-data section: add a version, never
reinterpret one.

## Open questions

- Is it one setting per player, or one setting plus a "which player has the
  headphones" choice? The first is more honest about what it controls; the
  second is fewer controls in the UI.
- Does it announce the selection only, or also the rejected-press feedback the
  game withholds from everyone today? The second is more useful and a larger
  departure from the current rule.
- Does the browser build get it at all? A DOM live region is more casually
  inspectable than an accessibility tree, and the answer might reasonably be
  desktop-only.
- What happens in a Replay? Replays are complete historical records and already
  show both players' resolved selections after the fact, so nothing here
  applies - but confirm rather than assume.
