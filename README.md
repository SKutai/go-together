# Go Together

Go Together is a local two-player strategy game built with Rust and
[Bevy](https://bevy.org/). It starts from Go's placement, capture, liberty, and
territory ideas, then gives both players hidden, revisable selections on the
same 9x9 board.

Every Match opens with **Round 0**: both invisible cursors privately build a
rolling set of three intersections before a 3-2-1-Go reveal. Shared opening
points become permanent Gray stones; all other choices become their player's
stones before ordinary Round 1 begins.

Development targets native macOS, Windows, and Linux, plus an HTML5 build for
current desktop browsers. No release artifacts are published yet.

The Match rules, clock, authoritative state, privacy-scoped views, and Replay
loading, validation, and playback also compile as a headless Rust library with
no Bevy dependency, for testing or for a future non-Bevy consumer such as a
server or a Replay analysis tool. See [Project structure](#project-structure)
and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#headless-core-and-bevy-client).

The timing between the two selections changes the result: moves more than 0.1
seconds apart resolve in order, while moves within 0.1 seconds resolve from the
same starting board. Same-point ties and positional-repetition rejections
create permanent neutral Gray stones, which provide no liberties to adjacent
groups - exactly like the edge of the board.

Read the exact implemented rules in [docs/GAME_RULES.md](docs/GAME_RULES.md).

## Features

- Local simultaneous input: keyboard and controllers on desktop, and keyboard
  plus standard-mapped browser gamepads where the browser/device supports them
- Hidden selections that can be revised until the round resolves
- A hidden, interactive Round 0 with three-choice FIFO openings and simultaneous reveal
- Sequential and simultaneous move resolution with neutral Gray outcomes
- Go-like captures, suicidal group self-removal, full positional repetition,
  territory, and optional capture-point scoring
- Active cleanup: select one of your own stones to remove adjacent opposing
  groups that have no liberties - the only way to clear a group a simultaneous
  round left standing
- Configurable Match/round time, round-count, no-board-change, and
  point-target limits; a running Round Time also draws a bar below the board
  that shrinks toward the centre and cannot be hidden
- Per-player Teleport Markers for quick cursor movement
- Remappable keyboard and controller profiles saved in Settings
- Separate Game and UI sound levels
- Original generated marimba audio and a Home-screen Easter egg
- Automatic Replay saving for completed Matches
- Persisted browser Settings and a deliberately bounded browser Replay library
- Replay search, sorting, rename, delete, significant-event stepping, and
  0.5x/1x/2x playback
- Silent Replay seeking with visible selections, Teleport Markers, and
  capture-aware cursor-following score badges
- Replay Viewer Match Settings modal for the immutable rule snapshot and every
  recorded end reason; Back returns to the Replay Library, and the Replay
  Library is the only route from a Replay to Home - by its Home button, or by
  Escape or a controller's East or Start button

## Controls

### Keyboard defaults

| Side | Move | Select | Set Teleport Marker | Teleport |
| --- | --- | --- | --- | --- |
| White/P2 (left) | E S D F | 2 | Backtick | 1 |
| Black/P1 (right) | Arrow keys | M | B | N |

Hold **Space** for three seconds to restart during or after a Match. Hold both
players' **Select** inputs for three seconds to end and score starting in Round
1. With defaults, the end gesture is **M + 2**. During Round 0, each sidebar has
a row of dots showing only that player's opening-choice count; cursors and
choices remain hidden. A fourth distinct choice replaces the oldest, while
choosing one of the player's current three again leaves the order unchanged,
plays a wooden clack, and shows a cross on that player's row until their next
choice. The temporary movement/Select hint in each sidebar still
disappears independently after that player first moves, makes an accepted
selection, sets a Teleport Marker, or teleports; a rejected Select press
leaves it up.

### Controller defaults

| Action | Input |
| --- | --- |
| Move | D-pad |
| Select | South face button |
| Set Teleport Marker | West face button |
| Teleport | East face button |
| Restart Match | North face button |

The first connected controller is assigned to White/P2 and the second to
Black/P1. Keyboard and assigned controllers remain active together. Settings
can remap every gameplay input and change timer, round, motion, and sound
preferences;
turning Round Time or Match Time on switches the matching readout on for you,
and you can switch it off again.
In a browser, keyboard play is supported; gamepads work only when the browser
and device expose the standard mapping expected by the Web Gamepad API.

### Menus without a pointer

Every menu works without a mouse: **Tab** or the arrow keys highlight a button
and **Enter** or **Space** presses it, while a controller's D-pad and South
face button do the same - on every screen outside a match, dialogs included,
and inside the Help modal wherever it is open, mid-match included.
**F1** opens the Help modal on the title screen and during a match, as the
**Help** button on either does, and closes it again; once open, its page
arrows and Close work from the keyboard or a controller, and **Escape** or a
controller's back button closes it from anywhere, which is what its Close
button does. No controller button opens it during a match yet - Start is
already the pad's back button there, and none of the other buttons a binding
cannot claim has been given the job. A pad cannot press the Help button either,
since the Match screen carries no focus ring during play, so opening it there
takes F1 or a pointer. In
Settings that includes rebinding: highlight a binding and press it, and the
next key or controller input becomes the binding; a number field takes typed
digits once pressed and **Enter** commits them. In the Replay Library the
search field is typed once pressed, and **Enter** or **Escape** leaves it; a
rename dialog opens already taking text, where **Enter** saves the new label
and **Escape** discards it. While a binding, a number or a text field is
waiting for input, those keys go to it and the highlight stays put
until it is done or cancelled (a controller can open a number or text field
and back out of it, but only a keyboard can type into it). Every screen can
also be left without a mouse: **Escape** steps back from every screen below
the title screen - closing whatever is open first, the Help modal included -
and a controller goes back with **Start** on every screen, and with the East
face button as well on the menu screens. East also closes the Help modal during a
match - the one *menu* use it has there, since East is otherwise Teleport to
Marker and is read as back only while that modal is open. During a match, **Escape** or
**Start** asks for confirmation before returning to the title screen
(**Enter** or South confirms), because leaving abandons the match. The one
carve-out: while Settings is capturing a controller binding, East is an
input to bind rather than back, and Start is the controller's way to cancel
the capture. These menu inputs are fixed rather than remappable and belong to
neither player.

## Build and run locally

Install the [stable Rust toolchain](https://www.rust-lang.org/tools/install).
Bevy may also require platform development packages; follow the
[Bevy platform setup guide](https://bevy.org/learn/quick-start/getting-started/setup/).
On Debian or Ubuntu, the packages used by CI are:

```sh
sudo apt-get install g++ libasound2-dev libudev-dev libwayland-dev \
  libx11-dev libxkbcommon-dev pkg-config
```

Then clone and run:

```sh
git clone https://github.com/SKutai/go-together.git
cd go-together
cargo run
```

For the browser build, install the pinned Trunk version once:

```sh
cargo install trunk --locked --version 0.21.14
trunk serve
```

Open the local URL printed by Trunk. After the game code loads, click or tap the
canvas, or focus it and press a key, to start the app inside a trusted browser
interaction. This lets Web Audio initialize without autoplay failures. Build the
self-contained itch.io directory with:

```sh
scripts/build-web.sh
```

That command produces and verifies `dist/index.html`, hashed JavaScript and
WebAssembly files at the `dist/` root, `dist/assets/`, `dist/web/`, and the MIT
license.
All runtime references are relative; there is no CDN, PWA, or service worker.
The build records no path of the machine it ran on, and the verification
fails if a file in `dist/` holds one.
See [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md) for packaging guidance.

Run the contributor checks with:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo clippy --locked --all-targets --no-default-features -- -D warnings
cargo test --locked --no-default-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --document-private-items
RUSTDOCFLAGS='-D warnings' cargo doc --locked --target wasm32-unknown-unknown --no-deps --document-private-items
cargo build --locked
cargo clippy --locked --target wasm32-unknown-unknown -- -D warnings
python3 tools/generate_marimba.py --check
python3 tools/check_markdown_links.py
scripts/build-web.sh
```

The two `--no-default-features` commands lint and test the headless core
alone - no Bevy, no window, including the `tests/headless_*.rs` integration
tests that drive it through the public API. Every configuration denies
warnings; see [docs/TESTING.md](docs/TESTING.md) for what that constrains
about where a helper may live. That file also documents the
property-test layer (`cargo test rules::properties`), the two deterministic
size/timing measurement tools (`cargo test replay_size_measurements`,
`cargo test --release replay_save_timing_measurements`), and how to fuzz a
parsing boundary manually if you have the nightly toolchain and `cargo-fuzz`
installed.

## Project structure

Go Together is both a library crate (`go_together`) and a binary crate built
from it (`src/main.rs`, `cargo run`). The library splits into an always-compiled
headless core with no Bevy dependency, and a Bevy client gated behind the
default `client` Cargo feature. `cargo build --no-default-features --lib`
compiles only the core - no rendering, windowing, audio, or storage backend.

| Path | Responsibility | Layer |
| --- | --- | --- |
| `src/lib.rs` | Crate root: module map and the `client`-gated `run` entry point | - |
| `src/main.rs` | Minimal binary entry point; calls `go_together::run()` | - |
| `src/rules/` | Board, captures, Gray outcomes, ko, and scoring (`values`, `board`, `scoring`, `resolution`) | Core |
| `src/opening.rs` | Round 0 rolling selections and simultaneous reveal | Core |
| `src/match_settings.rs` | Validated optional Match rules | Core |
| `src/clock.rs` | `MatchInstant`/`MatchDuration`: whole-microsecond Match time behind every authoritative timing decision | Core |
| `src/authority/` | Authoritative `MatchState`/`MatchPhase`: commands and outcomes (`values`, `outcomes`, `state`, `timing`) | Core |
| `src/view.rs` | Privacy-scoped read-only projections of `MatchState` (`PublicMatchView`, `PlayerMatchView`) | Core |
| `src/replay/` | Replay schema, compatible loading, validation, playback, and recording - the public API for reading and playing back a Replay | Core |
| `src/cue.rs` | Semantic `GameCue` vocabulary shared by Replay and the client's audio | Core |
| `src/app/` | Window setup, plugin composition, screen state, and system order | `client` |
| `src/gameplay/` | Wraps `MatchState` in a Bevy resource: countdown, semantic actions, holds, recording | `client` |
| `src/input/` | Semantic actions, keyboard bindings, controller assignment, and hysteresis | `client` |
| `src/platform/` | Native filesystem and browser-storage adapters | `client` |
| `src/diagnostics.rs` | The boundary between subsystem typed errors and what a player sees versus what only a log sees | `client` |
| `src/replay_library/` | The saved-Replay collection the Library screen browses, its catalog-entry projection, and search/sort | `client` |
| `src/settings/` | Settings schemas v1-v6, migrations, and atomic persistence | `client` |
| `src/audio.rs` | Bevy playback: marimba assets and the cue queue | `client` |
| `src/ui/` | Home, Match, Settings, Help, Replay Library, and Replay Viewer UI | `client` |
| `src/presentation.rs` | Shared colors - the only file that may construct one - and player-facing action terminology | `client` |
| `src/architecture.rs` | Test-only guards over the crate's own source: the checked module dependency table, the printable-ASCII rule for every string literal in production code, the rule that only `presentation.rs` constructs a color, and the directory scans the negative assertions use | - |
| `src/contrast.rs` | Test-only guard holding every color in `presentation.rs` to the WCAG contrast floor, over a table of which color is drawn on which | - |
| `tests/headless_match.rs` | Integration test driving a Match through the public library API only | - |
| `tests/headless_replay.rs` | Integration test loading, validating, inspecting, re-serializing, and playing back every Replay schema version through the public library API only | - |
| `tests/fixtures/replays/` | Literal on-disk Replays, one per schema version plus richer shapes, pinned to the builders that produced them | - |
| `assets/audio/` | Committed original marimba WAV files and provenance | - |
| `tools/generate_marimba.py` | Deterministic audio generator and `--check` mode | - |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the headless/client
boundary, plugin composition, state flow, dependency direction, and safe
extension recipes.

## Learning from the project

The code demonstrates:

- composing a Bevy app from focused `Plugin` types;
- modeling screens with `States`, `OnEnter`, `OnExit`, and run conditions;
- using ECS resources, components, messages, systems, and ordered system sets;
- separating physical keyboard/gamepad input from semantic game actions;
- keeping platform storage and asset-location differences behind adapters;
- keeping domain rules independent from Bevy for fast unit tests;
- modeling Round 0 selection/reveal rules as a pure serialized domain type;
- building fixed-window Bevy UI with reusable, role-based components;
- queuing semantic audio cues instead of playing sounds from gameplay logic;
- writing versioned JSON atomically and migrating older settings;
- reconstructing deterministic Replays from timestamped frame snapshots; and
- testing pure logic alongside small ECS integration apps.

Start with `src/main.rs`, then follow `app::run` into the plugin map. The
[architecture guide](docs/ARCHITECTURE.md) explains the frame pipeline before
you read individual systems.

## Making it your own

- **Change rules:** start in `src/rules/resolution.rs` (or `board.rs` for a
  move/capture/cleanup effect), then update the `MatchState` commands in
  `src/authority/state.rs` that call them. `src/gameplay/` changes only if a
  new outcome has to be recorded or given a sound. Update the exact rules
  document.
- **Change Round 0:** start in `src/opening.rs`, then update the authoritative
  lifecycle in `src/authority/` (`try_opening_select`, `reveal_round_zero`,
  and the countdown in `advance_clock`), event recording in `src/gameplay/`,
  and the Round 0 timeline check in `src/replay/validation/opening.rs`
  (Replay schema v4 and later).
- **Change board size:** update `BOARD_SIZE` in `src/rules/values.rs`, both
  live and Replay geometry, the decorative star-point table (`STAR_POINTS`
  in `src/app/visuals.rs`), compatibility tests, and layouts. Existing
  Replay boards serialize a fixed cell count, so this requires a schema
  migration.
- **Change controls:** edit defaults in `src/input/keyboard.rs` or
  `src/input/controller/bindings.rs`; persisted changes belong in
  `src/settings/schema.rs` with a new migration in `src/settings/compat.rs`.
- **Change sounds:** map semantic cues in `src/audio.rs` and regenerate original
  samples with `tools/generate_marimba.py`.
- **Add a screen:** add an `AppScreen` variant in `src/app/mod.rs`, create a
  focused plugin under `src/ui/`, and tag each independently spawned root with
  `DespawnOnExit(that screen)` - Bevy's own built-in state-scoped despawn.

More examples are in the
[safe extension recipes](docs/ARCHITECTURE.md#safe-extension-recipes).

## Local data and compatibility

On native platforms, Settings are stored in `controls.json`; each completed
Replay is one `replays/<match-uuid>.json` file.

| Platform | Application data directory |
| --- | --- |
| macOS | `~/Library/Application Support/Go Together/` |
| Windows | `%APPDATA%\Go Together\` |
| Linux/Unix | `$XDG_CONFIG_HOME/go-together/`, or `~/.config/go-together/` |

Settings schemas v1-v6 and Replay schemas v1-v6 remain loadable. Treat serde
field names, enum names, schema versions, Replay IDs, and filenames as a public
compatibility contract. The one name ever withdrawn is the keyboard key `f1`,
when F1 was reserved for the Help modal: a settings file that still names
it keeps everything but its keyboard section, which falls back to defaults
with a warning. Back up local data before experimenting with schema changes.

Settings v5 adds the Game tab. Optional numeric rules use bounded whole values:
round and no-board-change limits are 1-1000, point targets are 1-81, and
Round/Match Time are integer seconds from 1 through 3600 (displayed as
MM:SS). An enabled Round Time can never exceed Match Time: raising Round Time
raises Match Time with it, a Settings file whose Match Time is below its
enabled Round Time has it raised on load, and a Replay's immutable snapshot
that breaks the same invariant is rejected instead of normalized. Disabling a
rule retains its configured value but makes it inactive. Each new Match takes
an immutable snapshot.

Replay v4 stores `opening_completed` separately. Replay v5 adds the immutable
Match settings snapshot, ordered end reasons, capture totals, the
no-board-change streak, round clock, and timeout events. Persisted `round_count`
continues to mean completed ordinary rounds, so older Replay metadata is not
silently reinterpreted; the Replay Library screen describes an entry that
recorded a Round 0 as `Round 0 + N ordinary rounds` and keeps the historical
`N rounds` wording for older cards. Replay v6 (current) omits an event's board
on disk whenever it matches the previous event's - transparent on load, and
roughly halving typical Replay file size; see
[Replay size](docs/ARCHITECTURE.md#replay-size).

Native completed Replays remain unlimited until a player deletes them. Packaged
native builds look for bundled `assets/` next to the executable first, with the
current working directory retained only as a development fallback.

The browser adapter stores Settings and completed Replays in same-origin
`localStorage`, under the keys `io.github.skutai.go-together.settings.v1` and
`io.github.skutai.go-together.replays.v1`; the Replay value is a versioned
catalog envelope (currently version 1) holding each Replay in its own schema
version, and both key names are part of the compatibility contract above. It
keeps **exactly the newest three completed Replays** while the native library
remains unlimited. Storage quota limits, blocked storage, or
private-browsing policy can prevent persistence; the game must continue without
assuming a write succeeded. The small Replay limit and bounded catalog keep
browser storage conservative. Future format changes must version stored values,
read supported older versions, migrate deliberately, and isolate invalid records
rather than silently reinterpreting them.

## Generated audio

The 11 compact marimba samples and short Round 0 duplicate-selection clack are
original additive-synthesis assets with no downloaded or third-party samples.
The deterministic generator, parameters, license coverage, regeneration
command, and verification command are documented in
[assets/audio/README.md](assets/audio/README.md).

## Contributing

New contributors are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the
small-change workflow, architecture conventions, schema rules, audio updates,
and testing expectations.

- **A bug** goes in an
  [issue](https://github.com/SKutai/go-together/issues/new/choose), where a
  form asks for what a fix needs. An idea or a question can go in a blank
  one.
- **A suspected vulnerability** is reported privately instead:
  [docs/SECURITY.md](docs/SECURITY.md#reporting-a-vulnerability) says how.
- **A coding agent** working on a contribution should start from
  [AGENTS.md](AGENTS.md).

## License

Repository code and assets, including the original generated WAV files, are
distributed under the [MIT License](LICENSE), so you may learn from them, modify
them, reuse parts, and make the project your own subject to that license.
Release and trust boundaries are documented in
[Distribution](docs/DISTRIBUTION.md) and [Security](docs/SECURITY.md).
